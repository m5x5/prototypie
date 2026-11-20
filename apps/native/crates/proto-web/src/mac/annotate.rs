//! Annotate inside a `LiveView` design page (spec 02 "webview picker", interface spec 04 AN-IF-2xx).
//!
//! The page half lives in `annotate.js`: it is injected into every page, stays idle until Rust applies
//! a state, draws hover/selection/pins/popovers in its own shadow root and reports user actions as
//! JSON strings over [`HANDLER`]. This file holds the shared types, the per-view [`State`], the call
//! that hands the state to the page ([`State::apply_js`]) and the reader for page messages ([`parse`]).
//!
//! Page → Rust message shapes (field `t` names the kind):
//! - `{"t":"save","comment":"…","targets":[Target…]}`
//! - `{"t":"resolve","id":"…"}`, `{"t":"dismiss","id":"…"}`, `{"t":"reply","id":"…","text":"…"}`
//! - `{"t":"cancel"}`, `{"t":"escape"}`
//! - `{"t":"missing","ids":["…"]}`, `{"t":"popover","open":true}`
//!
//! A `Target` carries `label, path, fullPath, classes, nearbyText, nearbyElements, selectedText,
//! styles, a11y, attrs, rect`.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};

/// Page script (from `annotate.js`); `live.rs` adds it on each load. Idle until Rust applies a state;
/// answers over [`HANDLER`].
pub const SCRIPT: &str = include_str!("annotate.js");

/// Message channel name, page → Rust. Registered by `live.rs`; `on_message` refuses it.
pub const HANDLER: &str = "protoAnnotate";

/// Name of the page-side entry object (`window[ENTRY]`), used by `apply_js` and by probes.
pub const ENTRY: &str = "__protoAnnotate";

/// Tag name of the overlay host the script hangs under `<html>` (probes look it up).
#[doc(hidden)]
pub const OVERLAY_TAG: &str = "proto-annotate-overlay";

/// Picked HTML element, as reported by the page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebPick {
    /// Role and label, e.g. `button "Go"`.
    pub element: String,
    /// Compact selector; used to locate it again.
    pub element_path: String,
    /// Ancestor chain from `body` down.
    pub full_path: String,
    pub css_classes: String,
    pub nearby_text: String,
    pub nearby_elements: String,
    /// Highlighted text at pick time.
    pub selected_text: Option<String>,
    pub computed_styles: String,
    pub accessibility: String,
    /// A few telling attributes (id, name, …), capped.
    pub attributes: BTreeMap<String, String>,
    /// `[x, y, w, h]`, CSS px of the page.
    pub rect: [f32; 4],
    /// `[x, y, w, h]`, window points; Rust computes it.
    pub window_rect: [f32; 4],
}

/// Numbered marker the page draws for a stored note.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebPin {
    /// Echoed back in events.
    pub id: String,
    /// Label on the marker.
    pub number: u32,
    /// Selector of the marked element.
    pub element_path: String,
    pub comment: String,
    /// `pending` | `acknowledged` | `resolved`; picks the colour.
    pub status: String,
    /// `(who, text)` pairs; who is `human` or `agent`.
    pub thread: Vec<(String, String)>,
}

/// User action reported by the page.
#[derive(Clone, Debug, PartialEq)]
pub enum AnnotateEvent {
    /// New note: text plus picked elements.
    Save { comment: String, targets: Vec<WebPick> },
    /// Mark done.
    Resolve { id: String },
    /// Delete (dismiss).
    Dismiss { id: String },
    /// Answer in the conversation.
    Reply { id: String, text: String },
    /// Editor closed, nothing saved.
    Cancel,
}

/// Rust-side Annotate state per `LiveView`. `live.rs` writes the fields and pushes them again after
/// each load.
#[derive(Clone, Debug)]
pub(crate) struct State {
    /// Mode switched on.
    pub on: bool,
    /// Dark theme for the in-page UI.
    pub dark: bool,
    /// Window points per CSS px; lets the page scale its UI back.
    pub zoom: f32,
    /// Markers; shown even with the mode off.
    pub pins: Vec<WebPin>,
    /// Page reports an open popover.
    pub popover: bool,
    /// Pin ids without a matching element.
    pub missing: Vec<String>,
    /// Queue emptied by `take_annotate_events`.
    pub events: Vec<AnnotateEvent>,
}

