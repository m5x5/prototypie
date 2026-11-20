//! Edges and connection handles, drawn in world space under the nodes (so they pan and zoom with
//! the camera for free).
//!
//! - Parent → child edges between designs of the active space, as React Flow's default bezier
//!   from the parent's bottom handle to the child's top handle; animated (dashed) while the child
//!   generates; one label per sibling round (the prompt, on the middle sibling's edge).
//! - The features' extra edges (draft and combined-base edges).
//! - Each design card's handles; dragging from the bottom one draws a preview edge and drops on
//!   empty canvas, a design or a draft (`features::on_handle_drop`).

use std::collections::HashMap;

use rune::{Color, Drag, El, LineCap, Path, Phase, Shape, TextStyle, Tx, canvas, el, text};

use super::nodes::Mover;
use crate::app::state::AppState;
use crate::app::style::Palette;
use crate::features::DropTarget;

const STROKE: f32 = 1.5;
const HANDLE: f32 = 8.0;
const LABEL: TextStyle = TextStyle::new(11.0, 500.0);

/// A handle drag in progress: from a design's bottom handle to `end` (world points).
#[derive(Clone, Debug, PartialEq)]
pub struct Connect {
    pub from: String,
    pub start: [f32; 2],
    pub end: [f32; 2],
}

fn edge_color(p: &Palette) -> Color {
    rune::color::hex(if p.dark { "#3e3e3e" } else { "#b1b1b7" }).unwrap()
}

/// React Flow's bezier for a bottom → top connection.
fn bezier(a: [f32; 2], b: [f32; 2]) -> Path {
    let off = |d: f32| if d >= 0.0 { 0.5 * d } else { 0.25 * 25.0 * (-d).sqrt() };
    let o = off(b[1] - a[1]);
    Path::new().move_to(a).cubic_to([a[0], a[1] + o], [b[0], b[1] - o], b)
}

/// Point `t` (0..1) of the bezier, for labels.
fn bezier_at(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    let off = |d: f32| if d >= 0.0 { 0.5 * d } else { 0.25 * 25.0 * (-d).sqrt() };
    let o = off(b[1] - a[1]);
    let (c1, c2) = ([a[0], a[1] + o], [b[0], b[1] - o]);
    let u = 1.0 - t;
    let f = |i: usize| u * u * u * a[i] + 3.0 * u * u * t * c1[i] + 3.0 * u * t * t * c2[i] + t * t * t * b[i];
    [f(0), f(1)]
}

/// A dashed version of the bezier (5 on, 5 off), sampled into short segments.
/// `phase` (0..10) shifts the pattern along the edge, which animates it (React Flow's `dashdraw`).
fn dashed(shape: Shape, a: [f32; 2], b: [f32; 2], c: Color, phase: f32) -> Shape {
    let steps = 64;
    let pts: Vec<[f32; 2]> = (0..=steps).map(|i| bezier_at(a, b, i as f32 / steps as f32)).collect();
    // The pattern position at the start is `phase`: a dash on [0, 5), a gap on [5, 10). Start that
    // far into the current piece, so the pattern slides along the edge as the phase grows.
    let p = phase.rem_euclid(10.0);
    let (mut on, mut run) = (p < 5.0, p % 5.0);
    let (mut shape, mut seg) = (shape, vec![pts[0]]);
    for w in pts.windows(2) {
        run += (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        seg.push(w[1]);
        if run >= 5.0 {
            if on && seg.len() >= 2 {
                shape = shape.stroke_with(std::mem::take(&mut seg), false, STROKE, LineCap::Butt, c);
            }
            seg = vec![w[1]];
            run = 0.0;
            on = !on;
        }
    }
    if on && seg.len() >= 2 {
        shape = shape.stroke_with(seg, false, STROKE, LineCap::Butt, c);
    }
    shape
}

/// Bottom-centre and top-centre handle points of a node rect (world).
fn anchors(kind: Mover, r: [f32; 4]) -> ([f32; 2], [f32; 2]) {
    let k = if kind == Mover::Portal { crate::canvas::NodeKind::Portal } else { crate::canvas::NodeKind::Design };
    let (top, bottom) = crate::cards::anchors(k, [r[2], r[3]]);
    ([r[0] + bottom[0], r[1] + bottom[1]], [r[0] + top[0], r[1] + top[1]])
}

struct EdgeSpec {
    from: String,
    to: String,
    dashed: bool,
    label: Option<String>,
}

fn edge_specs(s: &AppState) -> Vec<EdgeSpec> {
    let snap = s.snap();
    let designs: Vec<_> = snap.designs_in(&s.route.space).collect();
    let ids: std::collections::HashSet<&str> = designs.iter().map(|d| d.id.as_str()).collect();
    // A round's siblings share one prompt: label only the middle edge of each round.
    let mut rounds: HashMap<(String, String), Vec<(f64, String)>> = HashMap::new();
    for d in &designs {
        if let Some(p) = d.parent_id.as_deref().filter(|p| ids.contains(p)) {
            let key = (p.to_string(), d.instruction.clone().unwrap_or_default());
            rounds.entry(key).or_default().push((d.position.x, d.id.clone()));
        }
    }
    let labelled: std::collections::HashSet<String> = rounds.into_values().map(|mut sib| {
        sib.sort_by(|a, b| a.0.total_cmp(&b.0));
        sib[(sib.len() - 1) / 2].1.clone()
    }).collect();
    let mut out: Vec<EdgeSpec> = designs.iter()
        .filter_map(|d| {
            let p = d.parent_id.as_deref().filter(|p| ids.contains(p))?;
            let label = d.instruction.as_deref().filter(|i| !i.is_empty() && labelled.contains(&d.id)).map(|i| truncate(i, 60));
            Some(EdgeSpec { from: p.to_string(), to: d.id.clone(), dashed: d.status == proto_core::DesignStatus::Generating, label })
        })
        .collect();
    out.extend(crate::features::extra_edges(s).into_iter().map(|e| EdgeSpec { from: e.from, to: e.to, dashed: e.dashed || e.animated, label: None }));
    out
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>().trim_end())
    }
}

