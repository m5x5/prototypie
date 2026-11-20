//! Headless LiveView probe: an offscreen window (never shown) hosts a LiveView; checks that the
//! page loads, lays out at `viewport_w` under the page zoom, and that eval works.
//! `cargo run -p proto-web --example live_probe -- in.html [viewport_w]`

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSRunLoop, NSSize};
use proto_web::{LiveOptions, LivePage, LiveView};

fn spin_until(mut done: impl FnMut() -> bool, limit: Duration) -> bool {
    let run_loop = NSRunLoop::currentRunLoop();
    let t = Instant::now();
    while !done() {
        if t.elapsed() > limit {
            return false;
        }
        unsafe { run_loop.runMode_beforeDate(NSDefaultRunLoopMode, &NSDate::dateWithTimeIntervalSinceNow(0.02)) };
    }
    true
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let html = std::fs::read_to_string(args.get(1).expect("in.html"))?;
    let vw: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1280);
    let mtm = MainThreadMarker::new().expect("main thread");
    NSApplication::sharedApplication(mtm).setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let rect = NSRect::new(NSPoint::new(-30000.0, -30000.0), NSSize::new(1000.0, 700.0));
    let window = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(NSWindow::alloc(mtm), rect, NSWindowStyleMask::Borderless, NSBackingStoreType::Buffered, false) };
    unsafe { window.setReleasedWhenClosed(false) };
    let host = window.contentView().expect("content view");

    let live = LiveView::attach_view(host, Arc::new(|| {}));
    let page = LivePage { key: "probe", rev: 1, html: &html, viewport_w: vw, viewport_h: None };
    let t = Instant::now();
    live.prepare(&page, &LiveOptions::default());
    let loaded = spin_until(|| live.is_loaded(), Duration::from_secs(15));
    println!("loaded={loaded} in {:?}", t.elapsed());
    // Box 640 wide: the page must still lay out at vw css px.
    let t = Instant::now();
    live.show([20.0, 30.0, 640.0, 480.0], &page, &LiveOptions::default());
    live.show([20.0, 30.0, 640.0, 480.0], &page, &LiveOptions::default());
    println!("show x2 (no reload) {:?}, shown={}", t.elapsed(), live.is_shown());
    let out = Rc::new(RefCell::new(None));
    let o = out.clone();
    live.eval("return { innerWidth, innerHeight, title: document.title }", move |r| *o.borrow_mut() = Some(r));
    spin_until(|| out.borrow().is_some(), Duration::from_secs(5));
    println!("eval: {:?}", out.borrow());
    live.move_to([0.0, 0.0, 320.0, 240.0]);
    let out = Rc::new(RefCell::new(None));
    let o = out.clone();
    live.eval("return innerWidth", move |r| *o.borrow_mut() = Some(r));
    spin_until(|| out.borrow().is_some(), Duration::from_secs(5));
    println!("after move_to 320: innerWidth {:?}", out.borrow());
    let t = Instant::now();
    live.hide();
    println!("hide {:?}, shown={}, keys={:?}", t.elapsed(), live.is_shown(), live.take_keys());
    Ok(())
}
