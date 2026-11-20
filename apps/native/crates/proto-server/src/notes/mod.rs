//! Prototypie's notes service: the store and routes under `/api/notes` (this crate serves them, kept in the
//! data dir), and `NotesService`, the typed handle the agent queue, `/api/implement`, project-info and the task
//! list use. The handle talks HTTP to the service at its `base`, so tests can point it at a test service or a
//! dead port. Spec: spec-public/04 AN-IF-3xx, spec-public/03 E, spec-public/05 AN-CLI-1xx.

pub(crate) mod model;
pub(crate) mod routes;
pub(crate) mod store;
pub(crate) mod watch;

#[cfg(test)]
mod tests;

use std::time::Duration;

use serde_json::{Value, json};

pub use store::NotesStore;

/// Lifecycle state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    /// Fresh, nobody on it yet.
    #[default]
    Pending,
    /// Taken by an agent.
    Acknowledged,
    /// Finished.
    Resolved,
    /// Thrown away.
    Dismissed,
}

impl Status {
    /// Lower-case word for `/api/agent-tasks` (`pending` … `dismissed`).
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Acknowledged => "acknowledged",
            Status::Resolved => "resolved",
            Status::Dismissed => "dismissed",
        }
    }

    /// The service's word back to the state; anything else is `None`.
    pub fn parse(s: &str) -> Option<Status> {
        match s {
            "pending" => Some(Status::Pending),
            "acknowledged" => Some(Status::Acknowledged),
            "resolved" => Some(Status::Resolved),
            "dismissed" => Some(Status::Dismissed),
            _ => None,
        }
    }
}

/// Thread entry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThreadMessage {
    /// Message id.
    pub id: String,
    /// `human` or `agent`.
    pub role: String,
    pub content: String,
    /// Unix ms.
    pub at_ms: i64,
}

/// Stored note. Only `id` is guaranteed; absent details are empty or `None`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Note {
    /// Service id.
    pub id: String,
    /// Text; server-made notes begin with a `[prototypie …]` tag.
    pub comment: String,
    /// Label of the subject.
    pub element: Option<String>,
    /// Path of the subject (element or file).
    pub element_path: Option<String>,
    /// Location string, e.g. `http://localhost:3124/` or `prototypie-rune://<space>/…`.
    pub url: Option<String>,
    pub status: Option<Status>,
    /// UTC `YYYY-MM-DDTHH:MM:SS.mmmZ`; callers sort it as text.
    pub created_at: Option<String>,
    /// Same format; `None` if never modified.
    pub updated_at: Option<String>,
    pub thread: Vec<ThreadMessage>,
}

/// Input for [`NotesService::create`].
pub struct NewNote<'a> {
    pub comment: &'a str,
    pub element: &'a str,
    pub element_path: &'a str,
}

/// Failure of [`NotesService::reply`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyError {
    /// Rejected; carries the code.
    Refused(u16),
    /// No usable answer.
    Unreachable,
}

/// A name that waited for notes recently (`GET /api/notes/info` → `watchers`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watcher {
    pub name: String,
    pub waiting: bool,
    pub last_seen_at: String,
}

const READ: Duration = Duration::from_millis(2500);
const CREATE: Duration = Duration::from_secs(3);
const CLOSE: Duration = Duration::from_secs(3);
const REPLY: Duration = Duration::from_secs(4);
const CONNECT: Duration = Duration::from_secs(2);

/// Cloneable handle; the server's own notes go under `app_url`. Shared across routes and the queue.
#[derive(Clone)]
pub struct NotesService {
    base: String,
    app_url: String,
    http: reqwest::Client,
}

/// Percent-encode one path segment (everything but unreserved characters), so any id arrives intact.
pub(crate) fn encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

