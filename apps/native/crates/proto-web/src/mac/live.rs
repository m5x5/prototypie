//! `LiveView`: a real, interactive WKWebView as a child of the Rune window's content NSView,
//! placed over a rect from Rune layout (docs/api-web.md). UI thread only.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ptr::NonNull;
use std::rc::{Rc, Weak};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSView};
use objc2_foundation::{NSError, NSPoint, NSRect, NSSize, NSString, NSURL};
use objc2_web_kit::{WKContentWorld, WKUserScript, WKUserScriptInjectionTime, WKWebView, WKWebViewConfiguration};
use rune::raw_window_handle::RawWindowHandle;

use super::annotate::{self, AnnotateEvent, Msg, WebPin};
use super::js;
use super::messages::{MessageHandler, body_json};
use super::nav::{NavDelegate, NavEvent};
use super::snapshots::Wake;

/// Keys a focused live view hands to the app instead of the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveKey {
    Escape,
    Left,
    Right,
}

/// The page a live view shows. `key` + `rev` identify it: `show` reloads only when they change.
#[derive(Clone, Copy, Debug)]
pub struct LivePage<'a> {
    pub key: &'a str,
    pub rev: u64,
    pub html: &'a str,
    pub viewport_w: u32,
    pub viewport_h: Option<u32>,
}

/// Extra page behaviour; changing it reloads on the next `show`/`prepare`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct LiveOptions {
    user_scripts: Vec<String>,
}

