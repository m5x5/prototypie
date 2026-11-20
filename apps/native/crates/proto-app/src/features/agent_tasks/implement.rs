//! "Implement" on a promoted design (the reference's `implement-button.tsx`): hand the design to the coding
//! agent (`POST /api/implement`), then show the real state of that request from the agent tasks: Gesendet →
//! In Arbeit → Erledigt. While a request is open or done, the button opens the task panel instead.

use std::collections::HashMap;

use proto_core::{Change, Design, RecordKind};
use rune::{Latest, Tx, Waker};
use serde_json::json;

use super::data::{AgentTask, TaskStatus};
use crate::app::state::AppState;
use crate::features::{http, nav, notify};

/// The Implement button's phase (worker-5 maps it onto its card button).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImplementPhase {
    Idle,
    Sent,
    Working,
    Done,
}

#[derive(Default)]
pub struct ImplementState {
    /// Requests on their way, by design id.
    pub sending: HashMap<String, ()>,
    req: Latest<(String, Result<bool, String>)>,
}

impl ImplementState {
    pub fn setup(&mut self, waker: Waker) {
        self.req.set_waker(waker);
    }
}

/// The newest implement task of the design or a linked copy; an open one wins over an older finished one.
pub fn task_for<'a>(s: &'a AppState, design_id: &str) -> Option<&'a AgentTask> {
    let data = s.features.tasks.poller.data.as_ref()?;
    let ids = super::view::linked_ids(s, design_id);
    let mut v: Vec<&AgentTask> = data
        .tasks
        .iter()
        .filter(|t| t.kind == "implement" && t.status != TaskStatus::Cancelled && t.design_id.as_ref().is_some_and(|d| ids.contains(d)))
        .collect();
    v.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    v.iter().find(|t| t.status.is_open()).or(v.first()).copied()
}

/// The button's phase (worker-5 draws it).
pub fn phase(s: &AppState, d: &Design) -> ImplementPhase {
    let p = match task_for(s, &d.id).map(|t| t.status) {
        None => ImplementPhase::Idle,
        Some(TaskStatus::Resolved | TaskStatus::Delivered) => ImplementPhase::Done,
        Some(TaskStatus::Working) => ImplementPhase::Working,
        Some(_) => ImplementPhase::Sent,
    };
    // Before the first poll (or without the notes service) a request this device made still counts as sent.
    let notes_service = s.features.tasks.poller.data.as_ref().is_some_and(|x| x.notes_service);
    if p == ImplementPhase::Idle && !notes_service && d.implement_requested_at.is_some() { ImplementPhase::Sent } else { p }
}

/// The card's press: send when idle, else open the task panel on the request.
pub fn press(s: &mut AppState, design_id: &str, tx: &mut Tx) {
    if let Some(t) = task_for(s, design_id).map(|t| t.id.clone()) {
        s.features.tasks.open_on(&t, tx);
        return;
    }
    send(s, design_id, false);
}

/// POST /api/implement (with `force` after "Erneut senden").
pub fn send(s: &mut AppState, design_id: &str, force: bool) {
    let Some(d) = s.snap().design(design_id).filter(|d| d.is_ready()) else { return };
    if s.features.implement.sending.contains_key(design_id) {
        return;
    }
    s.features.implement.sending.insert(design_id.to_string(), ());
    let body = json!({"designId": d.id, "title": d.title, "rationale": d.rationale, "html": d.html, "force": force}).to_string();
    s.features.implement.req.request((http::server_addr(), d.id.clone(), body), |(a, id, body)| {
        // 409 {done: true}: already sent or done elsewhere; just show that state (not an error).
        let r = match http::request(&a, "POST", "/api/implement", Some(&body)) {
            Ok(b) => Ok(serde_json::from_slice::<serde_json::Value>(&b).ok().and_then(|v| v["duplicate"].as_bool()).unwrap_or(false)),
            Err(e) if e.contains("already") => Ok(true),
            Err(e) => Err(e),
        };
        (id, r)
    });
}

pub fn poll(s: &mut AppState, tx: &mut Tx) -> bool {
    let Some((id, r)) = s.features.implement.req.take() else { return false };
    s.features.implement.sending.remove(&id);
    match r {
        Ok(duplicate) => {
            if !duplicate
                && let Some(mut d) = s.snap().design(&id).map(|d| Design::clone(&d))
            {
                d.implement_requested_at = Some(proto_core::ops::now_ms());
                if let Ok(c) = Change::put(RecordKind::Designs, &d.id, &d) {
                    nav::apply(s, vec![c], "implement");
                }
            }
            s.features.tasks.poller.reload(tx);
        }
        Err(e) => notify(s, "Implement failed", &e),
    }
    true
}
