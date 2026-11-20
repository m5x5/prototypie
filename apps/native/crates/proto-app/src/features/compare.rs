//! Compare (the reference's `features/compare`): a full-window, side-by-side view of a design and its ready
//! variations, oldest first, each in a fixed-width column (no post-load fitting). Columns show the card's
//! snapshot; live web views take over the column bodies once the shell's web view pool exists.

use std::sync::Arc;

use proto_core::Design;
use rune::{Color, El, TextStyle, col, el, row, text};

use crate::app::state::AppState;
use crate::app::style::Palette;
use crate::cards::preview::{self, Picture};
use crate::cards::theme::CardTheme;
use crate::features::icons;

const TITLE: TextStyle = TextStyle::new(16.0, 600.0);
const COL_TITLE: TextStyle = TextStyle::new(13.5, 500.0);

#[derive(Default)]
pub struct CompareState {
    /// The design whose results are compared; None = closed.
    pub base: Option<String>,
    /// The column that gets the live web view (0 = the original); the others show snapshots.
    pub focused: usize,
    /// The live WKWebView over the focused column (worker-5's controller, our own instance).
    pub live: crate::fullscreen::live::LiveCtl,
}

/// Ids in column order: the original, then its ready results.
pub fn column_ids(s: &AppState) -> Vec<String> {
    let Some(base) = s.features.compare.base.clone() else { return Vec::new() };
    let mut ids = vec![base.clone()];
    ids.extend(results(s, &base).iter().map(|d| d.id.clone()));
    ids
}

/// Drive the live view: over the focused column while open, hidden otherwise; its ← / → step the focus and
/// Esc closes. True when something visible changed.
pub fn poll(s: &mut AppState, clock: f64, tx: &mut rune::Tx) -> bool {
    use crate::fullscreen::live::KeyAsk;
    let ids = column_ids(s);
    let focused = s.features.compare.focused.min(ids.len().saturating_sub(1));
    let open = ids.get(focused).cloned();
    let snap = s.store().snapshot();
    let asks = s.features.compare.live.poll(&snap, open.as_deref(), clock, tx);
    let mut changed = false;
    for a in asks {
        changed = true;
        match a {
            KeyAsk::Close => close(s),
            KeyAsk::Step(d) => {
                let n = ids.len().max(1) as i32;
                s.features.compare.focused = ((focused as i32 + d).rem_euclid(n)) as usize;
            }
        }
    }
    changed
}

/// Ready children of `id` (parentId), oldest first.
pub fn results(s: &AppState, id: &str) -> Vec<Arc<Design>> {
    let mut v: Vec<_> = s.snap().children_of(id).into_iter().filter(|d| d.is_ready()).collect();
    // A round lands in one ms: equal createdAt → left to right, as the reference's insertion order shows them.
    v.sort_by(|a, b| a.created_at.total_cmp(&b.created_at).then(a.position.x.total_cmp(&b.position.x)));
    v
}

/// "Compare (n)": n ready children, 0 hides the button.
pub fn result_count(s: &AppState, id: &str) -> usize {
    s.snap().children_of(id).iter().filter(|d| d.is_ready()).count()
}

pub fn open(s: &mut AppState, id: &str) {
    if s.snap().design(id).is_some() {
        s.features.compare.base = Some(id.to_string());
        s.features.compare.focused = 0;
    }
}

/// Close; the web view is hidden in the same turn (it draws above everything).
pub fn close(s: &mut AppState) {
    s.features.compare.base = None;
    s.features.compare.live.hide();
}

/// Column width: phones 400, two or fewer designs half the window each (min 480), else 560.
pub fn column_w(viewport: Option<f64>, count: usize, window_w: f32) -> f32 {
    if viewport.unwrap_or(1280.0) < 600.0 {
        400.0
    } else if count <= 2 {
        ((window_w - 32.0 - 12.0) / 2.0).max(480.0)
    } else {
        560.0
    }
}