impl LiveOptions {
    /// A script injected at document end into the main frame on every load (WKUserScript).
    pub fn user_script(mut self, js: impl Into<String>) -> Self {
        self.user_scripts.push(js.into());
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nav {
    Empty,
    Loading,
    Loaded,
    Failed,
}

#[derive(PartialEq)]
struct Loaded {
    key: String,
    rev: u64,
    opts: LiveOptions,
}

const FOCUS_HANDLER: &str = "protoFocus";
const KEY_ESC: u16 = 53;
const KEY_LEFT: u16 = 123;
const KEY_RIGHT: u16 = 124;

struct Inner {
    mtm: MainThreadMarker,
    host: Retained<NSView>,
    /// Holds `view`; placed at the target rect, its bounds scale the view down (see `placement`).
    container: Retained<NSView>,
    view: Retained<WKWebView>,
    _nav: Retained<NavDelegate>,
    _messages: Retained<MessageHandler>,
    loaded: RefCell<Option<Loaded>>,
    nav: Cell<Nav>,
    /// Layout size of the current page: (viewport_w, viewport_h).
    layout: Cell<(u32, Option<u32>)>,
    rect: Cell<[f32; 4]>,
    shown: Cell<bool>,
    editable_focus: Cell<bool>,
    keys: RefCell<Vec<LiveKey>>,
    /// The page zoom `move_to` set (CSS px → window points).
    zoom: Cell<f32>,
    annotate: RefCell<annotate::State>,
    monitor: RefCell<Option<Retained<AnyObject>>>,
    handlers: RefCell<HashMap<String, Handler>>,
    base_url: RefCell<Option<String>>,
    wake: Wake,
}

type Handler = Box<dyn Fn(serde_json::Value)>;

/// A live WKWebView over a rect of a Rune window.
pub struct LiveView {
    inner: Rc<Inner>,
}

impl LiveView {
    /// Attaches to a Rune window (call from `App::on_window`; again after a recreate).
    /// `wake` fires when keys are queued or a page finished loading.
    pub fn attach(window: &rune::WindowHandle, wake: Wake) -> Result<LiveView, String> {
        let mtm = MainThreadMarker::new().ok_or("LiveView::attach must run on the main thread")?;
        let ptr = window.with_raw_handle(|h| match h.as_raw() {
            RawWindowHandle::AppKit(a) => Some(a.ns_view.as_ptr()),
            _ => None,
        })?;
        let ptr = ptr.ok_or("not an AppKit window")?;
        // SAFETY: winit's content view is alive while the window is; we retain it.
        let host = unsafe { Retained::retain(ptr.cast::<NSView>()) }.ok_or("no content view")?;
        Ok(LiveView { inner: Self::build(mtm, host, wake) })
    }

    /// Attaches to any NSView (headless probes and tests; apps use `attach`).
    #[doc(hidden)]
    pub fn attach_view(host: Retained<NSView>, wake: Wake) -> LiveView {
        let mtm = MainThreadMarker::new().expect("main thread");
        LiveView { inner: Self::build(mtm, host, wake) }
    }

    fn build(mtm: MainThreadMarker, host: Retained<NSView>, wake: Wake) -> Rc<Inner> {
        Rc::new_cyclic(|weak: &Weak<Inner>| {
            let config = unsafe { WKWebViewConfiguration::new(mtm) };
            let controller = unsafe { config.userContentController() };
            let w = weak.clone();
            let messages = MessageHandler::new(mtm, move |name, body| {
                if let Some(inner) = w.upgrade() {
                    on_message(&inner, name, body);
                }
            });
            for name in [FOCUS_HANDLER, annotate::HANDLER] {
                unsafe { controller.addScriptMessageHandler_name(ProtocolObject::from_ref(&*messages), &NSString::from_str(name)) };
            }
            let view = unsafe { WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), NSRect::ZERO, &config) };
            let w = weak.clone();
            let nav = NavDelegate::new(mtm, move |event, _| {
                if let Some(inner) = w.upgrade() {
                    inner.nav.set(match event {
                        NavEvent::Finished => Nav::Loaded,
                        NavEvent::Failed(_) | NavEvent::Terminated => Nav::Failed,
                    });
                    match event {
                        // The page is new: give it the annotate state.
                        NavEvent::Finished => apply_annotate(&inner),
                        // Reload on the next show.
                        NavEvent::Terminated => *inner.loaded.borrow_mut() = None,
                        NavEvent::Failed(_) => {}
                    }
                    (inner.wake)();
                }
            });
            unsafe { view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*nav))) };
            // The web view sits in a container whose bounds may be larger than its frame: AppKit
            // then scales the subtree (drawing, hit-testing, convertPoint) into the frame.
            let container = NSView::initWithFrame(NSView::alloc(mtm), NSRect::ZERO);
            container.setWantsLayer(true);
            container.setHidden(true);
            container.addSubview(&view);
            host.addSubview(&container);
            Inner {
                mtm,
                host,
                container,
                view,
                _nav: nav,
                _messages: messages,
                loaded: RefCell::new(None),
                nav: Cell::new(Nav::Empty),
                layout: Cell::new((1280, None)),
                rect: Cell::new([0.0; 4]),
                shown: Cell::new(false),
                editable_focus: Cell::new(false),
                keys: RefCell::new(Vec::new()),
                zoom: Cell::new(1.0),
                annotate: RefCell::new(annotate::State::default()),
                monitor: RefCell::new(None),
                handlers: RefCell::new(HashMap::new()),
                base_url: RefCell::new(Some(crate::spec::default_asset_base())),
                wake,
            }
        })
    }

    /// Base URL for relative URLs in pages loaded afterwards (default: `spec::default_asset_base()`).
    pub fn set_base_url(&self, base: Option<String>) {
        *self.inner.base_url.borrow_mut() = base;
    }

    /// Starts loading `page` while hidden (e.g. when a flight towards the live view starts).
    pub fn prepare(&self, page: &LivePage, opts: &LiveOptions) {
        load(&self.inner, page, opts);
    }

    /// Shows the page over `rect` (window logical px, top-left origin). Synchronous.
    pub fn show(&self, rect: [f32; 4], page: &LivePage, opts: &LiveOptions) {
        load(&self.inner, page, opts);
        self.move_to(rect);
        self.inner.container.setHidden(false);
        self.inner.shown.set(true);
        install_monitor(&self.inner);
    }

    /// Moves/resizes the view (and its zoom) to `rect`. Synchronous.
    pub fn move_to(&self, rect: [f32; 4]) {
        let inner = &self.inner;
        inner.rect.set(rect);
        let [x, y, w, h] = rect.map(|v| v as f64);
        let host_h = inner.host.bounds().size.height;
        let y = if inner.host.isFlipped() { y } else { host_h - y - h };
        let (vw, vh) = inner.layout.get();
        let Some(p) = placement(w, h, vw, vh) else { return };
        inner.container.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w.max(0.0), h.max(0.0))));
        let inner_size = NSSize::new(w / p.scale, h / p.scale);
        inner.container.setBoundsSize(inner_size);
        inner.view.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), inner_size));
        unsafe { inner.view.setPageZoom(p.page_zoom) };
        let zoom = p.zoom as f32;
        inner.zoom.set(zoom);
        // The overlay keeps its on-screen size: tell it the new zoom.
        let changed = {
            let mut a = inner.annotate.borrow_mut();
            let changed = (a.zoom - zoom).abs() > 1e-4;
            a.zoom = zoom;
            changed && a.active()
        };
        if changed {
            apply_annotate(inner);
        }
    }

    /// Hides the view (the page stays loaded) and gives keyboard focus back to the Rune view.
    pub fn hide(&self) {
        let inner = &self.inner;
        if let Some(window) = inner.view.window()
            && web_has_focus(inner)
        {
            window.makeFirstResponder(Some(&inner.host));
        }
        inner.container.setHidden(true);
        inner.shown.set(false);
        remove_monitor(inner);
    }

    pub fn is_shown(&self) -> bool {
        self.inner.shown.get()
    }

    /// Navigation of the current page finished.
    pub fn is_loaded(&self) -> bool {
        self.inner.nav.get() == Nav::Loaded
    }

    /// Esc/←/→ the view consumed since the last call (drain in `AppSpec::poll`).
    pub fn take_keys(&self) -> Vec<LiveKey> {
        std::mem::take(&mut self.inner.keys.borrow_mut())
    }

    /// Runs `js` as an async function body in the page; `done` gets its JSON-serialised result.
    pub fn eval(&self, js: &str, done: impl FnOnce(Result<serde_json::Value, String>) + 'static) {
        eval_in(&self.inner, js, done);
    }

    /// Annotate mode on or off (hover outline + label chip, click → note popover, clicks never
    /// reach the page), in the dark or light look. Kept across loads.
    pub fn set_annotate(&self, on: bool, dark: bool) {
        {
            let mut a = self.inner.annotate.borrow_mut();
            if (a.on, a.dark) == (on, dark) {
                return;
            }
            (a.on, a.dark) = (on, dark);
        }
        apply_annotate(&self.inner);
    }

    /// The open notes to show as numbered pins (also while annotate is off). Kept across loads.
    pub fn set_pins(&self, pins: Vec<WebPin>) {
        {
            let mut a = self.inner.annotate.borrow_mut();
            if a.pins == pins {
                return;
            }
            a.pins = pins;
        }
        apply_annotate(&self.inner);
    }

    /// Saves, resolves, replies… since the last call (drain in `AppSpec::poll`; `wake` fires).
    pub fn take_annotate_events(&self) -> Vec<AnnotateEvent> {
        std::mem::take(&mut self.inner.annotate.borrow_mut().events)
    }

    /// The view's current picture as straight RGBA, width, height (probes only: it converts on
    /// the calling thread).
    #[doc(hidden)]
    pub fn snapshot_rgba(&self, done: impl FnOnce(Option<(Vec<u8>, u32, u32)>) + 'static) {
        let done = RefCell::new(Some(done));
        let block = RcBlock::new(move |image: *mut objc2_app_kit::NSImage, _err: *mut NSError| {
            let Some(done) = done.borrow_mut().take() else { return };
            // SAFETY: WebKit passes a valid image or null.
            let cg = unsafe { image.as_ref() }.and_then(|img| unsafe { img.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None) });
            done(cg.and_then(|cg| {
                let (w, h) = super::image::size(&cg);
                super::image::to_rgba(&cg, w, h).map(|px| (px, w, h))
            }));
        });
        unsafe { self.inner.view.takeSnapshotWithConfiguration_completionHandler(None, &block) };
    }

    /// Ids of pins whose element isn't on the current page (refreshed after each load).
    pub fn missing_pins(&self) -> Vec<String> {
        self.inner.annotate.borrow().missing.clone()
    }
}