impl Default for State {
    /// Off, light, zoom 1, empty.
    fn default() -> Self {
        State { on: false, dark: false, zoom: 1.0, pins: Vec::new(), popover: false, missing: Vec::new(), events: Vec::new() }
    }
}

impl State {
    /// True if mode is on or markers exist. Gates re-applying on zoom in `live.rs`.
    pub fn active(&self) -> bool {
        self.on || !self.pins.is_empty()
    }

    /// True if Esc belongs to the page (mode on or popover open), not `LiveKey::Escape`.
    pub fn page_takes_esc(&self) -> bool {
        self.on || self.popover
    }

    /// The state as the JSON object the page's `apply` reads.
    fn payload(&self) -> Value {
        let pins: Vec<Value> = self
            .pins
            .iter()
            .map(|p| {
                json!({
                    "id": p.id,
                    "number": p.number,
                    "path": p.element_path,
                    "comment": p.comment,
                    "status": p.status,
                    "thread": p.thread.iter().map(|(who, text)| json!([who, text])).collect::<Vec<_>>(),
                })
            })
            .collect();
        json!({ "on": self.on, "dark": self.dark, "zoom": self.zoom, "pins": pins })
    }

    /// Async JS body; `live.rs` evals it to pass this state in. A page without the script yields
    /// `null` (no error).
    pub fn apply_js(&self) -> String {
        let state = serde_json::to_string(&self.payload()).unwrap_or_else(|_| String::from("{}"));
        format!(
            "const s = {state};\nconst a = window[{entry:?}];\nreturn a && typeof a.apply === \"function\" ? a.apply(s.on, s.dark, s.zoom, s.pins) : null;",
            entry = ENTRY
        )
    }
}

/// Parsed page message.
#[derive(Debug, PartialEq)]
pub(crate) enum Msg {
    /// Goes to `events`.
    Event(AnnotateEvent),
    /// Page had nothing to close; turns into `LiveKey::Escape`.
    Escape,
    /// Popover state: true = open.
    Popover(bool),
    /// Replaces `missing`.
    Missing(Vec<String>),
}

/// One target as the page sends it. Absent fields take their empty default; a present field of the
/// wrong type makes the target (and thus the whole target list) unreadable.
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Target {
    label: String,
    path: String,
    full_path: String,
    classes: String,
    nearby_text: String,
    nearby_elements: String,
    selected_text: Option<String>,
    styles: String,
    a11y: String,
    attrs: BTreeMap<String, String>,
    rect: [f32; 4],
}

impl Target {
    fn into_pick(self, frame: [f32; 4], zoom: f32) -> WebPick {
        let [l, t, w, h] = self.rect;
        WebPick {
            element: self.label,
            element_path: self.path,
            full_path: self.full_path,
            css_classes: self.classes,
            nearby_text: self.nearby_text,
            nearby_elements: self.nearby_elements,
            selected_text: self.selected_text,
            computed_styles: self.styles,
            accessibility: self.a11y,
            attributes: self.attrs,
            rect: self.rect,
            window_rect: [frame[0] + l * zoom, frame[1] + t * zoom, w * zoom, h * zoom],
        }
    }
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}

