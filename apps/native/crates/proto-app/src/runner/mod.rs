//! Generation runner (proto-worker-7): drains `s.composer.outbox`, starts proto-ai jobs (Codex or the Claude
//! agent queue, per `request.agent`), creates the placeholder cards and streams results into the store
//! (reference `src/store/core.ts` generate/follow + the in-place edit writes of `lib/server/ai-edit.ts`).
//!
//! Jobs run on the host's tokio runtime; a forwarder task per job sends its events over a channel and wakes the
//! UI; `poll` applies them with `s.apply`. Store writes happen here only, on the UI thread, as one batch per event.

pub mod changes;
mod host;
#[cfg(test)]
mod e2e_tests;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};

use futures_util::StreamExt;
use proto_ai::VariationEvent;
use proto_core::{Change, DesignKind, DesignStatus, Point, RecordKind, Run, VariationRequest};
use rune::{Tx, Waker};

pub use host::{AiHost, HostConfig};

use crate::app::data::Source;
use crate::app::state::AppState;
use crate::composer::Outgoing;

/// Which runner entry a job's events belong to.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    /// A generation run, by run id.
    Run(String),
    /// An in-place edit, by job id.
    Edit(String),
}

struct Msg {
    key: Key,
    event: VariationEvent,
}

/// A generation in flight.
pub struct ActiveRun {
    pub run: Run,
    pub job_id: String,
    /// A job-level error (`index: null`): what the cards still generating at the end show.
    failed: Option<String>,
}

/// How a run ended (reference: the job's status, an abort, or done).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The job finished; single cards may still have failed (see their status).
    Done,
    /// A job-level error (e.g. not logged in, planning failed).
    Failed(String),
    /// Cancelled with `cancel_run`.
    Cancelled,
}

/// A run that ended, for senders that follow up (spoke focus, "every direction failed" notices).
#[derive(Clone, Debug, PartialEq)]
pub struct Finished {
    pub run_id: String,
    /// Its cards by direction index (the reference's `generate()` result).
    pub design_ids: Vec<String>,
    pub outcome: Outcome,
}

/// Finished runs kept until taken (oldest dropped beyond this).
const MAX_FINISHED: usize = 100;

/// A fresh id for `Outgoing::Generate { run_id }`.
pub fn new_run_id() -> String {
    proto_core::ops::new_id(8)
}

struct ActiveEdit {
    design_id: String,
    instruction: String,
}

pub struct RunnerState {
    /// Built on first use (tests set `config` before).
    pub config: HostConfig,
    host: Option<AiHost>,
    wake: Option<Waker>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    runs: HashMap<String, ActiveRun>,
    edits: HashMap<String, ActiveEdit>,
    cancelled: HashSet<String>,
    /// Leftover runs/edits of an earlier process were cleaned up (once the real store is loaded).
    reaped: bool,
    /// Why the last request couldn't start (not tied to a card), newest last.
    pub notices: Vec<String>,
    finished: Vec<Finished>,
}

impl Default for RunnerState {
    fn default() -> Self {
        let (tx, rx) = channel();
        RunnerState {
            config: HostConfig::default(),
            host: None,
            wake: None,
            tx,
            rx,
            runs: HashMap::new(),
            edits: HashMap::new(),
            cancelled: HashSet::new(),
            reaped: false,
            notices: Vec::new(),
            finished: Vec::new(),
        }
    }
}

impl RunnerState {
    /// The generation host, if started (the embedded server must share its `Ai`).
    pub fn host(&self) -> Option<&AiHost> {
        self.host.as_ref()
    }

    /// Generations in flight (for an activity list).
    pub fn active_runs(&self) -> impl Iterator<Item = &ActiveRun> {
        self.runs.values()
    }

    /// All runs that ended and nobody took yet, oldest first. Prefer `take_finished_where` so other readers keep
    /// their entries.
    pub fn take_finished(&mut self) -> Vec<Finished> {
        std::mem::take(&mut self.finished)
    }

    /// The ended runs `mine` accepts (e.g. by run id), oldest first; the rest stay for other readers (bounded:
    /// the oldest unclaimed ones are dropped beyond 100).
    pub fn take_finished_where(&mut self, mut mine: impl FnMut(&Finished) -> bool) -> Vec<Finished> {
        let (taken, rest): (Vec<Finished>, Vec<Finished>) = std::mem::take(&mut self.finished).into_iter().partition(|f| mine(f));
        self.finished = rest;
        taken
    }

