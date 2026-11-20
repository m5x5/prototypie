//! Fullscreen design view (proto-worker-5). Spec: docs/spec-cards.md §4, §7.5.
//!
//! Opening and closing morph the card's preview into the page and back: both carry
//! `El::matched(cards::design::morph_id(id))`, and the card drops its own while the design is open.
//! Stepping through a set swaps the content without a morph.

pub mod live;
mod neighbours;
mod note;
mod view;

use rune::{Cx, El, Key, KeyEvent, Tx};

use crate::app::state::AppState;
use crate::app::style::Palette;

/// Which design is open, and the set ←/→ step through (canvas order), if any.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum View {
    #[default]
    Closed,
    Open {
        id: String,
        /// Two or more ready designs including `id`, in canvas order; None = just the one.
        set: Option<Vec<String>>,
    },
}

#[derive(Default)]
pub struct FullscreenState {
    pub view: View,
    /// The live web view over the preview box.
    live: live::LiveCtl,
    /// The designer's note box (autosaves).
    note: note::NoteBox,
    /// The preview box's laid-out width (pt), set by the view: the picture is asked for at
    /// this width × the backing scale, not at the card's size.
    box_w: std::cell::Cell<f32>,
    /// The design whose rationale is expanded ("Read more"); another design starts collapsed.
    pub(crate) rationale_open: Option<String>,
    /// The open design's neighbours' ids (Neighbours chips' pictures), for (design, snapshot version).
    neighbour_ids: ((String, u64), Vec<String>),
    /// The side panel is collapsed, the preview takes its width.
    pub aside_hidden: bool,
}

impl FullscreenState {
    /// The live web view controller (read access for features driving the page, e.g. Annotate).
    pub fn live(&self) -> &live::LiveCtl {
        &self.live
    }

    /// From the view: the preview box's laid-out width (pt).
    pub(crate) fn set_box_w(&self, w: f32) {
        self.box_w.set(w);
    }

    /// The preview box's drawn rect, for the view to bind.
    pub fn live_rect(&self) -> &rune::Live<[f32; 4]> {
        &self.live.rect
    }
}

/// The open design's id, if any.
pub fn open_id(s: &AppState) -> Option<&str> {
    match &s.fullscreen.view {
        View::Open { id, .. } => Some(id),
        View::Closed => None,
    }
}

/// From `cards::setup`: the waker the live web view wakes the app with.
pub fn setup(s: &mut AppState, wake: rune::Waker) {
    s.fullscreen.live.setup(wake);
}

/// Opens `id`. With `selection` (the canvas selection) holding 2+ ready designs including `id`,
/// ←/→ step through them in canvas order.
pub fn open(s: &mut AppState, id: &str, selection: &[String]) {
    let order = canvas_order(s.snap(), selection);
    let set = (order.len() > 1 && order.iter().any(|x| x == id)).then_some(order);
    s.fullscreen.view = View::Open { id: id.to_string(), set };
}

/// The card ↔ fullscreen flight: the reference's 320 ms `cubic-bezier(.2,.8,.2,1)`, as a spring.
pub fn morph_motion(tx: &mut Tx) {
    tx.animated();
    tx.motion(rune::Spring::new(0.32, 0.0));
}

/// Closes the view; the web view hides in this same turn, before the flight back starts.
pub fn close(s: &mut AppState) {
    note::flush(s);
    s.fullscreen.live.hide();
    s.fullscreen.view = View::Closed;
}

/// Steps `delta` through the set, wrapping; skips designs deleted meanwhile.
pub fn step(s: &mut AppState, delta: i32) {
    note::flush(s);
    let View::Open { id, set: Some(set) } = &s.fullscreen.view else { return };
    let live: Vec<String> = set.iter().filter(|i| s.snap().design(i).is_some()).cloned().collect();
    let Some(at) = live.iter().position(|x| x == id) else { return };
    let n = live.len() as i32;
    let next = live[((at as i32 + delta).rem_euclid(n)) as usize].clone();
    let set = set.clone();
    s.fullscreen.view = View::Open { id: next, set: Some(set) };
}

