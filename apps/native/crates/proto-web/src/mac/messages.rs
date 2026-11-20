//! `WKScriptMessageHandler` forwarding page messages (`window.webkit.messageHandlers[name]
//! .postMessage(x)`) to a Rust closure with the handler name and the body.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_web_kit::{WKScriptMessage, WKScriptMessageHandler, WKUserContentController};

type OnMessage = Box<dyn Fn(&str, &AnyObject)>;

pub struct MsgIvars {
    on_message: OnMessage,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ProtoWebMessageHandler"]
    #[ivars = MsgIvars]
    pub struct MessageHandler;

    unsafe impl NSObjectProtocol for MessageHandler {}

    unsafe impl WKScriptMessageHandler for MessageHandler {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn did_receive(&self, _controller: &WKUserContentController, message: &WKScriptMessage) {
            let name = unsafe { message.name() }.to_string();
            let body = unsafe { message.body() };
            (self.ivars().on_message)(&name, &body);
        }
    }
);

impl MessageHandler {
    pub fn new(mtm: MainThreadMarker, on_message: impl Fn(&str, &AnyObject) + 'static) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(MsgIvars { on_message: Box::new(on_message) });
        // SAFETY: NSObject's designated initializer on a freshly allocated instance.
        unsafe { msg_send![super(this), init] }
    }
}

/// A message body as JSON: strings that parse as JSON are taken as such (the convention is to
/// post `JSON.stringify(payload)`), other strings, numbers and booleans map directly.
pub fn body_json(body: &AnyObject) -> serde_json::Value {
    use objc2_foundation::{NSNumber, NSString};
    if let Some(s) = body.downcast_ref::<NSString>() {
        let s = s.to_string();
        return serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s));
    }
    if let Some(n) = body.downcast_ref::<NSNumber>() {
        return serde_json::json!(n.doubleValue());
    }
    serde_json::Value::Null
}
