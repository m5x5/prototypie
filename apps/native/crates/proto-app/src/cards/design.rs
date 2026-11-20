//! The design card (docs/spec-cards.md §2): a title label on the canvas above an elevated surface
//! with the preview and the action row.

use proto_core::{Design, DesignKind, DesignStatus};
use rune::{Color, El, LoopSpec, Spring, col, icon, row, spacer, text};

use super::actions;
use super::controls::{Badge, BadgeLook, Variant, btn_sm, icon_btn};
use super::facts::DesignFacts;
use super::icons::*;
use super::preview::{Picture, preview_box, thumb};
use super::theme::{CardTheme, NODE_W, T10, TITLE_H, XS, XS_MED};
use super::CardAction as A;

/// Per-card view state the canvas and fullscreen decide.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub selected: bool,
    pub in_poll: bool,
    /// The Implement request's state (features' agent tasks).
    pub implement: super::actions::ImplementPhase,
    /// Open Annotate notes (features).
    pub open_notes: usize,
    /// The design is open in fullscreen: the preview's matched element lives there meanwhile.
    pub in_fullscreen: bool,
    /// The "send again?" confirmation shows under the action row.
    pub confirm_resend: bool,
    /// No agent is connected (the capacity answered 0): a card waiting for one says so instead of "Designing…".
    pub no_agent: bool,
}

/// The `matched` id shared by a card's preview and the fullscreen preview (the morph).
pub fn morph_id(design_id: &str) -> String {
    format!("fs:{design_id}")
}

/// Shadow and outline changes on selection: the reference's 150 ms transition.
fn quick() -> Spring {
    Spring::new(0.15, 0.0)
}

pub fn node(d: &Design, f: &DesignFacts, ctx: Ctx, pic: Option<Picture>, pod: Option<El>, t: &CardTheme) -> El {
    let surface = col().key(format!("d:{}:surface", d.id)).w_full()
        .radius(12.0).border(1.0, t.border).fill(t.card)
        .box_shadow(t.card_shadow(ctx.selected))
        .outline(2.0, 2.0, if ctx.selected { t.primary.alpha(0.6) } else { Color::TRANSPARENT })
        .motion(quick())
        .child(preview_area(d, ctx, pic, t))
        .children(pod)
        // The action row shows only on the focused (selected) card.
        .children(ctx.selected.then(|| actions::row_for(d, f, ctx.in_poll, ctx.implement, t)))
        .children(ctx.confirm_resend.then(|| actions::resend_confirm(d, t)));
    col().key(format!("d:{}:node", d.id)).w(NODE_W)
        .child(title_label(d, ctx.open_notes, ctx.selected, t))
        .child(surface)
}

fn title_label(d: &Design, open_notes: usize, selected: bool, t: &CardTheme) -> El {
    let id = d.id.as_str();
    let k = |name: &str| format!("d:{id}:{name}");
    let title = text(d.title.clone())
        .font(if selected { XS_MED } else { XS })
        .color(if selected { t.fg } else { t.muted_fg })
        .single_line().truncate().help(d.title.clone()).shrink();
    let kind = (d.kind != DesignKind::Variation).then(|| {
        let label = match d.kind {
            DesignKind::Page => "Page",
            DesignKind::Spoke => "Spoke",
            DesignKind::Variation => "Variation",
        };
        let mut b = Badge::new(k("kind"), label, BadgeLook::Secondary, t);
        b.h = 16.0;
        b.px = 6.0;
        b.font = T10;
        b.build(t)
    });
    let notes_badge = (open_notes > 0).then(|| {
        let n = open_notes.to_string();
        let mut b = Badge::new(k("notes-badge"), &n, BadgeLook::Outline, t);
        b.glyph = Some(&MESSAGE_SQUARE_TEXT);
        b.build(t).help(format!("{open_notes} Annotate note(s)"))
    });
    let note = d.notes.as_ref().filter(|n| !n.trim().is_empty())
        .map(|n| icon(&NOTEBOOK_PEN, 14.0).color(t.muted_fg).help(n.clone()).label("Has a note"));
    let ready = d.is_ready();
    let star = if d.is_starred() { &STAR_FILLED } else { &STAR };
    let star_color = if d.is_starred() { t.amber_400 } else { t.muted_fg };
    row().key(k("title")).w_full().h(TITLE_H).pb(4.0).px(2.0).gap(6.0).center_y()
        .child(title)
        .children(kind)
        .children(notes_badge)
        .children(note)
        .child(spacer())
        .child(icon_btn(k("open"), "Open fullscreen", &MAXIMIZE_2, 24.0, 14.0, t.muted_fg, t, ready.then(|| A::OpenFullscreen(id.into()))))
        .child(icon_btn(k("star"), if d.is_starred() { "Unstar" } else { "Star" }, star, 24.0, 14.0, star_color, t, Some(A::ToggleStar(id.into()))))
        .child(icon_btn(k("delete"), "Delete", &TRASH, 24.0, 14.0, t.muted_fg, t, Some(A::Delete(id.into()))))
}

