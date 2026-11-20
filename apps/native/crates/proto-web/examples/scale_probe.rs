//! Headless check of small live views: below `SCALE_BELOW` the web view lays out at full size
//! with pageZoom 1 and its container scales it into the rect. Checks the layout width, that text
//! keeps its designed size (no 9pt floor), and that a real mouse click through the window lands
//! on the right element in both modes. `cargo run -p proto-web --example scale_probe`

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEvent, NSEventModifierFlags, NSEventType, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSRunLoop, NSSize};
use proto_web::{LiveOptions, LivePage, LiveView};

const PAGE: &str = r#"<!doctype html><html><head><style>
  body { margin: 0; font: 16px -apple-system, sans-serif; }
  .cta { position: absolute; left: 300px; top: 200px; width: 160px; height: 40px; font-size: 15px; }
  small { font-size: 11px; }
</style></head><body><small id="s">tiny caption</small><button id="go" class="cta">Start trial</button>
<script>window.__hits = []; addEventListener("click", (e) => __hits.push([e.target.id, e.clientX, e.clientY]), true);</script></body></html>"#;

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

fn eval(live: &LiveView, js: &str) -> serde_json::Value {
    let out = Rc::new(RefCell::new(None));
    let o = out.clone();
    live.eval(js, move |r| *o.borrow_mut() = Some(r));
    spin_until(|| out.borrow().is_some(), Duration::from_secs(5));
    out.borrow_mut().take().and_then(Result::ok).unwrap_or_default()
}

/// A real left click at window point `p` (top-left origin, like Rune's rects).
fn click(window: &NSWindow, p: [f64; 2]) {
    let h = window.contentView().map(|v| v.bounds().size.height).unwrap_or(0.0);
    let at = NSPoint::new(p[0], h - p[1]);
    for ty in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp] {
        let ev = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            ty, at, NSEventModifierFlags::empty(), 0.0, window.windowNumber(), None, 0, 1, 1.0,
        );
        if let Some(ev) = ev {
            window.sendEvent(&ev);
        }
        spin_until(|| false, Duration::from_millis(30));
    }
}

fn main() {
    let mtm = MainThreadMarker::new().expect("main thread");
    NSApplication::sharedApplication(mtm).setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let frame = NSRect::new(NSPoint::new(-30000.0, -30000.0), NSSize::new(1000.0, 700.0));
    let window = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(NSWindow::alloc(mtm), frame, NSWindowStyleMask::Borderless, NSBackingStoreType::Buffered, false) };
    unsafe { window.setReleasedWhenClosed(false) };
    let live = LiveView::attach_view(window.contentView().expect("content view"), Arc::new(|| {}));
    let page = LivePage { key: "scale", rev: 1, html: PAGE, viewport_w: 800, viewport_h: None };
    for (name, rect) in [("scaled 0.5", [20.0f32, 30.0, 400.0, 300.0]), ("pageZoom 0.95", [20.0, 30.0, 760.0, 570.0])] {
        live.show(rect, &page, &LiveOptions::default());
        spin_until(|| live.is_loaded(), Duration::from_secs(10));
        spin_until(|| false, Duration::from_millis(150));
        let facts = eval(&live, "__hits = []; return [innerWidth, getComputedStyle(go).fontSize, getComputedStyle(s).fontSize]");
        // The button's centre, page px (300 + 80, 200 + 20) → window points.
        let z = (rect[2] / 800.0) as f64;
        let p = [rect[0] as f64 + 380.0 * z, rect[1] as f64 + 220.0 * z];
        click(&window, p);
        let hits = eval(&live, "return __hits");
        println!("{name}: innerWidth, button, caption font = {facts}; NSEvent click at window {p:?} → {hits}");
        // AppKit's own hit test and conversion at that point.
        let content = window.contentView().expect("content view");
        let at = NSPoint::new(p[0], content.bounds().size.height - p[1]);
        let hit = content.hitTest(at);
        let class = hit.as_ref().map(|v| v.class().name().to_str().unwrap_or("?").to_string());
        let web = hit.as_ref().and_then(|v| {
            let mut cur = Some(v.clone());
            while let Some(c) = cur {
                if c.class().name().to_str().is_ok_and(|n| n.ends_with("WKWebView")) {
                    return Some(c);
                }
                // SAFETY: walking up a live view hierarchy on the main thread.
                cur = unsafe { c.superview() };
            }
            None
        });
        let local = web.as_ref().map(|w| w.convertPoint_fromView(at, Some(&content)));
        let flipped = web.as_ref().map(|w| w.isFlipped());
        println!("  hitTest → {class:?}; in WKWebView points {:?} (flipped {flipped:?}), expect ({:.0}, {:.0})", local.map(|l| (l.x, l.y)), 380.0 * z / (if z < 0.9 { z } else { 1.0 }), 220.0 * z / (if z < 0.9 { z } else { 1.0 }));
    }
    live.hide();
}
