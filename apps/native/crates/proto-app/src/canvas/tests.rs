//! The canvas' gestures through the real view: events reach the canvas over cards and empty space,
//! and the drawn world follows the camera exactly.

use rune::{Env, MagnifyEvent, Mods, Runtime, WheelEvent};

use crate::app::state::AppState;
use crate::cards::fixture;

const SIZE: (f32, f32) = (1280.0, 800.0);

fn runtime() -> Runtime<AppState> {
    let mut s = AppState::new();
    s.use_store(fixture::store());
    let mut rt = Runtime::new(s, crate::shell::view, SIZE, Env::new(true, 1.0));
    settle(&mut rt);
    rt
}

fn settle(rt: &mut Runtime<AppState>) {
    for _ in 0..240 {
        if !rt.frame(1.0 / 60.0) && !rt.has_pending_input() {
            return;
        }
    }
}

fn rect(rt: &Runtime<AppState>, key: &str) -> [f32; 4] {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no {key}")).rect
}

fn near(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() <= 0.5 && (a[1] - b[1]).abs() <= 0.5
}

const CARD: &str = "portal:fx-project";

#[test]
fn a_two_finger_scroll_pans_one_to_one_over_cards_and_empty_canvas() {
    let mut rt = runtime();
    let r0 = rect(&rt, CARD);
    // Over the card's middle, then over empty canvas below everything.
    for (x, y) in [(r0[0] + r0[2] / 2.0, r0[1] + r0[3] / 2.0), (200.0, 760.0)] {
        let before = rect(&rt, CARD);
        rt.wheel_event(WheelEvent::new(-37.0, 21.0).at(x, y).precise(true));
        settle(&mut rt);
        let after = rect(&rt, CARD);
        assert!(near([after[0], after[1]], [before[0] - 37.0, before[1] + 21.0]), "at ({x},{y}): {before:?} → {after:?}");
        assert!((after[2] - before[2]).abs() < 0.5, "a scroll never zooms");
    }
}

#[test]
fn a_pinch_and_cmd_scroll_keep_the_point_under_the_cursor() {
    let mut rt = runtime();
    let r = rect(&rt, CARD);
    let q = [r[0] + 40.0, r[1] + 30.0];
    rt.magnify_event(MagnifyEvent::new(0.5).at(q[0], q[1]));
    settle(&mut rt);
    let z = rect(&rt, CARD);
    assert!(near([z[0], z[1]], [q[0] - 40.0 * z[2] / r[2], q[1] - 30.0 * z[2] / r[2]]), "pinch anchor: {r:?} → {z:?}");
    assert!(z[2] > r[2] * 1.5, "zoomed in: {} → {}", r[2], z[2]);

    let cmd = Mods { cmd: true, ..Mods::default() };
    rt.wheel_event(WheelEvent::new(0.0, -30.0).at(q[0], q[1]).precise(true).mods(cmd));
    settle(&mut rt);
    let w = rect(&rt, CARD);
    let k = w[2] / z[2];
    assert!(k < 1.0, "⌘-scroll down zooms out");
    assert!(near([w[0], w[1]], [q[0] - (q[0] - z[0]) * k, q[1] - (q[1] - z[1]) * k]), "⌘-scroll anchor: {z:?} → {w:?}");
}

#[test]
fn scrolls_and_pinches_over_the_header_fades_reach_the_canvas() {
    let mut rt = runtime();
    let before = rect(&rt, CARD);
    rt.wheel_event(WheelEvent::new(0.0, 25.0).at(400.0, 20.0).precise(true));
    settle(&mut rt);
    let after = rect(&rt, CARD);
    assert!(near([after[0], after[1]], [before[0], before[1] + 25.0]), "{before:?} → {after:?}");
}

#[test]
fn dragging_empty_canvas_pans() {
    let mut rt = runtime();
    let before = rect(&rt, CARD);
    let (x, y) = (200.0, 760.0);
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_move(x + 30.0, y - 10.0);
    rt.pointer_move(x + 60.0, y - 20.0);
    rt.pointer_up(x + 60.0, y - 20.0);
    settle(&mut rt);
    let after = rect(&rt, CARD);
    assert!(near([after[0], after[1]], [before[0] + 60.0, before[1] - 20.0]), "{before:?} → {after:?}");
}

