//! After each drawn frame (spec 01 AN-UI-161): the drawn rects of the elements the overlay follows and, once per
//! hover change, the analysis of the hovered element. Handed to the next `poll` through a thread-local; the UI
//! is woken only when something changed, so a still screen costs nothing.

use std::cell::RefCell;
use std::collections::HashMap;

use super::logic::{self, Analysis, Fact};
use crate::app::state::AppState;

#[derive(Default)]
struct Slot {
    /// What the last delivered frame found (to see changes).
    keys: HashMap<String, [f32; 4]>,
    paths: HashMap<String, [f32; 4]>,
    /// The analysis request (by sequence number) already answered.
    answered: u64,
    /// Waiting for `poll`.
    fresh: Option<Found>,
}

/// What a frame found, for `poll`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub keys: HashMap<String, [f32; 4]>,
    pub paths: HashMap<String, [f32; 4]>,
    /// `(request sequence, analysis or None when the element wasn't in the frame)`.
    pub analysis: Option<(u64, Option<Analysis>)>,
}

thread_local! {
    static SLOT: RefCell<Slot> = RefCell::new(Slot::default());
}

/// The keys and paths whose drawn rects the overlay needs now.
pub fn wanted(s: &AppState) -> (Vec<String>, Vec<String>) {
    let a = &s.features.annotate;
    let mut keys = Vec::new();
    let mut paths = Vec::new();
    if a.on || a.list_open {
        for (_, n) in logic::numbered(&a.notes, &s.route.space) {
            if let Some(t) = n.targets.first().filter(|t| !t.is_web()) {
                keys.extend(t.key.clone());
            }
        }
    }
    if a.on {
        if let Some(h) = &a.hover {
            paths.push(h.path.clone());
            keys.extend(h.key.clone());
        }
    }
    (keys, paths)
}

fn color(c: rune::Color) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

/// From `rune::ElementInfo`.
pub fn fact(e: &rune::ElementInfo) -> Fact {
    Fact {
        path: e.path.clone(),
        key: e.key.clone(),
        role: e.role.to_string(),
        label: e.label.clone(),
        text: e.text.clone(),
        rect: e.rect,
        fill: color(e.fill),
        radius: e.radius,
        text_color: e.text_color.map(color),
    }
}

/// Collect what's wanted from a frame's facts: the last element with each wanted key or path wins.
pub fn collect(facts: &[Fact], keys: &[String], paths: &[String]) -> (HashMap<String, [f32; 4]>, HashMap<String, [f32; 4]>) {
    let mut k = HashMap::new();
    let mut p = HashMap::new();
    for f in facts {
        if let Some(key) = &f.key {
            if keys.contains(key) {
                k.insert(key.clone(), f.rect);
            }
        }
        if paths.contains(&f.path) {
            p.insert(f.path.clone(), f.rect);
        }
    }
    (k, p)
}

/// The post-draw hook.
pub fn on_frame_drawn(rt: &rune::Runtime<AppState>) {
    let s = &rt.state;
    let a = &s.features.annotate;
    let (keys, paths) = wanted(s);
    let request = a.analysis_request.clone().map(|p| (a.analysis_seq, p));
    let pending = SLOT.with(|sl| request.as_ref().filter(|(seq, _)| *seq != sl.borrow().answered).cloned());
    if keys.is_empty() && paths.is_empty() && pending.is_none() {
        return;
    }
    let facts: Vec<Fact> = rt.elements().iter().filter(|e| !e.exiting).map(fact).collect();
    let (k, p) = collect(&facts, &keys, &paths);
    let analysis = pending.map(|(seq, path)| (seq, logic::analyse(&facts, &path)));
    let wake = SLOT.with(|sl| {
        let mut sl = sl.borrow_mut();
        let moved = sl.keys != k || sl.paths != p;
        if !moved && analysis.is_none() {
            return false;
        }
        if let Some((seq, _)) = &analysis {
            sl.answered = *seq;
        }
        sl.keys = k.clone();
        sl.paths = p.clone();
        let prev = sl.fresh.take();
        // An unread analysis stays until `poll` takes it.
        let analysis = analysis.or_else(|| prev.and_then(|f| f.analysis));
        sl.fresh = Some(Found { keys: k, paths: p, analysis });
        true
    });
    if wake {
        if let Some(w) = &a.wake {
            w();
        }
    }
}

/// What frames found since the last call.
pub fn take() -> Option<Found> {
    SLOT.with(|sl| sl.borrow_mut().fresh.take())
}
