//! `NotesStore`: the notes in memory, one file per collection under `<data dir>/notes/`, changes one at a time.
//! Spec: spec-public/05 AN-CLI-120…133, -150…153, -505.
//!
//! Readers take the `RwLock` briefly and see only committed states. A change runs under the async `write`
//! lock: it copies the collection it touches, changes the copy, writes the file (atomically, on a blocking
//! thread) and only then swaps the copy in. A failed write leaves the committed state as it was.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::model::{
    self, ApiError, CollectionFile, CollectionMeta, NewNoteInput, StatusInput, StoredMessage, StoredNote, collection_json, note_json,
};
use super::watch::Registry;
use crate::util;

#[derive(Clone, Debug)]
pub(crate) struct Coll {
    pub meta: CollectionMeta,
    pub notes: Vec<StoredNote>,
}

#[derive(Default)]
pub(crate) struct Data {
    pub colls: HashMap<String, Coll>,
    /// Collection ids, oldest first (by `createdAt`, then load/creation order).
    pub order: Vec<String>,
    /// note id → collection id.
    pub note_at: HashMap<String, String>,
    /// Highest wake-up number handed out (0 = none).
    pub cursor: u64,
}

impl Data {
    fn find_note(&self, id: &str) -> Option<(&Coll, &StoredNote)> {
        let coll = self.colls.get(self.note_at.get(id)?)?;
        Some((coll, coll.notes.iter().find(|n| n.id == id)?))
    }

    fn first_active(&self, location: &str) -> Option<&Coll> {
        self.order.iter().filter_map(|id| self.colls.get(id)).find(|c| c.meta.state == "active" && c.meta.location == location)
    }

    fn fresh_collection_id(&self) -> String {
        loop {
            let id = model::new_id('c');
            if !self.colls.contains_key(&id) {
                return id;
            }
        }
    }

    fn fresh_note_id(&self, also: &HashSet<String>) -> String {
        loop {
            let id = model::new_id('n');
            if !self.note_at.contains_key(&id) && !also.contains(&id) {
                return id;
            }
        }
    }
}

/// Which notes a query or a watch wants (AN-CLI-130, -141).
#[derive(Clone, Debug, Default)]
pub struct Filter {
    /// Allowed states; empty = all.
    pub statuses: Vec<&'static str>,
    pub location: Option<String>,
    pub location_prefix: Option<String>,
    pub collection: Option<String>,
    /// `job`, `implement`, `project-info` or `none`.
    pub tag: Option<String>,
}

impl Filter {
    fn wants(&self, coll: &Coll, n: &StoredNote) -> bool {
        if !self.statuses.is_empty() && !self.statuses.contains(&n.status.as_str()) {
            return false;
        }
        if self.location.as_ref().is_some_and(|l| &n.location != l) {
            return false;
        }
        if self.location_prefix.as_ref().is_some_and(|p| !n.location.starts_with(p.as_str())) {
            return false;
        }
        if self.collection.as_ref().is_some_and(|c| &coll.meta.id != c) {
            return false;
        }
        if let Some(tag) = &self.tag {
            let kind = model::tag_of(&n.comment).and_then(|t| t["kind"].as_str().map(str::to_string));
            match (tag.as_str(), kind) {
                ("none", None) => {}
                ("none", Some(_)) | (_, None) => return false,
                (want, Some(k)) if want != k => return false,
                _ => {}
            }
        }
        true
    }
}

pub(crate) struct Shared {
    dir: PathBuf,
    data: RwLock<Data>,
    write: tokio::sync::Mutex<()>,
    /// The highest wake-up number, sent after each commit that handed one out.
    pub signal: tokio::sync::watch::Sender<u64>,
    pub watchers: Registry,
    pub shutdown: CancellationToken,
}

#[cfg(test)]
impl Shared {
    /// Direct access for tests that need large fills (limits).
    pub fn data_for_test(&self) -> std::sync::RwLockWriteGuard<'_, Data> {
        self.data.write().unwrap()
    }
}

/// The service's store. Cheap to clone.
#[derive(Clone)]
pub struct NotesStore(pub(crate) Arc<Shared>);

/// Where a new note goes.
pub enum Dest<'a> {
    Collection(&'a str),
    /// First active collection with this location, created when there is none.
    Location(String),
}

