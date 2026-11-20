//! One Prototypie per data dir. proto-core's store lock (worker-1) says when another process
//! holds the data dir; then a launch brings that process forward and exits before opening a
//! window (`hand_off`), and a launch that only finds out while loading (`locked_pid`) shows a
//! read-only copy with a banner (`shell::banner`), never a second writer.
//!
//! macOS dedupes launches of one bundle itself; this covers two different builds (dist/ app and
//! target/release binary) on the same data dir.

/// The pid of another live process holding the store lock in `dir`, if any.
pub fn holder(dir: &std::path::Path) -> Option<u32> {
    proto_core::lock_holder(dir)
}

/// The pid in a `Store::open` error that says another process holds the data dir.
pub fn locked_pid(e: &anyhow::Error) -> Option<u32> {
    match e.downcast_ref::<proto_core::StoreError>() {
        Some(proto_core::StoreError::Locked { pid }) => Some(*pid),
        _ => None,
    }
}

/// In `main`, before the app starts: when another process holds the data dir and can be brought
/// forward, do that and return true (the caller exits without opening a window). Tooling runs
/// (headless) read a copy and never hand off.
pub fn hand_off() -> bool {
    if rune::is_headless() {
        return false;
    }
    let Some(pid) = holder(&proto_core::default_data_dir()) else { return false };
    // 0: the holder's pid wasn't readable yet; the load then finds the lock and opens read-only.
    if pid == 0 || pid == std::process::id() {
        return false;
    }
    let ok = activate(pid);
    eprintln!("prototypie: already running (pid {pid}); {}", if ok { "brought it forward" } else { "couldn't bring it forward, opening read-only" });
    ok
}

#[cfg(target_os = "macos")]
fn activate(pid: u32) -> bool {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    let Ok(pid) = i32::try_from(pid) else { return false };
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        .is_some_and(|app| app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows))
}

#[cfg(not(target_os = "macos"))]
fn activate(_pid: u32) -> bool {
    false
}
