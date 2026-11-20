//! Annotate: a toolbar to point at any part of the app (or of a shown design's page), write a note about it and
//! hand it to the coding agents through Prototypie's notes service. Spec 01 (native UI), 02 §N (the page),
//! 03 §B–D (sync), 04 (interface). The facade here keeps the surface the rest of the app uses; the parts live in
//! `logic` (pure rules), `place` (placement), `wire` (service format), `client` (background worker), `source`
//! (code locations), `frame` (post-draw facts), `page` (live design page and store mirror) and `view`.

pub mod client;
pub mod frame;
mod icons;
pub mod logic;
pub mod model;
pub mod page;
pub mod place;
pub mod source;
mod view;
pub mod wire;
#[cfg(test)]
mod tests;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use rune::{Cx, El, Key, TextEdit, Tx, Waker};
use serde_json::{Value, json};

use self::client::{Client, Job, Outcome};
use self::logic::{Analysis, Reach};
use self::model::{Message, Note, Status, Target};
use crate::app::prefs::ANNOTATE_KEY;
use crate::app::state::AppState;
use crate::app::style::Palette;

/// The pointer's element.
#[derive(Clone, Debug, PartialEq)]
pub struct Hover {
    /// Runtime path.
    pub path: String,
    /// Nearest key.
    pub key: Option<String>,
}

/// A pointer move's three picks, filled by their callbacks in turn (AN-UI-041).
#[derive(Clone, Debug, Default)]
pub(crate) struct Probe {
    top: Option<Option<rune::Picked>>,
    app: Option<Option<rune::Picked>>,
}

/// Talking to the notes service.
#[derive(Default)]
pub struct Sync {
    pub(crate) client: Option<Client>,
    /// Clock of the last tick; None = the first tick is due.
    pub(crate) last_tick: Option<f64>,
    /// A tick is due at the next poll (mode switched on, address saved).
    pub(crate) now: bool,
    /// Notes whose create is out and unanswered (never sent twice, AN-SY-055).
    pub(crate) in_flight: HashSet<String>,
    /// Notes the service took without an id: not before this clock (AN-SY-055).
    pub(crate) retry_at: HashMap<String, f64>,
    /// Replies written while the note's create was out (AN-SY-057).
    pub(crate) held_replies: HashMap<String, Vec<String>>,
    /// Tests: decide writing without the global flag.
    pub(crate) writes: Option<bool>,
}

/// UI-thread state, stored at `AppState::features.annotate`.
#[derive(Default)]
pub struct AnnotateState {
    /// Picking mode active.
    pub on: bool,
    /// Targets collected for the next note.
    pub selection: Vec<Target>,
    /// Editor text; `Some` while the editor shows.
    pub draft: Option<TextEdit>,
    /// Notes known in this run, any space.
    pub notes: Vec<Note>,
    /// Note whose thread is shown.
    pub open_note: Option<String>,
    /// List panel shown.
    pub list_open: bool,
    /// Settings panel shown.
    pub settings_open: bool,
    /// Clock seconds from the latest `poll`; agent_tasks reads it for its "Kopiert" timer.
    pub(crate) clock: f64,

    pub(crate) hover: Option<Hover>,
    pub(crate) probe: Probe,
    /// Bumped on every hover change; names the analysis request.
    pub(crate) analysis_seq: u64,
    /// The hovered path to analyse after the next drawn frame.
    pub(crate) analysis_request: Option<String>,
    pub(crate) analysis: Option<Analysis>,
    /// Drawn rects from the last frame, by key and by path.
    pub(crate) drawn_keys: HashMap<String, [f32; 4]>,
    pub(crate) drawn_paths: HashMap<String, [f32; 4]>,
    /// The hovered key's rect as last drawn by the view (fallback for a click's target rect).
    pub(crate) key_rect: Cell<Option<[f32; 4]>>,
    /// The thread's reply field.
    pub(crate) reply: TextEdit,
    /// The settings' address field, and whether it got its first value.
    pub(crate) endpoint_edit: TextEdit,
    pub(crate) filled: bool,
    pub(crate) reach: Reach,
    /// Clock of the last "reachable" report.
    pub(crate) last_ok: f64,
    /// While the toolbar is dragged: its wished spot, and the spot when the press began.
    pub(crate) drag: Option<[f32; 2]>,
    pub(crate) drag_origin: [f32; 2],
    /// Where the last built view showed the toolbar.
    pub(crate) toolbar_at: Cell<[f32; 2]>,
    /// Window size the last view had.
    pub(crate) window: Cell<[f32; 2]>,
    /// Theme the last view drew in (start: dark).
    pub(crate) light: Cell<bool>,
    /// "Copied" shows until this clock.
    pub(crate) copied_until: f64,
    /// Web notes the shown page couldn't place.
    pub(crate) missing_web: Vec<String>,
    pub(crate) wake: Option<Waker>,
    pub(crate) sync: Sync,
    pub(crate) page: page::Memo,
    /// The source index was seen ready (one rebuild when it lands).
    pub(crate) index_seen: bool,
    pub(crate) last_conn: Option<logic::Conn>,
}

