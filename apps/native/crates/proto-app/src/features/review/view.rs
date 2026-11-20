//! Review mode: queue on the left, the design on stage with its round's siblings below, the feedback panel
//! on the right (the reference's `ReviewMode`). Pictures are the cards' snapshots for now.

use rune::color::hex;
use rune::{Color, El, ScrollSpec, TextStyle, col, el, row, text, text_input};

use super::{ReviewState, focus_of, generating, lane_name, queue_ids, siblings_of};
use crate::app::state::AppState;
use crate::app::style::Palette;
use crate::cards::preview::{self, Picture};
use crate::cards::theme::CardTheme;
use crate::features::icons;

const TITLE: TextStyle = TextStyle::new(14.0, 600.0);
const BODY: TextStyle = TextStyle::new(12.5, 400.0);
const LABEL: TextStyle = TextStyle::new(12.5, 500.0);
const SMALL: TextStyle = TextStyle::new(11.0, 400.0);
const MONO: TextStyle = TextStyle::new(11.0, 500.0);

fn amber() -> Color {
    hex("#f59e0b").expect("hex")
}

pub fn view(s: &AppState, p: &Palette, t: &CardTheme, window: [f32; 2]) -> Option<El> {
    let r: &ReviewState = &s.features.review;
    if !r.open {
        return None;
    }
    let snap = s.snap();
    let queue = queue_ids(snap);
    let pending = generating(snap);
    let cur = r.effective(snap, &queue);
    let pos = cur.as_ref().and_then(|c| queue.iter().position(|x| x == c));
    let mut status = match (queue.len(), pos) {
        (0, _) => "All caught up".to_string(),
        (n, Some(i)) => format!("{} of {n} to review", i + 1),
        (n, None) => format!("{n} to review"),
    };
    if !pending.is_empty() {
        status.push_str(&format!(" · {} still designing", pending.len()));
    }
    let mut head = row().key("review-head").h(48.0).px(16.0).gap(10.0).center_y()
        .child(icons::inbox(16.0, p.fg))
        .child(text("Review").font(TITLE).color(p.fg))
        .child(text(status).font(BODY).color(p.muted_fg).truncate().grow(1.0).min_w(0.0));
    if !queue.is_empty() {
        head = head.child(row().key("review-all").role("button").label("Mark all reviewed").h(28.0).px(10.0).gap(6.0).radius(6.0).center_y().hover(p.hover).press(p.press)
            .child(icons::check_check(14.0, p.fg)).child(text("Mark all reviewed").font(LABEL).color(p.fg)).on_press(super::mark_all));
    }
    head = head.child(row().key("review-close").role("button").label("Close review").size(28.0, 28.0).radius(6.0).center()
        .hover(p.hover).press(p.press).on_press(super::close).child(icons::x(16.0, p.fg)));

    let body = row().key("review-body").grow(1.0).min_h(0.0).w_full()
        .child(queue_list(s, &queue, &pending, cur.as_deref(), p))
        .child(stage(s, cur.as_deref(), p, t))
        .children(cur.as_deref().map(|c| feedback(s, c, p)));

    let w = (window[0] * 0.96).max(600.0);
    let h = (window[1] * 0.94).max(400.0);
    let card = col().key("review").role("dialog").label("Review")
        .focusable(true).autofocus().no_focus_ring().on_key(super::on_key).size(w, h).radius(14.0).clip_soft(0.0)
        .fill(p.canvas_bg).border(1.0, p.border)
        .child(head)
        .child(el().h(1.0).w_full().fill(p.border))
        .child(body);
    Some(el().key("review-modal").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center()
        .fill(Color::BLACK.alpha(0.5))
        .modal(false, super::close)
        .child(card))
}

fn ghost(key: &'static str, label: &'static str, p: &Palette) -> El {
    row().key(key).role("button").label(label).h(28.0).px(10.0).radius(6.0).center_y().hover(p.hover).press(p.press)
        .child(text(label).font(LABEL).color(p.fg))
}

