use super::*;
use crate::canvas::minimap::tests::{app_runtime, settle};
use rune::Runtime;

fn click(rt: &mut Runtime<AppState>, key: &str) {
    let r = rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no {key}")).rect;
    let (x, y) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

fn rect(rt: &Runtime<AppState>, key: &str) -> [f32; 4] {
    rt.elements().into_iter().find(|e| e.key.as_deref() == Some(key)).unwrap_or_else(|| panic!("no {key}")).rect
}

#[test]
fn the_pill_sits_bottom_left_on_the_minimaps_bottom_line() {
    let rt = app_runtime();
    let c = rect(&rt, "zoom-controls");
    assert_eq!([c[0], c[1] + c[3]], [LEFT, 800.0 - crate::canvas::minimap::BOTTOM], "{c:?}");
    assert_eq!([c[2], c[3]], [WIDTH, height(4)]);
    // 32pt round buttons, centred, PAD from the caps, a hairline GAP clear between each pair.
    let keys = ["zoom-in", "zoom-out", "zoom-fit", "zoom-lock"];
    let mut y = c[1] + PAD;
    for k in keys {
        let r = rect(&rt, k);
        assert_eq!(r, [c[0] + PAD, y, BUTTON, BUTTON], "{k}");
        y += BUTTON + 2.0 * GAP + 1.0;
    }
    let el = |k: &str| rt.elements().into_iter().find(|e| e.key.as_deref() == Some(k)).unwrap();
    let helps: Vec<_> = keys.iter().map(|k| el(k).help.unwrap_or_default()).collect();
    assert_eq!(helps, ["Zoom In", "Zoom Out", "Fit View (double-click: 100%)", "Toggle Interactivity"]);
    assert!(rt.elements().into_iter().all(|e| !e.text.as_deref().unwrap_or("").ends_with('%')), "no zoom % anywhere");
}

#[test]
fn reset_zooms_to_100_percent_about_the_centre() {
    let mut rt = app_runtime();
    let c = [rt.state.canvas.size[0] / 2.0, rt.state.canvas.size[1] / 2.0];
    let p = rt.state.canvas.camera.world(c);
    act(&mut rt.state, Action::Reset);
    assert!((rt.state.canvas.camera.zoom - 1.0).abs() < 1e-4);
    let back = rt.state.canvas.camera.screen(p);
    assert!((back[0] - c[0]).abs() < 0.5 && (back[1] - c[1]).abs() < 0.5, "{back:?}");
}

#[test]
fn zoom_in_and_out_scale_by_1_2_about_the_canvas_centre() {
    let mut rt = app_runtime();
    let c = [rt.state.canvas.size[0] / 2.0, rt.state.canvas.size[1] / 2.0];
    let p = rt.state.canvas.camera.world(c);
    let z0 = rt.state.canvas.camera.zoom;
    click(&mut rt, "zoom-in");
    assert!((rt.state.canvas.camera.zoom - z0 * STEP).abs() < 1e-4);
    let back = rt.state.canvas.camera.screen(p);
    assert!((back[0] - c[0]).abs() < 0.5 && (back[1] - c[1]).abs() < 0.5, "{back:?}");
    click(&mut rt, "zoom-out");
    click(&mut rt, "zoom-out");
    assert!((rt.state.canvas.camera.zoom - z0 / STEP).abs() < 1e-4);
}

#[test]
fn zoom_in_is_disabled_at_the_limit() {
    let mut rt = app_runtime();
    rt.state.canvas.camera.set([0.0, 0.0], MAX_ZOOM);
    settle(&mut rt);
    let z = rt.state.canvas.camera.zoom;
    click(&mut rt, "zoom-in");
    assert_eq!(rt.state.canvas.camera.zoom, z, "a disabled button does nothing");
    click(&mut rt, "zoom-out");
    assert!(rt.state.canvas.camera.zoom < z);
}

#[test]
fn fit_view_frames_the_content_again() {
    let mut rt = app_runtime();
    let fitted = (rt.state.canvas.camera.pan, rt.state.canvas.camera.zoom);
    rt.state.canvas.camera.set([-900.0, 300.0], 0.3);
    settle(&mut rt);
    click(&mut rt, "zoom-fit");
    let (pan, zoom) = (rt.state.canvas.camera.pan, rt.state.canvas.camera.zoom);
    assert!((zoom - fitted.1).abs() < 1e-3 && (pan[0] - fitted.0[0]).abs() < 0.5 && (pan[1] - fitted.0[1]).abs() < 0.5, "{pan:?} {zoom} vs {fitted:?}");
}

#[test]
fn the_lock_stops_selecting_and_moving_nodes_but_drags_still_pan() {
    let mut rt = app_runtime();
    click(&mut rt, "zoom-lock");
    assert!(!rt.state.canvas.controls.interactive());
    let card = "design:fx-v1";
    let r = rect(&rt, card);
    let (x, y) = (r[0] + r[2] / 2.0, r[1] + 40.0);
    let pos_before = rt.state.snap().designs_in(fixture_space()).into_iter().find(|d| d.id == "fx-v1").map(|d| (d.position.x, d.position.y));
    rt.state.route.selected.clear();
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(rt.state.route.selected.is_empty(), "a press doesn't select while locked");
    let pan0 = rt.state.canvas.camera.pan;
    rt.pointer_down(x, y);
    for i in 1..=5 {
        rt.pointer_move(x + 10.0 * i as f32, y);
        rt.frame(1.0 / 60.0);
    }
    rt.pointer_up(x + 50.0, y);
    settle(&mut rt);
    let pos_after = rt.state.snap().designs_in(fixture_space()).into_iter().find(|d| d.id == "fx-v1").map(|d| (d.position.x, d.position.y));
    assert_eq!(pos_before, pos_after, "the card didn't move in the store");
    assert!((rt.state.canvas.camera.pan[0] - pan0[0] - 50.0).abs() < 0.5, "the drag panned the canvas");
    // Unlocked again, a press selects.
    click(&mut rt, "zoom-lock");
    rt.pointer_move(x + 50.0, y);
    rt.pointer_down(x + 50.0, y);
    rt.pointer_up(x + 50.0, y);
    settle(&mut rt);
    assert_eq!(rt.state.route.selected, vec!["fx-v1".to_string()]);
}

fn fixture_space() -> &'static str {
    crate::cards::fixture::EXPLORATION
}
