//! Nodes on the canvas: the frame around each card (select, double-press, drag to move) and the
//! world rects the canvas keeps for edges, drops and fitting.
//!
//! Dragging moves the pressed node, or the whole selection when the pressed node is in it. The
//! first move commits once (it binds the moving nodes to one shared live offset); every move after
//! that only sets the live offset (`live_only`). The release writes the new positions in one store
//! batch (proto-core's `ops::move_design` / `ops::move_portal`, the features' `on_extra_node_move`
//! for drafts) and drops the offset in the same instant commit, so nothing jumps.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use proto_core::{Point, ops};
use rune::{Drag, El, Live, Phase, Tx, el};

use super::NodeKind;
use crate::app::state::AppState;
use crate::app::style::Palette;

/// What moves on the canvas, including the features' draft nodes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mover {
    #[default]
    Design,
    Portal,
    Extra,
}

impl From<NodeKind> for Mover {
    fn from(k: NodeKind) -> Self {
        match k {
            NodeKind::Design => Mover::Design,
            NodeKind::Portal => Mover::Portal,
        }
    }
}

/// A node drag, as a state machine.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum NodeDrag {
    #[default]
    Idle,
    /// Pressed on a node; no travel yet (a click selects through `on_press`).
    Pressed { id: String },
    /// Moving `nodes` (with their start positions) by `delta` world points.
    /// `edges`: an edge touches a moving node, so each move re-runs the view to redraw it.
    Moving { nodes: Vec<(Mover, String, [f32; 2])>, delta: [f32; 2], edges: bool },
}

/// Node state the canvas keeps (in `CanvasState::nodes`).
pub struct Nodes {
    /// Each node's world rect `[x, y, w, h]` and kind, from its last layout.
    pub rects: HashMap<String, (Mover, [f32; 4])>,
    pub drag: NodeDrag,
    /// The moving nodes' shared offset in world points.
    pub drag_live: Live<[f32; 2]>,
}

impl Default for Nodes {
    fn default() -> Self {
        Nodes { rects: HashMap::new(), drag: NodeDrag::Idle, drag_live: Live::new([0.0, 0.0]) }
    }
}

/// What the node frames depend on besides the cards' own data: the set of nodes being dragged
/// (bound to the live offset). Card layers that memoise their node frames include it in the key.
pub fn frame_key(s: &AppState) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if let NodeDrag::Moving { nodes, .. } = &s.canvas.nodes.drag {
        for (_, id, _) in nodes {
            id.hash(&mut h);
        }
    }
    h.finish()
}


/// Travel (screen points) before a press on a node becomes a move.
const DRAG_SLOP: f32 = 3.0;

/// What the canvas view tells the frames it builds (set for the duration of `canvas::view`, so
/// `node_frame` keeps the plain signature the cards call).
#[derive(Default)]
struct FrameCtx {
    moving: HashSet<String>,
    live: Option<Live<[f32; 2]>>,
    palette: Option<Palette>,
    /// For the frames' accessible names (card titles, space names).
    snap: Option<std::sync::Arc<proto_core::Snapshot>>,
}

thread_local! {
    static CTX: RefCell<FrameCtx> = RefCell::new(FrameCtx::default());
}

/// Run `f` (which builds the node layer) with the frame context of `s`.
pub(super) fn with_ctx<T>(s: &AppState, p: &Palette, f: impl FnOnce() -> T) -> T {
    let moving = match &s.canvas.nodes.drag {
        NodeDrag::Moving { nodes, .. } => nodes.iter().map(|(_, id, _)| id.clone()).collect(),
        _ => HashSet::new(),
    };
    let snap = Some(s.data.snap.clone());
    CTX.with(|c| *c.borrow_mut() = FrameCtx { moving, live: Some(s.canvas.nodes.drag_live.clone()), palette: Some(*p), snap });
    let out = f();
    CTX.with(|c| *c.borrow_mut() = FrameCtx::default());
    out
}

