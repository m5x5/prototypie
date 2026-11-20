//! `GET /api/agent-tasks` as proto-server answers it (docs/api-server.md §7). Fetched on a `rune::Latest`
//! worker through `features::http`, never on the UI thread.

use serde::Deserialize;

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Capacity {
    /// None when Rex isn't reachable: the pill shows no count instead of guessing.
    pub working: Option<u32>,
    pub total: Option<u32>,
    #[serde(default)]
    pub agents: Vec<Agent>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Agent {
    pub name: String,
    pub status: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Waiting,
    Working,
    Delivered,
    Resolved,
    Cancelled,
}

impl TaskStatus {
    /// Group order in the panel.
    pub const ORDER: [TaskStatus; 5] =
        [TaskStatus::Working, TaskStatus::Waiting, TaskStatus::Delivered, TaskStatus::Resolved, TaskStatus::Cancelled];

    pub fn label(self) -> &'static str {
        match self {
            TaskStatus::Working => "In Arbeit",
            TaskStatus::Waiting => "Wartet",
            TaskStatus::Delivered => "Geliefert",
            TaskStatus::Resolved => "Abgeschlossen",
            TaskStatus::Cancelled => "Abgebrochen",
        }
    }

    /// How many rows the group shows (the focused task always shows).
    pub fn shown(self) -> usize {
        match self {
            TaskStatus::Resolved => 8,
            TaskStatus::Cancelled => 3,
            _ => usize::MAX,
        }
    }

    pub fn is_open(self) -> bool {
        matches!(self, TaskStatus::Waiting | TaskStatus::Working)
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Step {
    pub key: String,
    pub label: String,
    pub at: Option<String>,
    pub done: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ThreadMessage {
    #[serde(default)]
    pub id: String,
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub at: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub space_id: String,
    #[serde(default)]
    pub space_name: String,
    #[serde(default)]
    pub space_path: Vec<String>,
    pub design_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTask {
    pub id: String,
    /// queue | note | implement
    pub kind: String,
    pub design_id: Option<String>,
    pub summary: Option<String>,
    pub title: String,
    #[serde(default)]
    pub request: String,
    pub job_id: Option<String>,
    pub index: Option<u32>,
    pub status: TaskStatus,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub steps: Vec<Step>,
    pub target: Option<Target>,
    pub note_id: Option<String>,
    #[serde(default)]
    pub thread: Vec<ThreadMessage>,
    /// Who works on it, when the server knows (an agent/session name).
    #[serde(default)]
    pub agent: Option<String>,
}

impl AgentTask {
    /// The worker on this item: the server's `agent`, else the first `proto-worker-N` (or `proto-lead`) an agent
    /// reply names (workers sign their replies in the notes service that way).
    pub fn worker(&self) -> Option<String> {
        if let Some(a) = self.agent.as_ref().filter(|a| !a.trim().is_empty()) {
            return Some(a.trim().to_string());
        }
        self.thread.iter().filter(|m| m.role == "agent").find_map(|m| {
            let at = m.content.find("proto-worker-").or_else(|| m.content.find("proto-lead"))?;
            let name: String = m.content[at..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
            Some(name.trim_end_matches('-').to_string())
        })
    }

    /// `proto-worker-8` → `W8`, `proto-lead` → `L`; a session name `prototypie-af [884479]` → `af` (its suffix, the
    /// part that tells sessions apart); anything else, its first two letters.
    pub fn worker_short(&self) -> Option<String> {
        let w = self.worker()?;
        let w = w.split(" [").next().unwrap_or(&w).trim().to_string();
        Some(if let Some(n) = w.strip_prefix("proto-worker-") {
            format!("W{n}")
        } else if w == "proto-lead" {
            "L".into()
        } else if let Some((_, tail)) = w.rsplit_once('-').filter(|(_, t)| (1..=4).contains(&t.len())) {
            tail.to_string()
        } else {
            w.chars().take(2).collect::<String>().to_uppercase()
        })
    }
}

/// Tasks of one queue job (same `jobId`), by direction number.
#[derive(Debug)]
pub struct JobGroup<'a> {
    pub job_id: String,
    pub items: Vec<&'a AgentTask>,
}

impl JobGroup<'_> {
    /// The group's place in the panel: the first of `TaskStatus::ORDER` any item has (working wins).
    pub fn status(&self) -> TaskStatus {
        TaskStatus::ORDER.into_iter().find(|st| self.items.iter().any(|t| t.status == *st)).unwrap_or(TaskStatus::Waiting)
    }
    pub fn count(&self, st: TaskStatus) -> usize {
        self.items.iter().filter(|t| t.status == st).count()
    }
    /// "3 von 5 geliefert · 2 in Arbeit · 1 wartet".
    pub fn summary(&self) -> String {
        let n = self.items.len();
        let done = self.count(TaskStatus::Delivered) + self.count(TaskStatus::Resolved);
        let mut parts = vec![format!("{done} von {n} geliefert")];
        for (st, word) in [(TaskStatus::Working, "in Arbeit"), (TaskStatus::Waiting, "wartet"), (TaskStatus::Cancelled, "abgebrochen")] {
            let c = self.count(st);
            if c > 0 {
                parts.push(format!("{c} {word}"));
            }
        }
        parts.join(" · ")
    }
}

/// Queue tasks sharing a job, two or more items each (single-item jobs stay plain rows), in the list's order.
pub fn job_groups(tasks: &[AgentTask]) -> Vec<JobGroup<'_>> {
    let mut out: Vec<JobGroup> = Vec::new();
    for t in tasks {
        let Some(job) = &t.job_id else { continue };
        match out.iter_mut().find(|g| &g.job_id == job) {
            Some(g) => g.items.push(t),
            None => out.push(JobGroup { job_id: job.clone(), items: vec![t] }),
        }
    }
    out.retain(|g| g.items.len() > 1);
    for g in &mut out {
        g.items.sort_by_key(|t| t.index.unwrap_or(u32::MAX));
    }
    out
}

#[cfg(test)]
mod group_tests {
    use super::*;

    fn task(id: &str, job: Option<&str>, index: u32, status: &str, reply: Option<&str>) -> AgentTask {
        let thread = reply.map(|r| serde_json::json!([{ "id": "m", "role": "agent", "content": r, "at": "2026-11-19T08:00:00Z" }])).unwrap_or(serde_json::json!([]));
        serde_json::from_value(serde_json::json!({
            "id": id, "kind": "queue", "title": "Onboarding", "jobId": job, "index": index, "status": status,
            "createdAt": "2026-11-19T08:00:00Z", "updatedAt": "2026-11-19T08:00:00Z", "steps": [], "thread": thread,
        })).unwrap()
    }

    #[test]
    fn same_job_items_group_with_a_summary_and_workers() {
        let tasks = vec![
            task("a", Some("J"), 2, "delivered", Some("proto-worker-6: done J#2")),
            task("b", Some("J"), 0, "working", Some("proto-worker-1 übernimmt")),
            task("c", Some("J"), 1, "waiting", None),
            task("d", Some("K"), 0, "working", None),
            task("e", None, 0, "working", None),
        ];
        let groups = job_groups(&tasks);
        assert_eq!(groups.len(), 1, "single-item jobs stay plain");
        let g = &groups[0];
        assert_eq!(g.items.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["b", "c", "a"]);
        assert_eq!(g.status(), TaskStatus::Working);
        assert_eq!(g.summary(), "1 von 3 geliefert · 1 in Arbeit · 1 wartet");
        assert_eq!(g.items[0].worker_short().as_deref(), Some("W1"));
        assert_eq!(g.items[2].worker().as_deref(), Some("proto-worker-6"));
        assert_eq!(g.items[1].worker(), None);
    }

    #[test]
    fn the_servers_claim_wins_over_reply_names() {
        let mut t: AgentTask = serde_json::from_value(serde_json::json!({
            "id": "a", "kind": "queue", "title": "x", "jobId": "J", "index": 0, "status": "working",
            "createdAt": "2026-11-19T08:00:00Z", "updatedAt": "2026-11-19T08:00:00Z", "steps": [],
            "agent": "proto-worker-2", "claimedAt": "2026-11-19T08:01:00Z",
            "thread": [{ "id": "m", "role": "agent", "content": "proto-worker-6 here", "at": "2026-11-19T08:00:00Z" }],
        })).unwrap();
        assert_eq!(t.worker_short().as_deref(), Some("W2"));
        t.agent = Some("prototypie-af [884479]".into());
        assert_eq!(t.worker_short().as_deref(), Some("af"), "session names shorten to their suffix");
        t.agent = Some("codex".into());
        assert_eq!(t.worker_short().as_deref(), Some("CO"));
        t.agent = None;
        assert_eq!(t.worker_short().as_deref(), Some("W6"));
    }
}

impl AgentTask {
    /// "<jobId>#<index>", "Implement" or "Notiz".
    pub fn reference(&self) -> String {
        match (&self.job_id, self.kind.as_str()) {
            (Some(job), _) => format!("{job}#{}", self.index.unwrap_or(0)),
            (None, "implement") => "Implement".into(),
            _ => "Notiz".into(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct AgentTasksData {
    #[serde(default)]
    pub capacity: Capacity,
    #[serde(default)]
    pub tasks: Vec<AgentTask>,
    /// Whether the server could read the notes service.
    #[serde(default, rename = "notesService")]
    pub notes_service: bool,
    /// Who waited for notes in the last 600 s (`prototypie notes watch`); None when the server didn't say.
    #[serde(default, rename = "notesWatchers")]
    pub notes_watchers: Option<Vec<NotesWatcher>>,
}

/// An agent that waited for notes recently (spec-public/05 AN-CLI-404).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesWatcher {
    pub name: String,
    #[serde(default)]
    pub waiting: bool,
    #[serde(default)]
    pub last_seen_at: String,
}

impl AgentTasksData {
    /// `(working, total)` when Rex answered.
    pub fn known(&self) -> Option<(u32, u32)> {
        Some((self.capacity.working?, self.capacity.total?))
    }

    /// Show "Noch kein Agent verbunden": Rex counts no agent, or (without Rex) the notes service is readable and
    /// no agent waited for notes in the last 600 s (AN-CLI-404).
    pub fn no_agent(&self) -> bool {
        match self.known() {
            Some((_, total)) => total == 0,
            None => self.notes_service && self.notes_watchers.as_ref().is_some_and(Vec::is_empty),
        }
    }

    /// Waiting + working tasks.
    pub fn open_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.status.is_open()).count()
    }
}

/// Fetch and parse the task list (blocking; call on a worker). `PROTOTYPIE_TASKS_FIXTURE=<file.json>` reads
/// a saved answer instead (headless snapshots without a server).
pub fn fetch(addr: &str) -> Result<AgentTasksData, String> {
    let body = match std::env::var_os("PROTOTYPIE_TASKS_FIXTURE") {
        Some(path) => std::fs::read(path).map_err(|e| e.to_string())?,
        None => crate::features::http::request(addr, "GET", "/api/agent-tasks", None)?,
    };
    serde_json::from_slice(&body).map_err(|e| format!("bad JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::features::http::parse_response;

    #[test]
    fn parses_plain_and_chunked_responses() {
        let plain = b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}";
        assert_eq!(parse_response(plain).unwrap(), b"{}");
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{\"\r\n1\r\n}\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked).unwrap(), b"{\"}");
        let err = b"HTTP/1.1 404 Not Found\r\n\r\n{\"error\":\"nope\"}";
        assert_eq!(parse_response(err).unwrap_err(), "nope");
    }

    #[test]
    fn parses_the_reference_payload() {
        let json = r#"{"capacity":{"working":2,"total":8,"agents":[{"name":"proto-worker-1","status":"working"}]},
            "tasks":[{"id":"n1","kind":"queue","title":"Header","request":"r","jobId":"EN8","index":0,"status":"working",
            "createdAt":"2026-11-18T10:00:00.000Z","updatedAt":"2026-11-18T10:05:00.000Z","steps":[{"key":"received","label":"Eingang","at":null,"done":true}],
            "thread":[]}],"notesService":true}"#;
        let d: AgentTasksData = serde_json::from_str(json).unwrap();
        assert_eq!(d.known(), Some((2, 8)));
        assert_eq!(d.open_count(), 1);
        assert_eq!(d.tasks[0].reference(), "EN8#0");
        assert!(d.notes_service);
        assert_eq!(d.notes_watchers, None);
        assert!(!d.no_agent());
    }

    #[test]
    fn no_agent_without_rex_comes_from_the_watchers() {
        // AN-CLI-662: Rex says nothing; no watcher → the connect block; one watcher → not.
        let none: AgentTasksData = serde_json::from_str(r#"{"capacity":{"working":null,"total":null,"agents":[]},"tasks":[],"notesService":true,"notesWatchers":[]}"#).unwrap();
        assert!(none.no_agent());
        let one: AgentTasksData = serde_json::from_str(r#"{"capacity":{"working":null,"total":null},"tasks":[],"notesService":true,
            "notesWatchers":[{"name":"proto-lead","waiting":true,"lastSeenAt":"2026-11-20T10:00:00.000Z"}]}"#).unwrap();
        assert!(!one.no_agent());
        assert_eq!(one.notes_watchers.as_ref().unwrap()[0].last_seen_at, "2026-11-20T10:00:00.000Z");
        let unreadable: AgentTasksData = serde_json::from_str(r#"{"capacity":{},"tasks":[],"notesService":false}"#).unwrap();
        assert!(!unreadable.no_agent());
        let rex_zero: AgentTasksData = serde_json::from_str(r#"{"capacity":{"working":0,"total":0},"tasks":[]}"#).unwrap();
        assert!(rex_zero.no_agent());
    }
}
