//! One writer per data dir: an exclusive advisory lock (`flock`) on `<data>/.lock`, held for a writing store's lifetime.
//!
//! The kernel drops the lock when the holding file is closed, including when the process exits or crashes, so a
//! stale lock file never blocks anyone. The file stays in place (removing it would race with a new holder) and
//! holds the owner's pid for the error message.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const LOCK_FILE: &str = ".lock";

/// Why a data dir can't be opened for writing.
#[derive(Debug)]
pub enum StoreError {
    /// Another store (in this or another process) is writing this data dir; `pid` is that process (0 in the rare
    /// case it couldn't be read). The holder is alive: the kernel drops the lock when a process exits.
    Locked { pid: u32 },
    /// The lock file couldn't be created or locked.
    Io { dir: PathBuf, source: std::io::Error },
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Locked { pid } => write!(f, "another Prototypie (pid {pid}) is already using this data dir"),
            StoreError::Io { dir, source } => write!(f, "couldn't lock {}: {source}", dir.join(LOCK_FILE).display()),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StoreError::Io { source, .. } => Some(source),
            StoreError::Locked { .. } => None,
        }
    }
}

/// The held lock. Dropping it releases the lock.
#[derive(Debug)]
pub(crate) struct DataLock {
    _file: File,
}

impl DataLock {
    /// Takes the lock without waiting; `StoreError::Locked` if someone else holds it. Creates `dir` if needed.
    pub(crate) fn acquire(dir: &Path) -> Result<DataLock, StoreError> {
        let io = |source| StoreError::Io { dir: dir.to_path_buf(), source };
        fs::create_dir_all(dir).map_err(io)?;
        let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join(LOCK_FILE)).map_err(io)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => return Err(StoreError::Locked { pid: read_pid(&mut file) }),
            Err(fs::TryLockError::Error(e)) => return Err(io(e)),
        }
        // Ours now: record the pid (best effort; the lock itself is what counts).
        let _ = file.set_len(0).and_then(|_| file.seek(SeekFrom::Start(0))).and_then(|_| write!(file, "{}", std::process::id())).and_then(|_| file.flush());
        Ok(DataLock { _file: file })
    }
}

/// The pid of the process whose store holds `dir`'s lock, or None if nobody does. Cheap (opens one small file,
/// never loads state.json, never creates anything), so `main` can hand over to a running instance before any
/// window shows. A returned pid is alive: the lock only exists while its holder does.
pub fn lock_holder(dir: &Path) -> Option<u32> {
    let mut file = File::open(dir.join(LOCK_FILE)).ok()?;
    // A shared lock succeeds only when no store holds the exclusive one (released again right away on drop).
    match file.try_lock_shared() {
        Ok(()) => None,
        Err(fs::TryLockError::WouldBlock) => Some(read_pid(&mut file)),
        Err(fs::TryLockError::Error(_)) => None,
    }
}

/// The holder writes its pid right after locking; retry briefly in case we looked in between. 0 if unreadable.
fn read_pid(file: &mut File) -> u32 {
    for _ in 0..10 {
        let mut s = String::new();
        if file.seek(SeekFrom::Start(0)).and_then(|_| file.read_to_string(&mut s)).is_ok()
            && let Ok(pid) = s.trim().parse()
        {
            return pid;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    0
}
