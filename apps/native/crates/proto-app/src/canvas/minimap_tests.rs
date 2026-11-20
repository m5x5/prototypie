use super::*;

#[test]
fn mapping_fits_nodes_and_viewport_like_react_flow() {
    // Nodes right of the viewport: the map spans both, centred, with the offset margin.
    let m = mapping(Some([1000.0, 0.0, 400.0, 300.0]), [0.0, 0.0, 800.0, 500.0]);
    let a = m.to_map([0.0, 0.0]);
    let b = m.to_map([1400.0, 500.0]);
    assert!(a[0] >= 0.0 && a[1] >= 0.0 && b[0] <= MAP_W + 1e-3 && b[1] <= MAP_H + 1e-3, "{a:?} {b:?}");
    // Centred horizontally or vertically.
    let (l, r) = (a[0], MAP_W - b[0]);
    let (t, btm) = (a[1], MAP_H - b[1]);
    assert!((l - r).abs() < 1e-2 || (t - btm).abs() < 1e-2, "l {l} r {r} t {t} b {btm}");
    // The stroke constant matches 2 world units at this mapping.
    assert!((2.0 * m.view_scale * m.scale - MASK_STROKE).abs() < 1e-3);
}

#[test]
fn mapping_without_nodes_frames_the_viewport() {
    let m = mapping(None, [100.0, 50.0, 1280.0, 800.0]);
    let a = m.to_map([100.0, 50.0]);
    let b = m.to_map([1380.0, 850.0]);
    assert!((a[0] + b[0] - MAP_W).abs() < 1e-2 && (a[1] + b[1] - MAP_H).abs() < 1e-2, "{a:?} {b:?}");
}

#[test]
fn visibility_intro_moves_hover_and_fade() {
    let mut v = Vis::Start;
    assert_eq!(step(&mut v, VisEvent::Tick, 10.0), Some(10.0 + INTRO));
    step(&mut v, VisEvent::Tick, 10.0 + INTRO);
    assert_eq!(v, Vis::Hidden);
    assert_eq!(step(&mut v, VisEvent::Moved, 20.0), Some(20.0 + HIDE_AFTER));
    // Moving again pushes the fade back.
    assert_eq!(step(&mut v, VisEvent::Moved, 21.0), Some(21.0 + HIDE_AFTER));
    step(&mut v, VisEvent::CornerIn, 21.5);
    assert_eq!(v, Vis::Hovered);
    // Hovered never times out, moves included.
    step(&mut v, VisEvent::Moved, 30.0);
    step(&mut v, VisEvent::Tick, 99.0);
    assert_eq!(v, Vis::Hovered);
    assert_eq!(step(&mut v, VisEvent::CornerOut, 100.0), Some(100.0 + HIDE_AFTER));
    step(&mut v, VisEvent::Tick, 100.0 + HIDE_AFTER);
    assert_eq!(v, Vis::Hidden);
}

// ---- through the real view (fixture store, the app's poll) ----

use rune::{Env, Runtime};

use crate::app::insets::INSPECTOR_OVERLAY;
use crate::app::state::AppState;
use crate::cards::fixture;