fn apply_annotate(inner: &Inner) {
    if inner.nav.get() != Nav::Loaded {
        // `didFinish` applies it.
        return;
    }
    let js = inner.annotate.borrow().apply_js();
    eval_in(inner, &js, |_| {});
}

/// `LiveView::eval` on the inner view.
fn eval_in(inner: &Inner, js: &str, done: impl FnOnce(Result<serde_json::Value, String>) + 'static) {
    let body = format!("return JSON.stringify(await (async () => {{ {js} }})() ?? null);");
    let done = RefCell::new(Some(done));
    let block = RcBlock::new(move |result: *mut AnyObject, err: *mut NSError| {
        let Some(done) = done.borrow_mut().take() else { return };
        // SAFETY: WebKit passes a valid object or null, and an error or null.
        if let Some(e) = unsafe { err.as_ref() } {
            return done(Err(e.localizedDescription().to_string()));
        }
        let text = unsafe { result.as_ref() }.and_then(|o| o.downcast_ref::<NSString>()).map(|s| s.to_string());
        done(Ok(text.and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(serde_json::Value::Null)));
    });
    let world = unsafe { WKContentWorld::pageWorld(inner.mtm) };
    unsafe { inner.view.callAsyncJavaScript_arguments_inFrame_inContentWorld_completionHandler(&NSString::from_str(&body), None, None, &world, Some(&block)) };
}