fn queue_list(s: &AppState, queue: &[String], pending: &[std::sync::Arc<proto_core::Design>], cur: Option<&str>, p: &Palette) -> El {
    let snap = s.snap();
    let mut list = col().key("review-queue").gap(2.0).padding(8.0).w_full()
        .child(text("QUEUE").font(MONO).color(p.muted_fg));
    if queue.is_empty() {
        list = list.child(text("Nothing waiting.").font(SMALL).color(p.muted_fg));
    }
    for (i, id) in queue.iter().enumerate() {
        let Some(d) = snap.design(id) else { continue };
        let active = cur == Some(id.as_str());
        let sub = lane_name(snap, id).unwrap_or_else(|| format!("{:?}", d.kind).to_lowercase());
        let target = id.clone();
        list = list.child(row().key(format!("rq-{id}")).role("button").label(d.title.clone())
            .w_full().min_w(0.0).gap(8.0).px(8.0).py(6.0).radius(6.0).items_start()
            .fill(if active { p.hover } else { Color::TRANSPARENT }).border(1.0, if active { p.border } else { Color::TRANSPARENT })
            .hover(p.hover).press(p.press)
            .on_press(move |s: &mut AppState| s.features.review.current = Some(target.clone()))
            .child(text((i + 1).to_string()).font(SMALL).color(p.muted_fg))
            .child(col().grow(1.0).min_w(0.0)
                .child(row().gap(4.0).center_y()
                    .children(d.is_starred().then(|| icons::star(12.0, hex("#fbbf24").expect("hex"), true)))
                    .child(text(d.title.clone()).font(LABEL).color(p.fg).truncate()))
                .child(text(sub).font(SMALL).color(p.muted_fg).truncate())));
    }
    if !pending.is_empty() {
        list = list.child(el().h(12.0)).child(text("DESIGNING").font(MONO).color(p.muted_fg));
        for d in pending {
            list = list.child(row().key(format!("rg-{}", d.id)).px(8.0).py(4.0).gap(6.0).center_y()
                .child(el().size(6.0, 6.0).radius(3.0).fill(super::dock::blue()))
                .child(text(d.title.clone()).font(SMALL).color(p.muted_fg).truncate()));
        }
    }
    el().key("review-queue-col").w(240.0).h_full().min_h(0.0).fill(p.hover).scroll(ScrollSpec { stick_bottom: false }).child(list)
}

fn picture(s: &AppState, id: &str, w: f32, h: f32, key: String, t: &CardTheme) -> El {
    let Some(d) = s.snap().design(id) else { return el().size(w, h) };
    let vw = d.viewport.unwrap_or(1280.0) as f32;
    let b = preview::PreviewBox { w, h, centred: false, transparent: false };
    let pic = s.cards.thumbs.get(id).map(|sn| Picture { pixels: &sn.pixels, rect: sn.rect, cropped: sn.crop.is_some() });
    preview::thumb(key, b, pic, vw, t)
}

fn stage(s: &AppState, cur: Option<&str>, p: &Palette, t: &CardTheme) -> El {
    let mut c = col().key("review-stage").grow(1.0).min_w(0.0).h_full().min_h(0.0).gap(12.0).padding(16.0);
    let Some(cur) = cur else {
        let pending = generating(s.snap()).len();
        let msg = if pending > 0 {
            format!("{pending} design{} still being made. Each one shows up here the moment it's ready.", if pending == 1 { " is" } else { "s are" })
        } else {
            "Break a page into spokes or start another round. Finished designs land here.".into()
        };
        return c.center().child(text("Inbox zero").font(TITLE).color(p.fg)).child(text(msg).font(BODY).color(p.muted_fg).wrap().max_w(320.0));
    };
    let sib = siblings_of(s.snap(), cur);
    // The live page covers this box once loaded; the snapshot shows until then (and in headless snaps).
    c = c.child(el().key("review-stage-pic").grow(1.0).min_h(0.0).w_full().radius(8.0).clip_soft(0.0).border(1.0, p.border)
        .drawn_rect_into(&s.features.review.live.rect)
        .child(picture(s, cur, 820.0, 615.0, format!("review-pic-{cur}"), t)));
    if sib.len() > 1 {
        let selected = s.features.review.selected(sib.len());
        let reviewed = s.snap().reviewed_ids();
        let mut strip = row().key("review-siblings").gap(12.0);
        for (i, d) in sib.iter().enumerate() {
            let n = i + 1;
            let on = selected.contains(&n);
            let is_cur = d.id == cur;
            let (open_id, toggle_n) = (d.id.clone(), n);
            let ring = if is_cur { p.fg } else if on { amber() } else { Color::TRANSPARENT };
            strip = strip.child(col().key(format!("rs-{}", d.id)).w(150.0).gap(4.0)
                .child(el().key(format!("rs-frame-{}", d.id)).role("button").label(format!("Open #{n}: {}", d.title))
                    .radius(6.0).clip_soft(0.0).outline(2.0, 0.0, ring).opacity(if is_cur { 1.0 } else { 0.85 })
                    .on_press(move |s: &mut AppState| s.features.review.current = Some(open_id.clone()))
                    .child(picture(s, &d.id, 146.0, 110.0, format!("rs-pic-{}", d.id), t))
                    .child(row().key(format!("rs-num-{}", d.id)).role("checkbox").label(format!("Select #{n} for another round"))
                        .checked(on).absolute(6.0, 6.0).size(20.0, 20.0).radius(5.0).center()
                        .fill(if on { amber() } else { p.fg.alpha(0.8) })
                        .on_press(move |s: &mut AppState| super::toggle_num(s, toggle_n))
                        .child(text(n.to_string()).font(MONO).color(if on { Color::WHITE } else { p.canvas_bg })))
                    .children((reviewed.contains(&d.id) || d.is_starred()).then(|| row().absolute(118.0, 6.0).gap(2.0)
                        .children(d.is_starred().then(|| icons::star(14.0, hex("#fbbf24").expect("hex"), true)))
                        .children(reviewed.contains(&d.id).then(|| icons::check(14.0, hex("#10b981").expect("hex")))))))
                .child(text(d.title.clone()).font(SMALL).color(p.fg).truncate()));
        }
        c = c.child(el().key("review-siblings-scroll").w_full().scroll_x().child(strip));
    }
    c
}