    pub fn is_running(&self, run_id: &str) -> bool {
        self.runs.contains_key(run_id)
    }

    pub fn is_busy(&self) -> bool {
        !self.runs.is_empty() || !self.edits.is_empty()
    }
}

/// From `AppState::setup`: remember how to wake the UI when a job sends an event.
pub fn setup(s: &mut AppState, wake: Waker) {
    s.runner.wake = Some(wake);
}

/// Start the host if needed (store data dir known by now, or the default one).
pub fn ensure_host(s: &mut AppState) -> Option<&AiHost> {
    if s.runner.host.is_none() {
        let dir = s.store().data_dir().map(|p| p.to_path_buf());
        match AiHost::start(&s.runner.config, dir) {
            Ok(h) => s.runner.host = Some(h),
            Err(e) => s.runner.notices.push(format!("Couldn't start generation: {e}")),
        }
    }
    s.runner.host.as_ref()
}

/// From `AppState::poll`: clean up leftovers once, start what the composer sent, apply job events.
pub fn poll(s: &mut AppState, _clock: f64, _tx: &mut Tx) -> bool {
    let mut changed = false;
    if !s.runner.reaped && s.data.source == Source::Ready {
        s.runner.reaped = true;
        let alive = |job: &str| s.runner.host.as_ref().is_some_and(|h| h.ai().jobs().exists(job));
        let changes = changes::reap(s.snap(), alive);
        changed |= apply(s, changes, "runner: interrupted runs");
        // Interrupted generations continue on their own: run each card's stored request again.
        let interrupted: Vec<String> = s.snap().designs()
            .filter(|d| d.status == DesignStatus::Error && d.error.as_deref().is_some_and(|e| e.starts_with("Interrupted")))
            // Once per card: a card that was already restarted by itself and got interrupted again waits for the
            // Retry button (otherwise every app start would queue the same job again and leave the old one behind).
            .filter(|d| d.request.as_ref().is_some_and(|r| !r.extra.contains_key("autoRetried")))
            .map(|d| d.id.clone())
            .collect();
        for id in interrupted {
            changed |= retry_with(s, &id, true);
        }
    }
    // The card's Retry button.
    for a in s.cards.take_actions_where(|a| matches!(a, crate::cards::CardAction::Retry(_))) {
        if let crate::cards::CardAction::Retry(id) = a {
            changed |= retry(s, &id);
        }
    }
    if !s.composer.outbox.is_empty() {
        for out in std::mem::take(&mut s.composer.outbox) {
            changed |= match out {
                Outgoing::Generate { space_id, parent_id, kind, label, request, origin, run_id } => {
                    let start = Start { space_id: &space_id, parent_id: parent_id.as_deref(), kind, label: label.as_deref(), origin, run_id };
                    start_generate(s, start, &request)
                }
                Outgoing::Edit { design_id, request } => start_edit(s, &design_id, &request),
            };
        }
    }
    while let Ok(msg) = s.runner.rx.try_recv() {
        changed |= match msg.key {
            Key::Run(run_id) => on_run_event(s, &run_id, msg.event),
            Key::Edit(job_id) => on_edit_event(s, &job_id, msg.event),
        };
    }
    changed
}

/// Run an errored card's stored request again where it stands, and drop the failed card. The stored request has
/// its base html blanked, so refill it from the cards it names (and the parent).
pub fn retry(s: &mut AppState, design_id: &str) -> bool {
    retry_with(s, design_id, false)
}

/// `auto`: started by the app itself after an interruption; the new card's request remembers it.
fn retry_with(s: &mut AppState, design_id: &str, auto: bool) -> bool {
    let Some(d) = s.snap().design(design_id) else { return false };
    if d.status != DesignStatus::Error {
        return false;
    }
    let Some(mut request) = d.request.clone() else { return false };
    let (space_id, parent_id, kind, origin) = (d.space_id.clone(), d.parent_id.clone(), d.kind, d.position);
    if request.base_html.is_none() {
        request.base_html = parent_id.as_deref().and_then(|p| s.snap().design(p)).map(|p| p.html.clone()).filter(|h| !h.is_empty());
    }
    for key in ["components", "bases"] {
        if let Some(serde_json::Value::Array(list)) = request.extra.get_mut(key) {
            for item in list.iter_mut() {
                let html = item.get("id").and_then(|v| v.as_str()).and_then(|id| s.snap().design(id)).map(|x| x.html.clone());
                if let (Some(html), serde_json::Value::Object(o)) = (html, item) {
                    o.insert("html".into(), serde_json::json!(html));
                }
            }
        }
    }
    if auto {
        request.extra.insert("autoRetried".into(), serde_json::json!(true));
    }
    let start = Start { space_id: &space_id, parent_id: parent_id.as_deref(), kind, label: None, origin: Some(origin), run_id: None };
    let started = start_generate(s, start, &request);
    apply(s, vec![Change::delete(RecordKind::Designs.path(design_id))], "runner: retry replaces the failed card");
    started
}