impl NotesStore {
    /// Load `<data_dir>/notes/` (AN-CLI-152). Never fails: unreadable files are set aside.
    pub fn open(data_dir: &Path) -> NotesStore {
        let dir = data_dir.join("notes");
        let data = load_dir(&dir);
        let (signal, _) = tokio::sync::watch::channel(data.cursor);
        NotesStore(Arc::new(Shared {
            dir,
            data: RwLock::new(data),
            write: tokio::sync::Mutex::new(()),
            signal,
            watchers: Registry::default(),
            shutdown: CancellationToken::new(),
        }))
    }

    /// End every running watch now (503) and refuse new ones (AN-CLI-142).
    pub fn shut_down(&self) {
        self.0.shutdown.cancel();
    }

    pub(crate) fn read(&self) -> std::sync::RwLockReadGuard<'_, Data> {
        self.0.data.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Write `coll` to disk, then commit it (and the new cursor). Must be called with the write lock held.
    async fn commit(&self, coll: Coll, cursor: u64) -> Result<(), ApiError> {
        let file = CollectionFile { format: model::FORMAT, collection: coll.meta.clone(), notes: coll.notes.clone() };
        let bytes = serde_json::to_vec_pretty(&file).map_err(ApiError::storage)?;
        let dir = self.0.dir.clone();
        let id = coll.meta.id.clone();
        tokio::task::spawn_blocking(move || write_atomic(&dir, &id, &bytes))
            .await
            .map_err(ApiError::storage)?
            .map_err(ApiError::storage)?;
        let woke = {
            let mut d = self.0.data.write().unwrap_or_else(|e| e.into_inner());
            let id = coll.meta.id.clone();
            if !d.colls.contains_key(&id) {
                d.order.push(id.clone());
            }
            for n in &coll.notes {
                d.note_at.insert(n.id.clone(), id.clone());
            }
            d.colls.insert(id, coll);
            let woke = cursor > d.cursor;
            d.cursor = d.cursor.max(cursor);
            woke.then_some(d.cursor)
        };
        if let Some(c) = woke {
            self.0.signal.send_replace(c);
        }
        Ok(())
    }

    fn new_collection(d: &Data, location: String) -> Result<Coll, ApiError> {
        if d.colls.len() >= model::MAX_COLLECTIONS {
            return Err(ApiError::limit("The notes service", &format!("{} collections", model::MAX_COLLECTIONS)));
        }
        let meta = CollectionMeta { id: d.fresh_collection_id(), location, state: "active".into(), created_at: util::iso_now(), updated_at: None };
        Ok(Coll { meta, notes: Vec::new() })
    }

    fn count(&self, coll: &Coll) -> Value {
        collection_json(&coll.meta, &coll.notes)
    }

    // ---- collections ---------------------------------------------------------------------------------------

    /// `(collection, created)` (AN-CLI-121).
    pub async fn create_collection(&self, location: String, reuse: bool) -> Result<(Value, bool), ApiError> {
        let _w = self.0.write.lock().await;
        let (coll, cursor) = {
            let d = self.read();
            if reuse && let Some(c) = d.first_active(&location) {
                return Ok((self.count(c), false));
            }
            (Self::new_collection(&d, location)?, d.cursor)
        };
        let out = self.count(&coll);
        self.commit(coll, cursor).await?;
        Ok((out, true))
    }

    pub fn list_collections(&self, location: Option<&str>, state: Option<&str>) -> Vec<Value> {
        let d = self.read();
        let mut list: Vec<&Coll> = d.order.iter().filter_map(|id| d.colls.get(id)).collect();
        list.retain(|c| location.is_none_or(|l| c.meta.location == l) && state.is_none_or(|s| c.meta.state == s));
        // Stable: equal times keep creation order.
        list.sort_by(|a, b| a.meta.created_at.cmp(&b.meta.created_at));
        list.into_iter().map(|c| self.count(c)).collect()
    }

    /// The collection and all its notes by `seq`.
    pub fn get_collection(&self, id: &str) -> Option<(Value, Vec<Value>)> {
        let d = self.read();
        let c = d.colls.get(id)?;
        let mut notes: Vec<&StoredNote> = c.notes.iter().collect();
        notes.sort_by_key(|n| n.seq);
        Some((self.count(c), notes.into_iter().map(|n| note_json(id, n)).collect()))
    }