/// The edge layer (one shape) and its labels, for the world element.
pub fn layer(s: &AppState, p: &Palette, reduced_motion: bool) -> Vec<El> {
    let rects = &s.canvas.nodes.rects;
    // While nodes are dragged, their edges follow (the frames themselves ride a live offset).
    let (moving, delta): (std::collections::HashSet<&str>, [f32; 2]) = match &s.canvas.nodes.drag {
        super::nodes::NodeDrag::Moving { nodes, delta, .. } => (nodes.iter().map(|(_, id, _)| id.as_str()).collect(), *delta),
        _ => (Default::default(), [0.0, 0.0]),
    };
    let at = |id: &str| rects.get(id).map(|&(k, r)| {
        let r = if moving.contains(id) { [r[0] + delta[0], r[1] + delta[1], r[2], r[3]] } else { r };
        (k, r)
    });
    let c = edge_color(p);
    // Curves first (world points), so the shape element can cover exactly their bounds.
    let mut curves: Vec<([f32; 2], [f32; 2], Option<f32>, Color)> = Vec::new();
    let mut labels = Vec::new();
    for e in edge_specs(s) {
        let (Some((fk, fr)), Some((tk, tr))) = (at(&e.from), at(&e.to)) else { continue };
        let (a, _) = anchors(fk, fr);
        let (_, b) = anchors(tk, tr);
        let dash = e.dashed.then_some(if reduced_motion { 0.0 } else { s.canvas.dash_phase });
        curves.push((a, b, dash, c));
        if let Some(l) = e.label {
            let m = bezier_at(a, b, 0.5);
            labels.push(el().key(format!("edge-label:{}", e.to)).pin(Some(m[0]), Some(m[1]), None, None)
                .child(el().px(6.0).py(3.0).radius(4.0).fill(if p.dark { rune::color::hex("#141414").unwrap() } else { Color::WHITE })
                    .offset(-0.5, -0.5)
                    .child(text(l).font(LABEL).color(p.muted_fg).max_lines(1))));
        }
    }
    if let Some(cn) = &s.canvas.connect {
        curves.push((cn.start, cn.end, None, p.fg.alpha(0.7)));
    }
    // The element spans the curves' bounds (a bezier stays inside its control points' hull), not
    // 1×1 at the origin: Rune culls an element by its own rect, so a tiny one at the world origin
    // dropped every edge once zoom and pan took the origin off screen.
    let Some(bounds) = curves_bounds(&curves) else { return labels };
    let o = [bounds[0], bounds[1]];
    let local = |q: [f32; 2]| [q[0] - o[0], q[1] - o[1]];
    let mut shape = Shape::new();
    for (a, b, dash, col) in curves {
        let (a, b) = (local(a), local(b));
        shape = match dash {
            Some(phase) => dashed(shape, a, b, col, phase),
            None => shape.stroke_path(&bezier(a, b), STROKE, LineCap::Round, col),
        };
    }
    let mut out = vec![canvas(shape).key("edges").pin(Some(o[0]), Some(o[1]), None, None).size(bounds[2], bounds[3]).inert()];
    out.extend(labels);
    out
}

/// The world rect `[x, y, w, h]` holding every curve (its ends and bezier control points), plus
/// the stroke on each side. None without curves.
fn curves_bounds<T, U>(curves: &[([f32; 2], [f32; 2], T, U)]) -> Option<[f32; 4]> {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for (a, z, ..) in curves {
        let off = |d: f32| if d >= 0.0 { 0.5 * d } else { 0.25 * 25.0 * (-d).sqrt() };
        let o = off(z[1] - a[1]);
        for q in [*a, *z, [a[0], a[1] + o], [z[0], z[1] - o]] {
            b = [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[0]), b[3].max(q[1])];
        }
    }
    (b[0] <= b[2]).then(|| [b[0] - STROKE, b[1] - STROKE, b[2] - b[0] + 2.0 * STROKE, b[3] - b[1] + 2.0 * STROKE])
}

