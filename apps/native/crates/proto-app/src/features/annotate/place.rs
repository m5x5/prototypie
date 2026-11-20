//! Where the toolbar and the panels go: the free-spot rule around live web views (AN-UI-030/031), anchoring at a
//! target (AN-UI-102) and under or over the toolbar (AN-UI-103). Rects are `[x, y, w, h]` in window points.

/// The toolbar's size for collisions (AN-UI-022).
pub const TOOLBAR: [f32; 2] = [216.0, 40.0];
/// Default toolbar spot: left, under the 48 pt header with 12 pt air.
pub const TOOLBAR_HOME: [f32; 2] = [16.0, 60.0];
/// Panel width.
pub const PANEL_W: f32 = 300.0;

fn clamp(p: [f32; 2], size: [f32; 2], win: [f32; 2]) -> [f32; 2] {
    let x = p[0].clamp(4.0, (win[0] - size[0] - 4.0).max(4.0));
    let y = p[1].clamp(4.0, (win[1] - size[1] - 4.0).max(4.0));
    [x, y]
}

/// Area two rects really share (touching edges share none).
fn overlap(a: [f32; 4], b: [f32; 4]) -> f32 {
    let w = (a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0]);
    let h = (a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1]);
    if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
}

/// The first candidate (wish, beside each taken rect, the window corners), clamped into the window, that covers
/// no taken rect; else the one covering the least (earliest on ties).
pub fn free_spot(wish: [f32; 2], size: [f32; 2], taken: &[[f32; 4]], win: [f32; 2]) -> [f32; 2] {
    let [b, h] = size;
    let [w_w, w_h] = win;
    let mut cands = vec![wish];
    for r in taken {
        let (right, bottom) = (r[0] + r[2], r[1] + r[3]);
        cands.push([right + 12.0, bottom - h]);
        cands.push([r[0] - b - 12.0, bottom - h]);
        cands.push([r[0], bottom + 8.0]);
        cands.push([r[0], r[1] - h - 8.0]);
        cands.push([w_w - b - 4.0, bottom - h]);
    }
    cands.extend([[16.0, 60.0], [16.0, w_h - h - 16.0], [w_w - b - 16.0, 60.0], [w_w - b - 16.0, w_h - h - 16.0]]);
    let mut best: Option<([f32; 2], f32)> = None;
    for c in cands {
        let c = clamp(c, size, win);
        let rect = [c[0], c[1], b, h];
        let covered: f32 = taken.iter().map(|t| overlap(rect, *t)).sum();
        if covered == 0.0 {
            return c;
        }
        if best.is_none_or(|(_, a)| covered < a) {
            best = Some((c, covered));
        }
    }
    best.map_or(clamp(wish, size, win), |(c, _)| c)
}

/// Placement at an anchor rect (AN-UI-102): under it when the height fits, else above; then the free-spot rule.
pub fn at_anchor(anchor: [f32; 4], height: f32, taken: &[[f32; 4]], win: [f32; 2]) -> [f32; 2] {
    let x = anchor[0].clamp(16.0, (win[0] - PANEL_W - 16.0).max(16.0));
    let below = anchor[1] + anchor[3] + 10.0;
    let y = if below + height < win[1] - 16.0 { below } else { (anchor[1] - height - 10.0).max(16.0) };
    free_spot([x, y], [PANEL_W, height], taken, win)
}

/// Placement of the list and settings from the toolbar's shown spot (AN-UI-103).
pub fn by_toolbar(toolbar: [f32; 2], height: f32, taken: &[[f32; 4]], win: [f32; 2]) -> [f32; 2] {
    let [tx, ty] = toolbar;
    let wish = if ty > win[1] / 2.0 { [tx, (ty - height - 8.0).max(16.0)] } else { [tx, ty + 40.0 + 8.0] };
    free_spot(wish, [PANEL_W, height], taken, win)
}

/// Whether `r` overlaps any of `views` (pins under live web views aren't drawn, AN-UI-081).
pub fn under_any(r: [f32; 4], views: &[[f32; 4]]) -> bool {
    views.iter().any(|v| overlap(r, *v) > 0.0)
}