    pub async fn set_collection_state(&self, id: &str, state: &str) -> Result<Value, ApiError> {
        let _w = self.0.write.lock().await;
        let (mut coll, cursor) = {
            let d = self.read();
            let c = d.colls.get(id).ok_or_else(ApiError::collection_not_found)?;
            if c.meta.state == state {
                return Ok(self.count(c));
            }
            (c.clone(), d.cursor)
        };
        coll.meta.state = state.to_string();
        coll.meta.updated_at = Some(model::next_time(coll.meta.updated_at.as_deref()).0);
        let out = self.count(&coll);
        self.commit(coll, cursor).await?;
        Ok(out)
    }

    // ---- notes ---------------------------------------------------------------------------------------------

    /// `(note, created)`: an existing note with the same `clientRef` in that collection answers instead (AN-CLI-122).
    pub async fn add_note(&self, into: Dest<'_>, input: NewNoteInput) -> Result<(Value, bool), ApiError> {
        let _w = self.0.write.lock().await;
        let (mut coll, cursor, note_id) = {
            let d = self.read();
            let coll = match &into {
                Dest::Collection(id) => d.colls.get(*id).cloned().ok_or_else(ApiError::collection_not_found)?,
                Dest::Location(loc) => match d.first_active(loc) {
                    Some(c) => c.clone(),
                    None => Self::new_collection(&d, loc.clone())?,
                },
            };
            if let Some(r) = &input.client_ref
                && let Some(n) = coll.notes.iter().find(|n| n.client_ref.as_ref() == Some(r))
            {
                return Ok((note_json(&coll.meta.id, n), false));
            }
            if coll.notes.len() >= model::MAX_NOTES_PER_COLLECTION {
                return Err(ApiError::limit("This collection", &format!("{} notes", model::MAX_NOTES_PER_COLLECTION)));
            }
            (coll, d.cursor, d.fresh_note_id(&HashSet::new()))
        };
        let seq = cursor + 1;
        let location = input.location.clone().unwrap_or_else(|| coll.meta.location.clone());
        let note = StoredNote {
            id: note_id,
            location,
            seq,
            kind: input.kind,
            status: "pending".into(),
            closed_by: None,
            closed_at: None,
            comment: input.comment,
            element: input.element,
            element_path: input.element_path,
            design_id: input.design_id,
            client_ref: input.client_ref,
            client_timestamp: input.client_timestamp,
            created_at: util::iso_now(),
            updated_at: None,
            target: input.target,
            thread: Vec::new(),
        };
        let out = note_json(&coll.meta.id, &note);
        coll.notes.push(note);
        self.commit(coll, seq).await?;
        Ok((out, true))
    }

    pub fn note(&self, id: &str) -> Option<Value> {
        let d = self.read();
        let (c, n) = d.find_note(id)?;
        Some(note_json(&c.meta.id, n))
    }

    /// Notes by `seq`, at most `limit`; `(notes, more)`.
    pub fn query(&self, f: &Filter, limit: usize) -> (Vec<Value>, bool) {
        let d = self.read();
        let mut hits: Vec<(&Coll, &StoredNote)> =
            d.colls.values().flat_map(|c| c.notes.iter().map(move |n| (c, n))).filter(|(c, n)| f.wants(c, n)).collect();
        hits.sort_by_key(|(_, n)| n.seq);
        let more = hits.len() > limit;
        (hits.into_iter().take(limit).map(|(c, n)| note_json(&c.meta.id, n)).collect(), more)
    }

