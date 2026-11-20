//! Icons the features draw, as shapes in a 24-unit view box (lucide geometry, stroke 2, round caps).

use rune::{Color, El, LineCap, Shape, canvas};

fn icon(shape: Shape, size: f32) -> El {
    canvas(shape.view_box(24.0, 24.0)).size(size, size).inert()
}

fn sw(size: f32) -> f32 {
    2.0 * size / 24.0
}

/// lucide `x`.
pub fn x(size: f32, c: Color) -> El {
    let w = sw(size);
    icon(Shape::new().line([18.0, 6.0], [6.0, 18.0], w, LineCap::Round, c).line([6.0, 6.0], [18.0, 18.0], w, LineCap::Round, c), size)
}

/// The Claude "minion" (the reference's pixel creature, 16×12): orange body, dark eyes, four legs. `size` is the
/// height; the width follows at 16:12.
pub fn minion(size: f32, _c: Color) -> El {
    let body = rune::color::hex("#D97757").expect("hex");
    let eye = rune::color::hex("#1a1a1a").expect("hex");
    let r = |s: Shape, x: f32, y: f32, w: f32, h: f32, c: Color| s.fill(vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]], c);
    let mut s = Shape::new();
    for (x, y, w, h) in [(2.0, 2.0, 12.0, 7.0), (0.0, 4.0, 2.0, 2.0), (14.0, 4.0, 2.0, 2.0), (3.0, 9.0, 1.0, 2.0), (5.0, 9.0, 1.0, 2.0), (10.0, 9.0, 1.0, 2.0), (12.0, 9.0, 1.0, 2.0)] {
        s = r(s, x, y, w, h, body);
    }
    s = r(s, 4.0, 3.0, 1.0, 2.0, eye);
    s = r(s, 11.0, 3.0, 1.0, 2.0, eye);
    canvas(s.view_box(16.0, 12.0)).size(size * 16.0 / 12.0, size).inert()
}

/// lucide `telescope` (Explore), simplified to its tube and legs.
pub fn telescope(size: f32, c: Color) -> El {
    let w = sw(size);
    let s = Shape::new()
        .stroke_with(vec![[4.1, 14.4], [16.6, 4.5], [19.1, 9.2], [6.6, 19.1]], true, w, LineCap::Round, c)
        .line([12.0, 15.0], [16.0, 22.0], w, LineCap::Round, c)
        .line([12.0, 15.0], [8.0, 22.0], w, LineCap::Round, c);
    icon(s, size)
}

/// lucide `arrow-up-from-line` (Promote).
pub fn arrow_up_from_line(size: f32, c: Color) -> El {
    let w = sw(size);
    let s = Shape::new()
        .stroke(vec![[18.0, 9.0], [12.0, 3.0], [6.0, 9.0]], w, c)
        .line([12.0, 3.0], [12.0, 15.0], w, LineCap::Round, c)
        .line([5.0, 21.0], [19.0, 21.0], w, LineCap::Round, c);
    icon(s, size)
}

/// lucide `bookmark` (design guidelines).
pub fn bookmark(size: f32, c: Color) -> El {
    icon(Shape::new().stroke_with(vec![[6.0, 3.0], [18.0, 3.0], [18.0, 21.0], [12.0, 16.0], [6.0, 21.0]], true, sw(size), LineCap::Round, c), size)
}

/// lucide `star`, filled when `on`.
pub fn star(size: f32, c: Color, on: bool) -> El {
    let mut pts = Vec::new();
    for i in 0..10 {
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        let r = if i % 2 == 0 { 10.0 } else { 4.4 };
        pts.push([12.0 + r * a.cos(), 12.5 + r * a.sin()]);
    }
    let mut s = Shape::new();
    if on {
        s = s.fill(pts.clone(), c);
    }
    icon(s.stroke_with(pts, true, sw(size), LineCap::Round, c), size)
}

