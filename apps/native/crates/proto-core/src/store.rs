//! The record store: one shared, versioned state per data dir.
//!
//! Reads are lock-free snapshots (`Arc<Snapshot>`). A write (`apply`) holds the state lock only for the in-memory
//! swap; disk I/O (atomic save of state.json, the NDJSON log) happens on the store's writer thread. Every accepted
//! batch is one new version and is broadcast to subscribers.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, RwLock};

use anyhow::Result;
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::lock::DataLock;
use crate::persist::{self, Msg};
use crate::record::{split_path, ApplyError, Change, ChangeEvent, ChangeMeta, Cursor, StateFile};
use crate::snapshot::{Entry, Snapshot};

/// Subscribers that fall this many events behind get `RecvError::Lagged` and should resync with `read(0)`.
const EVENT_CAPACITY: usize = 1024;

/// A handle to the store. Clone it freely: all clones share one state, writer thread and event channel.
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

struct Inner {
    /// None for an in-memory store.
    dir: Option<PathBuf>,
    state: Arc<RwLock<Arc<Snapshot>>>,
    events: broadcast::Sender<ChangeEvent>,
    /// Disk writer; None for an in-memory store. Wrapped for `Sync`.
    writer: Option<std::sync::Mutex<mpsc::Sender<Msg>>>,
    /// The writer thread (it holds the data dir's lock until its final save).
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Inner {
    /// Last handle gone: close the writer's channel and wait for its final save, so the data dir is unlocked (and
    /// state.json current) when the drop returns. Blocking for as long as that save takes.
    fn drop(&mut self) {
        self.writer.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = self.snapshot();
        f.debug_struct("Store").field("dir", &self.inner.dir).field("epoch", &s.epoch).field("version", &s.version).finish()
    }
}

impl Store {
    /// Opens (or creates) the store in `dir`, loading `dir/state.json`. Blocking: the file can be 100+ MB, so call
    /// this off the UI thread (e.g. in a startup thread) and hand the `Store` over.
    ///
    /// One writer per data dir: takes an exclusive lock on `dir/.lock` for the store's lifetime. If another store
    /// (this process or another) has it, fails with [`StoreError::Locked`](crate::StoreError) `{ pid, dir }`; find it
    /// with `err.downcast_ref::<StoreError>()`. Read-only users load a copy instead (`Store::from_state`).
    pub fn open(dir: impl AsRef<Path>) -> Result<Store> {
        let dir = dir.as_ref().to_path_buf();
        let lock = DataLock::acquire(&dir)?;
        let (snap, dirty) = match persist::load(&dir)? {
            Some(file) => {
                let (snap, stray) = Snapshot::from_file(file);
                (snap, stray > 0)
            }
            None => (Snapshot::empty(Arc::from(persist::new_epoch())), false),
        };
        let state = Arc::new(RwLock::new(Arc::new(snap)));
        let (tx, rx) = mpsc::channel();
        let thread = persist::spawn_writer(dir.clone(), lock, state.clone(), rx, dirty);
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let inner = Inner { dir: Some(dir), state, events, writer: Some(std::sync::Mutex::new(tx)), thread: Some(thread) };
        Ok(Store { inner: Arc::new(inner) })
    }

    /// A store that never touches disk (tests, previews, `--snap` renders).
    pub fn in_memory() -> Store {
        let state = Arc::new(RwLock::new(Arc::new(Snapshot::empty(Arc::from(persist::new_epoch())))));
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Store { inner: Arc::new(Inner { dir: None, state, events, writer: None, thread: None }) }
    }

    /// An in-memory store holding `file` (e.g. a fixture or a parsed reference state).
    pub fn from_state(file: StateFile) -> Store {
        let store = Store::in_memory();
        let (snap, _) = Snapshot::from_file(file);
        *store.inner.state.write().expect("state lock") = Arc::new(snap);
        store
    }

    /// The data dir (None for an in-memory store). Other crates keep their own files under it, and write nothing
    /// when it is None (never fall back to [`persist::default_data_dir`]: headless runs use in-memory stores).
    pub fn data_dir(&self) -> Option<&Path> {
        self.inner.dir.as_deref()
    }

    /// The current state. Cheap (an `Arc` clone); the snapshot never changes, newer writes make a new one.
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.inner.state.read().expect("state lock").clone()
    }

    pub fn cursor(&self) -> Cursor {
        let s = self.snapshot();
        Cursor { epoch: s.epoch.to_string(), version: s.version }
    }

    /// `GET /api/state?since=N&epoch=E`: records changed after `since`, or everything for `since == 0` or another epoch.
    pub fn read(&self, since: u64, epoch: Option<&str>) -> StateFile {
        self.snapshot().read(since, epoch)
    }

