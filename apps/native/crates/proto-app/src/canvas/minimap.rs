//! The glass minimap (reference `glass-minimap.tsx` around React Flow's `<MiniMap>`): a 176×116
//! map of the space's nodes and the visible viewport in a GLASS panel, bottom-right of the canvas
//! and clear of the floating inspector. It shows while the camera moves, while the pointer is near
//! the corner, and for 3.5 s after the space opens, then fades out 1.2 s later. Dragging on it pans
//! (the viewport follows the pointer, as React Flow's `pannable`), the wheel zooms about the
//! canvas centre (`zoomable`). No zoom % (the user didn't want one); the settled zoom only feeds the
//! zoom pill's limits.
//!
//! Live-only: the map is a function of the camera (React Flow fits the node bounds *and* the
//! viewport), so `poll` (before every frame) recomputes it and sets Live values for the node
//! layer's offset and scale, the viewport rect and the panel's opacity. A pan or pinch never re-runs
//! the view. The view re-runs only when the node set changes, or once a zoom has settled (the zoom buttons' limits).

use std::cell::RefCell;
use std::rc::Rc;

use rune::{Color, Cx, Drag, El, Live, Phase, Shadow, Spring, Tx, WheelEvent, el};

use crate::app::insets::{EDGE, INSPECTOR_OVERLAY};
use crate::app::state::AppState;
use crate::app::style::{Palette, glass};

/// The map's size (React Flow `style={{ width: 176, height: 116 }}`).
pub const MAP_W: f32 = 176.0;
pub const MAP_H: f32 = 116.0;
/// Glass padding around the map (`p-1.5`) and corner radius (`rounded-[14px]`).
pub const PAD: f32 = 6.0;
pub const RADIUS: f32 = 14.0;
/// Distance from the canvas' right edge: clear of the inspector (`md:!mr-[388px]`).
pub const RIGHT: f32 = INSPECTOR_OVERLAY + EDGE;
pub const BOTTOM: f32 = EDGE;
/// React Flow's `offsetScale`: margin around the content in map px.
const OFFSET_SCALE: f32 = 5.0;
/// Seconds after the last pan/zoom (or leaving the corner) before it fades out.
pub const HIDE_AFTER: f64 = 1.2;
/// Shown this long after the space opens, so people learn it exists.
pub const INTRO: f64 = 3.5;
/// The hot corner: this much of the free canvas' bottom-right reveals the map.
pub const CORNER: [f32; 2] = [230.0, 170.0];
/// The zoom label updates once a zoom has rested this long (no re-run per pinch frame).
const LABEL_SETTLE: f64 = 0.15;
/// React Flow's `maskStrokeWidth={2}` in map px: 2 world units × viewScale × the map's fit scale,
/// which `mapping` makes constant (MAP_H / (MAP_H + 2·OFFSET_SCALE) · 2).
const MASK_STROKE: f32 = 2.0 * MAP_H / (MAP_H + 2.0 * OFFSET_SCALE);

/// World → map: `map = (world - origin) * scale + offset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mapping {
    pub origin: [f32; 2],
    pub scale: f32,
    pub offset: [f32; 2],
    /// React Flow's `viewScale`: world units per map px before the fit (drag speed).
    pub view_scale: f32,
}

impl Mapping {
    pub fn to_map(&self, p: [f32; 2]) -> [f32; 2] {
        [(p[0] - self.origin[0]) * self.scale + self.offset[0], (p[1] - self.origin[1]) * self.scale + self.offset[1]]
    }
}