/// Reads a [`HANDLER`] message. `frame` (view rect in window points) and `zoom` give
/// `WebPick::window_rect`. Unknown or broken input: None.
pub(crate) fn parse(v: &Value, frame: [f32; 4], zoom: f32) -> Option<Msg> {
    let event = |e| Some(Msg::Event(e));
    match text(v, "t")? {
        "save" => {
            let comment = text(v, "comment")?.to_string();
            let targets: Vec<Target> = serde_json::from_value(v.get("targets")?.clone()).ok()?;
            event(AnnotateEvent::Save { comment, targets: targets.into_iter().map(|t| t.into_pick(frame, zoom)).collect() })
        }
        "resolve" => event(AnnotateEvent::Resolve { id: text(v, "id")?.to_string() }),
        "dismiss" => event(AnnotateEvent::Dismiss { id: text(v, "id")?.to_string() }),
        "reply" => event(AnnotateEvent::Reply { id: text(v, "id")?.to_string(), text: text(v, "text")?.to_string() }),
        "cancel" => event(AnnotateEvent::Cancel),
        "escape" => Some(Msg::Escape),
        "popover" => Some(Msg::Popover(v.get("open")?.as_bool()?)),
        "missing" => {
            let ids = v.get("ids")?.as_array()?;
            Some(Msg::Missing(ids.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: [f32; 4] = [10.0, 20.0, 640.0, 400.0];

    fn save_with(target: Value) -> Value {
        json!({ "t": "save", "comment": "bigger", "targets": [target] })
    }

    #[test]
    fn save_translates_targets_and_window_rect() {
        let v = save_with(json!({
            "label": "button \"Go\"",
            "path": "main > .cta",
            "fullPath": "body > main > button.cta",
            "classes": "cta",
            "nearbyText": "[before: \"x\"] Go",
            "nearbyElements": "h1",
            "selectedText": null,
            "styles": "font-size: 15px",
            "a11y": "focusable",
            "attrs": { "id": "go", "type": "button" },
            "rect": [100, 50, 80, 20],
            "extra": "ignored"
        }));
        let Some(Msg::Event(AnnotateEvent::Save { comment, targets })) = parse(&v, FRAME, 0.5) else { panic!("not a save") };
        assert_eq!(comment, "bigger");
        assert_eq!(targets.len(), 1);
        let t = &targets[0];
        assert_eq!(t.element, "button \"Go\"");
        assert_eq!(t.element_path, "main > .cta");
        assert_eq!(t.full_path, "body > main > button.cta");
        assert_eq!(t.css_classes, "cta");
        assert_eq!(t.nearby_text, "[before: \"x\"] Go");
        assert_eq!(t.nearby_elements, "h1");
        assert_eq!(t.selected_text, None);
        assert_eq!(t.computed_styles, "font-size: 15px");
        assert_eq!(t.accessibility, "focusable");
        assert_eq!(t.attributes.get("id").map(String::as_str), Some("go"));
        assert_eq!(t.attributes.keys().collect::<Vec<_>>(), ["id", "type"]);
        assert_eq!(t.rect, [100.0, 50.0, 80.0, 20.0]);
        assert_eq!(t.window_rect, [60.0, 45.0, 40.0, 10.0]);
    }

    #[test]
    fn save_defaults_missing_fields_and_keeps_selected_text() {
        let v = save_with(json!({ "path": "#a", "selectedText": "hi there" }));
        let Some(Msg::Event(AnnotateEvent::Save { targets, .. })) = parse(&v, FRAME, 1.0) else { panic!() };
        assert_eq!(targets[0].element, "");
        assert_eq!(targets[0].selected_text.as_deref(), Some("hi there"));
        assert_eq!(targets[0].rect, [0.0; 4]);
        assert_eq!(targets[0].window_rect, [10.0, 20.0, 0.0, 0.0]);
        // An empty target list is readable.
        let v = json!({ "t": "save", "comment": "c", "targets": [] });
        assert_eq!(parse(&v, FRAME, 1.0), Some(Msg::Event(AnnotateEvent::Save { comment: "c".into(), targets: vec![] })));
    }

    #[test]
    fn save_with_broken_fields_is_dropped() {
        // Non-finite numbers arrive as null.
        assert_eq!(parse(&save_with(json!({ "rect": [1, 2, null, 4] })), FRAME, 1.0), None);
        assert_eq!(parse(&save_with(json!({ "rect": [1, 2, 3] })), FRAME, 1.0), None);
        assert_eq!(parse(&save_with(json!({ "attrs": { "id": 5 } })), FRAME, 1.0), None);
        assert_eq!(parse(&save_with(json!({ "label": 3 })), FRAME, 1.0), None);
        assert_eq!(parse(&save_with(json!("not an object")), FRAME, 1.0), None);
        assert_eq!(parse(&json!({ "t": "save", "targets": [] }), FRAME, 1.0), None);
        assert_eq!(parse(&json!({ "t": "save", "comment": 1, "targets": [] }), FRAME, 1.0), None);
        assert_eq!(parse(&json!({ "t": "save", "comment": "c" }), FRAME, 1.0), None);
        assert_eq!(parse(&json!({ "t": "save", "comment": "c", "targets": {} }), FRAME, 1.0), None);
    }

    #[test]
    fn other_kinds_translate() {
        let p = |v: Value| parse(&v, FRAME, 1.0);
        assert_eq!(p(json!({ "t": "resolve", "id": "n1" })), Some(Msg::Event(AnnotateEvent::Resolve { id: "n1".into() })));
        assert_eq!(p(json!({ "t": "dismiss", "id": "n1" })), Some(Msg::Event(AnnotateEvent::Dismiss { id: "n1".into() })));
        assert_eq!(p(json!({ "t": "reply", "id": "n1", "text": "thanks" })), Some(Msg::Event(AnnotateEvent::Reply { id: "n1".into(), text: "thanks".into() })));
        assert_eq!(p(json!({ "t": "cancel" })), Some(Msg::Event(AnnotateEvent::Cancel)));
        assert_eq!(p(json!({ "t": "escape" })), Some(Msg::Escape));
        assert_eq!(p(json!({ "t": "popover", "open": true })), Some(Msg::Popover(true)));
        assert_eq!(p(json!({ "t": "popover", "open": false })), Some(Msg::Popover(false)));
        assert_eq!(p(json!({ "t": "missing", "ids": ["p3", 4, null, "p5"] })), Some(Msg::Missing(vec!["p3".into(), "p5".into()])));
        assert_eq!(p(json!({ "t": "missing", "ids": [] })), Some(Msg::Missing(vec![])));
    }

    #[test]
    fn unknown_or_malformed_messages_are_dropped() {
        let p = |v: Value| parse(&v, FRAME, 1.0);
        assert_eq!(p(json!({ "t": "explode" })), None);
        assert_eq!(p(json!({ "kind": "cancel" })), None);
        assert_eq!(p(json!("cancel")), None);
        assert_eq!(p(Value::Null), None);
        assert_eq!(p(json!({ "t": "resolve" })), None);
        assert_eq!(p(json!({ "t": "resolve", "id": 7 })), None);
        assert_eq!(p(json!({ "t": "reply", "id": "n1" })), None);
        assert_eq!(p(json!({ "t": "popover", "open": "yes" })), None);
        assert_eq!(p(json!({ "t": "missing", "ids": "p3" })), None);
    }

    #[test]
    fn default_state_and_esc_ownership() {
        let mut s = State::default();
        assert!(!s.on && !s.dark && s.zoom == 1.0 && s.pins.is_empty() && !s.popover && s.missing.is_empty() && s.events.is_empty());
        assert!(!s.page_takes_esc() && !s.active());
        s.popover = true;
        assert!(s.page_takes_esc() && !s.active());
        s.popover = false;
        s.on = true;
        assert!(s.page_takes_esc() && s.active());
        s.on = false;
        s.pins.push(WebPin { id: "p".into(), number: 1, ..Default::default() });
        assert!(!s.page_takes_esc() && s.active());
    }

    #[test]
    fn apply_js_carries_the_whole_state() {
        let mut s = State { on: true, dark: true, zoom: 0.5, ..Default::default() };
        s.pins.push(WebPin {
            id: "p1".into(),
            number: 2,
            element_path: "#go".into(),
            comment: "a \"quoted\" </script> comment".into(),
            status: "acknowledged".into(),
            thread: vec![("agent".into(), "On it.".into())],
        });
        let js = s.apply_js();
        assert!(js.contains("\"on\":true"), "{js}");
        assert!(js.contains("\"dark\":true"));
        assert!(js.contains("\"zoom\":0.5"));
        assert!(js.contains("window[\"__protoAnnotate\"]"));
        let start = js.find('{').unwrap();
        let end = js.find(";\n").unwrap();
        let state: Value = serde_json::from_str(&js[start..end]).unwrap();
        assert_eq!(state["pins"][0]["path"], "#go");
        assert_eq!(state["pins"][0]["number"], 2);
        assert_eq!(state["pins"][0]["thread"], json!([["agent", "On it."]]));
        assert_eq!(state["pins"][0]["comment"], "a \"quoted\" </script> comment");
        assert!(State::default().apply_js().contains("\"on\":false"));
    }

    #[test]
    fn script_names_match_the_rust_side() {
        assert!(SCRIPT.contains(&format!("const ENTRY = \"{ENTRY}\"")));
        assert!(SCRIPT.contains(&format!("const CHANNEL = \"{HANDLER}\"")));
        assert!(SCRIPT.contains(&format!("const HOST_TAG = \"{OVERLAY_TAG}\"")));
        // Every message kind `parse` reads is one the script sends.
        for kind in ["save", "resolve", "dismiss", "reply", "cancel", "escape", "missing", "popover"] {
            assert!(SCRIPT.contains(&format!("\"{kind}\"")), "{kind}");
        }
    }
}
