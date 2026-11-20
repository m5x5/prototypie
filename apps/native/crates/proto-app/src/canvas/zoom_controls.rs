//! The canvas' zoom control, bottom-left: one vertical glass pill (the minimap's GLASS surface,
//! fully rounded ends) holding zoom in, zoom out, fit view and the interactivity lock as 32pt
//! round buttons (the header's ghost icon buttons) split by hairlines. Its bottom edge sits on the
//! minimap's.
//!
//! Zoom in/out scale by 1.2 about the canvas centre (React Flow `zoomIn`/`zoomOut` → d3
//! `scaleBy`) and are disabled at the zoom limits. Fit frames the space's content in the area the header and inspector leave
//! free; double-clicking it zooms to 100%. The lock turns off node dragging, connecting and selecting (React Flow's
//! `nodesDraggable`/`nodesConnectable`/`elementsSelectable`); panning and zooming stay.
//!
//! Live-only: pressing a zoom button moves the camera without re-running the view; the limits
//! follow `minimap::poll`'s settled zoom, which re-runs once a zoom has rested.

use rune::{Color, Cx, El, Icon, Shadow, Tx, col, el, icon};

use super::camera::{MAX_ZOOM, MIN_ZOOM};
use super::minimap;
use crate::app::insets::EDGE;
use crate::app::state::AppState;
use crate::app::style::{Palette, glass};

/// The buttons: the header's ghost icon size, 16pt glyphs.
pub const BUTTON: f32 = 32.0;
/// Glass around the buttons, so a round cap sits concentric with the end buttons.
pub const PAD: f32 = 6.0;
/// Space between a button and a hairline.
pub const GAP: f32 = 4.0;
pub const WIDTH: f32 = BUTTON + 2.0 * PAD;
pub const RADIUS: f32 = WIDTH / 2.0;
pub const LEFT: f32 = EDGE;
pub const BOTTOM: f32 = minimap::BOTTOM;
const GLYPH: f32 = 16.0;
/// `zoomIn`/`zoomOut` step.
pub const STEP: f32 = 1.2;

/// The controls' own state (in `CanvasState::controls`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Controls {
    /// The lock is on: nodes can't be dragged, connected or selected.
    locked: bool,
}

impl Controls {
    /// Nodes take drags, connections and selection (React Flow's `isInteractive`).
    pub fn interactive(&self) -> bool {
        !self.locked
    }

    pub fn toggle_lock(&mut self) {
        self.locked = !self.locked;
    }
}

// Lucide icons (as the rest of the app's chrome): plus, minus, maximize, lock, lock-open.
const fn lucide(name: &'static str, body: &'static str) -> Icon {
    Icon { name, view: 24.0, body }
}
static PLUS: Icon = lucide(
    "lucide-plus",
    r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14"/><path d="M12 5v14"/></g>"#,
);
static MINUS: Icon = lucide(
    "lucide-minus",
    r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14"/></g>"#,
);
static FIT: Icon = lucide(
    "lucide-maximize",
    r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 3H5a2 2 0 0 0-2 2v3"/><path d="M21 8V5a2 2 0 0 0-2-2h-3"/><path d="M3 16v3a2 2 0 0 0 2 2h3"/><path d="M16 21h3a2 2 0 0 0 2-2v-3"/></g>"#,
);
static LOCK: Icon = lucide(
    "lucide-lock",
    r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></g>"#,
);
static UNLOCK: Icon = lucide(
    "lucide-lock-open",
    r#"<g fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 9.9-1"/></g>"#,
);

/// What each segment does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ZoomIn,
    /// Double-clicking fit: back to 100% about the centre.
    Reset,
    ZoomOut,
    Fit,
    Lock,
}

/// Runs a segment's action (the press handler; tests call it directly).
pub fn act(s: &mut AppState, a: Action) {
    let size = if s.canvas.size[0] > 0.0 { s.canvas.size } else { [1280.0, 800.0] };
    let centre = [size[0] / 2.0, size[1] / 2.0];
    match a {
        Action::ZoomIn => s.canvas.camera.zoom_about(centre, STEP),
        Action::ZoomOut => s.canvas.camera.zoom_about(centre, 1.0 / STEP),
        Action::Reset => {
            let z = s.canvas.camera.zoom;
            s.canvas.camera.zoom_about(centre, 1.0 / z);
        }
        Action::Fit => {
            if let Some(bounds) = super::content_bounds(s.snap(), &s.route.space) {
                s.canvas.fit(bounds, 0.1, !s.canvas.inspector_hidden);
            }
        }
        Action::Lock => s.canvas.controls.toggle_lock(),
    }
}

