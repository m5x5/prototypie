//! The selected design's header: its title (press to copy), the prompt it was made from (an info
//! popover with "Reuse prompt") and its rationale (one line until expanded).

use proto_core::Design;
use rune::{Cx, El, Tx, col, el, row, text};

use super::icons::{self, CHECK, INFO};
use super::style::{self, Ink};
use crate::app::state::AppState;

/// How long "copied" shows after copying the title.
const COPIED_FOR: f64 = 1.5;
/// Width the rationale has on one line (panel 340 − 2×16 padding − 2×1 border).
const TEXT_W: f32 = 306.0;

/// The details' own state (inside `InspectorState`).
#[derive(Default, Debug)]
pub struct DetailsState {
    /// The title was copied; shown until this clock time (seconds).
    pub copied_until: Option<f64>,
    /// The design whose rationale is expanded.
    pub rationale_open: Option<String>,
    /// The design whose "Made from" popover is open.
    pub made_from_open: Option<String>,
}

impl DetailsState {
    /// Fires the "copied" timer. True when something visible changed.
    pub fn poll(&mut self, clock: f64) -> bool {
        match self.copied_until {
            Some(t) if clock >= t => {
                self.copied_until = None;
                true
            }
            _ => false,
        }
    }
}

pub fn view(s: &AppState, _cx: &mut Cx<AppState>, d: &Design, k: &Ink) -> El {
    let st = &s.inspector.details;
    let copied = st.copied_until.is_some();
    let title = d.title.clone();
    let title_btn = style::focusable(el(), k).key("insp-title").role("button").label(format!("Titel kopieren: {}", d.title))
        .min_w(0.0).shrink().radius(style::R_SM)
        .help(if copied { "Kopiert" } else { "Titel kopieren" })
        .on_press_tx(move |s: &mut AppState, tx: &mut Tx| {
            tx.copy(title.clone());
            let until = tx.clock() + COPIED_FOR;
            s.inspector.details.copied_until = Some(until);
            tx.wake_at(until);
        })
        .child(text(d.title.clone()).font(style::TITLE).color(k.fg).truncate());
    let mut head = row().key("insp-title-row").w_full().gap(6.0).center_y()
        .child(title_btn)
        .children(copied.then(|| icons::draw(&CHECK, 14.0, k.muted_fg)));
    if d.instruction.as_deref().is_some_and(|p| !p.is_empty()) {
        head = head.child(made_from_button(&d.id, k));
    }
    col().key("insp-details").w_full().gap(8.0)
        .child(head)
        .children(rationale(s, d, k))
}

fn made_from_button(id: &str, k: &Ink) -> El {
    let id = id.to_string();
    style::focusable(el(), k).key("insp-made-from").role("button").label("Prompt this design was made from").help("Made from…")
        .size(14.0, 14.0).center()
        .on_press(move |s: &mut AppState| {
            let open = &mut s.inspector.details.made_from_open;
            *open = if open.as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) };
        })
        .child(icons::draw(&INFO, 14.0, k.muted_fg))
}

/// The open "Made from" popover, drawn outside the panel's scroll clip.
pub fn overlays(s: &AppState, d: Option<&Design>, k: &Ink) -> Vec<El> {
    let Some(d) = d else { return Vec::new() };
    let open = s.inspector.details.made_from_open.as_deref() == Some(d.id.as_str());
    match d.instruction.as_deref().filter(|p| open && !p.is_empty()) {
        Some(prompt) => vec![made_from_popover(prompt, k)],
        None => Vec::new(),
    }
}

/// The "Made from" popover under the info button: the prompt and "Reuse prompt".
fn made_from_popover(prompt: &str, k: &Ink) -> El {
    let p = prompt.to_string();
    let body = col().key("insp-made-from-pop").w(288.0).padding(12.0).gap(8.0)
        .child(text("Made from").font(style::XS_MEDIUM).color(k.fg))
        .child(col().key("insp-made-from-scroll").w_full().max_h(240.0)
            .scroll(rune::ScrollSpec { stick_bottom: false })
            .child(text(prompt.to_string()).font(style::XS).color(k.muted_fg).wrap().w_full()))
        .child(style::focusable(el(), k).key("insp-reuse").role("button").label("Reuse prompt").autofocus()
            .on_press(move |s: &mut AppState| {
                s.inspector.details.made_from_open = None;
                crate::composer::set_text(s, &p);
            })
            .child(text("Reuse prompt").font(style::XS_MEDIUM).color(k.muted_fg)));
    style::popover(body, k)
        .anchored("insp-made-from", rune::Side::Bottom, rune::Align::Start, 6.0)
        .modal(true, |s: &mut AppState| s.inspector.details.made_from_open = None)
}

/// The rationale: one truncated line, "Read more" when it doesn't fit; expanded it wraps.
fn rationale(s: &AppState, d: &Design, k: &Ink) -> Option<El> {
    if d.rationale.trim().is_empty() {
        return None;
    }
    let open = s.inspector.details.rationale_open.as_deref() == Some(d.id.as_str());
    let clamped = !open && overflows(&d.rationale);
    let line = text(d.rationale.clone()).font(style::XS).color(k.muted_fg).w_full();
    let line = if open { line.wrap() } else { line.truncate() };
    let id = d.id.clone();
    let toggle = (open || clamped).then(|| {
        style::focusable(el(), k).key("insp-rationale-toggle").role("button")
            .on_press(move |s: &mut AppState| {
                let r = &mut s.inspector.details.rationale_open;
                *r = if r.is_some() { None } else { Some(id.clone()) };
            })
            .child(text(if open { "Show less" } else { "Read more" }).font(style::XS_MEDIUM).color(k.muted_fg))
    });
    Some(col().key("insp-rationale").w_full().items_start().child(line).children(toggle))
}

/// Whether `t` needs more than one line at the panel's text width: a newline, or an estimate
/// of its 12pt width (≈ 0.52 em per character in the UI face) past `TEXT_W`.
fn overflows(t: &str) -> bool {
    t.contains('\n') || t.chars().count() as f32 * 12.0 * 0.52 > TEXT_W
}