/// A note from the service's JSON; `None` without an id. Wrong types count as absent (AN-SY-183).
pub(crate) fn note_from(v: &Value) -> Option<Note> {
    let id = v["id"].as_str().filter(|s| !s.is_empty())?.to_string();
    let thread = v["thread"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter(|m| m.is_object())
                .map(|m| ThreadMessage {
                    id: text(&m["id"]).unwrap_or_default(),
                    role: text(&m["role"]).unwrap_or_default(),
                    content: text(&m["content"]).unwrap_or_default(),
                    at_ms: m["createdAtMs"].as_i64().or_else(|| m["createdAt"].as_str().and_then(model::ms_of)).unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Note {
        id,
        comment: text(&v["comment"]).unwrap_or_default(),
        element: text(&v["element"]),
        element_path: text(&v["elementPath"]),
        url: text(&v["location"]),
        status: v["status"].as_str().and_then(Status::parse),
        created_at: text(&v["createdAt"]),
        updated_at: text(&v["updatedAt"]),
        thread,
    })
}

fn notes_from(v: &Value) -> Option<Vec<Note>> {
    Some(v["notes"].as_array()?.iter().filter_map(note_from).collect())
}

impl NotesService {
    /// No I/O. `base` defaults to [`crate::DEFAULT_NOTES_URL`], `app_url` to [`crate::DEFAULT_APP_URL`].
    pub fn new(base: impl Into<String>, app_url: impl Into<String>) -> Self {
        let base = base.into().trim().trim_end_matches('/').to_string();
        // Loopback only, never through a proxy; building the client does no I/O.
        let http = reqwest::Client::builder().no_proxy().connect_timeout(CONNECT).build().unwrap_or_default();
        NotesService { base, app_url: app_url.into(), http }
    }

    /// Configured service address (without a trailing `/`).
    pub fn base(&self) -> &str {
        &self.base
    }

    /// Location for server-made notes.
    pub fn app_url(&self) -> &str {
        &self.app_url
    }

    /// `(status, body)`; `None` on transport errors or a body that isn't JSON.
    async fn call(&self, method: reqwest::Method, path: &str, query: &[(&str, &str)], body: Option<Value>, limit: Duration) -> Option<(u16, Value)> {
        let mut req = self.http.request(method, format!("{}{path}", self.base)).timeout(limit);
        if !query.is_empty() {
            req = req.query(query);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let res = req.send().await.ok()?;
        let status = res.status().as_u16();
        let bytes = res.bytes().await.ok()?;
        Some((status, serde_json::from_slice(&bytes).ok()?))
    }

    async fn get_ok(&self, path: &str, query: &[(&str, &str)]) -> Option<Value> {
        match self.call(reqwest::Method::GET, path, query, None, READ).await? {
            (200, v) => Some(v),
            _ => None,
        }
    }

    /// Probe: does the notes service answer (`GET /api/notes/info` within 2.5 s)?
    pub async fn reachable(&self) -> bool {
        self.get_ok("/api/notes/info", &[]).await.is_some_and(|v| v["service"] == "prototypie-notes")
    }

    /// Who waited for notes in the last 600 s. `None` = unreadable.
    pub async fn watchers(&self) -> Option<Vec<Watcher>> {
        let v = self.get_ok("/api/notes/info", &[]).await?;
        if v["service"] != "prototypie-notes" {
            return None;
        }
        Some(
            v["watchers"]
                .as_array()?
                .iter()
                .filter_map(|w| {
                    Some(Watcher {
                        name: text(&w["name"])?,
                        waiting: w["waiting"].as_bool().unwrap_or(false),
                        last_seen_at: text(&w["lastSeenAt"]).unwrap_or_default(),
                    })
                })
                .collect(),
        )
    }

    /// Pending notes, all locations. `None` = unreadable.
    pub async fn open_notes(&self) -> Option<Vec<Note>> {
        notes_from(&self.get_ok("/api/notes", &[("limit", "5000")]).await?)
    }

    /// Locations that have notes. `None` = unreadable.
    pub async fn locations(&self) -> Option<Vec<String>> {
        let v = self.get_ok("/api/notes/collections", &[]).await?;
        let mut out: Vec<String> = Vec::new();
        for c in v["collections"].as_array()? {
            if c["noteCount"].as_u64().unwrap_or(0) > 0
                && let Some(l) = c["location"].as_str()
                && !out.iter().any(|x| x == l)
            {
                out.push(l.to_string());
            }
        }
        Some(out)
    }

    /// Notes of one location, every state. `None` = unreadable.
    pub async fn notes_at(&self, location: &str) -> Option<Vec<Note>> {
        notes_from(&self.get_ok("/api/notes", &[("status", "all"), ("location", location), ("limit", "5000")]).await?)
    }

    /// Lookup by id. `None` = unknown or unreadable.
    pub async fn note(&self, id: &str) -> Option<Note> {
        note_from(&self.get_ok(&format!("/api/notes/{}", encode_segment(id)), &[]).await?["note"])
    }

    /// Adds a note at `app_url` (one request: the service finds or makes the collection); its id, `None` on failure.
    pub async fn create(&self, note: NewNote<'_>) -> Option<String> {
        let body = json!({
            "location": self.app_url,
            "comment": note.comment,
            "element": note.element,
            "elementPath": note.element_path,
            "kind": "feedback",
            "clientTimestamp": crate::util::now_ms(),
            "target": { "position": { "x": 0, "y": 0 } },
        });
        match self.call(reqwest::Method::POST, "/api/notes", &[], Some(body), CREATE).await? {
            (200 | 201, v) => v["note"]["id"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
            _ => None,
        }
    }

    /// Sets resolved/dismissed on the agent's behalf. Fire and forget, short timeout.
    pub async fn close(&self, id: &str, status: proto_ai::NoteStatus) {
        let status = match status {
            proto_ai::NoteStatus::Resolved => "resolved",
            proto_ai::NoteStatus::Dismissed => "dismissed",
        };
        let path = format!("/api/notes/{}/status", encode_segment(id));
        let _ = self.call(reqwest::Method::POST, &path, &[], Some(json!({ "status": status, "by": "agent" })), CLOSE).await;
    }

    /// Adds a thread entry; returns the note afterwards.
    pub async fn reply(&self, id: &str, role: &str, content: &str) -> Result<Note, ReplyError> {
        let path = format!("/api/notes/{}/thread", encode_segment(id));
        match self.call(reqwest::Method::POST, &path, &[], Some(json!({ "role": role, "content": content })), REPLY).await {
            Some((200..=299, v)) => note_from(&v["note"]).ok_or(ReplyError::Unreachable),
            Some((code, _)) if code >= 400 => Err(ReplyError::Refused(code)),
            _ => Err(ReplyError::Unreachable),
        }
    }
}

/// Queue hook: `announce` maps to [`NotesService::create`], `set_status` to [`NotesService::close`].
impl proto_ai::Notifier for NotesService {
    fn announce(&self, comment: String, element: String, element_path: String) -> futures_util::future::BoxFuture<'static, Option<String>> {
        let ns = self.clone();
        Box::pin(async move { ns.create(NewNote { comment: &comment, element: &element, element_path: &element_path }).await })
    }

    fn set_status(&self, note_id: String, status: proto_ai::NoteStatus) -> futures_util::future::BoxFuture<'static, ()> {
        let ns = self.clone();
        Box::pin(async move { ns.close(&note_id, status).await })
    }
}