/// Whether any edge of the active space has an end in `ids` (a drag then redraws edges per move).
pub fn touches(s: &AppState, ids: &std::collections::HashSet<&str>) -> bool {
    edge_specs(s).iter().any(|e| ids.contains(e.from.as_str()) || ids.contains(e.to.as_str()))
}

/// Whether the active space has animated (dashed) edges.
pub fn has_animated(s: &AppState) -> bool {
    edge_specs(s).iter().any(|e| e.dashed)
}

/// A design card's handles: the top (target) dot and the bottom (source) dot, which starts a
/// connection drag. Children of the node frame, at the card's anchors.
pub fn handles(id: &str, p: &Palette) -> Vec<El> {
    let dot = |k: &str| el().key(format!("{k}:{id}")).size(HANDLE, HANDLE).radius(HANDLE / 2.0)
        .fill(p.muted_fg).border(1.0, p.canvas_bg);
    let top_y = crate::cards::TITLE_H - HANDLE / 2.0;
    let from = id.to_string();
    vec![
        el().pin(Some(0.0), Some(top_y), Some(0.0), None).h(HANDLE).center().inert().child(dot("handle-in")),
        el().pin(Some(0.0), None, Some(0.0), Some(-HANDLE / 2.0)).h(HANDLE).center()
            .child(dot("handle-out").cursor(rune::Cursor::Crosshair)
                .on_drag(move |s: &mut AppState, d: &mut Drag, tx: &mut Tx| on_handle_drag(s, &from, d, tx))),
    ]
}

fn on_handle_drag(s: &mut AppState, from: &str, d: &Drag, tx: &mut Tx) {
    // Locked (zoom controls): handles don't connect; a drag on one pans the canvas.
    if !s.canvas.controls.interactive() {
        return super::canvas_drag(s, d, tx);
    }
    let zoom = s.canvas.camera.zoom;
    match d.phase {
        _ if d.cancelled() => s.canvas.connect = None,
        Phase::Down => {
            let Some(&(k, r)) = s.canvas.nodes.rects.get(from) else { return };
            let start = anchors(k, r).0;
            s.canvas.connect = Some(Connect { from: from.to_string(), start, end: start });
        }
        Phase::Up => {
            let Some(cn) = s.canvas.connect.take() else { return };
            let end = [cn.start[0] + d.dx / zoom, cn.start[1] + d.dy / zoom];
            let target = drop_target(s, from, end);
            crate::features::on_handle_drop(s, from, target, tx);
        }
        _ => {
            if let Some(cn) = &mut s.canvas.connect {
                cn.end = [cn.start[0] + d.dx / zoom, cn.start[1] + d.dy / zoom];
            }
        }
    }
}

/// What a connection dropped at world point `pt` lands on.
fn drop_target(s: &AppState, from: &str, pt: [f32; 2]) -> DropTarget {
    let hit = s.canvas.nodes.rects.iter().find(|(id, (_, r))| {
        id.as_str() != from && pt[0] >= r[0] && pt[0] <= r[0] + r[2] && pt[1] >= r[1] && pt[1] <= r[1] + r[3]
    });
    match hit {
        Some((id, (Mover::Design, _))) => DropTarget::Design(id.clone()),
        Some((id, (Mover::Extra, _))) => DropTarget::Extra(id.clone()),
        _ => DropTarget::Empty(pt),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_edge_element_covers_every_curve_far_from_the_origin() {
        // Upward and downward edges, far from the world origin (which used to be the element).
        let curves = [([5000.0, 3000.0], [5400.0, 3600.0], (), ()), ([6000.0, 4000.0], [5800.0, 3500.0], (), ())];
        let r = curves_bounds(&curves).unwrap();
        for (a, b, ..) in curves {
            for i in 0..=32 {
                let q = bezier_at(a, b, i as f32 / 32.0);
                assert!(q[0] >= r[0] && q[1] >= r[1] && q[0] <= r[0] + r[2] && q[1] <= r[1] + r[3], "{q:?} outside {r:?}");
            }
        }
        assert!(r[0] > 4900.0 && r[1] > 2900.0, "{r:?}");
        assert_eq!(curves_bounds::<(), ()>(&[]), None);
    }

    #[test]
    fn bezier_ends_on_its_anchors_and_bends_vertically() {
        let (a, b) = ([100.0, 50.0], [300.0, 250.0]);
        assert_eq!(bezier_at(a, b, 0.0), a);
        assert_eq!(bezier_at(a, b, 1.0), b);
        let m = bezier_at(a, b, 0.5);
        assert!((m[0] - 200.0).abs() < 1e-3 && (m[1] - 150.0).abs() < 1e-3);
        // Leaves the source straight down: a point just after the start is still at its x.
        assert!((bezier_at(a, b, 0.05)[0] - a[0]).abs() < 2.0);
    }

    #[test]
    fn labels_truncate_to_sixty_characters() {
        let long = "x".repeat(80);
        assert_eq!(truncate(&long, 60).chars().count(), 60);
        assert_eq!(truncate("short", 60), "short");
    }
}
