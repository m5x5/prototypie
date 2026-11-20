//! The canvas engine: camera (pan, zoom, pinch), the dot grid and the world layer the cards draw
//! into. The world is one element under the camera's live offset and scale; cards put their
//! nodes in it at world coordinates (see `crate::cards::world_nodes`).
//!
//! Input: a two-finger scroll or wheel pans, a pinch (or ⌘/Ctrl + wheel) zooms about the pointer,
//! a drag on empty canvas pans. Handlers that only move the camera say `tx.live_only()`.

pub mod camera;
mod dots;
pub mod inertia;
pub mod edges;
pub mod minimap;
pub mod nodes;
pub mod zoom_controls;

pub use nodes::frame_key;
#[cfg(test)]
mod tests;

use rune::{Cx, Drag, El, Live, MagnifyEvent, Phase, Tx, WheelEvent, el};

use crate::app::insets;
use crate::app::state::AppState;
use crate::app::style::Palette;
use camera::Camera;

/// What a node on the canvas is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeKind {
    Design,
    /// A nested space's portal card (project or exploration).
    Portal,
}

/// A gesture on empty canvas: a drag pans; with ⇧ (React Flow's selection key) it draws a
/// marquee that selects the nodes fully inside it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Gesture {
    #[default]
    Idle,
    Panning { pan_at_press: [f32; 2] },
    /// From canvas point `start`; the box is shown through `CanvasState::marquee_live` once
    /// `shown` (the first move mounts it).
    Marquee { start: [f32; 2], shown: bool },
}

/// Travel (points) after which a press on empty canvas counts as a pan, not a click.
const CLICK_SLOP: f32 = 3.0;
/// React Flow's animated edge: the dash moves 10pt per 0.5s.
const DASH_SPEED: f32 = 20.0;
/// Animated edges redraw at this rate.
const DASH_FPS: f64 = 20.0;

pub struct CanvasState {
    pub camera: Camera,
    /// The canvas element's size in points, from its last layout (0 before the first frame).
    pub size: [f32; 2],
    pub gesture: Gesture,
    /// Frame the active space's content on the next poll (set on entering a space and when the
    /// store loads).
    pub fit_pending: bool,
    /// Node rects, the node drag (`nodes`).
    pub nodes: nodes::Nodes,
    /// A connection being dragged from a design's handle (`edges`).
    pub connect: Option<edges::Connect>,
    /// The last gesture on empty canvas travelled (a pan or a marquee): its release isn't a click.
    pub dragged: bool,
    /// The marquee box `[x, y, w, h]` in canvas points while one is drawn.
    pub marquee_live: Live<[f32; 4]>,
    /// The animated edges' dash phase (0..10), and whether the space has any (per snapshot).
    pub dash_phase: f32,
    animated: Option<(u64, String, bool)>,
    /// The glass minimap (`minimap`).
    pub minimap: minimap::Minimap,
    /// The zoom controls' lock (`zoom_controls`).
    pub controls: zoom_controls::Controls,
    /// The inspector sidebar is collapsed: fit, the minimap and the multi-selection bar use its width again.
    pub inspector_hidden: bool,
}

impl Default for CanvasState {
    fn default() -> Self {
        CanvasState {
            camera: Camera::default(),
            size: [0.0, 0.0],
            gesture: Gesture::Idle,
            fit_pending: false,
            nodes: nodes::Nodes::default(),
            connect: None,
            dragged: false,
            marquee_live: Live::new([0.0, 0.0, 0.0, 0.0]),
            dash_phase: 0.0,
            animated: None,
            minimap: minimap::Minimap::default(),
            controls: zoom_controls::Controls::default(),
            inspector_hidden: false,
        }
    }
}

impl CanvasState {
    /// Called by `AppState::enter` when the active space changes; M2 restores the space's saved
    /// viewport or fits its content here.
    pub fn on_enter_space(&mut self) {
        self.gesture = Gesture::Idle;
        self.fit_pending = true;
        self.nodes.rects.clear();
        self.nodes.drag = nodes::NodeDrag::Idle;
        self.connect = None;
        self.minimap.on_enter_space();
    }