impl AnnotateState {
    /// Whether this run may talk to the service.
    pub(crate) fn writes(&self) -> bool {
        self.sync.writes.unwrap_or_else(crate::features::http::writes_allowed)
    }

    /// Mode on or list open: the short tick.
    pub(crate) fn active(&self) -> bool {
        self.on || self.list_open
    }

    pub(crate) fn dark(&self) -> bool {
        !self.light.get()
    }

    /// The window size, 1280 × 800 until known.
    pub(crate) fn win(&self) -> [f32; 2] {
        let w = self.window.get();
        if w[0] > 0.0 && w[1] > 0.0 { w } else { [1280.0, 800.0] }
    }

    pub(crate) fn note(&self, id: &str) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }
}

// ---- prefs (AN-UI-190) ----

fn prefs_entry(s: &AppState) -> Option<&serde_json::Map<String, Value>> {
    s.prefs.features.get(ANNOTATE_KEY).and_then(Value::as_object)
}

/// The saved address, else the default.
pub fn endpoint(s: &AppState) -> String {
    prefs_entry(s).and_then(|o| o.get("endpoint")).and_then(Value::as_str).filter(|e| !e.is_empty())
        .map_or_else(|| logic::DEFAULT_ENDPOINT.to_string(), str::to_string)
}

/// The saved toolbar spot: a list whose first two entries are numbers.
pub fn saved_pos(s: &AppState) -> Option<[f32; 2]> {
    let a = prefs_entry(s)?.get("pos")?.as_array()?;
    Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32])
}

fn prefs_object(s: &mut AppState) -> &mut serde_json::Map<String, Value> {
    let e = s.prefs_mut().features.entry(ANNOTATE_KEY.to_string()).or_insert_with(|| json!({}));
    if !e.is_object() {
        *e = json!({});
    }
    e.as_object_mut().expect("object")
}

pub(crate) fn save_pos(s: &mut AppState, p: [f32; 2]) {
    prefs_object(s).insert("pos".into(), json!([p[0].round() as i64, p[1].round() as i64]));
}

// ---- facade (AN-IF-102…107) ----

/// Startup hook; stores the waker. No network here.
pub fn setup(s: &mut AppState, wake: Waker) {
    s.features.annotate.wake = Some(wake);
}

/// Flips the mode (AN-UI-020).
pub fn toggle(s: &mut AppState) {
    let a = &mut s.features.annotate;
    a.on = !a.on;
    a.hover = None;
    a.selection.clear();
    a.draft = None;
    a.open_note = None;
    a.analysis_request = None;
    a.analysis = None;
    if a.on {
        source::start(a.wake.clone());
        a.sync.now = true;
    }
}

/// First key handler of the features: ⌘⇧A and Esc (AN-UI-180/181). Returns true if it used the key.
pub fn on_key(s: &mut AppState, ev: &rune::KeyEvent) -> bool {
    let m = ev.mods;
    let letter_a = match &ev.key {
        Key::Command(c) => c.eq_ignore_ascii_case(&'a'),
        Key::Text(t) => m.cmd && t.eq_ignore_ascii_case("a"),
        _ => false,
    };
    if letter_a && m.cmd && m.shift && !m.alt {
        toggle(s);
        return true;
    }
    if ev.key != Key::Escape {
        return false;
    }
    let a = &mut s.features.annotate;
    if a.draft.is_some() || !a.selection.is_empty() {
        a.draft = None;
        a.selection.clear();
    } else if a.open_note.is_some() {
        a.open_note = None;
    } else if a.settings_open {
        a.settings_open = false;
    } else if a.list_open {
        a.list_open = false;
    } else if a.on {
        toggle(s);
    } else {
        return false;
    }
    true
}