thread_local! {
    /// View runs on this test's thread (tests run in parallel threads).
    static VIEWS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn counted_view(s: &AppState, cx: &mut rune::Cx<AppState>) -> rune::El {
    VIEWS.with(|v| v.set(v.get() + 1));
    crate::shell::view(s, cx)
}

/// The app's runtime as `run` builds it: the counted view plus the app's poll (cards, features…).
fn app_runtime(space: Option<&str>) -> Runtime<AppState> {
    let mut s = AppState::new();
    s.use_store(fixture::store());
    if let Some(sp) = space {
        s.enter_space(sp, None);
    }
    let mut rt = Runtime::new(s, counted_view, SIZE, Env::new(true, 1.0));
    rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
    rt.on_key = Some(crate::on_key);
    settle(&mut rt);
    rt
}

#[test]
fn panning_and_pinching_never_rebuild_the_view() {
    for space in [None, Some(fixture::EXPLORATION)] {
        let mut rt = app_runtime(space);
        rt.check_live_only(false);
        let views = VIEWS.with(std::cell::Cell::get);
        crate::app::state::POLL_TRUES.with(|m| m.borrow_mut().clear());
        for i in 0..60 {
            rt.wheel_event(WheelEvent::new(-3.0, (i % 7) as f32 - 3.0).at(500.0, 400.0).precise(true));
            rt.frame(1.0 / 120.0);
        }
        for i in 0..30 {
            rt.magnify_event(MagnifyEvent::new(if i < 15 { 0.02 } else { -0.02 }).at(500.0, 400.0));
            rt.frame(1.0 / 120.0);
        }
        let rebuilt = VIEWS.with(std::cell::Cell::get) - views;
        let trues = crate::app::state::POLL_TRUES.with(|m| format!("{:?}", m.borrow()));
        assert!(rebuilt <= 2, "{space:?}: 90 gesture frames re-ran the view {rebuilt} times (poll trues {trues})");
    }
}

#[test]
fn a_released_pinch_coasts_live_only_and_rests() {
    use rune::GesturePhase;
    let mut rt = app_runtime(Some(fixture::EXPLORATION));
    rt.check_live_only(false);
    rt.magnify_event(MagnifyEvent::new(0.0).at(500.0, 400.0).phase(GesturePhase::Began));
    for _ in 0..12 {
        rt.magnify_event(MagnifyEvent::new(0.04).at(500.0, 400.0));
        rt.frame(1.0 / 60.0);
    }
    rt.magnify_event(MagnifyEvent::new(0.0).at(500.0, 400.0).phase(GesturePhase::Ended));
    rt.frame(1.0 / 120.0);
    let released = rt.state.canvas.camera.zoom;
    assert!(rt.state.canvas.camera.coasting(), "a quick pinch coasts");
    let views = VIEWS.with(std::cell::Cell::get);
    let mut frames = 0;
    while rt.state.canvas.camera.coasting() {
        rt.frame(1.0 / 120.0);
        frames += 1;
        assert!(frames < 90, "rests within 0.75 s");
    }
    assert!(rt.state.canvas.camera.zoom > released * 1.1, "{released} → {}", rt.state.canvas.camera.zoom);
    let rebuilt = VIEWS.with(std::cell::Cell::get) - views;
    assert!(rebuilt <= 1, "the coast re-ran the view {rebuilt} times");
}

#[test]
fn a_one_finger_drag_on_a_card_never_zooms() {
    let mut rt = app_runtime(Some(fixture::EXPLORATION));
    let zoom = rt.state.canvas.camera.zoom;
    let r = rect(&rt, "design:fx-v1");
    let (x, y) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    for i in 1..=20 {
        rt.pointer_move(x + 3.0 * i as f32, y + 2.0 * i as f32);
        rt.frame(1.0 / 120.0);
    }
    rt.pointer_up(x + 60.0, y + 40.0);
    settle(&mut rt);
    assert_eq!(rt.state.canvas.camera.zoom, zoom, "a drag changed the zoom");
}

fn drag(rt: &mut Runtime<AppState>, from: (f32, f32), by: (f32, f32), check_mid: impl FnOnce(&Runtime<AppState>)) {
    let (x, y) = from;
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    for i in 1..=10 {
        let t = i as f32 / 10.0;
        rt.pointer_move(x + by.0 * t, y + by.1 * t);
        rt.frame(1.0 / 120.0);
    }
    check_mid(rt);
    rt.pointer_up(x + by.0, y + by.1);
    settle(rt);
}

#[test]
fn dragging_a_card_moves_it_and_writes_its_position_once() {
    let mut rt = app_runtime(Some(fixture::EXPLORATION));
    let key = "design:fx-v1";
    let r = rect(&rt, key);
    let zoom = rt.state.canvas.camera.zoom;
    let v0 = rt.state.snap().version;
    let start = rt.state.snap().design("fx-v1").unwrap().position;
    drag(&mut rt, (r[0] + r[2] / 2.0, r[1] + 60.0), (80.0, 40.0), |rt| {
        let m = rect(rt, key);
        assert!(near([m[0], m[1]], [r[0] + 80.0, r[1] + 40.0]), "follows the pointer mid-drag: {r:?} → {m:?}");
        assert_eq!(rt.state.snap().version, v0, "nothing written mid-drag");
    });
    let after = rect(&rt, key);
    assert!(near([after[0], after[1]], [r[0] + 80.0, r[1] + 40.0]), "stays where it was dropped: {after:?}");
    let pos = rt.state.snap().design("fx-v1").unwrap().position;
    assert!(((pos.x - start.x) as f32 - 80.0 / zoom).abs() < 1.0 && ((pos.y - start.y) as f32 - 40.0 / zoom).abs() < 1.0, "{start:?} → {pos:?}");
    assert_eq!(rt.state.snap().version, v0 + 1, "one store version for the drop");
    assert_eq!(rt.state.route.selected, vec!["fx-v1".to_string()]);
}

#[test]
fn dragging_a_selected_card_moves_the_whole_selection() {
    let mut rt = app_runtime(Some(fixture::EXPLORATION));
    rt.update(|s, _| s.route.selected = vec!["fx-v1".into(), "fx-phone".into()]);
    settle(&mut rt);
    let (a, b) = (rect(&rt, "design:fx-v1"), rect(&rt, "design:fx-phone"));
    drag(&mut rt, (a[0] + a[2] / 2.0, a[1] + 60.0), (-50.0, 30.0), |_| {});
    let (a2, b2) = (rect(&rt, "design:fx-v1"), rect(&rt, "design:fx-phone"));
    assert!(near([a2[0], a2[1]], [a[0] - 50.0, a[1] + 30.0]) && near([b2[0], b2[1]], [b[0] - 50.0, b[1] + 30.0]), "{a2:?} {b2:?}");
}

#[test]
fn parent_edges_are_drawn_and_a_handle_drop_on_empty_canvas_reaches_the_features() {
    let mut rt = app_runtime(Some(fixture::EXPLORATION));
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("edges")), "edge layer");
    let h = rect(&rt, "handle-out:fx-base");
    let before = crate::features::extra_nodes(&rt.state, &crate::app::style::palette(true)).len();
    drag(&mut rt, (h[0] + h[2] / 2.0, h[1] + h[3] / 2.0), (0.0, 260.0), |rt| {
        assert!(rt.state.canvas.connect.is_some(), "a preview edge while dragging");
    });
    assert!(rt.state.canvas.connect.is_none());
    let after = crate::features::extra_nodes(&rt.state, &crate::app::style::palette(true)).len();
    assert_eq!(after, before + 1, "a drop on empty canvas starts a draft");
}