fn key(kind: Mover, id: &str) -> String {
    match kind {
        Mover::Design => format!("design:{id}"),
        Mover::Portal => format!("portal:{id}"),
        Mover::Extra => format!("extra:{id}"),
    }
}

/// The frame shared by cards and drafts: placement, layout tracking, select, drag.
pub(super) fn frame(kind: Mover, id: &str, pos: [f32; 2], content: El) -> El {
    // A Tab stop named after its card (VoiceOver: "Breadcrumb rail, button"); arrows move it.
    let name = CTX.with(|c| {
        let c = c.borrow();
        let snap = c.snap.as_ref()?;
        match kind {
            Mover::Design => snap.design(id).map(|d| d.title.clone()),
            Mover::Portal => snap.space(id).map(|sp| sp.name.clone()),
            Mover::Extra => None,
        }
    });
    let mut e = el().key(key(kind, id)).pin(Some(pos[0]), Some(pos[1]), None, None)
        .role("button")
        .label(name.unwrap_or_else(|| if kind == Mover::Extra { "Draft".into() } else { id.to_string() }));
    let moving = CTX.with(|c| {
        let c = c.borrow();
        c.moving.contains(id).then(|| c.live.clone()).flatten()
    });
    if let Some(live) = moving {
        e = e.live_offset(&live);
    }
    let handles = match (kind, CTX.with(|c| c.borrow().palette)) {
        (Mover::Design, Some(p)) => super::edges::handles(id, &p),
        _ => Vec::new(),
    };
    let (layout_id, select_id, drag_id) = (id.to_string(), id.to_string(), id.to_string());
    e.on_layout(move |s: &mut AppState, r: [f32; 4]| {
        s.canvas.nodes.rects.insert(layout_id.clone(), (kind, r));
    })
    .on_press_tx(move |s: &mut AppState, tx: &mut Tx| {
        // Locked (zoom controls): nodes aren't selectable.
        if !s.canvas.controls.interactive() {
            return;
        }
        let m = tx.mods();
        select(s, &select_id, m.shift || m.cmd || m.ctrl)
    })
    .on_drag(move |s: &mut AppState, d: &mut Drag, tx: &mut Tx| {
        // Locked: nodes don't move; a drag on one pans the canvas (React Flow's non-draggable nodes).
        if !s.canvas.controls.interactive() {
            return super::canvas_drag(s, d, tx);
        }
        on_drag(s, kind, &drag_id, d, tx)
    })
    .child(content)
    .children(handles)
}

/// A press selects the node; ⇧ toggles it in the selection.
fn select(s: &mut AppState, id: &str, shift: bool) {
    let sel = &mut s.route.selected;
    if shift {
        if let Some(i) = sel.iter().position(|x| x == id) {
            sel.remove(i);
        } else {
            sel.push(id.to_string());
        }
    } else if !(sel.len() > 1 && sel.iter().any(|x| x == id)) {
        // A plain press on a node of a multi-selection keeps it (it may start a group drag).
        *sel = vec![id.to_string()];
    }
}