/// Per-frame update (AN-UI-160): frame facts, worker results, the tick, the shown page. True when a rebuild is
/// needed.
pub fn poll(s: &mut AppState, clock: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    s.features.annotate.clock = clock;
    if !s.features.annotate.filled {
        let e = endpoint(s);
        let a = &mut s.features.annotate;
        a.endpoint_edit = TextEdit::new(e);
        a.filled = true;
    }
    changed |= take_frame(&mut s.features.annotate);
    changed |= apply_outcomes(s);
    tick(s, clock, tx);
    changed |= page::poll(s);
    let a = &mut s.features.annotate;
    if a.copied_until > 0.0 && clock >= a.copied_until {
        a.copied_until = 0.0;
        changed = true;
    }
    if !a.index_seen && source::ready().is_some() {
        a.index_seen = true;
        changed = true;
    }
    let c = logic::conn(a.reach, logic::waiting(&a.notes), clock, a.last_ok, a.active());
    if a.last_conn != Some(c) {
        changed |= a.last_conn.is_some();
        a.last_conn = Some(c);
    }
    changed
}

/// Post-draw hook; reads facts about drawn elements on request. Returns at once when nothing was requested.
pub fn on_frame_drawn(rt: &rune::Runtime<AppState>) {
    frame::on_frame_drawn(rt);
}

/// Overlay elements, appended last to the root (above `app`).
pub fn layer(s: &AppState, cx: &mut Cx<AppState>, shell: &Palette) -> Vec<El> {
    view::layer(s, cx, shell)
}

// ---- frame facts ----

fn take_frame(a: &mut AnnotateState) -> bool {
    let Some(f) = frame::take() else { return false };
    let mut changed = a.drawn_keys != f.keys || a.drawn_paths != f.paths;
    a.drawn_keys = f.keys;
    a.drawn_paths = f.paths;
    if let Some((seq, analysis)) = f.analysis {
        if seq == a.analysis_seq && a.analysis_request.is_some() {
            a.analysis_request = None;
            a.analysis = analysis;
            changed = true;
        }
    }
    changed
}

// ---- hover and picking (§4, §5) ----

/// Set the hover; a change asks for one analysis after the next drawn frame (AN-UI-043).
pub(crate) fn set_hover(a: &mut AnnotateState, path: Option<String>) {
    let next = path.map(|p| Hover { key: logic::nearest_key(&p), path: p });
    if next == a.hover {
        return;
    }
    a.hover = next;
    a.analysis = None;
    match &a.hover {
        Some(h) => {
            a.analysis_seq += 1;
            a.analysis_request = Some(h.path.clone());
        }
        None => a.analysis_request = None,
    }
}

/// Keys of this feature's interactive surfaces (toolbar, panels, popover, pins) start with this.
pub(crate) const UI_PREFIX: &str = "annotate-ui";

fn own_ui(p: &rune::Picked) -> bool {
    p.path().split('/').any(|seg| seg.starts_with(UI_PREFIX))
}

/// A pointer move over the capture surface: the topmost element in the window, then in `app` (and in the canvas
/// world under the minimap's pass-through layer) decide the hover.
pub(crate) fn pointer_moved(tx: &mut Tx, x: f32, y: f32) {
    tx.element_at(x, y, |s: &mut AppState, p, _| {
        s.features.annotate.probe = Probe { top: Some(p), app: None };
    });
    tx.element_at_in("app", x, y, |s: &mut AppState, p, _| {
        s.features.annotate.probe.app = Some(p);
    });
    tx.element_at_in("canvas-world", x, y, |s: &mut AppState, world, _| {
        let a = &mut s.features.annotate;
        let probe = std::mem::take(&mut a.probe);
        let path = resolve_pick(probe.top.flatten(), probe.app.flatten(), world);
        set_hover(a, path);
    });
}

/// AN-UI-041 steps 1–4.
pub(crate) fn resolve_pick(top: Option<rune::Picked>, app: Option<rune::Picked>, world: Option<rune::Picked>) -> Option<String> {
    if top.as_ref().is_some_and(own_ui) {
        return None;
    }
    let app = app?;
    if app.key() == Some("minimap-layer") {
        return world.map(|w| w.path().to_string());
    }
    Some(app.path().to_string())
}

fn focused_edit(text: &str) -> TextEdit {
    let mut e = TextEdit::new(text);
    e.anchor = text.len();
    e.focus = text.len();
    e.focused = true;
    e
}

