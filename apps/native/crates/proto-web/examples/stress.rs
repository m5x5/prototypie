//! Memory stress for the thumbnail pipeline: drives `Snapshots` (cache, LRU, worker, pool) like a
//! canvas that pans across many designs, and prints the cache's entries and decoded bytes, the live
//! web views, the queue, this process's RSS and the WebKit helper processes' RSS. Headless like
//! `snap` (no Dock icon, no window on screen).
//! `cargo run --release -p proto-web --example stress -- [designs=500] [visible=24] [every=50]`
//! Pass 1 pans at out_w 756, pass 2 at 378, pass 3 at 756 again (re-renders what was evicted).

use std::sync::Arc;
use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
use proto_web::{SnapshotLimits, Snapshots, Thumb, ThumbSpec};

/// A distinct small design: colour, layout and text vary with `i`.
fn design(i: usize) -> String {
    let hue = (i * 37) % 360;
    let cols = 1 + i % 4;
    let cards: String = (0..(3 + i % 9)).map(|k| format!("<div class=c><h3>Card {k} of design {i}</h3><p>Lorem ipsum dolor sit amet {i}-{k}.</p></div>")).collect();
    format!(
        r#"<!doctype html><html><head><style>
        body {{ margin: 0; font: 15px -apple-system, sans-serif; background: hsl({hue} 40% 96%); }}
        header {{ padding: 28px 40px; background: hsl({hue} 60% 45%); color: white; font-size: 28px; font-weight: 700; }}
        main {{ display: grid; grid-template-columns: repeat({cols}, 1fr); gap: 16px; padding: 32px 40px; }}
        .c {{ background: white; border-radius: 12px; padding: 16px; box-shadow: 0 4px 16px hsl({hue} 30% 50% / .2); }}
        </style></head><body><header>Design #{i}</header><main>{cards}</main></body></html>"#
    )
}

fn kb(cmd: &str) -> (usize, usize) {
    // (processes, total RSS KiB) from `ps`, for lines matching `cmd`.
    let out = std::process::Command::new("ps").args(["-axo", "rss=,comm="]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    out.lines()
        .filter(|l| l.contains(cmd))
        .filter_map(|l| l.split_whitespace().next().and_then(|n| n.parse::<usize>().ok()))
        .fold((0, 0), |(n, sum), r| (n + 1, sum + r))
}

fn self_rss_kb() -> usize {
    let pid = std::process::id().to_string();
    std::process::Command::new("ps").args(["-o", "rss=", "-p", &pid]).output().ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok()).unwrap_or(0)
}

/// This process's physical footprint in KiB (`footprint`: includes compressed memory, which RSS
/// leaves out; the decoded pictures compress well, so RSS under-reports them).
fn self_footprint_kb() -> usize {
    let pid = std::process::id().to_string();
    let out = std::process::Command::new("footprint").arg(&pid).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let Some(rest) = out.split("Footprint: ").nth(1) else { return 0 };
    let mut it = rest.split_whitespace();
    let (Some(n), Some(unit)) = (it.next().and_then(|n| n.parse::<f64>().ok()), it.next()) else { return 0 };
    (n * match unit {
        "GB" => 1024.0 * 1024.0,
        "MB" => 1024.0,
        "KB" => 1.0,
        _ => 1.0 / 1024.0,
    }) as usize
}