/// Ready designs among `ids` in reading order on the canvas: rows top to bottom (tops within
/// 160 count as one row), then left to right.
pub fn canvas_order(snap: &proto_core::Snapshot, ids: &[String]) -> Vec<String> {
    const ROW: f64 = 160.0;
    let mut seen = std::collections::HashSet::new();
    let mut ds: Vec<_> = ids.iter().filter(|i| seen.insert(i.as_str())).filter_map(|i| snap.design(i)).filter(|d| d.is_ready()).collect();
    ds.sort_by(|a, b| {
        if (a.position.y - b.position.y).abs() < ROW { a.position.x.total_cmp(&b.position.x) } else { a.position.y.total_cmp(&b.position.y) }
    });
    ds.into_iter().map(|d| d.id.clone()).collect()
}

/// Esc while Annotate is open (annotate mode, a draft, a note, its settings or list) goes to Annotate, which
/// closes its innermost open thing first; only then Esc closes the fullscreen view. True when Annotate took it.
pub fn annotate_takes_escape(s: &mut AppState) -> bool {
    let a = &s.features.annotate;
    if !(a.on || a.draft.is_some() || !a.selection.is_empty() || a.open_note.is_some() || a.settings_open || a.list_open) {
        return false;
    }
    let esc = KeyEvent { key: Key::Escape, mods: Default::default() };
    crate::features::annotate::on_key(s, &esc)
}

/// Keys for the open view (←/→/Esc); called first by the app's `on_key`. True consumes.
pub fn on_key(s: &mut AppState, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if open_id(s).is_none() {
        return false;
    }
    let plain = !(ev.mods.cmd || ev.mods.ctrl || ev.mods.alt);
    match ev.key {
        Key::Escape => {
            if annotate_takes_escape(s) {
                return true;
            }
            morph_motion(tx);
            close(s);
            true
        }
        Key::Left if plain => {
            step(s, -1);
            true
        }
        Key::Right if plain => {
            step(s, 1);
            true
        }
        Key::Up if plain => neighbours::go_up_down(s, false),
        Key::Down if plain => neighbours::go_up_down(s, true),
        _ => false,
    }
}

/// The fullscreen page when open, drawn above everything.
pub fn overlay(s: &AppState, cx: &mut Cx<AppState>, p: &Palette) -> Option<El> {
    let id = open_id(s)?.to_string();
    let d = s.snap().design(&id)?;
    let t = crate::cards::theme(s, cx);
    Some(view::page(s, cx, &d, p, &t))
}

/// Every frame, from `AppState::poll`: drives the live web view and applies the keys it consumed
/// (Esc, ←/→ while the page has focus). Returns whether the view must be rebuilt.
pub fn poll(s: &mut AppState, clock: f64, tx: &mut Tx) -> bool {
    let snap = s.store().snapshot();
    let open = open_id(s).map(str::to_string);
    let asks = s.fullscreen.live.poll(&snap, open.as_deref(), clock, tx);
    let mut changed = !asks.is_empty();
    changed |= note::poll(s, open.as_deref(), clock, tx);
    // A sharp picture under the morph and wherever the live view isn't up (yet): the card's
    // picture is sized for the canvas zoom, far smaller than this box.
    if let Some(id) = open.as_deref() {
        let w = s.fullscreen.box_w.get();
        if w > 0.0 {
            changed |= s.cards.thumbs.request_extra(&snap, id, (w * crate::cards::thumbs::BACKING).ceil() as u32);
        }
        // The Neighbours chips' pictures: the graph scan only when the design or data changed.
        let key = (id.to_string(), snap.version);
        if s.fullscreen.neighbour_ids.0 != key {
            let ids = snap.design(id).map(|d| neighbours::Neighbours::of(&snap, &d).ids()).unwrap_or_default();
            s.fullscreen.neighbour_ids = (key, ids);
        }
        for n in s.fullscreen.neighbour_ids.1.clone() {
            changed |= s.cards.thumbs.request_extra(&snap, &n, neighbours::CHIP_OUT_W);
        }
    }
    for a in asks {
        match a {
            live::KeyAsk::Close => {
                if annotate_takes_escape(s) {
                    changed = true;
                    continue;
                }
                morph_motion(tx);
                close(s);
            }
            live::KeyAsk::Step(d) => step(s, d),
        }
    }
    changed
}