    /// Frame world `bounds` in the canvas area left free by the header and the inspector.
    pub fn fit(&mut self, bounds: [f32; 4], padding: f32, inspector_open: bool) {
        let size = if self.size[0] > 0.0 { self.size } else { [1280.0, 800.0] };
        self.camera.fit(bounds, size, insets::canvas_insets(inspector_open), padding, 1.0);
    }

}

/// A drag on empty canvas: pan, or with ⇧ a marquee. Pans are live-only; a marquee commits when it
/// appears and when it selects, and is live-only in between.
fn canvas_drag(s: &mut AppState, d: &Drag, tx: &mut Tx) {
    let c = &mut s.canvas;
    if d.phase == Phase::Down {
        // A press stops a zoom coast at once.
        c.camera.stop();
        c.dragged = false;
        // Locked (zoom controls): no marquee selection, a ⇧-drag pans like any drag.
        c.gesture = if tx.mods().shift && c.controls.interactive() {
            Gesture::Marquee { start: [d.start_x, d.start_y], shown: false }
        } else {
            Gesture::Panning { pan_at_press: c.camera.pan }
        };
        c.marquee_live.set([d.start_x, d.start_y, 0.0, 0.0]);
        tx.live_only();
        return;
    }
    let travelled = d.dx.hypot(d.dy) >= CLICK_SLOP;
    c.dragged |= travelled;
    let done = d.phase == Phase::Up || d.cancelled();
    match c.gesture {
        Gesture::Panning { pan_at_press } => {
            let pan = if d.cancelled() { pan_at_press } else { [pan_at_press[0] + d.dx, pan_at_press[1] + d.dy] };
            c.camera.set(pan, c.camera.zoom);
            if done {
                c.gesture = Gesture::Idle;
            }
            tx.live_only();
        }
        Gesture::Marquee { start, shown } => {
            let end = [start[0] + d.dx, start[1] + d.dy];
            let r = [start[0].min(end[0]), start[1].min(end[1]), (end[0] - start[0]).abs(), (end[1] - start[1]).abs()];
            c.marquee_live.set(r);
            if done {
                c.gesture = Gesture::Idle;
                if !d.cancelled() && travelled {
                    let (a, b) = (c.camera.world([r[0], r[1]]), c.camera.world([r[0] + r[2], r[1] + r[3]]));
                    let mut hit: Vec<(f32, f32, String)> = c.nodes.rects.iter()
                        .filter(|(_, (_, n))| n[0] >= a[0] && n[1] >= a[1] && n[0] + n[2] <= b[0] && n[1] + n[3] <= b[1])
                        .map(|(id, (_, n))| (n[1], n[0], id.clone()))
                        .collect();
                    hit.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)));
                    s.route.selected = hit.into_iter().map(|(_, _, id)| id).collect();
                }
            } else if shown {
                // Mounted already: a move only retargets the live box.
                tx.live_only();
            } else {
                // The first move commits once, which mounts the box.
                c.gesture = Gesture::Marquee { start, shown: true };
            }
        }
        Gesture::Idle => tx.live_only(),
    }
}

/// A click on empty canvas (no travel) clears the selection, as React Flow's `onPaneClick`.
fn canvas_click(s: &mut AppState) {
    if std::mem::take(&mut s.canvas.dragged) {
        return;
    }
    s.route.selected.clear();
}