/// lucide `maximize-2`.
pub fn maximize(size: f32, c: Color) -> El {
    let w = sw(size);
    let s = Shape::new()
        .stroke(vec![[15.0, 3.0], [21.0, 3.0], [21.0, 9.0]], w, c)
        .stroke(vec![[9.0, 21.0], [3.0, 21.0], [3.0, 15.0]], w, c)
        .line([21.0, 3.0], [14.0, 10.0], w, LineCap::Round, c)
        .line([3.0, 21.0], [10.0, 14.0], w, LineCap::Round, c);
    icon(s, size)
}

/// lucide `inbox`.
pub fn inbox(size: f32, c: Color) -> El {
    let w = sw(size);
    let s = Shape::new()
        .stroke(vec![[22.0, 12.0], [16.0, 12.0], [14.0, 15.0], [10.0, 15.0], [8.0, 12.0], [2.0, 12.0]], w, c)
        .stroke_with(vec![[5.45, 5.11], [2.0, 12.0], [2.0, 18.0], [4.0, 20.0], [20.0, 20.0], [22.0, 18.0], [22.0, 12.0], [18.55, 5.11], [16.76, 4.0], [7.24, 4.0]], true, w, LineCap::Round, c);
    icon(s, size)
}

/// lucide `check`.
pub fn check(size: f32, c: Color) -> El {
    icon(Shape::new().stroke(vec![[20.0, 6.0], [9.0, 17.0], [4.0, 12.0]], sw(size), c), size)
}

/// The lane type's icon: clay = wand, spoke = boxes, round = refresh, merge = merge (simplified shapes).
pub fn lane(size: f32, c: Color, t: Option<proto_core::LaneType>) -> El {
    use proto_core::LaneType;
    let w = sw(size);
    let s = match t {
        Some(LaneType::Clay) => Shape::new().line([4.0, 20.0], [15.0, 9.0], w, LineCap::Round, c).line([18.0, 3.0], [18.0, 7.0], w, LineCap::Round, c).line([16.0, 5.0], [20.0, 5.0], w, LineCap::Round, c),
        Some(LaneType::Spoke) => Shape::new().stroke_with(vec![[3.0, 3.0], [11.0, 3.0], [11.0, 11.0], [3.0, 11.0]], true, w, LineCap::Round, c).stroke_with(vec![[13.0, 13.0], [21.0, 13.0], [21.0, 21.0], [13.0, 21.0]], true, w, LineCap::Round, c),
        Some(LaneType::Round) => Shape::new().stroke(vec![[20.0, 8.0], [17.0, 4.5], [12.0, 3.0], [6.0, 5.5], [3.0, 12.0]], w, c).stroke(vec![[4.0, 16.0], [7.0, 19.5], [12.0, 21.0], [18.0, 18.5], [21.0, 12.0]], w, c),
        Some(LaneType::Merge) => Shape::new().stroke(vec![[6.0, 3.0], [6.0, 9.0], [12.0, 15.0], [12.0, 21.0]], w, c).stroke(vec![[18.0, 3.0], [18.0, 9.0], [12.0, 15.0]], w, c),
        None => Shape::new().stroke_with(vec![[12.0, 3.0], [21.0, 12.0], [12.0, 21.0], [3.0, 12.0]], true, w, LineCap::Round, c),
    };
    icon(s, size)
}

/// lucide `arrow-up`.
pub fn arrow_up(size: f32, c: Color) -> El {
    let w = sw(size);
    icon(Shape::new().stroke(vec![[5.0, 12.0], [12.0, 5.0], [19.0, 12.0]], w, c).line([12.0, 19.0], [12.0, 5.0], w, LineCap::Round, c), size)
}

/// lucide `arrow-down`.
pub fn arrow_down(size: f32, c: Color) -> El {
    let w = sw(size);
    icon(Shape::new().stroke(vec![[19.0, 12.0], [12.0, 19.0], [5.0, 12.0]], w, c).line([12.0, 5.0], [12.0, 19.0], w, LineCap::Round, c), size)
}

/// lucide `arrow-left`.
pub fn arrow_left(size: f32, c: Color) -> El {
    let w = sw(size);
    icon(Shape::new().stroke(vec![[12.0, 19.0], [5.0, 12.0], [12.0, 5.0]], w, c).line([19.0, 12.0], [5.0, 12.0], w, LineCap::Round, c), size)
}