fn feedback(s: &AppState, cur: &str, p: &Palette) -> El {
    let snap = s.snap();
    let Some(d) = snap.design(cur) else { return el() };
    let r = &s.features.review;
    let sib = siblings_of(snap, cur);
    let num = sib.iter().position(|x| x.id == cur).map(|i| i + 1);
    let mut info = col().w_full().min_w(0.0).gap(6.0)
        .child(row().gap(8.0).items_start()
            .children(num.filter(|_| sib.len() > 1).map(|n| row().size(20.0, 20.0).radius(5.0).center().fill(p.fg)
                .child(text(n.to_string()).font(MONO).color(p.canvas_bg))))
            .child(text(d.title.clone()).font(TITLE).color(p.fg).wrap().grow(1.0).min_w(0.0)));
    let mut badges = row().gap(6.0).flex_wrap().child(badge(&format!("{:?}", d.kind), p));
    if let Some(l) = lane_name(snap, cur) {
        badges = badges.child(badge(&l, p));
    }
    if snap.is_reviewed(cur) {
        badges = badges.child(badge("Reviewed", p));
    }
    info = info.child(badges);
    if !d.rationale.is_empty() {
        info = info.child(text(d.rationale.clone()).font(BODY).color(p.muted_fg).wrap());
    }
    if let Some(f) = focus_of(snap, cur) {
        info = info.child(text(format!("Component: {f}")).font(BODY).color(p.muted_fg).wrap());
    }
    if let Some(h) = snap.hub_of(cur) {
        info = info.child(text(format!("Hub: {}", h.title)).font(BODY).color(p.muted_fg).wrap());
    }
    let actions = row().w_full().gap(6.0)
        .child(action("review-star", if d.is_starred() { "Starred" } else { "Star" }, "S", false, p,
            icons::star(13.0, if d.is_starred() { hex("#fbbf24").expect("hex") } else { p.fg }, d.is_starred())).on_press(super::toggle_star))
        .child(action("review-discard", "Discard", "X", false, p, icons::trash(13.0, p.fg)).on_press(super::discard))
        .child(action("review-done", "Done", "↵", true, p, icons::check(13.0, p.canvas_bg)).on_press(super::done));
    let field = text_input(&r.feedback, BODY).color(p.fg).key("review-feedback").label("Feedback")
        .placeholder("Talk or type. Numbers at the start pick siblings from this round.", p.muted_fg)
        .w_full().min_h(84.0)
        .on_edit(|s: &mut AppState| &mut s.features.review.feedback);
    let mut fb = col().w_full().min_w(0.0).gap(8.0)
        .child(row().w_full().justify_between().center_y()
            .child(text("Feedback").font(LABEL).color(p.fg))
            .child(text("Try “3 and 6, more playful”").font(SMALL).color(p.muted_fg)))
        .child(el().key("review-feedback-box").padding(8.0).radius(8.0).border(1.0, p.border).child(field));
    let selected = r.selected(sib.len());
    let targets: Vec<usize> = if selected.is_empty() { num.into_iter().collect() } else { selected.clone() };
    if sib.len() > 1 {
        let mut nums = row().gap(4.0).center_y().flex_wrap().child(text("Round on").font(SMALL).color(p.muted_fg));
        for n in 1..=sib.len() {
            let on = targets.contains(&n);
            nums = nums.child(row().key(format!("round-on-{n}")).role("checkbox").label(format!("Include #{n}")).checked(on)
                .size(24.0, 24.0).radius(5.0).center().border(1.0, if on { amber() } else { p.border })
                .fill(if on { amber().alpha(0.15) } else { Color::TRANSPARENT })
                .on_press(move |s: &mut AppState| super::toggle_num(s, n))
                .child(text(n.to_string()).font(MONO).color(if on { amber() } else { p.muted_fg })));
        }
        fb = fb.child(nums);
    }
    let mut picker = row().gap(2.0).padding(2.0).radius(8.0).border(1.0, p.border).center_y();
    for n in 1..=5u32 {
        let on = r.round_count == n;
        picker = picker.child(row().key(format!("round-count-{n}")).role("radio").label(n.to_string()).checked(on)
            .size(24.0, 24.0).radius(6.0).center().fill(if on { p.fg } else { Color::TRANSPARENT })
            .on_press(move |s: &mut AppState| s.features.review.round_count = n)
            .child(text(n.to_string()).font(MONO).color(if on { p.canvas_bg } else { p.muted_fg })));
    }
    fb = fb.child(row().gap(8.0).center_y().child(text("Each").font(SMALL).color(p.muted_fg)).child(picker));
    let on_text = targets.iter().map(|n| format!("#{n}")).collect::<Vec<_>>().join(", ");
    let can_round = !targets.is_empty() && d.is_ready();
    let round = row().key("review-round").role("button").label("Another round").disabled(!can_round)
        .h(34.0).w_full().radius(8.0).center().gap(6.0).fill(if can_round { p.fg } else { p.fg.alpha(0.35) })
        .child(icons::refresh(13.0, p.canvas_bg))
        .child(text(if on_text.is_empty() { "Another round".into() } else { format!("Another round on {on_text}") }).font(LABEL).color(p.canvas_bg))
        .child(text("⌘↵").font(SMALL).color(p.canvas_bg.alpha(0.7)));
    fb = fb.child(if can_round { round.press(Color::BLACK.alpha(0.15)).on_press(super::another_round) } else { round });
    let show = row().key("review-show").role("button").label("Show on canvas").h(28.0).px(10.0).gap(6.0).radius(6.0).center_y().hover(p.hover).press(p.press)
        .child(icons::locate(13.0, p.muted_fg)).child(text("Show on canvas").font(LABEL).color(p.muted_fg)).on_press(super::show_on_canvas);
    let legend = [("J / K", "next / previous in queue"), ("← / →", "move through this round"), ("1-9", "pick siblings for a round"), ("S · X · ↵", "star · discard · done"), ("F · ⌘↵", "feedback · another round")]
        .iter()
        .fold(col().gap(3.0).padding(12.0).fill(p.hover), |c, (k, v)| {
            c.child(row().gap(10.0).child(text(*k).font(MONO).color(p.fg).w(64.0)).child(text(*v).font(SMALL).color(p.muted_fg)))
        });
    col().key("review-feedback-panel").w(340.0).h_full().min_h(0.0).border(1.0, p.border)
        .child(el().grow(1.0).min_h(0.0).w_full().scroll(ScrollSpec { stick_bottom: false })
            .child(col().w_full().min_w(0.0).gap(14.0).padding(16.0).child(info).child(actions).child(el().h(1.0).w_full().fill(p.border)).child(fb).child(show)))
        .child(legend)
}

fn badge(label: &str, p: &Palette) -> El {
    row().h(20.0).px(8.0).radius(10.0).center_y().border(1.0, p.border).child(text(label.to_string()).font(SMALL).color(p.fg))
}

fn action(key: &'static str, label: &str, kbd: &'static str, primary: bool, p: &Palette, glyph: El) -> El {
    let (fill, fg) = if primary { (p.fg, p.canvas_bg) } else { (Color::TRANSPARENT, p.fg) };
    row().key(key).role("button").label(label.to_string()).h(30.0).grow(1.0).px(8.0).gap(4.0).radius(8.0).center()
        .fill(fill).border(1.0, if primary { fill } else { p.border }).hover(p.hover).press(p.press)
        .child(glyph)
        .child(text(label.to_string()).font(LABEL).color(fg))
        .child(text(kbd).font(SMALL).color(fg.alpha(0.6)))
}