fn apply(s: &mut AppState, changes: Vec<Change>, reason: &str) -> bool {
    if changes.is_empty() {
        return false;
    }
    if let Err(e) = s.apply(changes, reason) {
        s.runner.notices.push(format!("Couldn't save: {e}"));
    }
    true
}

/// Follow job `job_id`'s events (replay from the start) into the channel, waking the UI per event.
fn forward(s: &RunnerState, key: Key, job_id: &str) {
    let Some(host) = &s.host else { return };
    let Some(stream) = host.ai().jobs().subscribe(job_id, 0) else { return };
    let (tx, wake) = (s.tx.clone(), s.wake.clone());
    host.handle().spawn(async move {
        let mut stream = Box::pin(stream);
        while let Some(event) = stream.next().await {
            if tx.send(Msg { key: key.clone(), event }).is_err() {
                break;
            }
            if let Some(w) = &wake {
                w();
            }
        }
    });
}

struct Start<'a> {
    space_id: &'a str,
    parent_id: Option<&'a str>,
    kind: DesignKind,
    label: Option<&'a str>,
    origin: Option<Point>,
    run_id: Option<String>,
}

fn finished(s: &mut AppState, f: Finished) {
    let list = &mut s.runner.finished;
    list.push(f);
    let drop = list.len().saturating_sub(MAX_FINISHED);
    list.drain(..drop);
}

fn start_generate(s: &mut AppState, start: Start, request: &VariationRequest) -> bool {
    let Start { space_id, parent_id, kind, label, origin, run_id } = start;
    // A pre-allocated id that is already running would mix two runs: take a fresh one instead.
    let run_id = run_id.filter(|id| !id.is_empty() && !s.runner.runs.contains_key(id)).unwrap_or_else(new_run_id);
    let ai_req = match changes::to_ai_request(request, space_id) {
        Ok(r) => r,
        Err(e) => {
            s.runner.notices.push(e.clone());
            finished(s, Finished { run_id, design_ids: Vec::new(), outcome: Outcome::Failed(e) });
            return true;
        }
    };
    let guidelines = changes::guidelines(s.snap(), space_id);
    if ensure_host(s).is_none() {
        let msg = s.runner.notices.last().cloned().unwrap_or_default();
        finished(s, Finished { run_id, design_ids: Vec::new(), outcome: Outcome::Failed(msg) });
        return true;
    }
    let host = s.runner.host.as_ref().expect("host started");
    let job_id = host.enter(|ai| ai.start_variations(ai_req, guidelines));
    let run = changes::new_run(s.snap(), &run_id, &job_id, space_id, parent_id, kind, label, request, origin);
    apply(s, vec![changes::put_run(&run)], &format!("runner: start {run_id}"));
    s.runner.runs.insert(run_id.clone(), ActiveRun { run, job_id: job_id.clone(), failed: None });
    forward(&s.runner, Key::Run(run_id), &job_id);
    true
}