/// lucide `arrow-right`.
pub fn arrow_right(size: f32, c: Color) -> El {
    let w = sw(size);
    icon(Shape::new().stroke(vec![[12.0, 5.0], [19.0, 12.0], [12.0, 19.0]], w, c).line([5.0, 12.0], [19.0, 12.0], w, LineCap::Round, c), size)
}

/// lucide `vote` (simplified: a ballot box with a check).
pub fn vote(size: f32, c: Color) -> El {
    let w = sw(size);
    let s = Shape::new()
        .stroke(vec![[9.0, 12.0], [11.0, 14.0], [15.0, 10.0]], w, c)
        .stroke_with(vec![[5.0, 7.0], [19.0, 7.0], [19.0, 19.0], [5.0, 19.0]], true, w, LineCap::Round, c)
        .line([3.0, 22.0], [21.0, 22.0], w, LineCap::Round, c);
    icon(s, size)
}

/// lucide `trash-2` (simplified).
pub fn trash(size: f32, c: Color) -> El {
    let w = sw(size);
    let s = Shape::new()
        .line([3.0, 6.0], [21.0, 6.0], w, LineCap::Round, c)
        .stroke(vec![[19.0, 6.0], [18.0, 20.0], [17.0, 21.0], [7.0, 21.0], [6.0, 20.0], [5.0, 6.0]], w, c)
        .stroke(vec![[8.0, 6.0], [8.0, 4.0], [10.0, 2.0], [14.0, 2.0], [16.0, 4.0], [16.0, 6.0]], w, c);
    icon(s, size)
}

/// lucide `refresh-cw` (simplified to two arcs with heads).
pub fn refresh(size: f32, c: Color) -> El {
    let w = sw(size);
    let s = Shape::new()
        .stroke(vec![[3.0, 12.0], [5.0, 7.0], [9.0, 4.0], [14.0, 3.5], [18.0, 5.5], [21.0, 8.0]], w, c)
        .stroke(vec![[21.0, 3.0], [21.0, 8.0], [16.0, 8.0]], w, c)
        .stroke(vec![[21.0, 12.0], [19.0, 17.0], [15.0, 20.0], [10.0, 20.5], [6.0, 18.5], [3.0, 16.0]], w, c)
        .stroke(vec![[3.0, 21.0], [3.0, 16.0], [8.0, 16.0]], w, c);
    icon(s, size)
}

/// lucide `locate-fixed` (simplified: ring, centre, four ticks).
pub fn locate(size: f32, c: Color) -> El {
    let w = sw(size);
    let mut ring = Vec::new();
    for i in 0..=24 {
        let a = i as f32 * std::f32::consts::TAU / 24.0;
        ring.push([12.0 + 7.0 * a.cos(), 12.0 + 7.0 * a.sin()]);
    }
    let s = Shape::new().stroke(ring, w, c)
        .line([12.0, 2.0], [12.0, 5.0], w, LineCap::Round, c).line([12.0, 19.0], [12.0, 22.0], w, LineCap::Round, c)
        .line([2.0, 12.0], [5.0, 12.0], w, LineCap::Round, c).line([19.0, 12.0], [22.0, 12.0], w, LineCap::Round, c)
        .line([12.0, 12.0], [12.01, 12.0], w * 1.6, LineCap::Round, c);
    icon(s, size)
}

/// lucide `check-check`.
pub fn check_check(size: f32, c: Color) -> El {
    let w = sw(size);
    icon(Shape::new().stroke(vec![[18.0, 6.0], [7.0, 17.0], [2.0, 12.0]], w, c).stroke(vec![[22.0, 10.0], [14.5, 17.5], [13.0, 16.0]], w, c), size)
}

/// lucide `send` (paper plane).
pub fn send(size: f32, c: Color) -> El {
    let w = sw(size);
    let s = Shape::new()
        .stroke_with(vec![[22.0, 2.0], [15.0, 22.0], [11.0, 13.0], [2.0, 9.0]], true, w, LineCap::Round, c)
        .line([22.0, 2.0], [11.0, 13.0], w, LineCap::Round, c);
    icon(s, size)
}