/// The camera's wheel and pinch handlers, for the window's root element: the canvas fills the
/// window from its top-left, so root points are canvas points, and scrolls and pinches over the
/// header's fades (siblings of the canvas, not inside it) still pan and zoom, as in the reference.
/// Scroll areas inside panels keep their own scrolls (the innermost handler wins).
pub fn gestures(root: El) -> El {
    root.on_wheel(|s: &mut AppState, e: WheelEvent, tx: &mut Tx| {
        s.canvas.camera.wheel(&e);
        tx.live_only();
        true
    })
    .on_magnify(|s: &mut AppState, e: MagnifyEvent, tx: &mut Tx| {
        s.canvas.camera.magnify(&e, tx.clock());
        tx.live_only();
        // A coast steps from the poll (`poll_inertia`); ask for the first frame.
        if s.canvas.camera.coasting() {
            tx.wake_after(std::time::Duration::ZERO);
        }
        true
    })
}

/// The canvas: background, dots and the world layer, filling its parent. Drags on empty canvas
/// pan; wheel and pinch come from `gestures` on the root.
pub fn view(s: &AppState, cx: &mut Cx<AppState>, p: &Palette) -> El {
    let size = cx.layout_size("canvas").unwrap_or([1440.0, 900.0]);
    let content = nodes::with_ctx(s, p, || {
        s.canvas.camera.reduced_motion.set(cx.is_reduced_motion());
        let mut v = edges::layer(s, p, cx.is_reduced_motion());
        v.extend(nodes(s, cx));
        v.extend(crate::features::extra_nodes(s, p).into_iter()
            .map(|n| nodes::frame(nodes::Mover::Extra, &n.id, n.pos, n.content.size(n.size[0], n.size[1]))));
        v
    });
    let world = content.into_iter().fold(
        el().key("canvas-world")
            .pin(Some(0.0), Some(0.0), None, None)
            // Sized, not 0×0: under a changing live_scale, Rune (rev 4a2e437) draws the boxes inside
            // a zero-size parent with the scale applied twice while text gets it once ("cards grow,
            // text shrinks" mid-pinch). Layout rects are right either way; only the drawing differs.
            .size(size[0], size[1])
            .origin(0.0, 0.0)
            .live_offset(&s.canvas.camera.live_pan)
            .live_scale(&s.canvas.camera.live_zoom),
        |w, node| w.child(node),
    );
    el().key("canvas")
        .role("group")
        .label("Canvas")
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(p.canvas_bg)
        .clip_soft(0.0)
        .on_layout(|s: &mut AppState, r: [f32; 4]| s.canvas.size = [r[2], r[3]])
        .on_drag(|s: &mut AppState, d: &mut Drag, tx: &mut Tx| canvas_drag(s, d, tx))
        .on_press(canvas_click)
        .child(dots::layer(size, p.dots, &s.canvas.camera))
        .child(world)
        .children(matches!(s.canvas.gesture, Gesture::Marquee { shown: true, .. }).then(|| {
            el().key("marquee").pin(Some(0.0), Some(0.0), None, None).size(1.0, 1.0)
                .live_rect(&s.canvas.marquee_live)
                .fill(crate::app::style::brand().alpha(0.08)).border(1.0, crate::app::style::brand().alpha(0.6))
                .inert()
        }))
        .child(zoom_controls::view(s, cx, p))
        .child(minimap::view(s, cx, p))
}

/// A node on the canvas: `content` (a card from `crate::cards`) placed at world `pos`, keyed
/// `"<kind>:<id>"`. The frame owns the node's input: a press selects it (⇧ toggles it in the
/// selection), a drag moves it (or the selection it belongs to) and writes the new positions on
/// release, a double-press calls `cards::on_node_double_press`. Design frames carry the edge
/// handles. Card controls inside keep their own presses (topmost wins).
pub fn node_frame(kind: NodeKind, id: &str, pos: [f32; 2], content: El) -> El {
    let open_id = id.to_string();
    nodes::frame(kind.into(), id, pos, content)
        .on_double_press_tx(move |s: &mut AppState, tx: &mut Tx| crate::cards::on_node_double_press(s, kind, &open_id, tx))
}

