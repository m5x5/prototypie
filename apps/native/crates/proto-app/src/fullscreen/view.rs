//! The fullscreen page's layout (docs/spec-cards.md §4.2): header with set controls and step
//! pills, the preview box (the morph's landing spot; the live web view goes over it once landed),
//! plus the sidebar.

use proto_core::Design;
use rune::{Cx, El, Transition, col, el, row, text};

use super::View;
use crate::app::state::AppState;
use crate::app::style::Palette;
use crate::cards::icons::{CHEVRON_LEFT, CHEVRON_RIGHT, PANEL_RIGHT, X};
use crate::cards::theme::{TITLE, XS, XS_NUM};
use crate::cards::{CardTheme, icon_btn, morph_id, preview};

const ASIDE_W: f32 = 288.0;
const GAP: f32 = 16.0;
/// The page's padding inside the window.
const PAGE_PAD: f32 = 16.0;

pub fn page(s: &AppState, cx: &mut Cx<AppState>, d: &Design, p: &Palette, t: &CardTheme) -> El {
    let View::Open { set, .. } = &s.fullscreen.view else { unreachable!("page is only built while open") };
    let steps: Vec<(String, String)> = match set {
        Some(set) => set.iter().filter_map(|i| s.snap().design(i).map(|x| (x.id.clone(), x.title.clone()))).collect(),
        None => Vec::new(),
    };
    let at = steps.iter().position(|(i, _)| *i == d.id);
    let multi = steps.len() > 1 && at.is_some();

    let body = cx.layout_size("fs:body").unwrap_or([1280.0 - 32.0, 800.0 - 140.0]);
    // The modal is the full-window layer; inside it the page content is the control it focuses on
    // open (`autofocus` names a descendant, as HTML's attribute in a dialog; on the modal itself
    // it's ignored and the modal falls back to its first text field, the note). So ←/→/Esc reach
    // `on_key`, and the sidebar's fields take focus only when clicked (the reference's
    // initialFocus on the dialog).
    let content = col().key("fs:content").w_full().h_full().gap(12.0)
        .focusable(true).autofocus().no_focus_ring()
        .child(header(d, &steps, at, multi, cx.chrome_insets().leading, t))
        .child(
            row().key("fs:body").w_full().grow(1.0).min_h(0.0).gap(GAP)
                .child(preview_box(s, d, body, crate::cards::picture(s, &d.id), s.fullscreen.live_rect(), t))
                .children((!s.fullscreen.aside_hidden).then(|| aside(s, d, p, t))),
        );
    col().key("fullscreen").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(t.background).padding(PAGE_PAD)
        .modal_tx(false, |s: &mut AppState, tx: &mut rune::Tx| {
            // Esc / outside press: Annotate closes first when it is open.
            if super::annotate_takes_escape(s) {
                return;
            }
            super::morph_motion(tx);
            super::close(s);
        })
        .transition(Transition::opacity())
        .child(content)
        .child(icon_btn("fs:close".into(), "Close", &X, 28.0, 16.0, t.fg, t, None)
            .on_press_tx(|s: &mut AppState, tx: &mut rune::Tx| {
                super::morph_motion(tx);
                super::close(s);
            })
            .pin(None, Some(8.0), Some(8.0), None))
        // Collapses the side panel (the preview takes its width), left of the close button.
        .child(icon_btn("fs:aside-toggle".into(), if s.fullscreen.aside_hidden { "Show side panel" } else { "Hide side panel" }, &PANEL_RIGHT, 28.0, 16.0, t.fg, t, None)
            .on_press(|s: &mut AppState| s.fullscreen.aside_hidden = !s.fullscreen.aside_hidden)
            .pin(None, Some(8.0), Some(44.0), None))
}

