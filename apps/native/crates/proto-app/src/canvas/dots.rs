//! The dotted canvas background: one shape element holding a lattice of dots, in screen space
//! under its own live offset and scale (`Camera::live_dots_*`), so pans and zooms only move it.
//! It's rebuilt only when the canvas size changes.

use rune::{Color, El, LineCap, Shape, canvas};

use super::camera::{Camera, DOT_GAP, DOT_MIN};

/// Dot diameter in layer points (scaled with the zoom's level of detail, as React Flow scales
/// `size={1}` with the zoom).
const DOT: f32 = 1.2;

/// A lattice covering a canvas of `size` at the densest spacing (`DOT_MIN`), plus one cell of
/// margin each side for the phase shift.
pub fn layer(size: [f32; 2], color: Color, camera: &Camera) -> El {
    let cols = (size[0] / DOT_MIN).ceil() as usize + 2;
    let rows = (size[1] / DOT_MIN).ceil() as usize + 2;
    let mut shape = Shape::new();
    for j in 0..rows {
        for i in 0..cols {
            let p = [i as f32 * DOT_GAP, j as f32 * DOT_GAP];
            // A zero-length segment with round caps is a dot; Rune drops 0-length strokes.
            shape = shape.line(p, [p[0] + 0.01, p[1]], DOT, LineCap::Round, color);
        }
    }
    canvas(shape)
        .key("canvas-dots")
        .size(cols as f32 * DOT_GAP, rows as f32 * DOT_GAP)
        .pin(Some(0.0), Some(0.0), None, None)
        .origin(0.0, 0.0)
        .live_offset(&camera.live_dots_offset)
        .live_scale(&camera.live_dots_scale)
        .live_opacity(&camera.live_dots_opacity)
        .inert()
}
