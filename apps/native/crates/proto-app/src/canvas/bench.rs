//! Bisecting the node layer's cost during pans (`cargo test --release -p proto-app canvas::bench
//! -- --ignored --nocapture`). Reads a copy of the app data's state.json into an in-memory store.

use std::cell::Cell;
use std::time::Instant;

use proto_core::{StateFile, Store};
use rune::{Cx, El, Env, Runtime, WheelEvent, el};

use super::{NodeKind, node_frame};
use crate::app::state::{AppState, ROOT_SPACE};
use crate::cards::{NODE_H, NODE_W, PORTAL_H, PORTAL_W};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Full,
    Nothing,
    EmptyBoxes,
    FramesOnly,
}

thread_local! {
    pub static MODE: Cell<Mode> = const { Cell::new(Mode::Full) };
}

fn positions(s: &AppState) -> Vec<(NodeKind, String, [f32; 2], [f32; 2])> {
    let snap = s.snap();
    let mut out = Vec::new();
    for (i, sp) in snap.child_spaces(&s.route.space).iter().enumerate() {
        let p = snap.portal(&sp.id).map_or([i as f32 * (PORTAL_W + 80.0), 0.0], |p| [p.x as f32, p.y as f32]);
        out.push((NodeKind::Portal, sp.id.clone(), p, [PORTAL_W, PORTAL_H]));
    }
    for d in snap.designs_in(&s.route.space) {
        out.push((NodeKind::Design, d.id.clone(), [d.position.x as f32, d.position.y as f32], [NODE_W, NODE_H]));
    }
    out
}

pub fn override_nodes(s: &AppState, _cx: &mut Cx<AppState>) -> Option<Vec<El>> {
    let fill = rune::Color::WHITE.alpha(0.08);
    match MODE.with(Cell::get) {
        Mode::Full => None,
        Mode::Nothing => Some(Vec::new()),
        Mode::EmptyBoxes => Some(positions(s).into_iter()
            .map(|(_, id, p, sz)| el().key(id).pin(Some(p[0]), Some(p[1]), None, None).size(sz[0], sz[1]).radius(12.0).fill(fill))
            .collect()),
        Mode::FramesOnly => Some(positions(s).into_iter()
            .map(|(k, id, p, sz)| node_frame(k, &id, p, el().size(sz[0], sz[1]).radius(12.0).fill(fill)))
            .collect()),
    }
}

fn state(space: &str) -> Option<AppState> {
    let path = proto_core::default_data_dir().join("state.json");
    let bytes = std::fs::read(&path).ok()?;
    let file: StateFile = serde_json::from_slice(&bytes).ok()?;
    let mut s = AppState::new();
    s.use_store(Store::from_state(file));
    if space != ROOT_SPACE {
        s.enter_space(space, None);
    }
    Some(s)
}

/// The project with the most designs directly in its explorations… simply: the space with the
/// most designs.
fn busiest_space(s: &AppState) -> String {
    let snap = s.snap();
    snap.spaces().map(|sp| (snap.designs_in(&sp.id).count(), sp.id.clone())).max().map(|(_, id)| id).unwrap_or_default()
}

fn pan_cost(space: &str, mode: Mode) -> Option<(usize, f64, f64, u64)> {
    MODE.with(|m| m.set(mode));
    let s = state(space)?;
    let nodes = s.snap().child_spaces(space).len() + s.snap().designs_in(space).count();
    let mut rt = Runtime::new(s, super::tests_view, (1440.0, 900.0), Env::new(true, 2.0));
    rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
    rt.check_live_only(false);
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
    let views0 = super::VIEW_RUNS.with(Cell::get);
    crate::app::state::POLL_TRUES.with(|m| m.borrow_mut().clear());
    let mut times = Vec::new();
    for i in 0..120 {
        rt.wheel_event(WheelEvent::new(-4.0, if i % 2 == 0 { 3.0 } else { -2.0 }).at(600.0, 450.0).precise(true));
        let t = Instant::now();
        rt.frame(1.0 / 120.0);
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    Some((nodes, times[60], times[114], super::VIEW_RUNS.with(Cell::get) - views0))
}

/// What a pan frame costs when something makes poll return true (the view re-runs every frame).
fn rebuild_cost(space: &str, mode: Mode) -> Option<(f64, f64)> {
    MODE.with(|m| m.set(mode));
    let s = state(space)?;
    let mut rt = Runtime::new(s, super::tests_view, (1440.0, 900.0), Env::new(true, 2.0));
    rt.check_live_only(false);
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
    let mut times = Vec::new();
    for i in 0..60 {
        rt.wheel_event(WheelEvent::new(-4.0, if i % 2 == 0 { 3.0 } else { -2.0 }).at(600.0, 450.0).precise(true));
        rt.invalidate();
        let t = Instant::now();
        rt.frame(1.0 / 120.0);
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    Some((times[30], times[57]))
}

#[test]
#[ignore = "bench: needs the app's data dir"]
fn pan_cost_by_node_layer() {
    let Some(s) = state(ROOT_SPACE) else {
        eprintln!("no state.json in the data dir; skipped");
        return;
    };
    let busy = busiest_space(&s);
    for space in [ROOT_SPACE.to_string(), busy] {
        for mode in [Mode::Nothing, Mode::EmptyBoxes, Mode::FramesOnly, Mode::Full] {
            if let Some((n, p50, p95, views)) = pan_cost(&space, mode) {
                let trues = crate::app::state::POLL_TRUES.with(|m| format!("{:?}", m.borrow()));
                eprintln!("{space:>12} {n:>4} nodes {mode:>11?}: frame p50 {p50:6.2} ms p95 {p95:6.2} ms, view re-runs {views}/120, poll trues {trues}");
            }
        }
        for mode in [Mode::Nothing, Mode::Full] {
            if let Some((p50, p95)) = rebuild_cost(&space, mode) {
                eprintln!("{space:>12}  {mode:?}, view re-run every frame: p50 {p50:6.2} ms p95 {p95:6.2} ms");
            }
        }
    }
}