/// `leading`: the window chrome's leading inset (the traffic lights), 0 without them. The title
/// row sits in the titlebar band, so it starts after them, as the shell header's left group does
/// (`leading + 8`); the page's own 16 pt padding already counts toward that.
fn header(d: &Design, steps: &[(String, String)], at: Option<usize>, multi: bool, leading: f32, t: &CardTheme) -> El {
    let pl = if leading > 0.0 { (leading + 8.0 - PAGE_PAD).max(0.0) } else { 0.0 };
    let mut title_row = row().key("fs:title-row").w_full().gap(8.0).pl(pl).pr(40.0).center_y();
    if multi {
        let at = at.unwrap_or(0);
        title_row = title_row.child(
            row().key("fs:stepper").gap(2.0).center_y()
                .child(icon_btn("fs:prev".into(), "Previous design (←)", &CHEVRON_LEFT, 28.0, 16.0, t.fg, t, None)
                    .on_press(|s: &mut AppState| super::step(s, -1)))
                .child(row().key("fs:pos").min_w(40.0).center()
                    .child(text(format!("{} / {}", at + 1, steps.len())).font(XS_NUM).color(t.muted_fg)))
                .child(icon_btn("fs:next".into(), "Next design (→)", &CHEVRON_RIGHT, 28.0, 16.0, t.fg, t, None)
                    .on_press(|s: &mut AppState| super::step(s, 1))),
        );
    }
    title_row = title_row.child(text(d.title.clone()).font(TITLE).color(t.fg).single_line().truncate().shrink());
    let mut head = col().key("fs:header").w_full().gap(8.0).child(title_row);
    if multi {
        let mut strip = row().key("fs:steps").w_full().gap(4.0).clip_soft(0.0);
        for (i, (id, title)) in steps.iter().enumerate() {
            let current = Some(i) == at;
            let (fill, fg, border) = if current { (Some(t.primary), t.primary_fg, t.primary) } else { (None, t.muted_fg, t.border) };
            let target = id.clone();
            let mut pill = row().key(format!("fs:step:{id}")).role("button").label(title.clone()).help(title.clone())
                .h(28.0).px(10.0).gap(6.0).radius(14.0).border(1.0, border).max_w(224.0).center_y().shrink()
                .child(text((i + 1).to_string()).font(XS_NUM).color(fg.alpha(0.7)))
                .child(text(title.clone()).font(XS).color(fg).single_line().truncate())
                .on_press(move |s: &mut AppState| {
                    if let View::Open { id, .. } = &mut s.fullscreen.view {
                        *id = target.clone();
                    }
                });
            pill = match fill {
                Some(f) => pill.fill(f),
                None => pill.hover(t.muted),
            };
            strip = strip.child(pill);
        }
        head = head.child(strip);
    }
    head
}

/// The preview's box in the body (`body` = the body row's size), per §4.2's table.
pub fn box_size(viewport: Option<f64>, viewport_h: Option<f64>, body: [f32; 2]) -> ([f32; 2], bool) {
    let avail = (body[0] - ASIDE_W - GAP).max(0.0);
    let bh = body[1].max(0.0);
    let vw = viewport.filter(|v| *v > 0.0).unwrap_or(1280.0) as f32;
    match viewport_h.filter(|h| *h > 0.0).map(|h| h as f32) {
        Some(vh) if vh > vw => {
            let h = bh.min(vh);
            ([(h * vw / vh).min(avail), h], true)
        }
        Some(vh) => {
            let w = avail.min(vw);
            let h = (w * vh / vw).min(bh);
            ([h * vw / vh, h], false)
        }
        None if vw < 600.0 => ([vw.min(avail), bh], true),
        None => ([avail, bh], false),
    }
}

fn preview_box(s: &AppState, d: &Design, body: [f32; 2], pic: Option<preview::Picture>, rect: &rune::Live<[f32; 4]>, t: &CardTheme) -> El {
    let ([w, h], centred) = box_size(d.viewport, d.viewport_height, body);
    s.fullscreen.set_box_w(w);
    let pb = preview::PreviewBox { w, h, centred: false, transparent: false };
    let frame = el().key("fs:preview").size(w, h).radius(8.0).clip_rounded(8.0).border(1.0, t.border).fill(t.muted)
        .matched(morph_id(&d.id))
        .drawn_rect_into(rect)
        .child(preview::thumb(format!("fs:thumb:{}", d.id), pb, pic, d.viewport.unwrap_or(1280.0) as f32, t));
    let slot = row().key("fs:preview-slot").grow(1.0).min_w(0.0).h_full();
    if centred { slot.justify_center().items_center().child(frame) } else { slot.items_start().child(frame) }
}