/// The top of the surface: the thumbnail, or the generating / error state. Clipped to the
/// surface's top corners (not the surface itself, so edge handles stay whole).
fn preview_area(d: &Design, ctx: Ctx, pic: Option<Picture>, t: &CardTheme) -> El {
    let id = d.id.as_str();
    let inner_w = NODE_W - 2.0;
    let area = col().key(format!("d:{id}:preview-area")).w_full().clip_corners([11.0, 11.0, 0.0, 0.0]);
    match d.status {
        DesignStatus::Ready => {
            // A narrow tall crop makes the box taller (spec §2.3).
            let tall = pic.as_ref().filter(|p| p.cropped).map(|p| [p.rect[2], p.rect[3]]);
            let b = preview_box(d.viewport, d.viewport_height, inner_w, tall);
            let vw = d.viewport.unwrap_or(1280.0) as f32;
            // The morph's shared element lives on the card unless fullscreen holds it. A matched
            // element must *leave the tree* for the flight to start, so while fullscreen is open
            // the card shows a differently keyed placeholder (the old element leaves, the
            // fullscreen box mounts in the same commit; the reverse on close).
            let th = if ctx.in_fullscreen {
                thumb(format!("d:{id}:thumb-held"), b, None, vw, t)
            } else {
                thumb(format!("d:{id}:thumb"), b, pic, vw, t).matched(morph_id(id))
            };
            area.child(th)
        }
        // Generating through the agent queue with no agent connected: nothing is working on it.
        DesignStatus::Generating if ctx.no_agent && d.request.as_ref().and_then(|r| r.agent.as_deref()) == Some("claude") => area.child(
            col().key(format!("d:{id}:generating")).w_full().aspect_ratio(4.0 / 3.0).fill(t.muted.alpha(0.4))
                .center().gap(6.0).padding(16.0)
                .child(icon(&CLOCK, 20.0).color(t.muted_fg))
                .child(text("Wartet auf einen Agenten").font(XS_MED).color(t.fg))
                .child(text("Es ist kein Agent verbunden. Verbinde einen über das Agenten-Panel.").font(XS).color(t.muted_fg).wrap().align(rune::Align::Center)),
        ),
        DesignStatus::Generating => area.child(
            col().key(format!("d:{id}:generating")).w_full().aspect_ratio(4.0 / 3.0).fill(t.muted.alpha(0.4))
                .center().gap(8.0)
                .child(icon(&LOADER_CIRCLE, 20.0).color(t.muted_fg).looping(LoopSpec::rotation(1.0)))
                .child(text("Designing…").font(XS).color(t.muted_fg)),
        ),
        DesignStatus::Error => {
            let msg = d.error.clone().unwrap_or_else(|| "Generation failed".into());
            area.child(
                col().key(format!("d:{id}:error")).w_full().aspect_ratio(4.0 / 3.0).fill(t.destructive.alpha(0.05))
                    .padding(16.0).center().gap(8.0)
                    .child(icon(&TRIANGLE_ALERT, 20.0).color(t.destructive))
                    .child(text(msg).font(XS).color(t.destructive).wrap().align(rune::Align::Center))
                    .child(btn_sm(format!("d:{id}:retry"), "Retry", Some(&ROTATE_CCW), Variant::Outline, None, t, Some(A::Retry(id.into()))).mt(4.0)),
            )
        }
    }
}