/// React Flow's minimap geometry: the viewBox around the union of the node bounds and the visible
/// world rect `view`, padded by `offsetScale`, fitted into the map (SVG `xMidYMid meet`).
pub fn mapping(nodes: Option<[f32; 4]>, view: [f32; 4]) -> Mapping {
    let b = match nodes {
        Some(n) => union(n, view),
        None => view,
    };
    let (bw, bh) = (b[2].max(1e-3), b[3].max(1e-3));
    let view_scale = (bw / MAP_W).max(bh / MAP_H);
    let (vw, vh) = (view_scale * MAP_W, view_scale * MAP_H);
    let off = OFFSET_SCALE * view_scale;
    let x = b[0] - (vw - bw) / 2.0 - off;
    let y = b[1] - (vh - bh) / 2.0 - off;
    let (w, h) = (vw + 2.0 * off, vh + 2.0 * off);
    let scale = (MAP_W / w).min(MAP_H / h);
    Mapping { origin: [x, y], scale, offset: [(MAP_W - w * scale) / 2.0, (MAP_H - h * scale) / 2.0], view_scale }
}

fn union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let (l, t) = (a[0].min(b[0]), a[1].min(b[1]));
    let (r, btm) = ((a[0] + a[2]).max(b[0] + b[2]), (a[1] + a[3]).max(b[1] + b[3]));
    [l, t, r - l, btm - t]
}

/// Whether the map shows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Vis {
    /// Before the first poll (the intro starts there).
    #[default]
    Start,
    /// Showing until the clock reaches `hide_at`.
    Shown { hide_at: f64 },
    /// The pointer is in the corner: showing until it leaves.
    Hovered,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VisEvent {
    /// The camera moved (pan, zoom, fit).
    Moved,
    CornerIn,
    CornerOut,
    /// Time passed (a poll).
    Tick,
}

/// The visibility machine. Returns the wake deadline the new state needs, if any.
pub fn step(v: &mut Vis, e: VisEvent, now: f64) -> Option<f64> {
    *v = match (*v, e) {
        (Vis::Start, _) => Vis::Shown { hide_at: now + INTRO },
        (Vis::Hovered, VisEvent::CornerOut) => Vis::Shown { hide_at: now + HIDE_AFTER },
        (Vis::Hovered, _) => Vis::Hovered,
        (_, VisEvent::CornerIn) => Vis::Hovered,
        (_, VisEvent::Moved) => Vis::Shown { hide_at: now + HIDE_AFTER },
        (Vis::Shown { hide_at }, VisEvent::Tick) if now >= hide_at => Vis::Hidden,
        (v, _) => v,
    };
    match *v {
        Vis::Shown { hide_at } => Some(hide_at),
        _ => None,
    }
}

/// A drag on the map, as a state machine: the last cumulative drag delta seen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum MapDrag {
    #[default]
    Idle,
    Panning { last: [f32; 2] },
}

/// The camera the visibility timer last saw.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Synced {
    pan: [f32; 2],
    zoom: f32,
}

/// What maps the camera onto the minimap's Lives, shared with the camera's follower so a pan or
/// pinch moves the map in the same frame as the canvas (a poll sync would trail by a frame:
/// input dispatches after `poll`).
struct Follow {
    nodes: Option<[f32; 4]>,
    size: [f32; 2],
    mapping: Mapping,
    nodes_offset: Live<[f32; 2]>,
    nodes_scale: Live<f32>,
    view: Live<[f32; 4]>,
}

impl Follow {
    /// Recompute the mapping for the camera `pan`/`zoom` and set the Lives.
    fn apply(&mut self, pan: [f32; 2], zoom: f32) {
        let size = if self.size[0] > 0.0 { self.size } else { [1280.0, 800.0] };
        let view = [-pan[0] / zoom, -pan[1] / zoom, size[0] / zoom, size[1] / zoom];
        let m = mapping(self.nodes, view);
        self.mapping = m;
        self.nodes_offset.set([m.offset[0] - m.origin[0] * m.scale, m.offset[1] - m.origin[1] * m.scale]);
        self.nodes_scale.set(m.scale);
        let p = m.to_map([view[0], view[1]]);
        self.view.set([p[0], p[1], view[2] * m.scale, view[3] * m.scale]);
    }
}