#[test]
fn cards_stay_rigid_through_live_pans_at_any_zoom() {
    let mut rt = app_runtime(Some(fixture::EXPLORATION));
    rt.update(|s, _| s.canvas.camera.set([40.0, 60.0], 0.4));
    settle(&mut rt);
    let r = rect(&rt, "design:fx-v1");
    for _ in 0..5 {
        rt.wheel_event(WheelEvent::new(-6.0, -3.0).at(500.0, 500.0).precise(true));
        rt.frame(1.0 / 60.0);
    }
    settle(&mut rt);
    let p = rect(&rt, "design:fx-v1");
    assert!((p[2] - r[2]).abs() < 0.5 && (p[3] - r[3]).abs() < 0.5, "size changed during a pan: {r:?} → {p:?}");
    assert!(near([p[0], p[1]], [r[0] - 30.0, r[1] - 15.0]), "moved 1:1: {r:?} → {p:?}");
}

#[test]
fn a_click_on_empty_canvas_clears_the_selection_but_a_pan_does_not() {
    let mut rt = app_runtime(Some(fixture::EXPLORATION));
    rt.update(|s, _| s.route.selected = vec!["fx-v1".into()]);
    settle(&mut rt);
    drag(&mut rt, (200.0, 760.0), (60.0, -20.0), |_| {});
    assert_eq!(rt.state.route.selected, vec!["fx-v1".to_string()], "a pan keeps the selection");
    rt.pointer_move(200.0, 760.0);
    rt.pointer_down(200.0, 760.0);
    rt.pointer_up(200.0, 760.0);
    settle(&mut rt);
    assert!(rt.state.route.selected.is_empty(), "a click clears it");
}

