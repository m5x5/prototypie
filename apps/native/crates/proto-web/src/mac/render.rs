//! Main-thread WKWebView renderer: a small pool of offscreen web views in a borderless window
//! that never shows. One job = load html → wait for navigation → run the job's measure script →
//! the job's plan picks the capture rect and size → `takeSnapshotWithConfiguration`. Each slot is an explicit state machine; a timeout fails a
//! job that never finishes. The CGImage → RGBA work is left to the caller (a worker thread).

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSBackingStoreType, NSImage, NSWindow, NSWindowStyleMask};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;
use objc2_foundation::{NSError, NSNumber, NSURL, NSPoint, NSRect, NSSize, NSString};
use objc2_web_kit::{WKContentWorld, WKSnapshotConfiguration, WKWebView, WKWebViewConfiguration};

use super::nav::{NavDelegate, NavEvent, NavId, nav_id};
use crate::spec::{Measured, Plan};

/// A load that hasn't finished by then (a stalled subresource) is measured and captured as it is.
const LOAD_TIMEOUT: Duration = Duration::from_secs(5);
/// A measure script that hasn't answered by then (web fonts never ready) captures unmeasured.
const MEASURE_TIMEOUT: Duration = Duration::from_secs(3);
/// A snapshot WebKit never delivers fails the job.
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(4);

/// A snapshot as WebKit returned it, before conversion to RGBA (at `plan.out_w` x `plan.out_h`).
pub struct RawSnap {
    pub image: CFRetained<CGImage>,
    pub plan: Plan,
    pub measured: Measured,
}
// SAFETY: CGImage is immutable and documented thread safe; the RGBA conversion reads it on a worker.
unsafe impl Send for RawSnap {}

pub type Done = Box<dyn FnOnce(Result<RawSnap, String>)>;

pub struct Job {
    /// Groups jobs of one subject (a design id) for `retain_queued`.
    pub tag: String,
    pub html: String,
    /// Base URL for the page's relative URLs (the app's asset origin, e.g. `/vendor/...`).
    pub base_url: Option<String>,
    /// Layout viewport in CSS px while loading and measuring.
    pub viewport_w: u32,
    pub layout_h: u32,
    /// Async function body run after navigation; returns a JSON string for `js::parse_measured`.
    pub measure_js: String,
    /// Turns the measurements into the capture rect and pixel size.
    pub plan: Box<dyn FnOnce(&Measured) -> Plan>,
    pub done: Done,
}

enum SlotState {
    Idle,
    Loading(Job),
    Measuring(Job),
    Snapshotting(Job),
}

struct Slot {
    /// Created on first use (a prewarmed view for slot 0), so launching renders nothing yet.
    web: OnceCell<(Retained<WKWebView>, Retained<NavDelegate>)>,
    /// The navigation the current job's load started; events of any other are ignored.
    nav: Cell<NavId>,
    state: RefCell<SlotState>,
    /// Bumped per job, so a late callback or timeout of an earlier job changes nothing.
    attempt: Cell<u64>,
    /// Bumped per phase, so a phase timeout fires only if that phase is still current.
    step: Cell<u64>,
}

struct Inner {
    mtm: MainThreadMarker,
    window: Retained<NSWindow>,
    config: Retained<WKWebViewConfiguration>,
    weak: Weak<Inner>,
    slots: Vec<Slot>,
    queue: RefCell<VecDeque<Job>>,
}

/// A web view started ahead of the first render (`prewarm`), with its window and configuration;
/// `view` is None until prewarm's second step.
struct Warm {
    window: Retained<NSWindow>,
    config: Retained<WKWebViewConfiguration>,
    view: Option<Retained<WKWebView>>,
}

thread_local! {
    static WARM: RefCell<Option<Warm>> = const { RefCell::new(None) };
}

/// Starts WebKit's processes ahead of the first snapshot: an offscreen web view loads an empty
/// page, and the first `Renderer` adopts it. Call once on the main thread after the app's first
/// frame. The work (window ~20 ms, web view ~40 ms, the content process launch ~50 ms in debug)
/// runs in three main-queue turns, so frames can draw in between. A no-op off the main thread or
/// when already warm; a render that starts meanwhile takes over whatever is ready.
pub fn prewarm() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    if WARM.with(|w| w.borrow().is_some()) {
        return;
    }
    let window = offscreen_window(mtm);
    let config = unsafe { WKWebViewConfiguration::new(mtm) };
    WARM.with(|w| *w.borrow_mut() = Some(Warm { window, config, view: None }));
    DispatchQueue::main().exec_async(|| {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let made = WARM.with(|w| match w.borrow_mut().as_mut() {
            Some(w) if w.view.is_none() => {
                w.view = Some(new_view(mtm, &w.config, &w.window));
                true
            }
            _ => false,
        });
        if made {
            DispatchQueue::main().exec_async(|| {
                WARM.with(|w| {
                    if let Some(Warm { view: Some(v), .. }) = &*w.borrow() {
                        unsafe { v.loadHTMLString_baseURL(&NSString::from_str(""), None) };
                    }
                })
            });
        }
    });
}