/// The minimap's state (in `CanvasState::minimap`).
pub struct Minimap {
    vis: Vis,
    synced: Option<Synced>,
    follow: Rc<RefCell<Follow>>,
    /// The camera calls `follow` on every set (installed on the first poll).
    following: bool,
    /// Hash of the node rects the view last drew.
    built_nodes: u64,
    /// The zoom the label (and the zoom buttons' limits) show, and when a newer one lands.
    label_zoom: f32,
    label_due: Option<f64>,
    drag: MapDrag,
    pub live_nodes_offset: Live<[f32; 2]>,
    pub live_nodes_scale: Live<f32>,
    pub live_view: Live<[f32; 4]>,
    pub live_opacity: Live<f32>,
    pub live_lift: Live<[f32; 2]>,
}

impl Default for Minimap {
    fn default() -> Self {
        let (nodes_offset, nodes_scale, view) = (Live::new([0.0, 0.0]), Live::new(1.0), Live::new([0.0; 4]));
        Minimap {
            vis: Vis::Start,
            synced: None,
            following: false,
            built_nodes: 0,
            label_zoom: 1.0,
            label_due: None,
            drag: MapDrag::Idle,
            live_nodes_offset: nodes_offset.clone(),
            live_nodes_scale: nodes_scale.clone(),
            live_view: view.clone(),
            live_opacity: Live::new(1.0),
            live_lift: Live::new([0.0, 0.0]),
            follow: Rc::new(RefCell::new(Follow {
                nodes: None,
                size: [0.0, 0.0],
                mapping: mapping(None, [0.0, 0.0, 1.0, 1.0]),
                nodes_offset,
                nodes_scale,
                view,
            })),
        }
    }
}

impl Minimap {
    /// The zoom as last settled (for the label and the zoom buttons' limits).
    pub fn settled_zoom(&self) -> f32 {
        self.label_zoom
    }

    pub fn vis(&self) -> Vis {
        self.vis
    }

    /// The current world → map mapping.
    pub fn mapping(&self) -> Mapping {
        self.follow.borrow().mapping
    }

    /// A new space: the intro plays again and the map refits.
    pub fn on_enter_space(&mut self) {
        self.vis = Vis::Start;
        self.synced = None;
    }

    fn apply_vis(&self) {
        let on = !matches!(self.vis, Vis::Hidden);
        self.live_opacity.set(if on { 1.0 } else { 0.0 });
        // `translate-y-1.5` while hidden.
        self.live_lift.set([0.0, if on { 0.0 } else { 6.0 }]);
    }
}

/// The union of the nodes' world rects, and an order-independent hash of them.
fn node_bounds(s: &AppState) -> (Option<[f32; 4]>, u64) {
    use std::hash::{Hash, Hasher};
    let mut b: Option<[f32; 4]> = None;
    let mut sig = 0u64;
    for (id, (_, r)) in &s.canvas.nodes.rects {
        b = Some(b.map_or(*r, |b| union(b, *r)));
        let mut h = std::collections::hash_map::DefaultHasher::new();
        id.hash(&mut h);
        r.map(f32::to_bits).hash(&mut h);
        sig = sig.wrapping_add(h.finish());
    }
    (b, sig)
}