/// A press on the capture surface (AN-UI-050).
pub(crate) fn click(s: &mut AppState, shift: bool) {
    let a = &mut s.features.annotate;
    let Some(h) = a.hover.clone() else { return };
    let src = h.key.as_deref().and_then(source::find);
    let t = logic::target_from_hover(&h.path, a.analysis.as_ref(), a.key_rect.get(), src);
    match a.selection.iter().position(|x| x.full_path == t.full_path) {
        Some(i) if shift => {
            a.selection.remove(i);
        }
        Some(_) => {
            a.open_note = None;
            open_draft(a);
        }
        None => {
            a.selection.push(t);
            a.open_note = None;
            if !shift {
                open_draft(a);
            }
        }
    }
}

fn open_draft(a: &mut AnnotateState) {
    match &mut a.draft {
        Some(d) => d.focused = true,
        None => a.draft = Some(focused_edit("")),
    }
}

/// Cancel or Esc in the editor (AN-UI-054).
pub(crate) fn cancel_draft(s: &mut AppState) {
    let a = &mut s.features.annotate;
    a.draft = None;
    a.selection.clear();
}

/// Save or Enter (AN-UI-055).
pub(crate) fn save_draft(s: &mut AppState) {
    let a = &s.features.annotate;
    let text = a.draft.as_ref().map_or("", |d| d.text.as_str()).trim().to_string();
    if text.is_empty() || a.selection.is_empty() {
        return;
    }
    let targets = s.features.annotate.selection.clone();
    let win = s.features.annotate.win();
    create_note(s, &text, targets, win);
    let a = &mut s.features.annotate;
    a.selection.clear();
    a.draft = None;
}

// ---- notes (§6, AN-SY-050) ----

/// A new note: mirrored (design notes), appended, sent at once.
pub(crate) fn create_note(s: &mut AppState, comment: &str, targets: Vec<Target>, window: [f32; 2]) {
    let now = proto_core::ops::now_ms();
    let n = Note {
        id: logic::local_id(now),
        server_id: None,
        space: s.route.space.clone(),
        comment: comment.trim().to_string(),
        targets,
        timestamp: now,
        status: Status::Queued,
        thread: Vec::new(),
        window,
    };
    if n.design_id().is_some() {
        page::mirror_new(s, &n);
    }
    let job = Job::Send { local: n.id.clone(), space: n.space.clone(), body: wire::note_body(&n) };
    let id = n.id.clone();
    s.features.annotate.notes.push(n);
    if submit(s, job) {
        s.features.annotate.sync.in_flight.insert(id);
    }
}

/// Resolve / Delete by the user (AN-UI-092, AN-SY-080).
pub(crate) fn set_status(s: &mut AppState, id: &str, status: Status) {
    let a = &mut s.features.annotate;
    let Some(n) = a.notes.iter_mut().find(|n| n.id == id) else { return };
    n.status = status;
    let (sid, design) = (n.server_id.clone(), n.design_id().map(str::to_string));
    if a.open_note.as_deref() == Some(id) {
        a.open_note = None;
    }
    if let Some(d) = design {
        page::mirror_resolved(s, &d, id);
    }
    if let Some(sid) = sid {
        submit(s, Job::Status { id: sid, status: status.word().into(), by: "human".into() });
    }
}

/// A reply from the thread (AN-UI-132, AN-SY-082).
pub(crate) fn reply(s: &mut AppState, id: &str, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let a = &mut s.features.annotate;
    let Some(n) = a.notes.iter_mut().find(|n| n.id == id) else { return };
    n.thread.push(Message { role: "human".into(), content: text.to_string() });
    match n.server_id.clone() {
        Some(sid) => {
            submit(s, Job::Reply { id: sid, text: text.to_string() });
        }
        None if a.sync.in_flight.contains(id) => a.sync.held_replies.entry(id.to_string()).or_default().push(text.to_string()),
        None => {}
    }
}

/// Enter in the reply field.
pub(crate) fn send_reply(s: &mut AppState) {
    let a = &mut s.features.annotate;
    let text = std::mem::take(&mut a.reply).text.clone();
    if let Some(id) = a.open_note.clone() {
        reply(s, &id, &text);
    }
}

/// Save in the settings (AN-UI-142).
pub(crate) fn save_endpoint(s: &mut AppState) {
    let t = s.features.annotate.endpoint_edit.text.trim().to_string();
    let v = if t.is_empty() || t == logic::DEFAULT_ENDPOINT { Value::Null } else { json!(t) };
    prefs_object(s).insert("endpoint".into(), v);
    let e = endpoint(s);
    let a = &mut s.features.annotate;
    if let Some(c) = &a.sync.client {
        c.submit(Job::Endpoint(e));
    }
    a.reach = Reach::Unknown;
    a.settings_open = false;
    a.sync.now = true;
}