fn new_view(mtm: MainThreadMarker, config: &WKWebViewConfiguration, window: &NSWindow) -> Retained<WKWebView> {
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1280.0, 960.0));
    let view = unsafe { WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, config) };
    // A transparent page (a phone design without its page background) must stay transparent in the snapshot, so the
    // card behind it shows through in light and dark; WebKit paints white under it by default.
    unsafe {
        let off = objc2_foundation::NSNumber::new_bool(false);
        let key = objc2_foundation::NSString::from_str("drawsBackground");
        let _: () = objc2::msg_send![&*view, setValue: &*off, forKey: &*key];
    }
    if let Some(content) = window.contentView() {
        content.addSubview(&view);
    }
    view
}

/// Slot `i`'s web view, created (or the prewarmed one adopted) on first use.
fn view(inner: &Inner, i: usize) -> &WKWebView {
    &inner.slots[i].web.get_or_init(|| {
        // Slot 0 adopts the prewarmed view; taking it also stops prewarm's pending steps.
        let warm = if i == 0 { WARM.with(|w| w.borrow_mut().take()).and_then(|w| w.view) } else { None };
        let view = warm.unwrap_or_else(|| new_view(inner.mtm, &inner.config, &inner.window));
        let weak = inner.weak.clone();
        let delegate = NavDelegate::new(inner.mtm, move |event, id| {
            if let Some(inner) = weak.upgrade() {
                on_nav(&inner, i, event, id);
            }
        });
        unsafe { view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
        (view, delegate)
    }).0
}

thread_local! {
    static CURRENT: RefCell<Weak<Inner>> = const { RefCell::new(Weak::new()) };
}

/// Main-thread snapshot renderer. Create once; clone freely (shared pool).
#[derive(Clone)]
pub struct Renderer {
    inner: Rc<Inner>,
}

impl Renderer {
    /// `slots` web views render in parallel (each has its own web content process). Cheap: the
    /// views are made on first use; a `prewarm`ed view and its window are adopted.
    pub fn new(mtm: MainThreadMarker, slots: usize) -> Renderer {
        // Keep the prewarmed window and configuration so the adopted view stays in its window.
        let (window, config) = WARM.with(|w| w.borrow().as_ref().map(|w| (w.window.clone(), w.config.clone())))
            .unwrap_or_else(|| (offscreen_window(mtm), unsafe { WKWebViewConfiguration::new(mtm) }));
        let inner = Rc::new_cyclic(|weak: &Weak<Inner>| Inner {
            mtm,
            window,
            config,
            weak: weak.clone(),
            slots: (0..slots.max(1))
                .map(|_| Slot { web: OnceCell::new(), nav: Cell::new(0), state: RefCell::new(SlotState::Idle), attempt: Cell::new(0), step: Cell::new(0) })
                .collect(),
            queue: RefCell::new(VecDeque::new()),
        });
        CURRENT.with(|c| *c.borrow_mut() = Rc::downgrade(&inner));
        Renderer { inner }
    }

    /// Queues a job; it starts as soon as a slot is free.
    pub fn submit(&self, job: Job) {
        self.inner.queue.borrow_mut().push_back(job);
        pump(&self.inner);
    }

    /// Jobs waiting for a slot (not counting the ones rendering).
    pub fn queued(&self) -> usize {
        self.inner.queue.borrow().len()
    }

    /// Drops queued jobs for which `keep` is false (their `done` is not called).
    pub fn retain_queued(&self, mut keep: impl FnMut(&Job) -> bool) {
        self.inner.queue.borrow_mut().retain(|j| keep(j));
    }

    /// Fails the job rendering for `tag` right away (a newer request replaced it).
    pub fn cancel_running(&self, tag: &str) {
        let hit = self.inner.slots.iter().position(|s| match &*s.state.borrow() {
            SlotState::Loading(j) | SlotState::Measuring(j) | SlotState::Snapshotting(j) => j.tag == tag,
            SlotState::Idle => false,
        });
        if let Some(i) = hit {
            fail(&self.inner, i, "replaced".into());
        }
    }

    /// Web views made so far (slots are filled on first use).
    pub fn views(&self) -> usize {
        self.inner.slots.iter().filter(|s| s.web.get().is_some()).count()
    }

    /// True while any slot renders or jobs wait.
    pub fn busy(&self) -> bool {
        !self.inner.queue.borrow().is_empty() || self.inner.slots.iter().any(|s| !matches!(*s.state.borrow(), SlotState::Idle))
    }
}

fn offscreen_window(mtm: MainThreadMarker) -> Retained<NSWindow> {
    // Far off every display and never ordered in: nothing shows, nothing takes focus.
    let rect = NSRect::new(NSPoint::new(-30000.0, -30000.0), NSSize::new(1280.0, 960.0));
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(NSWindow::alloc(mtm), rect, NSWindowStyleMask::Borderless, NSBackingStoreType::Buffered, false)
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window
}

fn pump(inner: &Rc<Inner>) {
    for (i, slot) in inner.slots.iter().enumerate() {
        if !matches!(*slot.state.borrow(), SlotState::Idle) {
            continue;
        }
        let Some(job) = inner.queue.borrow_mut().pop_front() else { return };
        start(inner, i, job);
    }
}

fn start(inner: &Rc<Inner>, i: usize, job: Job) {
    let slot = &inner.slots[i];
    let attempt = slot.attempt.get() + 1;
    slot.attempt.set(attempt);
    let web = view(inner, i);
    web.setFrameSize(NSSize::new(job.viewport_w as f64, job.layout_h as f64));
    let html = NSString::from_str(&job.html);
    let base = job.base_url.as_deref().and_then(|u| NSURL::URLWithString(&NSString::from_str(u)));
    *slot.state.borrow_mut() = SlotState::Loading(job);
    let nav = unsafe { web.loadHTMLString_baseURL(&html, base.as_deref()) };
    slot.nav.set(nav_id(nav.as_deref()));
    arm(inner, i, LOAD_TIMEOUT);
}

fn trace(i: usize, what: &str) {
    if std::env::var_os("PROTO_WEB_TRACE").is_some() {
        eprintln!("[proto-web] slot {i}: {what}");
    }
}

/// Starts a new phase for slot `i` and schedules its timeout.
fn arm(inner: &Rc<Inner>, i: usize, after: Duration) {
    let slot = &inner.slots[i];
    let step = slot.step.get() + 1;
    slot.step.set(step);
    trace(i, &format!("arm step {step} for {after:?}"));
    let when = DispatchTime::try_from(after).unwrap_or(DispatchTime::FOREVER);
    let _ = DispatchQueue::main().after(when, move || {
        let inner = CURRENT.with(|c| c.borrow().upgrade());
        if let Some(inner) = inner
            && inner.slots[i].step.get() == step
        {
            on_timeout(&inner, i);
        }
    });
}

/// A phase ran out of time: a stalled load is measured anyway, an unanswered measure captures
/// with defaults, a missing snapshot fails.
fn on_timeout(inner: &Rc<Inner>, i: usize) {
    trace(i, "timeout");
    let slot = &inner.slots[i];
    let phase = match &*slot.state.borrow() {
        SlotState::Idle => return,
        SlotState::Loading(_) => 0,
        SlotState::Measuring(_) => 1,
        SlotState::Snapshotting(_) => 2,
    };
    match phase {
        0 => {
            unsafe { view(inner, i).stopLoading() };
            on_nav(inner, i, NavEvent::Finished, slot.nav.get());
        }
        1 => snapshot(inner, i, Measured::default()),
        _ => fail(inner, i, "snapshot timed out".into()),
    }
}

/// Ends the slot's job with an error and moves on.
fn fail(inner: &Rc<Inner>, i: usize, why: String) {
    let slot = &inner.slots[i];
    let prev = std::mem::replace(&mut *slot.state.borrow_mut(), SlotState::Idle);
    slot.attempt.set(slot.attempt.get() + 1);
    slot.step.set(slot.step.get() + 1);
    let job = match prev {
        SlotState::Idle => return,
        SlotState::Loading(j) | SlotState::Measuring(j) | SlotState::Snapshotting(j) => j,
    };
    unsafe { view(inner, i).stopLoading() };
    (job.done)(Err(why));
    pump(inner);
}

fn on_nav(inner: &Rc<Inner>, i: usize, event: NavEvent, id: NavId) {
    trace(i, &format!("nav {event:?}"));
    // An earlier load's event (the prewarm's about:blank, a replaced job): not this job's.
    // Termination concerns whatever runs (id 0).
    if id != 0 && id != inner.slots[i].nav.get() {
        return;
    }
    let slot = &inner.slots[i];
    match event {
        NavEvent::Finished => {
            let prev = std::mem::replace(&mut *slot.state.borrow_mut(), SlotState::Idle);
            let SlotState::Loading(job) = prev else {
                *slot.state.borrow_mut() = prev;
                return;
            };
            *slot.state.borrow_mut() = SlotState::Measuring(job);
            arm(inner, i, MEASURE_TIMEOUT);
            measure(inner, i);
        }
        // Only a failure while loading ends the job: stopping a stalled load (timeout) reports a
        // cancelled navigation after the job already moved on to measuring.
        NavEvent::Failed(why) if matches!(*slot.state.borrow(), SlotState::Loading(_)) => fail(inner, i, why),
        NavEvent::Failed(_) => {}
        NavEvent::Terminated => fail(inner, i, "web content process terminated".into()),
    }
}

fn measure(inner: &Rc<Inner>, i: usize) {
    let slot = &inner.slots[i];
    let attempt = slot.attempt.get();
    let script = match &*slot.state.borrow() {
        SlotState::Measuring(job) => NSString::from_str(&job.measure_js),
        _ => return,
    };
    let weak = Rc::downgrade(inner);
    let block = RcBlock::new(move |result: *mut AnyObject, _err: *mut NSError| {
        let Some(inner) = weak.upgrade() else { return };
        if inner.slots[i].attempt.get() != attempt {
            return;
        }
        // SAFETY: the script returns a JSON string; anything else (or an error) measures nothing.
        let json = unsafe { result.as_ref() }
            .and_then(|o| o.downcast_ref::<NSString>())
            .map(|s| s.to_string());
        if !matches!(&*inner.slots[i].state.borrow(), SlotState::Measuring(_)) {
            return;
        }
        let measured = json.as_deref().and_then(super::js::parse_measured).unwrap_or_default();
        snapshot(&inner, i, measured);
    });
    let world = unsafe { WKContentWorld::pageWorld(inner.mtm) };
    unsafe { view(inner, i).callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(&script, None, None, &world, Some(&block)) };
}

fn snapshot(inner: &Rc<Inner>, i: usize, measured: Measured) {
    trace(i, &format!("snapshot {measured:?}"));
    let slot = &inner.slots[i];
    let prev = std::mem::replace(&mut *slot.state.borrow_mut(), SlotState::Idle);
    let SlotState::Measuring(mut job) = prev else {
        *slot.state.borrow_mut() = prev;
        return;
    };
    let plan_fn = std::mem::replace(&mut job.plan, Box::new(|_| unreachable!("plan taken")));
    let plan = plan_fn(&measured);
    let [x, y, w, h] = plan.rect;
    // The view must contain the rect; a crop of tall content can reach below the layout viewport.
    let frame_h = (job.layout_h as f64).max((y + h).ceil());
    if frame_h > job.layout_h as f64 {
        view(inner, i).setFrameSize(NSSize::new(job.viewport_w as f64, frame_h));
    }
    *slot.state.borrow_mut() = SlotState::Snapshotting(job);
    arm(inner, i, SNAPSHOT_TIMEOUT);

    let config = unsafe { WKSnapshotConfiguration::new(inner.mtm) };
    unsafe { config.setRect(NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))) };
    // snapshotWidth is in points; the image comes back at the window's backing scale.
    let scale = view(inner, i).window().map(|w| w.backingScaleFactor()).unwrap_or(2.0).max(1.0);
    let points = (plan.out_w as f64 / scale).max(1.0);
    unsafe { config.setSnapshotWidth(Some(&NSNumber::new_f64(points))) };

    let attempt = slot.attempt.get();
    let weak = Rc::downgrade(inner);
    let block = RcBlock::new(move |image: *mut NSImage, err: *mut NSError| {
        let Some(inner) = weak.upgrade() else { return };
        let slot = &inner.slots[i];
        if slot.attempt.get() != attempt {
            return;
        }
        // SAFETY: WebKit passes a valid image or null, and an error or null.
        let image = unsafe { image.as_ref() };
        let cg = image.and_then(|img| unsafe { img.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None) });
        let Some(cg) = cg else {
            let why = unsafe { err.as_ref() }.map(|e| e.localizedDescription().to_string()).unwrap_or_else(|| "no image".into());
            fail(&inner, i, why);
            return;
        };
        let prev = std::mem::replace(&mut *slot.state.borrow_mut(), SlotState::Idle);
        slot.attempt.set(attempt + 1);
        slot.step.set(slot.step.get() + 1);
        let SlotState::Snapshotting(job) = prev else { return };
        (job.done)(Ok(RawSnap { image: cg.into(), plan, measured }));
        pump(&inner);
    });
    unsafe { view(inner, i).takeSnapshotWithConfiguration_completionHandler(Some(&config), &block) };
}