    /// Pending notes with `seq > after` (and, with `replies`, open notes with a person's reply after it), by
    /// `seq`, at most `max`, plus the cursor for the answer (AN-CLI-140/141). One read lock for both.
    pub fn since(&self, after: u64, f: &Filter, replies: bool, max: usize) -> (Vec<Value>, Vec<String>, u64) {
        let d = self.read();
        let after = if after > d.cursor { 0 } else { after };
        let mut pending = f.clone();
        pending.statuses = vec!["pending"];
        let mut hits: Vec<(u64, &Coll, &StoredNote, bool)> = Vec::new();
        for c in d.colls.values() {
            for n in &c.notes {
                if n.seq > after && pending.wants(c, n) {
                    hits.push((n.seq, c, n, false));
                    continue;
                }
                if replies && (n.status == "pending" || n.status == "acknowledged") {
                    let mut open = f.clone();
                    open.statuses = vec![];
                    let reply = n.thread.iter().filter(|m| m.role == "human").filter_map(|m| m.seq).filter(|s| *s > after).max();
                    if let Some(s) = reply
                        && open.wants(c, n)
                    {
                        hits.push((s, c, n, true));
                    }
                }
            }
        }
        hits.sort_by_key(|h| h.0);
        let mut cursor = d.cursor;
        if hits.len() > max {
            hits.truncate(max);
            cursor = hits.last().map_or(cursor, |h| h.0);
        }
        let reply_ids = hits.iter().filter(|h| h.3).map(|h| h.2.id.clone()).collect();
        (hits.into_iter().map(|(_, c, n, _)| note_json(&c.meta.id, n)).collect(), reply_ids, cursor)
    }

    /// Whether anything new for this watch exists (cheap check before the batch window).
    pub fn has_since(&self, after: u64, f: &Filter, replies: bool) -> bool {
        !self.since(after, f, replies, 1).0.is_empty()
    }

    /// AN-CLI-131.
    pub async fn set_status(&self, id: &str, input: StatusInput) -> Result<Value, ApiError> {
        let _w = self.0.write.lock().await;
        let (mut coll, cursor) = {
            let d = self.read();
            let (c, n) = d.find_note(id).ok_or_else(ApiError::note_not_found)?;
            if let Some(want) = &input.if_status
                && !want.contains(&n.status)
            {
                let mut e = ApiError::new(409, "status_conflict", format!("The note is {}, not {}", n.status, want.join(" or ")));
                e.note = Some(note_json(&c.meta.id, n));
                return Err(e);
            }
            let same = n.status == input.status;
            if same && input.message.is_none() {
                return Ok(note_json(&c.meta.id, n));
            }
            if input.message.is_some() && n.thread.len() >= model::MAX_MESSAGES_PER_NOTE {
                return Err(ApiError::limit("This note's thread", &format!("{} messages", model::MAX_MESSAGES_PER_NOTE)));
            }
            (c.clone(), d.cursor)
        };
        let mut new_cursor = cursor;
        let n = coll.notes.iter_mut().find(|n| n.id == id).expect("found above");
        let (at, ms) = model::next_time(n.updated_at.as_deref());
        if n.status != input.status {
            if input.status == "pending" {
                new_cursor += 1;
                n.seq = new_cursor;
            }
            n.status = input.status.clone();
            if input.status == "resolved" || input.status == "dismissed" {
                n.closed_by = Some(input.by.clone());
                n.closed_at = Some(at.clone());
            } else {
                n.closed_by = None;
                n.closed_at = None;
            }
        }
        if let Some(text) = &input.message {
            let role = if input.by == "human" { "human" } else { "agent" };
            let ids: HashSet<&str> = n.thread.iter().map(|m| m.id.as_str()).collect();
            let mid = fresh_message_id(&ids);
            let seq = (role == "human").then(|| {
                new_cursor += 1;
                new_cursor
            });
            n.thread.push(StoredMessage { id: mid, role: role.into(), content: text.clone(), created_at: at.clone(), created_at_ms: ms, seq });
        }
        n.updated_at = Some(at);
        let out = note_json(&coll.meta.id, n);
        self.commit(coll, new_cursor).await?;
        Ok(out)
    }

