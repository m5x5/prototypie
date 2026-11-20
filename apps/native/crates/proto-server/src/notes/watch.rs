//! Long-poll for new notes: a signal per new wake-up number, the watcher register, the end at shutdown.
//! Spec: spec-public/05 AN-CLI-140…142.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::model::{self, ApiError};
use super::store::{Filter, NotesStore};
use crate::util;

/// Names that waited recently (only in memory).
#[derive(Default)]
pub struct Registry {
    names: Mutex<HashMap<String, Seen>>,
    active: AtomicUsize,
}

struct Seen {
    at: Instant,
    at_text: String,
    waiting: usize,
}

/// How long a watcher stays listed after its last wait.
const LISTED_FOR: Duration = Duration::from_secs(600);

impl Registry {
    /// `[{name, waiting, lastSeenAt}]` of the last 600 s, newest first.
    pub fn list(&self) -> Vec<Value> {
        let names = self.names.lock().unwrap_or_else(|e| e.into_inner());
        let mut list: Vec<(&String, &Seen)> = names.iter().filter(|(_, s)| s.waiting > 0 || s.at.elapsed() < LISTED_FOR).collect();
        list.sort_by(|a, b| b.1.at.cmp(&a.1.at));
        list.into_iter().map(|(n, s)| json!({ "name": n, "waiting": s.waiting > 0, "lastSeenAt": s.at_text })).collect()
    }

    /// A slot for one waiting call, or `None` when 32 already wait.
    fn enter(&self, name: &str) -> Option<Slot<'_>> {
        if self.active.fetch_add(1, Ordering::SeqCst) >= model::MAX_WATCHERS {
            self.active.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        let mut names = self.names.lock().unwrap_or_else(|e| e.into_inner());
        let e = names.entry(name.to_string()).or_insert_with(|| Seen { at: Instant::now(), at_text: String::new(), waiting: 0 });
        e.at = Instant::now();
        e.at_text = util::iso_now();
        e.waiting += 1;
        Some(Slot { reg: self, name: name.to_string() })
    }
}

/// Held while a call waits; dropping it (answer sent or caller gone) frees the slot.
struct Slot<'a> {
    reg: &'a Registry,
    name: String,
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.reg.active.fetch_sub(1, Ordering::SeqCst);
        let mut names = self.reg.names.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = names.get_mut(&self.name) {
            e.waiting = e.waiting.saturating_sub(1);
            e.at = Instant::now();
            e.at_text = util::iso_now();
        }
    }
}

/// Parameters of one `GET /api/notes/watch`.
#[derive(Clone, Debug)]
pub struct WatchParams {
    pub after: u64,
    pub timeout: Duration,
    pub batch: Duration,
    pub filter: Filter,
    pub by: String,
    pub replies: bool,
}

/// At most this many notes per answer.
pub const MAX_BATCH: usize = 200;

fn answer(store: &NotesStore, p: &WatchParams, timed_out: bool) -> Value {
    let (notes, replies, cursor) = store.since(p.after, &p.filter, p.replies, MAX_BATCH);
    let mut v = json!({ "notes": notes, "count": notes.len(), "cursor": cursor, "timedOut": timed_out && notes.is_empty() });
    if p.replies {
        v["replies"] = json!(replies);
    }
    v
}

/// Wait for matching notes (AN-CLI-141). Dropping the future (caller gone) ends it without side effects.
pub async fn watch(store: &NotesStore, p: WatchParams) -> Result<Value, ApiError> {
    let shared = &store.0;
    if shared.shutdown.is_cancelled() {
        return Err(ApiError::shutting_down());
    }
    let Some(_slot) = shared.watchers.enter(&p.by) else {
        return Err(ApiError::new(429, "too_many_watchers", format!("Too many agents are waiting at once ({})", model::MAX_WATCHERS)));
    };
    // Subscribe before looking, so a note made in between still wakes us.
    let mut rx = shared.signal.subscribe();
    if store.has_since(p.after, &p.filter, p.replies) {
        return Ok(answer(store, &p, false));
    }
    let deadline = tokio::time::Instant::now() + p.timeout;
    loop {
        tokio::select! {
            _ = shared.shutdown.cancelled() => return Err(ApiError::shutting_down()),
            _ = tokio::time::sleep_until(deadline) => return Ok(answer(store, &p, true)),
            changed = rx.changed() => {
                if changed.is_err() {
                    return Err(ApiError::shutting_down());
                }
                if store.has_since(p.after, &p.filter, p.replies) {
                    break;
                }
            }
        }
    }
    // The batch window: notes arriving close together come as one answer.
    tokio::select! {
        _ = shared.shutdown.cancelled() => Err(ApiError::shutting_down()),
        _ = tokio::time::sleep(p.batch) => Ok(answer(store, &p, false)),
    }
}
