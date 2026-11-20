//! Agent capacity and tasks (`GET /api/agent-tasks`): the header pill, its panel, and later the inspector's
//! "Beim Agenten" section and the Implement button (docs/spec-features.md §5).

pub mod data;
pub mod implement;
pub mod poller;
pub mod view;

use std::collections::HashMap;

use rune::{Latest, TextEdit, Tx, Waker};

use poller::Poller;

#[derive(Default)]
pub struct TasksState {
    pub poller: Poller,
    pub panel_open: bool,
    /// The expanded task row in the panel.
    pub expanded: Option<String>,
    /// Expanded same-job groups (by job id).
    pub expanded_jobs: std::collections::HashSet<String>,
    /// Half-written notes to the agent, per note id (notes service).
    pub notes: HashMap<String, TextEdit>,
    /// Why the last note to a task wasn't sent, per note id.
    pub note_errors: HashMap<String, String>,
    note_req: Latest<(String, Result<(), String>)>,
    /// The "Befehle kopieren" button shows "Kopiert" until this time.
    pub copied_until: f64,
    /// The binary shown in the connect commands; None = this process's own (scenes fix it for stable snapshots).
    pub exe: Option<String>,
    /// Whether the button currently shows it (so the end of it redraws once).
    pub copied: bool,
}

impl TasksState {
    pub fn setup(&mut self, waker: Waker) {
        self.note_req.set_waker(waker.clone());
        self.poller.setup(waker);
    }

    pub fn toggle_panel(&mut self, tx: &mut Tx) {
        self.panel_open = !self.panel_open;
        if self.panel_open {
            self.poller.reload(tx);
        }
    }

    /// Open the panel on one task, expanded ("Im Aufgabenpanel öffnen").
    pub fn open_on(&mut self, task_id: &str, tx: &mut Tx) {
        self.panel_open = true;
        self.expanded = Some(task_id.to_string());
        self.poller.reload(tx);
    }

    pub fn toggle_task(&mut self, id: &str) {
        self.expanded = if self.expanded.as_deref() == Some(id) { None } else { Some(id.to_string()) };
    }

    /// "Notiz an den Agenten": append the text to the task's thread in the notes service.
    pub fn send_note(&mut self, note_id: &str) {
        let text = self.notes.get(note_id).map(|e| e.text.trim().to_string()).unwrap_or_default();
        if text.is_empty() {
            self.note_errors.insert(note_id.into(), "Bitte einen Text eingeben.".into());
            return;
        }
        self.note_errors.remove(note_id);
        let body = serde_json::json!({ "text": text }).to_string();
        let path = format!("/api/agent-tasks/{}/notes", crate::features::nested::project::url_escape(note_id));
        self.note_req.request((crate::features::http::server_addr(), path, body, note_id.to_string()), |(a, path, body, id)| {
            (id, crate::features::http::request(&a, "POST", &path, Some(&body)).map(|_| ()))
        });
    }

    /// Returns (visible change, a note was sent).
    pub fn poll(&mut self, now: f64, tx: &mut Tx) -> (bool, bool) {
        let mut changed = self.poller.poll(now, self.panel_open, tx);
        let copied = self.copied_until > now;
        if copied != self.copied {
            self.copied = copied;
            changed = true;
        }
        let mut sent = false;
        if let Some((id, r)) = self.note_req.take() {
            changed = true;
            match r {
                Ok(()) => {
                    self.notes.remove(&id);
                    self.poller.reload(tx);
                    sent = true;
                }
                Err(e) => {
                    self.note_errors.insert(id, if e.is_empty() { "Notiz nicht gesendet".into() } else { e });
                }
            }
        }
        (changed, sent)
    }
}

/// The connect block as it shows with no agent (spec-public/05 AN-CLI-405): panel open, Rex counts 0 agents, no
/// tasks, a fixed binary path for a stable snapshot; nothing is fetched while the snapshot is taken.
fn connect_scene(s: &mut crate::app::state::AppState, copied: bool) {
    s.use_store(crate::features::scenes::fixture());
    s.enter_space("zzfx", None);
    let t = &mut s.features.tasks;
    t.panel_open = true;
    t.exe = Some("/Applications/Prototypie.app/Contents/MacOS/prototypie".into());
    t.poller.data = Some(data::AgentTasksData {
        capacity: data::Capacity { working: Some(0), total: Some(0), agents: Vec::new() },
        tasks: Vec::new(),
        notes_service: true,
        notes_watchers: Some(Vec::new()),
    });
    t.poller.phase = poller::Phase::Waiting { due: f64::MAX };
    t.copied = copied;
    t.copied_until = if copied { f64::MAX } else { 0.0 };
}

/// `--snap` scenes of the agent panel.
pub const SCENES: &[(&str, fn(&mut crate::app::state::AppState))] = &[
    ("agents-connect", |s| connect_scene(s, false)),
    ("agents-connect-copied", |s| connect_scene(s, true)),
];