pub fn view(s: &AppState, p: &Palette, t: &CardTheme, window: [f32; 2]) -> Option<El> {
    let base_id = s.features.compare.base.as_deref()?;
    let base = s.snap().design(base_id)?;
    let mut all = vec![base.clone()];
    all.extend(results(s, base_id));
    let n = all.len();
    let mut strip = row().key("compare-strip").gap(12.0).h_full().items_stretch();
    let focused = s.features.compare.focused.min(n.saturating_sub(1));
    for (i, d) in all.iter().enumerate() {
        let w = column_w(d.viewport, n, window[0]);
        strip = strip.child(column(s, d, i, i == focused, w, p, t));
    }
    let header = row().key("compare-head").h(24.0).gap(8.0).center_y()
        .child(text(format!("{}: side by side", base.title)).font(TITLE).color(p.fg).truncate().grow(1.0).min_w(0.0))
        .child(row().key("compare-close").role("button").label("Close").size(32.0, 32.0).radius(8.0).center()
            .hover(p.hover).press(p.press).on_press(close).child(icons::x(16.0, p.fg)));
    Some(col().key("compare").role("dialog").label("Compare")
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(p.canvas_bg).padding(16.0).gap(12.0)
        .modal(false, close)
        .child(header)
        .child(el().key("compare-scroll").grow(1.0).min_h(0.0).w_full().scroll_x().child(strip)))
}

fn column(s: &AppState, d: &Design, index: usize, focused: bool, w: f32, p: &Palette, t: &CardTheme) -> El {
    let original = index == 0;
    let id = d.id.clone();
    let (star_id, open_id) = (id.clone(), id.clone());
    let starred = d.is_starred();
    let title = if original { format!("Original · {}", d.title) } else { d.title.clone() };
    let head = row().key(format!("compare-h-{id}")).h(32.0).gap(4.0).center_y()
        .child(text(title).font(COL_TITLE).color(p.fg).truncate().grow(1.0).min_w(0.0))
        .child(row().key(format!("compare-star-{id}")).role("button").label(if starred { "Unstar" } else { "Star" })
            .size(28.0, 28.0).radius(6.0).center().hover(p.hover).press(p.press)
            .on_press(move |s: &mut AppState| toggle_star(s, &star_id))
            .child(icons::star(16.0, if starred { amber() } else { p.muted_fg }, starred)))
        .child(row().key(format!("compare-open-{id}")).role("button").label("Open fullscreen")
            .size(28.0, 28.0).radius(6.0).center().hover(p.hover).press(p.press)
            .on_press(move |s: &mut AppState| {
                close(s);
                crate::fullscreen::open(s, &open_id, &[]);
            })
            .child(icons::maximize(16.0, p.muted_fg)));
    let vw = d.viewport.unwrap_or(1280.0) as f32;
    // The body: the live page in the end; for now the card's snapshot, full column width from the top.
    let mut body = el().key(format!("compare-body-{id}")).w(w).grow(1.0).min_h(0.0).clip_soft(0.0).radius(6.0);
    if let Some(snap) = s.cards.thumbs.get(&d.id) {
        let rect = snap.rect;
        let h = w * rect[3] / rect[2].max(1.0);
        let b = preview::PreviewBox { w, h, centred: false, transparent: true };
        body = body.child(preview::thumb(format!("compare-px-{id}"), b, Some(Picture { pixels: &snap.pixels, rect, cropped: snap.crop.is_some() }), vw, t));
    } else {
        // No frame around the designs (reference): an empty body stays the page colour until the picture lands.
    }
    // The focused column's body carries the live web view; a press on another column moves it there.
    if focused {
        body = body.drawn_rect_into(&s.features.compare.live.rect);
    }
    col().key(format!("compare-col-{id}")).role("group").label(d.title.clone()).w(w).gap(8.0).h_full().min_h(0.0)
        .border(1.0, if focused { p.fg.alpha(0.12) } else { Color::TRANSPARENT }).radius(8.0)
        .on_press(move |s: &mut AppState| s.features.compare.focused = index)
        .child(head).child(body)
}

fn amber() -> Color {
    rune::color::hex("#fbbf24").expect("hex")
}

fn toggle_star(s: &mut AppState, id: &str) {
    let on = !s.snap().design(id).is_some_and(|d| d.is_starred());
    if let Some(c) = proto_core::ops::set_starred(s.snap(), id, on) {
        crate::features::nav::apply(s, vec![c], "star");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn column_widths_follow_the_reference() {
        assert_eq!(super::column_w(Some(390.0), 5, 1280.0), 400.0);
        assert_eq!(super::column_w(None, 2, 1280.0), 618.0);
        assert_eq!(super::column_w(None, 2, 900.0), 480.0);
        assert_eq!(super::column_w(None, 3, 1280.0), 560.0);
    }
}
