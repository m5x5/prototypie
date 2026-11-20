//! Where floating chrome sits over the canvas, so fitView, the minimap and toasts keep clear of it.

/// Header band height (also the macOS titlebar band with the traffic lights).
pub const HEADER_H: f32 = 48.0;
/// Inspector panel width.
pub const INSPECTOR_W: f32 = 340.0;
/// Gap between floating panels and the window edges.
pub const EDGE: f32 = 16.0;
/// The inspector's top: just below the header controls.
pub const INSPECTOR_TOP: f32 = HEADER_H + 4.0;
/// What the inspector takes off the canvas' right edge: panel + inset + gap.
pub const INSPECTOR_OVERLAY: f32 = INSPECTOR_W + 2.0 * EDGE;

/// The canvas area left free by the floating header and inspector: `[left, top, right, bottom]`
/// insets in points.
pub fn canvas_insets(inspector_open: bool) -> [f32; 4] {
    [0.0, HEADER_H, if inspector_open { INSPECTOR_OVERLAY } else { 0.0 }, 0.0]
}