    /// AN-CLI-132.
    pub async fn add_message(&self, id: &str, role: &str, content: &str) -> Result<Value, ApiError> {
        let _w = self.0.write.lock().await;
        let (mut coll, cursor) = {
            let d = self.read();
            let (c, n) = d.find_note(id).ok_or_else(ApiError::note_not_found)?;
            if n.thread.len() >= model::MAX_MESSAGES_PER_NOTE {
                return Err(ApiError::limit("This note's thread", &format!("{} messages", model::MAX_MESSAGES_PER_NOTE)));
            }
            (c.clone(), d.cursor)
        };
        let mut new_cursor = cursor;
        let n = coll.notes.iter_mut().find(|n| n.id == id).expect("found above");
        let (at, ms) = model::next_time(n.updated_at.as_deref());
        let ids: HashSet<&str> = n.thread.iter().map(|m| m.id.as_str()).collect();
        let mid = fresh_message_id(&ids);
        let seq = (role == "human").then(|| {
            new_cursor += 1;
            new_cursor
        });
        n.thread.push(StoredMessage { id: mid, role: role.into(), content: content.into(), created_at: at.clone(), created_at_ms: ms, seq });
        n.updated_at = Some(at);
        let out = note_json(&coll.meta.id, n);
        self.commit(coll, new_cursor).await?;
        Ok(out)
    }

    /// `GET /api/notes/info` (AN-CLI-133).
    pub fn info(&self) -> Value {
        let (cursor, counts, collections) = {
            let d = self.read();
            let mut counts = json!({ "pending": 0, "acknowledged": 0, "resolved": 0, "dismissed": 0 });
            for n in d.colls.values().flat_map(|c| c.notes.iter()) {
                if let Some(v) = counts.get_mut(n.status.as_str()) {
                    *v = json!(v.as_u64().unwrap_or(0) + 1);
                }
            }
            (d.cursor, counts, d.colls.len())
        };
        json!({
            "service": "prototypie-notes",
            "apiVersion": 1,
            "appVersion": env!("CARGO_PKG_VERSION"),
            "pid": std::process::id(),
            "cursor": cursor,
            "counts": counts,
            "collections": collections,
            "watchers": self.0.watchers.list(),
        })
    }
}

fn fresh_message_id(taken: &HashSet<&str>) -> String {
    loop {
        let id = model::new_id('m');
        if !taken.contains(id.as_str()) {
            return id;
        }
    }
}

// ---- files -------------------------------------------------------------------------------------------------

/// `<dir>/<id>.json.tmp`, fsync, rename to `<id>.json`, fsync the folder (AN-CLI-151).
fn write_atomic(dir: &Path, id: &str, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!("{id}.json.tmp"));
    let dest = dir.join(format!("{id}.json"));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, &dest)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result?;
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

fn corrupt_name(name: &str) -> String {
    format!("{name}.corrupt-{}", chrono::Utc::now().format("%Y%m%dT%H%M%SZ"))
}

/// Load every `c[0-9a-z]{10}.json` (no symlinks); delete leftover `*.tmp`; set unreadable files aside; leave
/// newer formats alone (AN-CLI-152, -505).
fn load_dir(dir: &Path) -> Data {
    let mut data = Data::default();
    let Ok(rd) = std::fs::read_dir(dir) else { return data };
    let mut files: Vec<CollectionFile> = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if !meta.file_type().is_file() {
            continue;
        }
        if name.ends_with(".tmp") {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let Some(stem) = name.strip_suffix(".json").filter(|s| model::is_collection_id(s)) else { continue };
        let parsed = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let format = parsed.as_ref().and_then(|v| v["format"].as_u64());
        if let Some(f) = format
            && f > model::FORMAT
        {
            eprintln!("notes: {name} has format {f}, newer than this build reads; left alone");
            continue;
        }
        match parsed.and_then(|v| serde_json::from_value::<CollectionFile>(v).ok()).filter(|f| f.collection.id == stem) {
            Some(f) => files.push(f),
            None => {
                let to = dir.join(corrupt_name(&name));
                eprintln!("notes: {name} is unreadable; moved to {}", to.display());
                let _ = std::fs::rename(&path, &to);
            }
        }
    }
    files.sort_by(|a, b| (&a.collection.created_at, &a.collection.id).cmp(&(&b.collection.created_at, &b.collection.id)));
    for f in files {
        let id = f.collection.id.clone();
        for n in &f.notes {
            data.cursor = data.cursor.max(n.seq);
            for m in &n.thread {
                data.cursor = data.cursor.max(m.seq.unwrap_or(0));
            }
            data.note_at.insert(n.id.clone(), id.clone());
        }
        data.order.push(id.clone());
        data.colls.insert(id, Coll { meta: f.collection, notes: f.notes });
    }
    data
}
