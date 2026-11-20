//! Lucide icons the shell needs, drawn as shapes in a 24-unit view box (stroke 2, round caps).

use rune::{Color, El, LineCap, Path, Shape, canvas};

fn icon(shape: Shape, size: f32) -> El {
    canvas(shape.view_box(24.0, 24.0)).size(size, size).inert()
}

/// Stroke width in points for a lucide icon drawn at `size` (2 units of 24).
fn sw(size: f32) -> f32 {
    2.0 * size / 24.0
}

/// lucide `layers`: the Prototypie mark.
pub fn layers(size: f32, c: Color) -> El {
    let w = sw(size);
    let s = Shape::new()
        .stroke_with(vec![[12.0, 2.0], [2.0, 7.0], [12.0, 12.0], [22.0, 7.0]], true, w, LineCap::Round, c)
        .stroke(vec![[2.0, 17.0], [12.0, 22.0], [22.0, 17.0]], w, c)
        .stroke(vec![[2.0, 12.0], [12.0, 17.0], [22.0, 12.0]], w, c);
    icon(s, size)
}

/// lucide `plus`.
pub fn plus(size: f32, c: Color) -> El {
    let w = sw(size);
    icon(Shape::new().line([5.0, 12.0], [19.0, 12.0], w, LineCap::Round, c).line([12.0, 5.0], [12.0, 19.0], w, LineCap::Round, c), size)
}

/// lucide `chevron-down` (the New pill's menu hint).
pub fn chevron_down(size: f32, c: Color) -> El {
    icon(Shape::new().stroke(vec![[6.0, 9.0], [12.0, 15.0], [18.0, 9.0]], sw(size), c), size)
}

/// lucide `chevron-right` (the breadcrumb separator).
pub fn chevron_right(size: f32, c: Color) -> El {
    icon(Shape::new().stroke(vec![[9.0, 6.0], [15.0, 12.0], [9.0, 18.0]], sw(size), c), size)
}

/// lucide `maximize` (fullscreen).
pub fn maximize(size: f32, c: Color) -> El {
    let w = sw(size);
    let corner = |a: [f32; 2], b: [f32; 2], d: [f32; 2]| Path::new().move_to(a).line_to(b).line_to(d);
    let s = Shape::new()
        .stroke_path(&corner([8.0, 3.0], [3.0, 3.0], [3.0, 8.0]), w, LineCap::Round, c)
        .stroke_path(&corner([21.0, 8.0], [21.0, 3.0], [16.0, 3.0]), w, LineCap::Round, c)
        .stroke_path(&corner([3.0, 16.0], [3.0, 21.0], [8.0, 21.0]), w, LineCap::Round, c)
        .stroke_path(&corner([16.0, 21.0], [21.0, 21.0], [21.0, 16.0]), w, LineCap::Round, c);
    icon(s, size)
}

/// lucide `minimize` (leave full screen): the four corners turned inwards.
pub fn minimize(size: f32, c: Color) -> El {
    let w = sw(size);
    let corner = |a: [f32; 2], b: [f32; 2], d: [f32; 2]| Path::new().move_to(a).line_to(b).line_to(d);
    let s = Shape::new()
        .stroke_path(&corner([8.0, 3.0], [8.0, 8.0], [3.0, 8.0]), w, LineCap::Round, c)
        .stroke_path(&corner([21.0, 8.0], [16.0, 8.0], [16.0, 3.0]), w, LineCap::Round, c)
        .stroke_path(&corner([3.0, 16.0], [8.0, 16.0], [8.0, 21.0]), w, LineCap::Round, c)
        .stroke_path(&corner([16.0, 21.0], [16.0, 16.0], [21.0, 16.0]), w, LineCap::Round, c);
    icon(s, size)
}

/// lucide `pencil` (rename), its outline traced with straight segments.
pub fn pencil(size: f32, c: Color) -> El {
    let w = sw(size);
    let body = vec![[17.2, 2.8], [21.2, 6.8], [7.8, 20.2], [2.5, 21.5], [3.8, 16.2]];
    icon(Shape::new().stroke_with(body, true, w, LineCap::Round, c).line([15.0, 5.0], [19.0, 9.0], w, LineCap::Round, c), size)
}

/// lucide `ellipsis` (more actions).
pub fn ellipsis(size: f32, c: Color) -> El {
    let w = sw(size) * 1.1;
    let dot = |s: Shape, x: f32| s.line([x, 12.0], [x + 0.01, 12.0], w, LineCap::Round, c);
    icon(dot(dot(dot(Shape::new(), 5.0), 12.0), 19.0), size)
}