impl LiveView {
    /// Receives `window.webkit.messageHandlers[name].postMessage(JSON.stringify(payload))`.
    pub fn on_message(&self, name: &str, f: impl Fn(serde_json::Value) + 'static) {
        let inner = &self.inner;
        if name == FOCUS_HANDLER || name == annotate::HANDLER {
            return;
        }
        let fresh = inner.handlers.borrow_mut().insert(name.to_string(), Box::new(f)).is_none();
        if fresh {
            let controller = unsafe { inner.view.configuration().userContentController() };
            unsafe { controller.addScriptMessageHandler_name(ProtocolObject::from_ref(&*inner._messages), &NSString::from_str(name)) };
        }
    }
}

impl Drop for LiveView {
    fn drop(&mut self) {
        remove_monitor(&self.inner);
        let controller = unsafe { self.inner.view.configuration().userContentController() };
        // The controller retains its handlers; break the cycle so everything is released.
        unsafe { controller.removeAllScriptMessageHandlers() };
        self.inner.container.removeFromSuperview();
    }
}

fn load(inner: &Rc<Inner>, page: &LivePage, opts: &LiveOptions) {
    let next = Loaded { key: page.key.to_string(), rev: page.rev, opts: opts.clone() };
    if inner.loaded.borrow().as_ref() == Some(&next) {
        return;
    }
    let controller = unsafe { inner.view.configuration().userContentController() };
    unsafe { controller.removeAllUserScripts() };
    for src in [js::FOCUS_TRACKER, annotate::SCRIPT].into_iter().chain(opts.user_scripts.iter().map(String::as_str)) {
        let script = unsafe {
            WKUserScript::initWithSource_injectionTime_forMainFrameOnly(WKUserScript::alloc(inner.mtm), &NSString::from_str(src), WKUserScriptInjectionTime::AtDocumentEnd, true)
        };
        unsafe { controller.addUserScript(&script) };
    }
    inner.layout.set((page.viewport_w.max(1), page.viewport_h));
    inner.editable_focus.set(false);
    {
        let mut a = inner.annotate.borrow_mut();
        a.popover = false;
        a.missing.clear();
    }
    inner.nav.set(Nav::Loading);
    let base = inner.base_url.borrow().as_deref().and_then(|u| NSURL::URLWithString(&NSString::from_str(u)));
    unsafe { inner.view.loadHTMLString_baseURL(&NSString::from_str(page.html), base.as_deref()) };
    *inner.loaded.borrow_mut() = Some(next);
}

fn on_message(inner: &Inner, name: &str, body: &AnyObject) {
    let value = body_json(body);
    if name == FOCUS_HANDLER {
        inner.editable_focus.set(value.as_f64().is_some_and(|v| v != 0.0) || value.as_bool() == Some(true));
        return;
    }
    if name == annotate::HANDLER {
        let Some(msg) = annotate::parse(&value, inner.rect.get(), inner.zoom.get()) else { return };
        let mut a = inner.annotate.borrow_mut();
        match msg {
            Msg::Event(e) => a.events.push(e),
            Msg::Escape => inner.keys.borrow_mut().push(LiveKey::Escape),
            Msg::Popover(open) => {
                a.popover = open;
                return;
            }
            Msg::Missing(ids) => a.missing = ids,
        }
        drop(a);
        (inner.wake)();
        return;
    }
    if let Some(f) = inner.handlers.borrow().get(name) {
        f(value);
    }
}

fn web_has_focus(inner: &Inner) -> bool {
    let Some(window) = inner.view.window() else { return false };
    window
        .firstResponder()
        .and_then(|r| r.downcast::<NSView>().ok())
        .is_some_and(|v| v.isDescendantOf(&inner.view))
}

/// Below this zoom the view is scaled as a whole instead of with pageZoom: WebKit lays text out
/// at ≥ 9pt on screen after pageZoom, so a small pageZoom would enlarge small design text.
pub(crate) const SCALE_BELOW: f64 = 0.9;