/// The world bounds `[x, y, w, h]` of a space's nodes (portal cards and design cards at their card
/// sizes), placed as `cards::world_nodes` places them. None for an empty space.
pub fn content_bounds(snap: &proto_core::Snapshot, space: &str) -> Option<[f32; 4]> {
    use crate::cards::{NODE_H, NODE_W, PORTAL_H, PORTAL_W};
    let mut b: Option<[f32; 4]> = None;
    let mut add = |x: f32, y: f32, w: f32, h: f32| {
        b = Some(match b {
            None => [x, y, x + w, y + h],
            Some([l, t, r, btm]) => [l.min(x), t.min(y), r.max(x + w), btm.max(y + h)],
        });
    };
    for (i, sp) in snap.child_spaces(space).iter().enumerate() {
        let [x, y] = snap.portal(&sp.id).map_or([i as f32 * (PORTAL_W + 80.0), 0.0], |p| [p.x as f32, p.y as f32]);
        add(x, y, PORTAL_W, PORTAL_H);
    }
    for d in snap.designs_in(space) {
        add(d.position.x as f32, d.position.y as f32, NODE_W, NODE_H);
    }
    b.map(|[l, t, r, btm]| [l, t, r - l, btm - t])
}

impl CanvasState {
    /// Whether `animated` is current for this snapshot version and space.
    pub fn dash_fresh(&self, version: u64, space: &str) -> bool {
        matches!(&self.animated, Some((v, sp, _)) if *v == version && sp == space)
    }

    /// Tick the animated edges' dash while the active space has any (20 fps). `has_animated` is
    /// given when `dash_fresh` was false. True when the phase moved (the edges redraw; under
    /// Reduce Motion the view draws them still).
    pub fn poll_dash(&mut self, version: u64, space: &str, has_animated: Option<bool>, clock: f64, tx: &mut Tx) -> bool {
        if let Some(has) = has_animated {
            self.animated = Some((version, space.to_string(), has));
        }
        let Some((_, _, true)) = self.animated else { return false };
        tx.wake_after(std::time::Duration::from_secs_f64(1.0 / DASH_FPS));
        let phase = ((clock * DASH_FPS).floor() / DASH_FPS) as f32 * DASH_SPEED % 10.0;
        // The dash runs from source to target: the pattern moves forward, so the phase falls.
        let phase = 10.0 - phase;
        let moved = (phase - self.dash_phase).abs() > 1e-3;
        self.dash_phase = phase;
        moved
    }

    /// Step a pinch's zoom coast (`inertia`) at the display's rate. Live-only: never re-runs the
    /// view; the % label and thumbnail resolution follow once the camera rests, as after any zoom.
    pub fn poll_inertia(&mut self, clock: f64, tx: &mut Tx) -> bool {
        if self.camera.tick(clock) {
            tx.wake_after(std::time::Duration::ZERO);
        }
        false
    }

    /// Run a pending fit for `space` once the data is there. Fits again after the first layout
    /// when the canvas' size wasn't known yet. True when the camera moved.
    pub fn poll_fit(&mut self, snap: &proto_core::Snapshot, space: &str, data_loading: bool) -> bool {
        if !self.fit_pending || data_loading {
            return false;
        }
        let sized = self.size[0] > 0.0;
        if let Some(bounds) = content_bounds(snap, space) {
            self.fit(bounds, 0.1, !self.inspector_hidden);
        }
        self.fit_pending = !sized;
        true
    }
}

/// The node layer's content. Tests can swap it (`bench::MODE`) to bisect the cost of cards.
fn nodes(s: &AppState, cx: &mut Cx<AppState>) -> Vec<El> {
    #[cfg(test)]
    if let Some(n) = bench::override_nodes(s, cx) {
        return n;
    }
    crate::cards::world_nodes(s, cx)
}

#[cfg(test)]
pub(crate) mod bench;

#[cfg(test)]
thread_local! {
    static VIEW_RUNS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The shell's view, counting runs (benches).
#[cfg(test)]
fn tests_view(s: &AppState, cx: &mut Cx<AppState>) -> El {
    VIEW_RUNS.with(|c| c.set(c.get() + 1));
    crate::shell::view(s, cx)
}
