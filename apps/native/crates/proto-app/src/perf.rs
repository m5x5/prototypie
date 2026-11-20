//! Launch timing marks (proto-worker-7). With `PROTOTYPIE_LAUNCH_LOG=1`, `mark("name")` prints
//! `prototypie-launch {"mark":"name","main_ms":…,"epoch_ms":…}` to stderr the first time each name is
//! marked: ms since `main` started and the wall clock, so a harness that noted the spawn time gets
//! process-relative times. Off (the default), a mark is one atomic load: no allocation, no lock.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static ON: AtomicBool = AtomicBool::new(false);
static START: OnceLock<Instant> = OnceLock::new();
static SEEN: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// First thing in `main`: reads the switch and starts the clock.
pub fn start() {
    let on = std::env::var_os("PROTOTYPIE_LAUNCH_LOG").is_some_and(|v| !v.is_empty() && v != "0");
    ON.store(on, Ordering::Relaxed);
    if on {
        START.get_or_init(Instant::now);
        mark("main");
    }
}

/// Note that the launch reached `name` (only the first time per name).
pub fn mark(name: &'static str) {
    if !ON.load(Ordering::Relaxed) {
        return;
    }
    {
        let mut seen = SEEN.lock().unwrap();
        if seen.contains(&name) {
            return;
        }
        seen.push(name);
    }
    let main_ms = START.get().map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
    let epoch_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    eprintln!("prototypie-launch {{\"mark\":\"{name}\",\"main_ms\":{main_ms:.1},\"epoch_ms\":{epoch_ms:.1}}}");
}