thread_local! {
    static VIEWS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn counted_view(s: &AppState, cx: &mut rune::Cx<AppState>) -> El {
    VIEWS.with(|v| v.set(v.get() + 1));
    crate::shell::view(s, cx)
}

pub(crate) fn app_runtime() -> Runtime<AppState> {
    let mut s = AppState::new();
    s.use_store(fixture::store());
    s.enter_space(fixture::EXPLORATION, None);
    let mut rt = Runtime::new(s, counted_view, (1280.0, 800.0), Env::new(true, 1.0));
    rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
    settle(&mut rt);
    rt
}

pub(crate) fn settle(rt: &mut Runtime<AppState>) {
    for _ in 0..240 {
        if !rt.frame(1.0 / 60.0) && !rt.has_pending_input() {
            return;
        }
    }
}

/// Run frames for `secs` of clock (timers fire on the way).
fn run_for(rt: &mut Runtime<AppState>, secs: f64) {
    for _ in 0..(secs * 60.0).ceil() as usize {
        rt.frame(1.0 / 60.0);
    }
}

/// The fixture's settle outlasts the intro, so the map starts hidden: hover the hot corner (the
/// camera stays put) until it is fully shown.
fn reveal(rt: &mut Runtime<AppState>) {
    let size = rt.state.canvas.size;
    rt.pointer_move(size[0] - INSPECTOR_OVERLAY - 40.0, size[1] - 30.0);
    run_for(rt, 0.8);
    assert_eq!(rt.state.canvas.minimap.vis(), Vis::Hovered);
}

fn presented(rt: &Runtime<AppState>, key: &str) -> ([f32; 4], f32) {
    rt.presented(key).unwrap_or_else(|| panic!("no {key}"))
}

fn opacity(rt: &Runtime<AppState>) -> f32 {
    presented(rt, "minimap").1
}

#[test]
fn the_map_shows_the_nodes_and_the_viewport_inside_its_glass_panel() {
    let mut rt = app_runtime();
    reveal(&mut rt);
    let (panel, _) = presented(&rt, "minimap");
    // Bottom-right, clear of the inspector: right edge at 1280 - 388, bottom at 800 - 16.
    assert!((panel[0] + panel[2] - (1280.0 - RIGHT)).abs() < 0.5 && (panel[1] + panel[3] - (800.0 - BOTTOM)).abs() < 0.5, "{panel:?}");
    assert!((panel[2] - (MAP_W + 2.0 * PAD)).abs() < 0.5 && (panel[3] - (MAP_H + 2.0 * PAD)).abs() < 0.5, "{panel:?}");
    let (map, _) = presented(&rt, "minimap-map");
    let (view, _) = presented(&rt, "minimap-view");
    // The viewport rect sits inside the map, and the nodes are drawn.
    assert!(view[0] >= map[0] - 0.5 && view[0] + view[2] <= map[0] + map[2] + 0.5, "{view:?} in {map:?}");
    assert!(!rt.state.canvas.nodes.rects.is_empty());
}

#[test]
fn the_viewport_rect_follows_pans_and_pinches_without_rebuilding_the_view() {
    let mut rt = app_runtime();
    reveal(&mut rt);
    rt.check_live_only(false);
    let views = VIEWS.with(std::cell::Cell::get);
    let (before, _) = presented(&rt, "minimap-view");
    for _ in 0..20 {
        rt.wheel_event(rune::WheelEvent::new(-15.0, 0.0).at(500.0, 400.0).precise(true));
        rt.frame(1.0 / 120.0);
    }
    let (after, _) = presented(&rt, "minimap-view");
    assert!(after[0] > before[0] + 1.0, "content panned left, the viewport moves right in the map: {before:?} → {after:?}");
    for i in 0..10 {
        rt.magnify_event(rune::MagnifyEvent::new(if i < 5 { 0.05 } else { 0.02 }).at(500.0, 400.0));
        rt.frame(1.0 / 120.0);
    }
    let (zoomed, _) = presented(&rt, "minimap-view");
    assert!(zoomed[2] < after[2] - 0.5, "zooming in shrinks the viewport rect: {after:?} → {zoomed:?}");
    // The rect is the camera's viewport through the current mapping.
    let s = &rt.state;
    let (map, _) = presented(&rt, "minimap-map");
    let m = s.canvas.minimap.mapping();
    let w = s.canvas.camera.world([0.0, 0.0]);
    let q = m.to_map(w);
    assert!((zoomed[0] - (map[0] + q[0])).abs() < 0.5 && (zoomed[1] - (map[1] + q[1])).abs() < 0.5, "{zoomed:?} vs map {map:?} + {q:?} (live {:?})", s.canvas.minimap.live_view.get());
    let rebuilt = VIEWS.with(std::cell::Cell::get) - views;
    assert_eq!(rebuilt, 0, "30 gesture frames re-ran the view {rebuilt} times");
    // Once the zoom rests, the settled zoom (the zoom buttons' limits) catches up.
    settle(&mut rt);
    run_for(&mut rt, 0.3);
    assert_eq!(rt.state.canvas.minimap.settled_zoom(), rt.state.canvas.camera.zoom);
}

#[test]
fn it_fades_out_after_the_intro_and_returns_while_the_canvas_moves() {
    // Shown on open: the intro runs from the first poll.
    let mut s = AppState::new();
    s.use_store(fixture::store());
    s.enter_space(fixture::EXPLORATION, None);
    let mut rt = Runtime::new(s, counted_view, (1280.0, 800.0), Env::new(true, 1.0));
    rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
    run_for(&mut rt, 1.0);
    assert!(opacity(&rt) > 0.9, "shown on open: {}", opacity(&rt));
    run_for(&mut rt, INTRO);
    assert!(opacity(&rt) < 0.05, "hidden after the intro: {}", opacity(&rt));
    rt.wheel_event(rune::WheelEvent::new(0.0, 20.0).at(500.0, 400.0).precise(true));
    run_for(&mut rt, 0.5);
    assert!(opacity(&rt) > 0.9, "a pan shows it: {}", opacity(&rt));
    run_for(&mut rt, HIDE_AFTER + 0.8);
    assert!(opacity(&rt) < 0.05, "fades {HIDE_AFTER}s after the last move: {}", opacity(&rt));
    // Hidden, it takes no input: a drag there pans the canvas, not the map.
    assert!(rt.state.canvas.minimap.vis() == Vis::Hidden);
}

#[test]
fn the_hot_corner_keeps_it_shown_until_the_pointer_leaves() {
    let mut rt = app_runtime();
    run_for(&mut rt, INTRO + 1.0);
    let (panel, _) = presented(&rt, "minimap");
    rt.pointer_move(panel[0] + 20.0, panel[1] + 20.0);
    run_for(&mut rt, 3.0);
    assert!(opacity(&rt) > 0.9, "hovering the corner shows it");
    rt.pointer_move(300.0, 300.0);
    run_for(&mut rt, HIDE_AFTER + 0.8);
    assert!(opacity(&rt) < 0.05, "leaving fades it");
}

#[test]
fn dragging_the_map_moves_the_viewport_with_the_pointer() {
    let mut rt = app_runtime();
    reveal(&mut rt);
    let (map, _) = presented(&rt, "minimap-map");
    let (x, y) = (map[0] + map[2] / 2.0, map[1] + map[3] / 2.0);
    let pan0 = rt.state.canvas.camera.pan;
    let k = rt.state.canvas.minimap.mapping().view_scale * rt.state.canvas.camera.zoom;
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    for i in 1..=4 {
        rt.pointer_move(x + 5.0 * i as f32, y);
        rt.frame(1.0 / 60.0);
    }
    rt.pointer_up(x + 20.0, y);
    settle(&mut rt);
    let pan = rt.state.canvas.camera.pan;
    // React Flow: the transform moves by -Δ · viewScale · zoom (the view follows the pointer).
    // viewScale changes as the view moves (the map fits the view too), so allow some slack.
    let moved = pan0[0] - pan[0];
    assert!(moved > 0.5 * 20.0 * k && moved < 2.0 * 20.0 * k, "pan {pan0:?} → {pan:?} (k {k})");
    assert!((pan[1] - pan0[1]).abs() < 1e-3);
}

#[test]
fn the_wheel_on_the_map_zooms_about_the_canvas_centre() {
    let mut rt = app_runtime();
    reveal(&mut rt);
    let (map, _) = presented(&rt, "minimap-map");
    let c = [rt.state.canvas.size[0] / 2.0, rt.state.canvas.size[1] / 2.0];
    let p = rt.state.canvas.camera.world(c);
    let z0 = rt.state.canvas.camera.zoom;
    rt.wheel_event(rune::WheelEvent::new(0.0, 100.0).at(map[0] + 20.0, map[1] + 20.0).precise(true));
    settle(&mut rt);
    let cam = &rt.state.canvas.camera;
    assert!((cam.zoom / z0 - 2f32.powf(0.2)).abs() < 1e-3, "{z0} → {}", cam.zoom);
    let back = cam.screen(p);
    assert!((back[0] - c[0]).abs() < 0.5 && (back[1] - c[1]).abs() < 0.5, "centre drifted to {back:?}");
}