/// From `AppState::poll`, before every frame: install the camera follower, refit the map when the
/// nodes or the canvas size change, run the visibility timer, settle the zoom label. True only when
/// the view must re-run (the node set changed, or the label's zoom).
pub fn poll(s: &mut AppState, clock: f64, tx: &mut Tx) -> bool {
    let (nodes, sig) = node_bounds(s);
    let (pan, zoom, size) = (s.canvas.camera.pan, s.canvas.camera.zoom, s.canvas.size);
    if !s.canvas.minimap.following {
        let f = s.canvas.minimap.follow.clone();
        s.canvas.camera.set_follower(Some(Rc::new(move |pan, zoom| f.borrow_mut().apply(pan, zoom))));
        s.canvas.minimap.following = true;
    }
    let mm = &mut s.canvas.minimap;
    {
        let mut f = mm.follow.borrow_mut();
        if f.nodes != nodes || f.size != size || mm.synced.is_none() {
            (f.nodes, f.size) = (nodes, size);
            f.apply(pan, zoom);
        }
    }
    let cur = Synced { pan, zoom };
    let moved = mm.synced.is_some_and(|p| p != cur);
    let zoomed = mm.synced.is_some_and(|p| p.zoom != zoom);
    mm.synced = Some(cur);

    let before = mm.vis;
    if moved {
        step(&mut mm.vis, VisEvent::Moved, clock);
    }
    if let Some(due) = step(&mut mm.vis, VisEvent::Tick, clock) {
        tx.wake_at(due);
    }
    if mm.vis != before {
        mm.apply_vis();
    }

    let mut rerun = false;
    if zoomed {
        mm.label_due = Some(clock + LABEL_SETTLE);
    }
    match mm.label_due {
        Some(due) if clock >= due => {
            mm.label_due = None;
            if mm.label_zoom != zoom {
                mm.label_zoom = zoom;
                rerun = true;
            }
        }
        Some(due) => tx.wake_at(due),
        // First poll, or a zoom set while nothing ran the timer (e.g. a fit before the first frame).
        None if mm.label_zoom != zoom && !zoomed => {
            mm.label_zoom = zoom;
            rerun = true;
        }
        None => {}
    }
    if sig != mm.built_nodes {
        mm.built_nodes = sig;
        rerun = true;
    }
    rerun
}

/// The hot corner and the minimap panel, for the canvas element (after the world).
pub fn view(s: &AppState, _cx: &mut Cx<AppState>, p: &Palette) -> El {
    let mm = &s.canvas.minimap;
    let fg = p.fg;
    let blocks = s.canvas.nodes.rects.values().map(|(_, r)| {
        // React Flow `nodeBorderRadius={40}`, in world units, so it scales with the map.
        el().pin(Some(r[0]), Some(r[1]), None, None).size(r[2], r[3]).radius(40.0).fill(fg.alpha(0.22))
    });
    let layer = blocks.fold(
        el().key("minimap-nodes")
            .pin(Some(0.0), Some(0.0), None, None)
            .size(MAP_W, MAP_H)
            .origin(0.0, 0.0)
            .live_offset(&mm.live_nodes_offset)
            .live_scale(&mm.live_nodes_scale),
        |l, b| l.child(b),
    );
    // The mask: everything outside the viewport dims (background/45%) under a foreground/55% stroke.
    let mask = if p.dark { rune::color::hex("#0a0a0a").expect("valid hex") } else { Color::WHITE };
    let view_rect = el().key("minimap-view")
        .pin(Some(0.0), Some(0.0), None, None)
        .live_rect(&mm.live_view)
        .border(MASK_STROKE, fg.alpha(0.55))
        .box_shadow(Shadow { x: 0.0, y: 0.0, blur: 0.0, spread: 4000.0, color: mask.alpha(0.45), inset: false });
    let map = el().key("minimap-map")
        .size(MAP_W, MAP_H)
        .clip_rounded(0.0)
        .child(layer)
        .child(view_rect);
    let fade = Spring::new(0.3, 0.0);
    let panel = glass(el(), p, 0.5)
        .key("minimap")
        .role("img")
        .label("Canvas minimap")
        .pin(None, None, Some(if s.canvas.inspector_hidden { EDGE } else { RIGHT }), Some(BOTTOM))
        .padding(PAD)
        .radius(RADIUS)
        .clip_rounded(RADIUS)
        .live_opacity(mm.live_opacity.smooth(fade))
        .live_offset(mm.live_lift.smooth(fade))
        .on_drag(|s: &mut AppState, d: &mut Drag, tx: &mut Tx| {
            map_drag(s, d);
            tx.live_only();
        })
        .on_wheel(|s: &mut AppState, e: WheelEvent, tx: &mut Tx| {
            map_wheel(s, &e);
            tx.live_only();
            true
        })
        .child(map);
    // The hot corner: not a hit target, so the canvas under it keeps its drags and scrolls.
    let corner = el().key("minimap-corner")
        .pin(None, None, Some(if s.canvas.inspector_hidden { 0.0 } else { INSPECTOR_OVERLAY }), Some(0.0))
        .size(CORNER[0], CORNER[1])
        .on_hover_change(|s: &mut AppState, inside: bool, tx: &mut Tx| {
            let mm = &mut s.canvas.minimap;
            let before = mm.vis;
            let due = step(&mut mm.vis, if inside { VisEvent::CornerIn } else { VisEvent::CornerOut }, tx.clock());
            if mm.vis != before {
                mm.apply_vis();
            }
            if let Some(due) = due {
                tx.wake_at(due);
            }
            tx.live_only();
        });
    el().key("minimap-layer").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).child(corner).child(panel)
}