/// One segment.
struct Segment {
    action: Action,
    key: &'static str,
    label: &'static str,
    help: &'static str,
    glyph: &'static Icon,
    disabled: bool,
}

/// The pill's segments top to bottom.
fn segments(zoom: f32, locked: bool) -> [Segment; 4] {
    [
        Segment { action: Action::ZoomIn, key: "zoom-in", label: "Zoom In", help: "Zoom In", glyph: &PLUS, disabled: zoom >= MAX_ZOOM - 1e-4 },
        Segment { action: Action::ZoomOut, key: "zoom-out", label: "Zoom Out", help: "Zoom Out", glyph: &MINUS, disabled: zoom <= MIN_ZOOM + 1e-4 },
        Segment {
            action: Action::Fit,
            key: "zoom-fit",
            label: "Fit View",
            help: "Fit View (double-click: 100%)",
            glyph: &FIT,
            disabled: false,
        },
        Segment {
            action: Action::Lock,
            key: "zoom-lock",
            label: if locked { "Unlock Canvas" } else { "Lock Canvas" },
            help: "Toggle Interactivity",
            glyph: if locked { &LOCK } else { &UNLOCK },
            disabled: false,
        },
    ]
}

/// The pill, pinned bottom-left of the canvas on the minimap's bottom line.
pub fn view(s: &AppState, _cx: &mut Cx<AppState>, p: &Palette) -> El {
    // Read from the settled zoom (`minimap::poll` keeps it), so zooming never re-runs the view
    // frame by frame.
    let zoom = s.canvas.minimap.settled_zoom();
    let locked = !s.canvas.controls.interactive();
    let segs = segments(zoom, locked);
    let n = segs.len();
    // The glass' top edge catches the light: a 1px inner highlight.
    let highlight = Color::WHITE.alpha(if p.dark { 0.09 } else { 0.7 });
    let pill = glass(col(), p, 0.5)
        .key("zoom-controls")
        .role("toolbar")
        .label("Zoom")
        .pin(Some(LEFT), None, None, Some(BOTTOM))
        .size(WIDTH, height(n))
        .padding(PAD)
        .gap(GAP)
        .radius(RADIUS)
        .box_shadow(Shadow { x: 0.0, y: 1.0, blur: 0.0, spread: 0.0, color: highlight, inset: true });
    segs.into_iter().enumerate().fold(pill, |c, (i, seg)| {
        let c = if i > 0 { c.child(hairline(p)) } else { c };
        c.child(segment(seg, p))
    })
}

/// The pill's height for `n` buttons.
pub const fn height(n: usize) -> f32 {
    2.0 * PAD + n as f32 * BUTTON + (n.saturating_sub(1)) as f32 * (2.0 * GAP + 1.0)
}

/// A hairline between buttons, inset from the sides, `GAP` clear of the buttons above and below.
fn hairline(p: &Palette) -> El {
    el().size(BUTTON, 1.0).inert().child(el().pin(Some(6.0), Some(0.0), None, None).size(BUTTON - 12.0, 1.0).fill(p.border))
}

fn segment(seg: Segment, p: &Palette) -> El {
    let fg = if seg.disabled { p.fg.alpha(0.3) } else { p.fg };
    // As the header's ghost icon buttons: 32pt round, 16pt glyph, hover/press tint.
    let b = el().key(seg.key)
        .role("button")
        .label(seg.label)
        .help(seg.help)
        .disabled(seg.disabled)
        .size(BUTTON, BUTTON)
        .radius(BUTTON / 2.0)
        .center()
        .child(icon(seg.glyph, GLYPH).color(fg));
    if seg.disabled {
        return b;
    }
    let action = seg.action;
    let b = b.hover(p.hover)
        .press(p.press)
        .focusable(true)
        .focus_ring(2.0, 1.0, p.fg.alpha(0.5))
        .on_press_tx(move |s: &mut AppState, tx: &mut Tx| {
            act(s, action);
            // Zooming and fitting only move the camera; the lock changes its icon.
            if action != Action::Lock {
                tx.live_only();
            }
        });
    if action == Action::Fit {
        b.on_double_press_tx(|s: &mut AppState, tx: &mut Tx| {
            act(s, Action::Reset);
            tx.live_only();
        })
    } else {
        b
    }
}

#[cfg(test)]
#[path = "zoom_controls_tests.rs"]
mod tests;
