//! The shown design's page (spec 02 §N): keep its annotate state and pins in step with ours, turn its events into
//! notes, and mirror design notes into the store (`notes/<designId>`, AN-WV-126) for the rest of the app.

use proto_core::{Change, DesignNote, RecordKind};
use proto_web::{AnnotateEvent, WebPick, WebPin};

use super::logic;
use super::model::{Note, Status, Target};
use crate::app::state::AppState;

/// What was last set on the shown page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Memo {
    /// (design, on, dark).
    mode: Option<(String, bool, bool)>,
    pins: Option<(String, Vec<WebPin>)>,
}

/// Pins for `design` (AN-WV-124): numbered among all open notes of the space, then filtered to the design.
pub fn pins_for(notes: &[Note], space: &str, design: &str) -> Vec<WebPin> {
    logic::numbered(notes, space).into_iter()
        .filter(|(_, n)| n.design_id() == Some(design))
        .filter_map(|(i, n)| {
            let path = n.targets.first().map(|t| t.element_path.clone()).filter(|p| !p.is_empty())?;
            Some(WebPin {
                id: n.id.clone(),
                number: i,
                element_path: path,
                comment: n.comment.clone(),
                status: if n.status == Status::Queued { "pending".into() } else { n.status.word().into() },
                thread: n.thread.iter().map(|m| (m.role.clone(), m.content.clone())).collect(),
            })
        })
        .collect()
}

/// A page pick as a note target (AN-WV-125).
pub fn target_of(p: &WebPick, design: &str) -> Target {
    let mut attributes: Vec<(String, String)> = p.attributes.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    attributes.sort();
    Target {
        element: p.element.clone(),
        element_path: p.element_path.clone(),
        full_path: p.full_path.clone(),
        css_classes: p.css_classes.clone(),
        nearby_text: p.nearby_text.clone(),
        nearby_elements: p.nearby_elements.clone(),
        selected_text: p.selected_text.clone(),
        styles: p.computed_styles.clone(),
        accessibility: p.accessibility.clone(),
        attributes,
        rect: p.window_rect,
        design_id: Some(design.to_string()),
        ..Target::default()
    }
}

/// Each poll (AN-WV-120…127). True when the page reported anything.
pub fn poll(s: &mut AppState) -> bool {
    let (design, rect, events, missing) = {
        let Some((design, view, rect)) = s.fullscreen.live().shown() else {
            s.features.annotate.page = Memo::default();
            return false;
        };
        let a = &s.features.annotate;
        let mode = (design.to_string(), a.on, a.dark());
        if a.page.mode.as_ref() != Some(&mode) {
            view.set_annotate(mode.1, mode.2);
        }
        let pins = pins_for(&a.notes, &s.route.space, design);
        let pins_changed = a.page.pins.as_ref().is_none_or(|(d, p)| d != design || *p != pins);
        if pins_changed {
            view.set_pins(pins.clone());
        }
        let memo = Memo { mode: Some(mode), pins: Some((design.to_string(), pins)) };
        let (events, missing) = (view.take_annotate_events(), view.missing_pins());
        s.features.annotate.page = memo;
        (design.to_string(), rect, events, missing)
    };
    let mut changed = false;
    if missing != s.features.annotate.missing_web {
        s.features.annotate.missing_web = missing;
        changed = true;
    }
    for e in events {
        changed = true;
        match e {
            AnnotateEvent::Save { comment, targets } => {
                let targets = targets.iter().map(|p| target_of(p, &design)).collect();
                super::create_note(s, &comment, targets, [rect[0] + rect[2], rect[1] + rect[3]]);
            }
            AnnotateEvent::Resolve { id } => super::set_status(s, &id, Status::Resolved),
            AnnotateEvent::Dismiss { id } => super::set_status(s, &id, Status::Dismissed),
            AnnotateEvent::Reply { id, text } => super::reply(s, &id, &text),
            AnnotateEvent::Cancel => {}
        }
    }
    changed
}

/// The store's copy of a new design note: replaces one with the same id and goes last.
pub fn mirror_new(s: &mut AppState, n: &Note) {
    let Some(design) = n.design_id() else { return };
    let mut list: Vec<DesignNote> = s.snap().notes(design).map(|l| l.iter().filter(|x| x.id != n.id).cloned().collect()).unwrap_or_default();
    list.push(DesignNote {
        id: n.id.clone(),
        design_id: design.to_string(),
        comment: n.comment.trim().to_string(),
        element: n.targets.iter().map(|t| t.element.as_str()).collect::<Vec<_>>().join(", "),
        element_path: n.targets.iter().map(|t| t.element_path.as_str()).collect::<Vec<_>>().join(" | "),
        selected_text: n.targets.first().and_then(|t| t.selected_text.clone()),
        timestamp: n.timestamp,
        resolved: None,
        extra: Default::default(),
    });
    let Ok(change) = Change::put(RecordKind::Notes, design, &list) else { return };
    crate::features::nav::apply(s, vec![change], "Annotate note");
}

/// Resolved or dismissed by the user: the store's copy counts as resolved.
pub fn mirror_resolved(s: &mut AppState, design: &str, id: &str) {
    let Some(list) = s.snap().notes(design) else { return };
    if !list.iter().any(|n| n.id == id) {
        return;
    }
    let list: Vec<DesignNote> = list.iter().cloned().map(|mut n| {
        if n.id == id {
            n.resolved = Some(true);
        }
        n
    }).collect();
    let Ok(change) = Change::put(RecordKind::Notes, design, &list) else { return };
    crate::features::nav::apply(s, vec![change], "Resolve Annotate note");
}