/// How a page of layout size `vw` × `vh` fills a `w` × `h` rect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    /// Page CSS px → window points, overall.
    pub zoom: f64,
    /// WebKit's pageZoom (1 when the view is scaled instead).
    pub page_zoom: f64,
    /// The container's scale: the web view is `w/scale` × `h/scale` points inside it.
    pub scale: f64,
}

pub(crate) fn placement(w: f64, h: f64, vw: u32, vh: Option<u32>) -> Option<Placement> {
    let zoom = match vh {
        Some(vh) => (w / vw.max(1) as f64).min(h / vh.max(1) as f64),
        None => w / vw.max(1) as f64,
    };
    if !zoom.is_finite() || zoom <= 0.0 {
        return None;
    }
    Some(if zoom < SCALE_BELOW { Placement { zoom, page_zoom: 1.0, scale: zoom } } else { Placement { zoom, page_zoom: zoom, scale: 1.0 } })
}

/// Which key, if any, the monitor takes from the page (pure, so it's testable).
/// `page_esc`: annotate mode or a popover in the page wants Esc itself.
pub(crate) fn intercept(key_code: u16, modifiers: NSEventModifierFlags, editable_focus: bool, page_esc: bool) -> Option<LiveKey> {
    let held = modifiers.intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option | NSEventModifierFlags::Shift);
    match key_code {
        KEY_ESC if page_esc => None,
        KEY_ESC => Some(LiveKey::Escape),
        KEY_LEFT if !held && !editable_focus => Some(LiveKey::Left),
        KEY_RIGHT if !held && !editable_focus => Some(LiveKey::Right),
        _ => None,
    }
}

fn install_monitor(inner: &Rc<Inner>) {
    if inner.monitor.borrow().is_some() {
        return;
    }
    let weak = Rc::downgrade(inner);
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        let pass = event.as_ptr();
        let Some(inner) = weak.upgrade() else { return pass };
        if !inner.shown.get() || !web_has_focus(&inner) {
            return pass;
        }
        // SAFETY: AppKit hands the monitor a valid event for the duration of the call.
        let ev = unsafe { event.as_ref() };
        let page_esc = inner.annotate.borrow().page_takes_esc();
        match intercept(ev.keyCode(), ev.modifierFlags(), inner.editable_focus.get(), page_esc) {
            Some(key) => {
                inner.keys.borrow_mut().push(key);
                (inner.wake)();
                std::ptr::null_mut()
            }
            None => pass,
        }
    });
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block) };
    *inner.monitor.borrow_mut() = monitor;
}

fn remove_monitor(inner: &Inner) {
    if let Some(m) = inner.monitor.borrow_mut().take() {
        unsafe { NSEvent::removeMonitor(&m) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_views_scale_the_view_large_ones_zoom_the_page() {
        let p = placement(640.0, 480.0, 1280, None).unwrap();
        assert_eq!((p.zoom, p.page_zoom, p.scale), (0.5, 1.0, 0.5));
        let p = placement(1200.0, 800.0, 1280, None).unwrap();
        assert!((p.page_zoom - 0.9375).abs() < 1e-9 && p.scale == 1.0);
        // Fixed screens fit whole: the smaller ratio wins.
        let p = placement(400.0, 400.0, 390, Some(844)).unwrap();
        assert!((p.zoom - 400.0 / 844.0).abs() < 1e-9 && p.page_zoom == 1.0);
        assert_eq!(placement(0.0, 0.0, 1280, None), None);
    }

    #[test]
    fn esc_always_arrows_only_unmodified_outside_fields() {
        let none = NSEventModifierFlags::empty();
        assert_eq!(intercept(KEY_ESC, NSEventModifierFlags::Command, true, false), Some(LiveKey::Escape));
        assert_eq!(intercept(KEY_LEFT, none, false, false), Some(LiveKey::Left));
        // Arrow keys carry the function/numeric-pad flags; those don't count as modifiers.
        assert_eq!(intercept(KEY_RIGHT, NSEventModifierFlags::Function | NSEventModifierFlags::NumericPad, false, false), Some(LiveKey::Right));
        assert_eq!(intercept(KEY_RIGHT, NSEventModifierFlags::Shift, false, false), None);
        assert_eq!(intercept(KEY_LEFT, none, true, false), None);
        assert_eq!(intercept(0, none, false, false), None);
        // Annotating (or a popover open in the page): Esc is the page's.
        assert_eq!(intercept(KEY_ESC, none, false, true), None);
        assert_eq!(intercept(KEY_LEFT, none, false, true), Some(LiveKey::Left));
    }
}