/// The side panel: the designer's note (spec §4.2.1), then features' sections for this design
/// (AI edit request box, Annotate notes). Keyed per design so stepping through a set gives
/// each design its own boxes.
fn aside(s: &AppState, d: &Design, p: &Palette, t: &CardTheme) -> El {
    let steps = match &s.fullscreen.view {
        View::Open { set: Some(set), .. } => set.iter().position(|i| *i == d.id).map(|at| (at, set.len())),
        _ => None,
    };
    // The side panel scrolls when its content (neighbours, note, notes, edit box, rationale) outgrows the window.
    let body = col().w_full().gap(12.0)
        .children(super::neighbours::view(s, d, steps, t))
        .child(super::note::view(s, d, t))
        .children(crate::features::fullscreen_sections(s, p, d))
        .children(rationale(s, d, t));
    col().key(format!("fs:aside:{}", d.id)).w(ASIDE_W).h_full().min_h(0.0)
        .child(el().key(format!("fs:aside-scroll:{}", d.id)).w_full().grow(1.0).min_h(0.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(body))
}

/// Lines the rationale shows until "Read more".
const RATIONALE_LINES: usize = 3;

/// The design's rationale below the side panel's inputs: muted, clamped to a few lines with
/// "Read more" / "Show less" (the reference inspector's `Rationale`).
fn rationale(s: &AppState, d: &Design, t: &CardTheme) -> Option<El> {
    let body = d.rationale.trim();
    if body.is_empty() {
        return None;
    }
    let open = s.fullscreen.rationale_open.as_deref() == Some(d.id.as_str());
    // Rune can't report a cut-short paragraph yet: estimate from ~6 pt per character at XS.
    let per_line = (ASIDE_W / 6.0) as usize;
    let long = body.lines().map(|l| l.chars().count().div_ceil(per_line).max(1)).sum::<usize>() > RATIONALE_LINES;
    let mut para = text(body.to_string()).font(XS).color(t.muted_fg).wrap();
    if long && !open {
        para = para.max_lines(RATIONALE_LINES).truncate();
    }
    let toggle = long.then(|| {
        let id = d.id.clone();
        text(if open { "Show less" } else { "Read more" }).font(crate::cards::theme::XS_MED).color(t.muted_fg)
            .key(format!("fs:rationale-toggle:{}", d.id)).role("button")
            .on_press(move |s: &mut AppState| {
                let f = &mut s.fullscreen.rationale_open;
                *f = if f.as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) };
            })
    });
    Some(col().key(format!("fs:rationale:{}", d.id)).w_full().gap(2.0).child(para).children(toggle))
}

#[cfg(test)]
mod tests {
    use super::box_size;

    const BODY: [f32; 2] = [1400.0, 700.0];
    const AVAIL: f32 = 1400.0 - 288.0 - 16.0;

    #[test]
    fn wide_design_fills_the_rest() {
        assert_eq!(box_size(None, None, BODY), ([AVAIL, 700.0], false));
    }

    #[test]
    fn phone_shows_at_native_width_centred() {
        assert_eq!(box_size(Some(390.0), None, BODY), ([390.0, 700.0], true));
    }

    #[test]
    fn portrait_screen_fits_the_height_never_above_native() {
        assert_eq!(box_size(Some(640.0), Some(960.0), BODY), ([700.0 * 640.0 / 960.0, 700.0], true));
        assert_eq!(box_size(Some(320.0), Some(480.0), BODY), ([320.0, 480.0], true));
    }

    #[test]
    fn landscape_screen_keeps_its_ratio() {
        let ([w, h], c) = box_size(Some(1280.0), Some(720.0), BODY);
        assert!(!c && (w / h - 1280.0 / 720.0).abs() < 0.01 && w <= AVAIL && h <= 700.0);
    }
}