fn start_edit(s: &mut AppState, design_id: &str, request: &VariationRequest) -> bool {
    let d = match changes::edit_target(s.snap(), design_id) {
        Ok(d) => d,
        Err(e) => {
            s.composer.edit_error = Some(e);
            return true;
        }
    };
    let mut v = serde_json::to_value(request).unwrap_or_default();
    v["designId"] = serde_json::json!(design_id);
    let req: proto_ai::EditRequest = match serde_json::from_value(v) {
        Ok(r) => r,
        Err(e) => {
            s.composer.edit_error = Some(format!("Invalid request: {e}"));
            return true;
        }
    };
    let agent = if req.agent == Some(proto_ai::AgentChoice::Claude) { "claude" } else { "codex" };
    let target = proto_ai::EditTarget { id: d.id.clone(), title: d.title.clone(), html: d.html.clone(), space_id: d.space_id.clone() };
    let guidelines = changes::guidelines(s.snap(), &d.space_id);
    let instruction = req.instruction.clone();
    let Some(host) = ensure_host(s) else { return true };
    let job_id = host.enter(|ai| ai.start_edit(target, req, guidelines));
    let start = changes::edit_start(s.snap(), design_id, &job_id, &instruction, agent);
    apply(s, start.into_iter().collect(), &format!("ai edit start {job_id}"));
    s.runner.edits.insert(job_id.clone(), ActiveEdit { design_id: design_id.to_string(), instruction });
    forward(&s.runner, Key::Edit(job_id.clone()), &job_id);
    true
}

fn on_run_event(s: &mut AppState, run_id: &str, event: VariationEvent) -> bool {
    let Some(mut active) = s.runner.runs.remove(run_id) else { return false };
    let (changes, done) = match event {
        VariationEvent::Plan { directions } => (changes::on_plan(s.snap(), &mut active.run, &directions), false),
        VariationEvent::Variation { index, variation } => {
            (changes::on_variation(s.snap(), &active.run, index, &variation).into_iter().collect(), false)
        }
        VariationEvent::Error { index: Some(i), message } => {
            (changes::on_error(s.snap(), &active.run, i, &message).into_iter().collect(), false)
        }
        VariationEvent::Error { index: None, message } => {
            active.failed = Some(message);
            (Vec::new(), false)
        }
        VariationEvent::Done => {
            let cancelled = s.runner.cancelled.remove(run_id);
            let mut failed = active.failed.clone();
            if cancelled {
                failed.get_or_insert_with(|| "Cancelled".into());
            }
            let outcome = match (&active.failed, cancelled) {
                (_, true) => Outcome::Cancelled,
                (Some(m), false) => Outcome::Failed(m.clone()),
                (None, false) => Outcome::Done,
            };
            let ids = active.run.design_ids.iter().filter(|id| !id.is_empty()).cloned().collect();
            finished(s, Finished { run_id: run_id.to_string(), design_ids: ids, outcome });
            (changes::finish_run(s.snap(), &active.run, failed.as_deref()), true)
        }
        VariationEvent::Job { .. } => (Vec::new(), false),
    };
    let changed = apply(s, changes, &format!("runner: {run_id}"));
    if !done {
        s.runner.runs.insert(run_id.to_string(), active);
    }
    changed
}

fn on_edit_event(s: &mut AppState, job_id: &str, event: VariationEvent) -> bool {
    let Some(edit) = s.runner.edits.get(job_id) else { return false };
    let (design_id, instruction) = (edit.design_id.clone(), edit.instruction.clone());
    let change = match event {
        VariationEvent::Variation { variation, .. } => changes::edit_finish(s.snap(), &design_id, job_id, &variation, &instruction),
        VariationEvent::Error { message, .. } => {
            let message = (message != "Cancelled").then_some(message.as_str());
            changes::edit_fail(s.snap(), &design_id, job_id, message)
        }
        VariationEvent::Done => {
            s.runner.edits.remove(job_id);
            None
        }
        _ => None,
    };
    let reason = format!("ai edit {job_id}");
    apply(s, change.into_iter().collect(), &reason)
}

/// Stop a generation (its cards still generating end as "Cancelled").
pub fn cancel_run(s: &mut AppState, run_id: &str) {
    let Some(active) = s.runner.runs.get(run_id) else { return };
    let job = active.job_id.clone();
    s.runner.cancelled.insert(run_id.to_string());
    if let Some(h) = &s.runner.host {
        h.ai().jobs().cancel(&job);
    }
}

/// Stop the edit running on `design_id` and clear its busy flag (also a stale one).
pub fn cancel_edit(s: &mut AppState, design_id: &str) -> bool {
    let Some(d) = s.snap().design(design_id) else { return false };
    let Some(job) = d.editing.as_ref().and_then(|e| e.get("jobId")).and_then(|j| j.as_str()).map(str::to_string) else {
        return false;
    };
    if let Some(h) = &s.runner.host {
        h.ai().jobs().cancel(&job);
    }
    let change = changes::edit_fail(s.snap(), design_id, &job, None);
    apply(s, change.into_iter().collect(), &format!("ai edit cancelled {job}"))
}