    /// Applies a batch as one version (last write per path wins). An empty batch returns the cursor unchanged.
    /// Rejects the whole batch if any path is invalid or a set has no value.
    pub fn apply(&self, changes: Vec<Change>, meta: ChangeMeta) -> Result<Cursor, ApplyError> {
        for c in &changes {
            if split_path(&c.path).is_none() {
                return Err(ApplyError::InvalidPath(c.path.clone()));
            }
            if !c.deleted && c.value.is_none() {
                return Err(ApplyError::MissingValue(c.path.clone()));
            }
        }
        let mut guard = self.inner.state.write().expect("state lock");
        if changes.is_empty() {
            return Ok(Cursor { epoch: guard.epoch.to_string(), version: guard.version });
        }
        let mut next = Snapshot::clone(&guard);
        let rev = next.version + 1;
        next.version = rev;
        let ts = persist::iso_utc(persist::now_ms());
        let mut lines = Vec::with_capacity(changes.len());
        let mut paths = Vec::with_capacity(changes.len());
        for c in changes {
            let (kind, _) = split_path(&c.path).expect("validated");
            let prev = next.records.get(c.path.as_str());
            let before = match prev {
                None => "absent",
                Some(e) if e.is_deleted() => "deleted",
                Some(_) => "live",
            };
            let op = if c.deleted { "delete" } else if before == "live" { "update" } else { "create" };
            let title = title_of(if c.deleted { prev.and_then(|e| e.value.as_deref()) } else { c.value.as_ref() });
            lines.push(log_line(&ts, rev, &c.path, op, before, prev.map(|e| e.rev), title, &meta));
            let value = if c.deleted { None } else { c.value.map(Arc::new) };
            paths.push(c.path.clone());
            next.records.insert(Arc::from(c.path), Arc::new(Entry::new(kind, rev, value)));
        }
        let epoch = next.epoch.to_string();
        *guard = Arc::new(next);
        drop(guard);

        if let Some(w) = &self.inner.writer {
            let _ = w.lock().expect("writer lock").send(Msg::Changed(lines));
        }
        let _ = self.inner.events.send(ChangeEvent {
            epoch: epoch.clone(),
            version: rev,
            client_id: meta.client_id,
            paths: paths.into(),
        });
        Ok(Cursor { epoch, version: rev })
    }

    /// One change event per accepted batch. Works without a tokio runtime (`try_recv` / `blocking_recv`);
    /// a receiver that lags behind gets `Lagged` and should resync with `read(0, None)`.
    pub fn subscribe(&self) -> broadcast::Receiver<ChangeEvent> {
        self.inner.events.subscribe()
    }

    /// Saves now if a save is pending and waits for it. Blocking: for shutdown and tests, not the UI thread.
    pub fn flush(&self) -> Result<()> {
        let Some(w) = &self.inner.writer else { return Ok(()) };
        let (tx, rx) = mpsc::channel();
        w.lock().expect("writer lock").send(Msg::Flush(tx)).map_err(|_| anyhow::anyhow!("writer thread gone"))?;
        rx.recv().map_err(|_| anyhow::anyhow!("writer thread gone"))?.map_err(|e| anyhow::anyhow!(e))
    }
}

fn title_of(v: Option<&Value>) -> Option<String> {
    let o = v?.as_object()?;
    let t = o.get("title").or_else(|| o.get("name"))?.as_str()?;
    Some(t.chars().take(80).collect())
}

/// One NDJSON audit line, same fields as the reference (`undefined` fields are left out).
#[allow(clippy::too_many_arguments)]
fn log_line(ts: &str, rev: u64, path: &str, op: &str, before: &str, prev_rev: Option<u64>, title: Option<String>, meta: &ChangeMeta) -> String {
    let mut o = serde_json::Map::new();
    o.insert("ts".into(), json!(ts));
    o.insert("rev".into(), json!(rev));
    if let Some(c) = &meta.client_id {
        o.insert("clientId".into(), json!(c));
    }
    o.insert("path".into(), json!(path));
    o.insert("op".into(), json!(op));
    o.insert("before".into(), json!(before));
    if let Some(p) = prev_rev {
        o.insert("prevRev".into(), json!(p));
    }
    if let Some(t) = title {
        o.insert("title".into(), json!(t));
    }
    if let Some(r) = &meta.reason {
        o.insert("reason".into(), json!(r));
    }
    if let Some(u) = &meta.user_agent {
        o.insert("ua".into(), json!(u));
    }
    Value::Object(o).to_string()
}
