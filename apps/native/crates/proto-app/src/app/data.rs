//! The app's data: proto-core's `Store`, the snapshot the view reads, and per-device prefs on disk.
//!
//! The app starts on an empty in-memory store and draws at once. `setup` opens the real store on
//! a thread (importing the reference `state.json` into an empty data dir first) and reads the
//! prefs; `poll` swaps them in when they arrive. After that, `poll` refreshes the snapshot when
//! the store's changes (from this app, the embedded server or agents) made a new one. Prefs are
//! written by a saver thread, never on the UI thread.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use proto_core::{Snapshot, Store};
use rune::Waker;

use super::prefs::Prefs;

const PREFS_FILE: &str = "prefs.json";

/// Where the store comes from, as a state: loading the real one, ready, a fixture (scenes, tests)
/// or failed (the app keeps running on an empty store and says why).
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// The empty store the app starts on, before `start_loading`.
    Idle,
    Loading,
    Ready,
    Fixture,
    Failed(String),
    /// Another Prototypie (`pid`) holds the data dir: the app shows a read-only copy of it (nothing
    /// is saved: no store writer, no prefs) and says so (`shell::banner`).
    Locked { pid: u32 },
}

/// What the loader thread hands back.
struct Loaded {
    store: Result<Store, String>,
    prefs: Option<Prefs>,
    dir: PathBuf,
    /// A tooling run (--snap, --script, --tree…): the store is an in-memory copy and nothing is
    /// ever written back (no store saves, no prefs, no import).
    read_only: bool,
    /// Another process holds the data dir (its pid); the store is a read-only copy.
    locked: Option<u32>,
}

pub struct Data {
    pub store: Store,
    pub snap: Arc<Snapshot>,
    pub source: Source,
    loaded: Arc<Mutex<Option<Loaded>>>,
    /// Sends prefs to the saver thread (set once the data dir is known).
    saver: Option<Sender<Prefs>>,
    saved_rev: u64,
}

impl Default for Data {
    fn default() -> Self {
        Data { source: Source::Idle, ..Data::fixture(Store::in_memory()) }
    }
}

impl Data {
    /// A given store (scenes, tests): nothing is loaded or saved.
    pub fn fixture(store: Store) -> Data {
        let snap = store.snapshot();
        Data { store, snap, source: Source::Fixture, loaded: Arc::default(), saver: None, saved_rev: 0 }
    }

    /// Start loading the real store and the prefs on a thread; `wake` asks for a frame when done.
    /// Does nothing for a fixture that a scene installed.
    pub fn start_loading(&mut self, wake: Waker) {
        if self.source != Source::Idle {
            return;
        }
        self.source = Source::Loading;
        let slot = self.loaded.clone();
        // Headless tooling runs see the user's data but must never change it.
        let read_only = rune::is_headless();
        std::thread::Builder::new()
            .name("proto-data-load".into())
            .spawn(move || {
                let dir = proto_core::default_data_dir();
                let mut locked = None;
                let store = if read_only {
                    read_only_copy(&dir)
                } else {
                    match (|| {
                        if let Some(reference) = proto_core::reference_state_path() {
                            proto_core::import_reference(&reference, &dir)?;
                        }
                        Store::open(&dir)
                    })() {
                        Ok(store) => Ok(store),
                        // Never a second writer: look at the holder's data, change nothing.
                        Err(e) => match super::instance::locked_pid(&e) {
                            Some(pid) => {
                                locked = Some(pid);
                                read_only_copy(&dir)
                            }
                            None => Err(format!("{e:#}")),
                        },
                    }
                };
                let prefs = std::fs::read(dir.join(PREFS_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok());
                let read_only = read_only || locked.is_some();
                *slot.lock().unwrap() = Some(Loaded { store, prefs, dir, read_only, locked });
                wake();
            })
            .expect("spawn loader");
    }

    /// Take a finished load (store, prefs) and pick up new snapshots. Returns the loaded prefs the
    /// first time, and whether anything visible changed.
    pub fn poll(&mut self) -> (Option<Prefs>, bool) {
        let mut prefs = None;
        let mut changed = false;
        if let Some(l) = self.loaded.lock().unwrap().take() {
            match l.store {
                Ok(store) => {
                    self.store = store;
                    self.source = match l.locked {
                        Some(pid) => Source::Locked { pid },
                        None => Source::Ready,
                    };
                    crate::perf::mark("store-loaded");
                }
                Err(e) => self.source = Source::Failed(e),
            }
            if !l.read_only {
                self.saver = Some(spawn_saver(l.dir));
            }
            prefs = l.prefs;
            changed = true;
        }
        let now = self.store.snapshot();
        if !Arc::ptr_eq(&now, &self.snap) {
            self.snap = now;
            changed = true;
        }
        (prefs, changed)
    }

    /// Queue `prefs` for saving when `rev` is newer than what was last queued.
    pub fn save_prefs(&mut self, prefs: &Prefs, rev: u64) {
        if rev == self.saved_rev {
            return;
        }
        if let Some(tx) = &self.saver {
            self.saved_rev = rev;
            let _ = tx.send(prefs.clone());
        }
    }
}

/// A thread that writes the newest prefs it's sent (tmp + rename), coalescing bursts.
fn spawn_saver(dir: PathBuf) -> Sender<Prefs> {
    let (tx, rx): (Sender<Prefs>, Receiver<Prefs>) = channel();
    std::thread::Builder::new()
        .name("proto-prefs-save".into())
        .spawn(move || {
            while let Ok(mut prefs) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    prefs = newer;
                }
                let Ok(bytes) = serde_json::to_vec_pretty(&prefs) else { continue };
                let tmp = dir.join(format!("{PREFS_FILE}.tmp"));
                if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&tmp, bytes).is_ok() {
                    let _ = std::fs::rename(&tmp, dir.join(PREFS_FILE));
                }
            }
        })
        .expect("spawn prefs saver");
    tx
}

/// The data dir's state.json as an in-memory store (tooling runs): nothing is written, not even
/// on flush. A missing or unreadable file gives an empty store.
fn read_only_copy(dir: &std::path::Path) -> Result<Store, String> {
    match std::fs::read(dir.join(proto_core::STATE_FILE)) {
        Ok(bytes) => serde_json::from_slice::<proto_core::StateFile>(&bytes)
            .map(Store::from_state)
            .map_err(|e| format!("reading state.json: {e}")),
        Err(_) => Ok(Store::in_memory()),
    }
}