#[test]
fn a_shift_drag_on_empty_canvas_selects_the_cards_inside_the_box() {
    let mut rt = app_runtime(Some(fixture::EXPLORATION));
    let (a, b) = (rect(&rt, "design:fx-base"), rect(&rt, "design:fx-v1"));
    let from = (a[0] - 10.0, a[1] - 10.0);
    let to = (b[0] + b[2] + 10.0, b[1].max(a[1]) + a[3].max(b[3]) + 10.0);
    let shift = Mods { shift: true, ..Mods::default() };
    rt.pointer_move(from.0, from.1);
    rt.physical_input(rune::PhysicalInput::Modifiers(shift));
    rt.pointer_down(from.0, from.1);
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        rt.pointer_move(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
        rt.frame(1.0 / 120.0);
    }
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("marquee")), "the box shows while dragging");
    rt.pointer_up(to.0, to.1);
    rt.physical_input(rune::PhysicalInput::Modifiers(Mods::default()));
    settle(&mut rt);
    let mut sel = rt.state.route.selected.clone();
    sel.sort();
    assert!(sel.contains(&"fx-base".to_string()) && sel.contains(&"fx-v1".to_string()), "{sel:?}");
    assert!(!rt.elements().iter().any(|e| e.key.as_deref() == Some("marquee")), "the box goes on release");
}

#[test]
fn edges_follow_a_dragged_card_and_dashes_animate_only_while_generating() {
    let mut rt = app_runtime(Some(fixture::EXPLORATION));
    let r = rect(&rt, "design:fx-v1");
    drag(&mut rt, (r[0] + r[2] / 2.0, r[1] + 60.0), (80.0, 40.0), |rt| {
        assert!(matches!(rt.state.canvas.nodes.drag, crate::canvas::nodes::NodeDrag::Moving { edges: true, .. }), "fx-v1 has a parent edge, so moves redraw edges");
    });
    // The fixture exploration has a generating design under a parent: its edge is dashed and moves.
    let before = rt.state.canvas.dash_phase;
    for _ in 0..12 {
        rt.frame(1.0 / 60.0);
    }
    assert!(crate::canvas::edges::has_animated(&rt.state) == (rt.state.canvas.dash_phase != before), "dash moves iff something generates");
}

fn focused_key(rt: &Runtime<AppState>) -> Option<String> {
    rt.elements().into_iter().find(|e| e.focused).and_then(|e| e.key)
}

fn key(rt: &mut Runtime<AppState>, key: rune::Key, shift: bool) {
    rt.key(rune::KeyEvent { key, mods: Mods { shift, ..Mods::default() } });
    settle(rt);
}

#[test]
fn tab_starts_in_the_header_and_nodes_are_named_tab_stops() {
    let mut rt = app_runtime(Some(fixture::EXPLORATION));
    key(&mut rt, rune::Key::Tab, false);
    assert_eq!(focused_key(&rt).as_deref(), Some("logo"), "the header comes first");
    let els = rt.elements();
    let canvas = els.iter().find(|e| e.key.as_deref() == Some("canvas")).unwrap();
    assert_eq!(canvas.label.as_deref(), Some("Canvas"));
    let node = els.iter().find(|e| e.key.as_deref() == Some("design:fx-base")).unwrap();
    assert_eq!(node.label.as_deref(), Some("Floating pill toolbar"));
    assert!(node.focusable, "nodes are Tab stops");
}

#[test]
fn arrow_keys_move_a_focused_selected_card() {
    let mut rt = app_runtime(Some(fixture::EXPLORATION));
    let r = rect(&rt, "design:fx-v1");
    rt.pointer_move(r[0] + r[2] / 2.0, r[1] + 60.0);
    rt.pointer_down(r[0] + r[2] / 2.0, r[1] + 60.0);
    rt.pointer_up(r[0] + r[2] / 2.0, r[1] + 60.0);
    settle(&mut rt);
    assert_eq!(focused_key(&rt).as_deref(), Some("design:fx-v1"), "a click focuses the node (web focus)");
    let start = rt.state.snap().design("fx-v1").unwrap().position;
    key(&mut rt, rune::Key::Right, false);
    key(&mut rt, rune::Key::Down, true);
    let pos = rt.state.snap().design("fx-v1").unwrap().position;
    assert_eq!((pos.x - start.x, pos.y - start.y), (5.0, 20.0));
}
