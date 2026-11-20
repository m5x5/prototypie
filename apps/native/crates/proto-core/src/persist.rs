//! Disk side of the store: the data dir, loading, the writer thread (debounced atomic saves + the append-only
//! log) and the one-time import of the reference app's state.

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::lock::{DataLock, StoreError};
use crate::record::StateFile;
use crate::snapshot::Snapshot;

pub const STATE_FILE: &str = "state.json";
pub const LOG_FILE: &str = "state-log.ndjson";
/// Bursts of writes are coalesced into one rewrite of state.json (it holds many MB of HTML).
const SAVE_DEBOUNCE: Duration = Duration::from_millis(300);

/// `$PROTOTYPIE_DATA`, else `~/Library/Application Support/Prototypie`. Only for the app's startup choice of where
/// to open the store; everything else uses `Store::data_dir()` and writes nothing when that is None.
pub fn default_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("PROTOTYPIE_DATA").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join("Library/Application Support/Prototypie")
}

/// The web app's state file (only ever read), for the one-time import of its designs: `state.json` in the folder
/// `$PROTOTYPIE_REFERENCE_DATA` names (the web app's `data/` directory). None when it isn't set: nothing is imported.
pub fn reference_state_path() -> Option<PathBuf> {
    let dir = std::env::var_os("PROTOTYPIE_REFERENCE_DATA").filter(|d| !d.is_empty())?;
    Some(PathBuf::from(dir).join(STATE_FILE))
}

/// Reads `<dir>/state.json`; a missing file is an empty state with a new epoch. Blocking (large file).
pub(crate) fn load(dir: &Path) -> Result<Option<StateFile>> {
    let path = dir.join(STATE_FILE);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let file: StateFile = serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(file))
}

/// Writes the snapshot to `<dir>/state.json` atomically (tmp file + rename: readers never see half a file).
pub(crate) fn save(dir: &Path, snap: &Snapshot) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let tmp = dir.join(format!("{STATE_FILE}.{}.{}.tmp", std::process::id(), now_ms()));
    {
        let mut w = BufWriter::new(File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?);
        serde_json::to_writer(&mut w, &snap.read(0, None))?;
        w.flush()?;
        w.get_ref().sync_all()?;
    }
    fs::rename(&tmp, dir.join(STATE_FILE)).context("replacing state.json")?;
    Ok(())
}

pub(crate) enum Msg {
    /// The state changed: save it soon, and append these log lines now.
    Changed(Vec<String>),
    /// Save now if anything is pending, then reply.
    Flush(Sender<Result<(), String>>),
}

/// The writer thread: owns all disk writes of a store, and the data dir's lock. Exits (after a final save) when the
/// store is dropped, and only then releases the lock.
pub(crate) fn spawn_writer(
    dir: PathBuf,
    lock: DataLock,
    state: Arc<RwLock<Arc<Snapshot>>>,
    rx: Receiver<Msg>,
    dirty_at_start: bool,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("proto-core-writer".into())
        .spawn(move || {
            let mut due: Option<Instant> = dirty_at_start.then(Instant::now);
            let save_now = |due: &mut Option<Instant>| -> Result<(), String> {
                if due.take().is_none() {
                    return Ok(());
                }
                let snap = state.read().map(|s| s.clone()).map_err(|_| "state lock poisoned".to_string())?;
                save(&dir, &snap).map_err(|e| {
                    eprintln!("[proto-core] could not write state.json: {e:#}");
                    format!("{e:#}")
                })
            };
            loop {
                let msg = match due {
                    Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                        Ok(m) => Some(m),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    },
                    None => match rx.recv() {
                        Ok(m) => Some(m),
                        Err(_) => break,
                    },
                };
                match msg {
                    None => {
                        if save_now(&mut due).is_err() {
                            // Retry later rather than spinning.
                            due = Some(Instant::now() + Duration::from_secs(2));
                        }
                    }
                    Some(Msg::Changed(lines)) => {
                        append_log(&dir, &lines);
                        due.get_or_insert_with(|| Instant::now() + SAVE_DEBOUNCE);
                    }
                    Some(Msg::Flush(reply)) => {
                        let _ = reply.send(save_now(&mut due));
                    }
                }
            }
            let _ = save_now(&mut due);
            drop(lock);
        })
        .expect("spawn proto-core writer thread")
}

fn append_log(dir: &Path, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let res = fs::create_dir_all(dir).and_then(|_| {
        let mut f = OpenOptions::new().create(true).append(true).open(dir.join(LOG_FILE))?;
        let mut buf = lines.join("\n");
        buf.push('\n');
        f.write_all(buf.as_bytes())
    });
    if let Err(e) = res {
        eprintln!("[proto-core] could not append to {LOG_FILE}: {e}");
    }
}

