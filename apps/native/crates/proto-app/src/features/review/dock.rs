//! "Review" in the header: waiting/generating at a glance; the panel lists the queue's head.

use rune::color::hex;
use rune::{Color, El, TextStyle, col, el, row, text};

use super::{generating, lane_name, queue_ids};
use crate::app::state::AppState;
use crate::app::style::{self, Palette};
use crate::features::icons;

const SHOWN: usize = 5;
const LABEL: TextStyle = TextStyle::new(12.5, 500.0);
const SMALL: TextStyle = TextStyle::new(11.5, 400.0);
const TITLE: TextStyle = TextStyle::new(13.0, 600.0);
/// The waiting count's badge: tabular figures so the pill doesn't jitter as the count changes.
const BADGE: TextStyle = TextStyle::new(11.0, 600.0).lh(14.0).features("'tnum' 1");

pub fn blue() -> Color {
    hex("#3b82f6").expect("hex")
}

pub fn button(s: &AppState, p: &Palette) -> El {
    let queued = queue_ids(s.snap()).len();
    let running = s.snap().runs().count();
    let waiting = if queued > 0 { format!("{queued} waiting") } else { "Nothing waiting".into() };
    let label = if running > 0 { format!("Review: {waiting}, {running} generating") } else { format!("Review: {waiting}") };
    // Icon, "Review" (over "n generating" while designs generate), then the waiting count as a white badge with black
    // figures in both themes (the user's Annotate note: no "waiting", just the number).
    let name = text("Review").font(LABEL).color(p.fg);
    let badge = (queued > 0).then(|| {
        row().key("review-dock-count").h(20.0).min_w(28.0).px(8.0).radius(10.0).center()
            .fill(Color::WHITE)
            .border(1.0, p.border)
            .child(text(queued.to_string()).font(BADGE).color(Color::BLACK))
    });
    style::glass_pill(row().key("review-dock").role("button").tab_index(1).label(label).expanded(s.features.review.dock_open)
        .h(36.0).pl(10.0).pr(12.0).gap(8.0).radius(18.0).center_y(), p)
        .hover(p.hover).press(p.press)
        .on_press(|s: &mut AppState| s.features.review.dock_open = !s.features.review.dock_open)
        .child(icons::inbox(16.0, p.fg))
        .child(name)
        .children(badge)
}

pub fn panel(s: &AppState, p: &Palette) -> Option<El> {
    if !s.features.review.dock_open || s.features.review.open {
        return None;
    }
    let queue = queue_ids(s.snap());
    let running = generating(s.snap()).len();
    let mut c = col().key("review-dock-panel").role("dialog").label("Review queue").w(320.0).radius(14.0)
        .child(row().px(14.0).py(10.0).justify_between().center_y()
            .child(text("Review queue").font(TITLE).color(p.fg))
            .child(text(if queue.is_empty() { "Nothing waiting".into() } else { format!("{} waiting", queue.len()) }).font(SMALL).color(p.muted_fg)));
    if running > 0 {
        c = c.child(row().px(14.0).pb(8.0).child(text(format!("{running} design{} generating", if running == 1 { "" } else { "s" })).font(SMALL).color(blue())));
    }
    if queue.is_empty() {
        c = c.child(row().px(14.0).py(20.0).justify_center().child(text("Every ready design has been reviewed.").font(SMALL).color(p.muted_fg)));
    } else {
        let mut list = col().key("review-dock-list").px(6.0).gap(2.0);
        for (i, id) in queue.iter().take(SHOWN).enumerate() {
            let Some(d) = s.snap().design(id) else { continue };
            let sub = lane_name(s.snap(), id).unwrap_or_else(|| format!("{:?}", d.kind).to_lowercase());
            let start = id.clone();
            list = list.child(row().key(format!("review-dock-{id}")).role("button").label(d.title.clone())
                .w_full().min_w(0.0).gap(8.0).px(8.0).py(6.0).radius(8.0).items_start()
                .hover(p.hover).press(p.press)
                .on_press(move |s: &mut AppState| super::open(s, Some(start.clone())))
                .child(text((i + 1).to_string()).font(SMALL).color(p.muted_fg))
                .child(col().grow(1.0).min_w(0.0)
                    .child(row().gap(4.0).center_y()
                        .children(d.is_starred().then(|| icons::star(12.0, hex("#fbbf24").expect("hex"), true)))
                        .child(text(d.title.clone()).font(LABEL).color(p.fg).truncate()))
                    .child(text(sub).font(SMALL).color(p.muted_fg).truncate())));
        }
        if queue.len() > SHOWN {
            list = list.child(row().px(8.0).py(4.0).child(text(format!("+{} more", queue.len() - SHOWN)).font(SMALL).color(p.muted_fg)));
        }
        c = c.child(list);
    }
    let start = row().key("review-start").role("button").label("Start reviewing").h(32.0).w_full().radius(8.0).center()
        .fill(if queue.is_empty() { p.fg.alpha(0.3) } else { p.fg }).press(Color::BLACK.alpha(0.15))
        .child(text("Start reviewing").font(LABEL).color(p.canvas_bg));
    let start = if queue.is_empty() { start } else { start.on_press(|s: &mut AppState| super::open(s, None)) };
    c = c.child(el().w_full().h(1.0).mt(8.0).fill(p.border)).child(row().padding(10.0).child(start));
    Some(style::glass(c, p, 0.92)
        .anchored("review-dock", rune::Side::Bottom, rune::Align::End, 8.0)
        .modal(true, |s: &mut AppState| s.features.review.dock_open = false))
}
