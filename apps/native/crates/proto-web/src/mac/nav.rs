//! `WKNavigationDelegate` that forwards navigation outcomes to a Rust closure.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::NSError;
use objc2_web_kit::{WKNavigation, WKNavigationDelegate, WKWebView};

/// What a navigation ended with.
#[derive(Debug)]
pub enum NavEvent {
    Finished,
    Failed(String),
    /// The web content process crashed or was killed; the view must load again.
    Terminated,
}

type OnEvent = Box<dyn Fn(NavEvent, NavId)>;

/// Identity of a `WKNavigation` (its address; 0 when WebKit passes none), to tell a load's own
/// events from those of an earlier load on the same view.
pub type NavId = usize;

pub fn nav_id(navigation: Option<&WKNavigation>) -> NavId {
    navigation.map_or(0, |n| n as *const WKNavigation as usize)
}

pub struct NavIvars {
    on_event: RefCell<OnEvent>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ProtoWebNavDelegate"]
    #[ivars = NavIvars]
    pub struct NavDelegate;

    unsafe impl NSObjectProtocol for NavDelegate {}

    unsafe impl WKNavigationDelegate for NavDelegate {
        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, _web_view: &WKWebView, navigation: Option<&WKNavigation>) {
            self.emit(NavEvent::Finished, nav_id(navigation));
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(&self, _web_view: &WKWebView, navigation: Option<&WKNavigation>, error: &NSError) {
            self.emit(NavEvent::Failed(error.localizedDescription().to_string()), nav_id(navigation));
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(&self, _web_view: &WKWebView, navigation: Option<&WKNavigation>, error: &NSError) {
            self.emit(NavEvent::Failed(error.localizedDescription().to_string()), nav_id(navigation));
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn did_terminate(&self, _web_view: &WKWebView) {
            self.emit(NavEvent::Terminated, 0);
        }
    }
);

impl NavDelegate {
    pub fn new(mtm: MainThreadMarker, on_event: impl Fn(NavEvent, NavId) + 'static) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(NavIvars { on_event: RefCell::new(Box::new(on_event)) });
        // SAFETY: NSObject's designated initializer on a freshly allocated instance.
        unsafe { msg_send![super(this), init] }
    }

    fn emit(&self, event: NavEvent, id: NavId) {
        // The handler may start the next step (another load), which can call back synchronously
        // only through a new run loop turn, so borrowing here doesn't re-enter.
        (self.ivars().on_event.borrow())(event, id);
    }
}