fn on_drag(s: &mut AppState, kind: Mover, id: &str, d: &Drag, tx: &mut Tx) {
    let zoom = s.canvas.camera.zoom;
    let world_delta = [d.dx / zoom, d.dy / zoom];
    if d.cancelled() {
        s.canvas.nodes.drag = NodeDrag::Idle;
        s.canvas.nodes.drag_live.set([0.0, 0.0]);
        return;
    }
    match (d.phase, &mut s.canvas.nodes.drag) {
        (Phase::Down, _) => {
            s.canvas.camera.stop();
            s.canvas.nodes.drag = NodeDrag::Pressed { id: id.to_string() };
            tx.live_only();
        }
        (Phase::Up, NodeDrag::Moving { .. }) => {
            let NodeDrag::Moving { nodes, .. } = std::mem::take(&mut s.canvas.nodes.drag) else { unreachable!() };
            commit_move(s, &nodes, world_delta);
            s.canvas.nodes.drag_live.set([0.0, 0.0]);
            tx.instant();
        }
        (Phase::Up, _) => s.canvas.nodes.drag = NodeDrag::Idle,
        (_, NodeDrag::Moving { delta, edges, .. }) => {
            *delta = world_delta;
            let edges = *edges;
            s.canvas.nodes.drag_live.set(world_delta);
            if !edges {
                tx.live_only();
            }
        }
        (_, NodeDrag::Pressed { .. }) if d.dx.hypot(d.dy) >= DRAG_SLOP => {
            // Start moving: the selection if the node is in it, else just this node (selected).
            if !s.route.selected.iter().any(|x| x == id) {
                s.route.selected = vec![id.to_string()];
            }
            let nodes: Vec<_> = s.route.selected.iter()
                .filter_map(|sid| s.canvas.nodes.rects.get(sid).map(|(k, r)| (*k, sid.clone(), [r[0], r[1]])))
                .collect();
            let nodes = if nodes.is_empty() {
                s.canvas.nodes.rects.get(id).map(|(_, r)| vec![(kind, id.to_string(), [r[0], r[1]])]).unwrap_or_default()
            } else {
                nodes
            };
            s.canvas.nodes.drag_live.set(world_delta);
            let moving: HashSet<&str> = nodes.iter().map(|(_, id, _)| id.as_str()).collect();
            let edges = super::edges::touches(s, &moving);
            s.canvas.nodes.drag = NodeDrag::Moving { nodes, delta: world_delta, edges };
            tx.instant();
        }
        _ => tx.live_only(),
    }
}

/// Write the moved nodes' new positions: one store batch for cards and portals, then the drafts.
fn commit_move(s: &mut AppState, nodes: &[(Mover, String, [f32; 2])], delta: [f32; 2]) {
    let snap = s.store().snapshot();
    let mut changes = Vec::new();
    let mut extras = Vec::new();
    for (kind, id, start) in nodes {
        let pos = [start[0] + delta[0], start[1] + delta[1]];
        let point = Point { x: pos[0].round() as f64, y: pos[1].round() as f64 };
        match kind {
            Mover::Design => changes.extend(ops::move_design(&snap, id, point)),
            Mover::Portal => changes.push(ops::move_portal(id, point)),
            Mover::Extra => extras.push((id.clone(), pos)),
        }
        if let Some((_, r)) = s.canvas.nodes.rects.get_mut(id) {
            (r[0], r[1]) = (pos[0], pos[1]);
        }
    }
    if !changes.is_empty()
        && let Err(e) = s.apply(changes, "drag")
    {
        eprintln!("canvas: moving nodes failed: {e}");
    }
    for (id, pos) in extras {
        crate::features::on_extra_node_move(s, &id, pos);
    }
}

/// Arrow keys move the selected nodes while a node frame has keyboard focus: 5 points a press,
/// 20 with ⇧ (React Flow). One store write per press. Called from the app's `on_key`.
pub fn on_key(s: &mut AppState, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
    use rune::Key;
    let focused_node = tx.focused().is_some_and(|k| k.starts_with("design:") || k.starts_with("portal:") || k.starts_with("extra:"));
    // Locked (zoom controls' lock): nodes don't move, as React Flow doesn't nudge non-draggable ones.
    if !focused_node || s.route.selected.is_empty() || !s.canvas.controls.interactive() {
        return false;
    }
    let step = if ev.mods.shift { 20.0 } else { 5.0 };
    let d = match ev.key {
        Key::Left => [-step, 0.0],
        Key::Right => [step, 0.0],
        Key::Up => [0.0, -step],
        Key::Down => [0.0, step],
        _ => return false,
    };
    let nodes: Vec<_> = s.route.selected.iter()
        .filter_map(|id| s.canvas.nodes.rects.get(id).map(|(k, r)| (*k, id.clone(), [r[0], r[1]])))
        .collect();
    if nodes.is_empty() {
        return false;
    }
    commit_move(s, &nodes, d);
    tx.instant();
    true
}