/// What [`import_reference`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOutcome {
    /// Copied; the new state has this many records.
    Imported { records: usize },
    /// The data dir already has a state.json: nothing was touched.
    AlreadyPresent,
}

/// Copies a reference `state.json` into `data_dir` once, before the store is opened, plus the reference's static
/// assets ([`import_reference_assets`] from `<repo>/public`, when `source` is `<repo>/data/state.json`). Never
/// writes the source, and never overwrites an existing state. The copy is validated (parsed) and written
/// atomically. Holds the data dir's lock while it writes: if another store has the dir open, it does nothing
/// (`AlreadyPresent`) when a state exists there, else fails with [`StoreError::Locked`]. Blocking.
pub fn import_reference(source: &Path, data_dir: &Path) -> Result<ImportOutcome> {
    let _lock = match DataLock::acquire(data_dir) {
        Ok(lock) => lock,
        Err(StoreError::Locked { .. }) if data_dir.join(STATE_FILE).exists() => return Ok(ImportOutcome::AlreadyPresent),
        Err(e) => return Err(e.into()),
    };
    if let Some(public) = source.parent().and_then(Path::parent).map(|repo| repo.join("public")).filter(|p| p.is_dir()) {
        import_reference_assets(&public, data_dir)?;
    }
    let dest = data_dir.join(STATE_FILE);
    if dest.exists() {
        return Ok(ImportOutcome::AlreadyPresent);
    }
    let bytes = fs::read(source).with_context(|| format!("reading {}", source.display()))?;
    let file: StateFile = serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", source.display()))?;
    fs::create_dir_all(data_dir).with_context(|| format!("creating {}", data_dir.display()))?;
    let tmp = data_dir.join(format!("{STATE_FILE}.import.{}.tmp", std::process::id()));
    fs::write(&tmp, &bytes).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, &dest).context("placing the imported state.json")?;
    Ok(ImportOutcome::Imported { records: file.records.len() })
}

/// Where the app's static files live: `<data_dir>/public`. Designs load assets root-relative (`/vendor/…`), so the
/// embedded server serves this directory at `/` and web views load design html with that server as base URL.
pub fn public_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("public")
}

/// The reference app's static folders the standalone app needs: `vendor/` (scripts and styles designs load) and
/// `imports/` (bundles for "Import").
pub const REFERENCE_ASSET_DIRS: [&str; 2] = ["vendor", "imports"];

/// What [`import_reference_assets`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssetImport {
    /// Files copied now.
    pub copied: usize,
    /// Files already present in the data dir (left as they are).
    pub kept: usize,
}

/// Copies `<reference_public>/{vendor,imports}` into [`public_dir`]`(data_dir)`: every file that isn't there yet
/// (each written atomically); existing files are never overwritten, the source is never written. Symlinks are
/// skipped. Idempotent, so it can run on every start. Blocking.
pub fn import_reference_assets(reference_public: &Path, data_dir: &Path) -> Result<AssetImport> {
    let mut out = AssetImport::default();
    let dest_root = public_dir(data_dir);
    for dir in REFERENCE_ASSET_DIRS {
        let src = reference_public.join(dir);
        if src.is_dir() {
            copy_missing(&src, &dest_root.join(dir), &mut out)?;
        }
    }
    Ok(out)
}

fn copy_missing(src: &Path, dest: &Path, out: &mut AssetImport) -> Result<()> {
    fs::create_dir_all(dest).with_context(|| format!("creating {}", dest.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = dest.join(entry.file_name());
        if kind.is_dir() {
            copy_missing(&entry.path(), &target, out)?;
        } else if kind.is_file() {
            if target.exists() {
                out.kept += 1;
                continue;
            }
            let tmp = dest.join(format!(".{}.import.tmp", entry.file_name().to_string_lossy()));
            fs::copy(entry.path(), &tmp).with_context(|| format!("copying {}", entry.path().display()))?;
            fs::rename(&tmp, &target).with_context(|| format!("placing {}", target.display()))?;
            out.copied += 1;
        }
    }
    Ok(())
}

pub(crate) fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

/// `2026-11-18T21:16:30.123Z` for a Unix time in milliseconds (like JS `toISOString`).
pub(crate) fn iso_utc(ms: u128) -> String {
    let secs = (ms / 1000) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem % 3600 / 60, rem % 60, ms % 1000)
}

/// A random v4-style UUID for a new state's epoch (no extra dependency: std's per-process random hasher keys).
pub(crate) fn new_epoch() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut bits = [0u8; 16];
    for (i, half) in bits.chunks_mut(8).enumerate() {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(now_ms());
        h.write_usize(i);
        h.write_u32(std::process::id());
        half.copy_from_slice(&h.finish().to_le_bytes());
    }
    bits[6] = (bits[6] & 0x0f) | 0x40;
    bits[8] = (bits[8] & 0x3f) | 0x80;
    let hex: String = bits.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}