/// The copy button (AN-UI-150).
pub(crate) fn copy_markdown(s: &mut AppState, tx: &mut Tx) {
    let md = logic::markdown(&s.features.annotate.notes, &s.route.space);
    tx.copy(md);
    let a = &mut s.features.annotate;
    a.copied_until = a.clock + 1.5;
    tx.wake_after(std::time::Duration::from_millis(1600));
}

// ---- the service (§15, spec 03 §B) ----

/// Hand a job to the worker, starting it on first need; nothing in runs that may not write. True when queued.
pub(crate) fn submit(s: &mut AppState, job: Job) -> bool {
    if !s.features.annotate.writes() {
        return false;
    }
    if s.features.annotate.sync.client.is_none() {
        let e = endpoint(s);
        let a = &mut s.features.annotate;
        a.sync.client = Client::start(&e, a.wake.clone());
    }
    match &s.features.annotate.sync.client {
        Some(c) => {
            c.submit(job);
            true
        }
        None => false,
    }
}

/// Worker results, in arrival order (AN-UI-160 step 2). True when something visible changed.
pub(crate) fn apply_outcomes(s: &mut AppState) -> bool {
    let outs = match &s.features.annotate.sync.client {
        Some(c) => c.take(),
        None => return false,
    };
    let mut changed = false;
    let mut follow = Vec::new();
    let a = &mut s.features.annotate;
    for o in outs {
        match o {
            Outcome::Reach(ok) => {
                let r = if ok { Reach::Yes } else { Reach::No };
                if ok {
                    a.last_ok = a.clock;
                }
                changed |= r != a.reach;
                a.reach = r;
            }
            Outcome::Sent { local, id } => {
                a.sync.in_flight.remove(&local);
                a.sync.retry_at.remove(&local);
                let held = a.sync.held_replies.remove(&local).unwrap_or_default();
                let Some(n) = a.notes.iter_mut().find(|n| n.id == local) else { continue };
                n.server_id = Some(id.clone());
                if n.status == Status::Queued {
                    n.status = Status::Pending;
                } else if !n.status.is_open() {
                    // Closed while its create was out: tell the service once (AN-SY-057).
                    follow.push(Job::Status { id: id.clone(), status: n.status.word().into(), by: "human".into() });
                }
                follow.extend(held.into_iter().map(|text| Job::Reply { id: id.clone(), text }));
                changed = true;
            }
            Outcome::SendFailed { local, error } => {
                a.sync.in_flight.remove(&local);
                if error == client::NO_NOTE_ID {
                    a.sync.retry_at.insert(local, a.clock + 30.0);
                }
            }
            Outcome::Content { space, notes } => {
                changed |= wire::merge(&mut a.notes, &space, &notes);
            }
        }
    }
    for j in follow {
        submit(s, j);
    }
    changed
}

/// The tick (AN-SY-061): resend what waits, refresh the current space, wake after one interval.
pub(crate) fn tick(s: &mut AppState, clock: f64, tx: &mut Tx) {
    if !s.features.annotate.writes() {
        return;
    }
    let a = &mut s.features.annotate;
    let iv = logic::interval(a.active());
    let due = a.sync.now || a.sync.last_tick.is_none_or(|t| clock - t >= iv);
    if !due {
        let left = iv - (clock - a.sync.last_tick.unwrap_or(clock));
        tx.wake_after(std::time::Duration::from_secs_f64(left.max(0.0)));
        return;
    }
    a.sync.now = false;
    a.sync.last_tick = Some(clock);
    let mut jobs: Vec<Job> = a.notes.iter()
        .filter(|n| n.server_id.is_none() && n.status == Status::Queued && !a.sync.in_flight.contains(&n.id))
        .filter(|n| a.sync.retry_at.get(&n.id).is_none_or(|t| clock >= *t))
        .map(|n| Job::Send { local: n.id.clone(), space: n.space.clone(), body: wire::note_body(n) })
        .collect();
    jobs.push(Job::Refresh { space: s.route.space.clone() });
    for j in jobs {
        if let Job::Send { local, .. } = &j {
            let local = local.clone();
            if submit(s, j) {
                s.features.annotate.sync.in_flight.insert(local);
            }
        } else {
            submit(s, j);
        }
    }
    tx.wake_after(std::time::Duration::from_secs_f64(iv));
}