fn main() {
    let args: Vec<usize> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let (n, visible, every) = (args.first().copied().unwrap_or(500), args.get(1).copied().unwrap_or(24), args.get(2).copied().unwrap_or(50));
    let mtm = MainThreadMarker::new().expect("main thread");
    NSApplication::sharedApplication(mtm).setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let run_loop = NSRunLoop::currentRunLoop();
    let spin = |ms: u64| {
        let until = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < until {
            unsafe { run_loop.runMode_beforeDate(NSDefaultRunLoopMode, &NSDate::dateWithTimeIntervalSinceNow(0.005)) };
        }
    };
    let htmls: Vec<String> = (0..n).map(design).collect();
    // WebKit helpers already running (Safari, other apps): reported as a baseline.
    let (base_wc, base_wc_kb) = kb("com.apple.WebKit.WebContent");
    let (base_any, base_any_kb) = kb("com.apple.WebKit");
    println!("baseline: WebContent {base_wc} procs {} MiB, all WebKit helpers {base_any} procs {} MiB", base_wc_kb / 1024, base_any_kb / 1024);
    println!("limits: {:?}", SnapshotLimits::default());

    let mut snaps = Snapshots::new(Arc::new(|| {}), SnapshotLimits::default());
    let t0 = Instant::now();
    let mut requests = 0usize;
    let mut peak = (0usize, 0usize, 0usize);
    let report = |label: &str, snaps: &Snapshots, requests: usize, peak: &mut (usize, usize, usize)| {
        let (bytes, entries) = snaps.usage();
        let (views, queued) = snaps.pool();
        let rss = self_rss_kb();
        let fp = self_footprint_kb();
        let (wc, wc_kb) = kb("com.apple.WebKit.WebContent");
        let (any, any_kb) = kb("com.apple.WebKit");
        *peak = (peak.0.max(fp), peak.1.max(bytes), peak.2.max(wc_kb.saturating_sub(base_wc_kb)));
        println!(
            "{label:>8} t={:>5.1}s req={requests:>5} entries={entries:>3} decoded={:>4} MiB views={views} queued={queued:>3} | footprint={:>4} MiB rss={:>4} MiB | WebContent +{} procs +{} MiB | WebKit +{} procs +{} MiB",
            t0.elapsed().as_secs_f32(), bytes >> 20, fp / 1024, rss / 1024, wc.saturating_sub(base_wc), wc_kb.saturating_sub(base_wc_kb) / 1024, any.saturating_sub(base_any), any_kb.saturating_sub(base_any_kb) / 1024,
        );
    };
    for (pass, out_w) in [(1, 756u32), (2, 378), (3, 756)] {
        // Pan: the visible window moves a page forward once all of it is Ready (or failed).
        let mut first = 0usize;
        let mut last_report = 0usize;
        while first < n {
            let mut all_ready = true;
            for (i, html) in htmls.iter().enumerate().skip(first).take(visible) {
                requests += 1;
                let spec = ThumbSpec { viewport_w: 1280, viewport_h: None, out_w };
                match snaps.request(&format!("d{i}"), 1, html, spec) {
                    Thumb::Ready(s) if s.pixels.size().0 >= out_w * 4 / 5 => {}
                    Thumb::Failed(_) => {}
                    _ => all_ready = false,
                }
            }
            snaps.poll();
            spin(16);
            if all_ready {
                first += visible;
            }
            if requests - last_report >= every * visible {
                last_report = requests;
                report(&format!("pass {pass}"), &snaps, requests, &mut peak);
            }
        }
        report(&format!("end {pass}"), &snaps, requests, &mut peak);
        if pass == 1 {
            // Zoom out and rest: shrink every cached picture to a 378 px ask, without WebKit.
            let t = Instant::now();
            let started = (0..n).filter(|i| snaps.reduce(&format!("d{i}"), 378)).count();
            let copy = t.elapsed();
            let mut last = usize::MAX;
            while snaps.usage().0 != last {
                last = snaps.usage().0;
                snaps.poll();
                spin(100);
            }
            println!("reduce: {started} started (copy-out on the main thread {copy:?} total), settled after {:?}", t.elapsed());
            let sizes: Vec<_> = (0..3).filter_map(|i| match snaps.request(&format!("d{i}"), 1, &htmls[i], ThumbSpec { viewport_w: 1280, viewport_h: None, out_w: 378 }) {
                Thumb::Ready(s) => Some((s.pixels.size(), s.serial)),
                _ => None,
            }).collect();
            println!("reduce: first pictures now {sizes:?}; pending renders after a 378 ask: {:?}", snaps.pool());
            report("reduced", &snaps, requests, &mut peak);
        }
    }
    snaps.clear();
    spin(500);
    report("cleared", &snaps, requests, &mut peak);
    println!("peaks: footprint {} MiB, decoded {} MiB, WebContent +{} MiB", peak.0 / 1024, peak.1 >> 20, peak.2 / 1024);
}