/// React Flow's minimap pan: the viewport follows the pointer, `viewScale` world units per map px.
pub fn map_drag(s: &mut AppState, d: &Drag) {
    let mm = &mut s.canvas.minimap;
    match (d.phase, mm.drag) {
        (Phase::Down, _) => mm.drag = MapDrag::Panning { last: [0.0, 0.0] },
        (_, MapDrag::Panning { last }) => {
            let delta = [d.dx - last[0], d.dy - last[1]];
            mm.drag = if d.phase == Phase::Up || d.cancelled() { MapDrag::Idle } else { MapDrag::Panning { last: [d.dx, d.dy] } };
            let k = mm.mapping().view_scale * s.canvas.camera.zoom;
            let pan = s.canvas.camera.pan;
            s.canvas.camera.set([pan[0] - delta[0] * k, pan[1] - delta[1] * k], s.canvas.camera.zoom);
        }
        _ => {}
    }
}

/// React Flow's minimap wheel: zoom about the canvas centre by `2^(-deltaY · k)` (k 0.002 for
/// pixel deltas, 0.05 per line, ×10 for a ctrl/pinch wheel on a Mac). `dy` here is `-deltaY`.
pub fn map_wheel(s: &mut AppState, e: &WheelEvent) {
    let k = if e.precise { 0.002 } else { 0.05 } * if e.mods.ctrl { 10.0 } else { 1.0 };
    let size = if s.canvas.size[0] > 0.0 { s.canvas.size } else { [1280.0, 800.0] };
    s.canvas.camera.zoom_about([size[0] / 2.0, size[1] / 2.0], (e.dy * k).exp2());
}

/// The fixture exploration, as the cards' scenes use it.
fn scene_fixture(s: &mut AppState) {
    s.use_store(crate::cards::fixture::store());
    s.enter_space(crate::cards::fixture::EXPLORATION, None);
}

/// `--snap` scenes for the minimap and the zoom controls (snaps render at the start of the intro,
/// so the map shows).
pub const SCENES: &[(&str, fn(&mut AppState))] = &[
    ("minimap", scene_fixture),
    ("minimap-light", |s| {
        scene_fixture(s);
        s.theme = crate::app::state::ThemePref::Light;
    }),
    // Zoomed in on part of the content, so the mask dims the rest of the map.
    ("minimap-zoomed-in", |s| {
        scene_fixture(s);
        s.canvas.fit_pending = false;
        s.canvas.camera.set([-150.0, 120.0], 1.2);
    }),
    ("controls-locked", |s| {
        scene_fixture(s);
        s.canvas.controls.toggle_lock();
    }),
];

#[cfg(test)]
#[path = "minimap_tests.rs"]
pub(crate) mod tests;
