//! Render one design's html to a PNG headlessly (no Dock icon, no window on screen), through the
//! same thumbnail pipeline `Snapshots` uses: preview CSS, measure + content crop, capture plan.
//! `cargo run -p proto-web --example snap -- in.html out.png [viewport_w] [out_w] [viewport_h]`
//! `REPEAT=n` renders n times in one process and prints each time (cold vs warm).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
use proto_web::mac::render::{Job, RawSnap, Renderer};
use proto_web::mac::{image, js};
use proto_web::spec::{self, ThumbSpec};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (input, output) = (args.get(1).expect("in.html"), args.get(2).expect("out.png"));
    let viewport_w: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1280);
    let out_w: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(760);
    let viewport_h: Option<u32> = args.get(5).and_then(|s| s.parse().ok());
    let repeat: usize = std::env::var("REPEAT").ok().and_then(|s| s.parse().ok()).unwrap_or(1).max(1);
    let html = std::fs::read_to_string(input)?;

    let mtm = MainThreadMarker::new().expect("main thread");
    // No Dock icon, never activates: nothing steals focus.
    NSApplication::sharedApplication(mtm).setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let run_loop = NSRunLoop::currentRunLoop();
    if std::env::var_os("PREWARM").is_some() {
        let t = Instant::now();
        proto_web::prewarm();
        println!("prewarm() on the main thread: {:?}", t.elapsed());
        // The app's first frames run meanwhile; give the content process a moment to start.
        let until = Instant::now() + std::time::Duration::from_millis(800);
        while Instant::now() < until {
            unsafe { run_loop.runMode_beforeDate(NSDefaultRunLoopMode, &NSDate::dateWithTimeIntervalSinceNow(0.02)) };
        }
    }
    let t = Instant::now();
    let renderer = Renderer::new(mtm, 1);
    println!("Renderer::new: {:?}", t.elapsed());
    let spec = ThumbSpec { viewport_w, viewport_h, out_w };

    let mut last: Option<RawSnap> = None;
    for i in 0..repeat {
        let t = Instant::now();
        let result: Rc<RefCell<Option<Result<RawSnap, String>>>> = Rc::new(RefCell::new(None));
        let slot = result.clone();
        renderer.submit(Job {
            tag: "snap".into(),
            html: spec::for_preview(&html, spec.phone()),
            base_url: Some(spec::default_asset_base()),
            viewport_w,
            layout_h: spec.layout_h(),
            measure_js: js::measure(viewport_w, spec.crops(), js::SETTLE_MS),
            plan: Box::new(move |m| spec::plan(&spec, m)),
            done: Box::new(move |r| *slot.borrow_mut() = Some(r)),
        });
        while result.borrow().is_none() {
            unsafe { run_loop.runMode_beforeDate(NSDefaultRunLoopMode, &NSDate::dateWithTimeIntervalSinceNow(0.02)) };
        }
        let raw = result.borrow_mut().take().unwrap().map_err(anyhow::Error::msg)?;
        println!("render {}: {:?} plan {:?} doc {}x{}", i + 1, t.elapsed(), raw.plan, raw.measured.doc_w, raw.measured.doc_h);
        last = Some(raw);
    }

    let raw = last.unwrap();
    let t = Instant::now();
    let rgba = image::to_rgba(&raw.image, raw.plan.out_w, raw.plan.out_h).expect("rgba");
    println!("rgba conversion {:?} (cg {:?})", t.elapsed(), image::size(&raw.image));
    let file = std::io::BufWriter::new(std::fs::File::create(output)?);
    let mut enc = png::Encoder::new(file, raw.plan.out_w, raw.plan.out_h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgba)?;
    println!("wrote {output}");
    Ok(())
}
