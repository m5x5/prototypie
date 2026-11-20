//! A design card's action row (docs/spec-cards.md §2.5), in the reference's order. Controls whose
//! behaviour belongs to another module only queue a `CardAction` for it.

use proto_core::{Design, DesignKind};
use rune::{El, col, row, spacer};

use super::controls::{Badge, BadgeLook, Variant, btn_sm, icon_btn};
use super::facts::DesignFacts;
use super::icons::*;
use super::theme::{CardTheme, XS_MED};
use super::CardAction as A;

pub use crate::features::agent_tasks::implement::ImplementPhase;

pub fn row_for(d: &Design, f: &DesignFacts, in_poll: bool, phase: ImplementPhase, t: &CardTheme) -> El {
    let id = d.id.as_str();
    let ready = d.is_ready();
    let k = |name: &str| format!("d:{id}:{name}");
    let when = |ok: bool, a: A| ok.then_some(a);
    let mut items: Vec<El> = Vec::new();

    // 1. Vary
    items.push(btn_sm(k("vary"), "Vary", Some(&SPARKLES), Variant::Secondary, None, t, when(ready, A::Vary(id.into()))));
    // 2. Compare (N)
    if f.ready_children > 0 {
        let label = format!("Compare ({})", f.ready_children);
        items.push(btn_sm(k("compare"), &label, Some(&COLUMNS_3), Variant::Ghost, None, t, Some(A::Compare(id.into())))
            .help("View this design and its variations side by side"));
    }
    // 3. Explore
    items.push(btn_sm(k("explore"), "Explore", Some(&TELESCOPE), Variant::Ghost, None, t, when(ready, A::Explore(id.into())))
        .help("Explore in a nested space"));
    // 4. Promote / Promoted
    if let Some(parent) = &f.promote_to {
        items.push(match &f.promoted_copy {
            Some(_) => btn_sm(k("promoted"), "Promoted", Some(&ARROW_UP_FROM_LINE), Variant::Ghost, Some(t.primary), t, Some(A::ShowPromoted(id.into())))
                .help(format!("Show the copy in {parent}")),
            None => btn_sm(k("promote"), "Promote", Some(&ARROW_UP_FROM_LINE), Variant::Ghost, None, t, when(ready, A::Promote(id.into())))
                .help(format!("Copy up to {parent}")),
        });
    }
    // 5. Implement: any finished design can be handed to the coding agent (the reference limited it to promoted
    // designs and their copies).
    if ready {
        items.push(implement(id, phase, t));
    }
    // 6. AI edit
    items.extend(ai_edit(d, t));
    // 7. Main
    if f.in_live_view || d.main_at.is_some() {
        let on = d.main_at.is_some();
        items.push(btn_sm(k("main"), "Main", Some(&CROWN), Variant::Ghost, on.then_some(t.primary), t, when(ready, A::ToggleMain(id.into())))
            .help(if on { "Live views show this variant. Click to unset." } else { "Show this variant in live views" }));
    }
    // 8. Guidelines
    if d.guidelines_at.is_some() {
        let mut b = Badge::new(k("guidelines"), "Guidelines", BadgeLook::Outline, t);
        b.glyph = Some(&BOOK_BOOKMARK);
        b.color = t.primary;
        b.border_color = Some(t.primary.alpha(0.4));
        items.push(b.build(t).role("button").label("Guidelines")
            .help(format!("Design guidelines of {}: new designs here follow them. Click to unset.", f.space_name))
            .on_press({
                let a = A::SetGuidelines(id.into(), false);
                move |s: &mut crate::app::state::AppState| super::dispatch(s, a.clone())
            }));
    } else {
        items.push(icon_btn(k("guidelines"), &format!("Set as the design guidelines of {}", f.space_name), &BOOK_BOOKMARK, 28.0, 14.0, t.fg, t,
            when(ready, A::SetGuidelines(id.into(), true))));
    }
    // 9. "from {space}"
    if let Some(src) = &f.source {
        let label = if src.space_id.is_some() { format!("from {}", src.space_name) } else { format!("from {} (deleted)", src.space_name) };
        let mut b = Badge::new(k("from"), &label, BadgeLook::Outline, t);
        b.glyph = Some(&CORNER_LEFT_DOWN);
        b.color = t.muted_fg;
        b.shrinks = true;
        let mut e = b.build(t);
        e = match &src.space_id {
            Some(space_id) => {
                let a = A::JumpToSource { space_id: space_id.clone(), design_id: src.design_id.clone() };
                e.role("button").label(label.clone()).help(format!("Jump into {}", src.space_name))
                    .on_press(move |s: &mut crate::app::state::AppState| super::dispatch(s, a.clone()))
            }
            None => e.help("Source space was deleted").dim(0.5),
        };
        // `ml-auto`: a growing spacer pushes it to the end of its line.
        items.push(spacer());
        items.push(e);
    }
    // 10. Spokes / Merge
    items.push(match f.hub_ready {
        Some(hub_ready) if d.kind == DesignKind::Spoke => {
            btn_sm(k("merge"), "Merge", Some(&MERGE), Variant::Ghost, None, t, when(ready && hub_ready, A::Merge(id.into())))
                .help("Merge this spoke back into its hub")
        }
        _ => btn_sm(k("spokes"), "Spokes", Some(&BOXES), Variant::Ghost, None, t, when(ready, A::Spokes(id.into()))),
    });
    // 11. Poll / In poll
    items.push(if in_poll {
        btn_sm(k("poll"), "In poll", Some(&CHECK), Variant::Secondary, Some(t.emerald), t, Some(A::TogglePoll(id.into())))
            .help("Remove from the stakeholder poll")
    } else {
        btn_sm(k("poll"), "Poll", Some(&VOTE), Variant::Ghost, None, t, when(ready, A::TogglePoll(id.into())))
            .help("Add to the stakeholder poll")
    });

    row().key(k("actions")).w_full().flex_wrap().gap(4.0).padding(8.0).center_y()
        .border_sides([1.0, 0.0, 0.0, 0.0], t.border)
        .children(items)
}

fn implement(id: &str, phase: ImplementPhase, t: &CardTheme) -> El {
    let key = format!("d:{id}:implement");
    let a = Some(A::Implement(id.into()));
    match phase {
        ImplementPhase::Idle => btn_sm(key, "Implement", Some(&CODE_XML), Variant::Brand, None, t, a)
            .help("Hand this design to the coding agent to build it into the product"),
        ImplementPhase::Sent => btn_sm(key, "Gesendet", Some(&SEND), Variant::Ghost, None, t, a).help("An den Agenten gesendet"),
        ImplementPhase::Working => btn_sm(key, "In Arbeit", Some(&LOADER_CIRCLE), Variant::Ghost, None, t, a)
            .help("Der Agent arbeitet daran · Aufgabe im Panel öffnen"),
        // Done: a split button, the chevron offers "Erneut senden …".
        ImplementPhase::Done => row().key(format!("d:{id}:implement-split")).center_y()
            .child(btn_sm(key, "Erledigt", Some(&CHECK), Variant::Ghost, None, t, a).help("Vom Agenten erledigt · Aufgabe im Panel öffnen"))
            .child(icon_btn(format!("d:{id}:implement-more"), "Erneut senden …", &CHEVRON_DOWN, 28.0, 14.0, t.fg, t, Some(A::AskResend(id.into())))),
    }
}

/// "Send again?" under the action row: the reference's confirm dialog, inline on the card.
pub fn resend_confirm(d: &Design, t: &CardTheme) -> El {
    let id = d.id.as_str();
    col().key(format!("d:{id}:resend")).w_full().gap(8.0).padding(10.0).border_sides([1.0, 0.0, 0.0, 0.0], t.border)
        .child(rune::text("Erneut an den Agenten senden?").font(super::theme::XS_MED).color(t.fg))
        .child(rune::text(format!("Der Agent hat „{}“ schon umgesetzt. Ein neuer Auftrag lässt ihn das Design noch einmal einbauen.", d.title))
            .font(super::theme::XS).color(t.muted_fg).wrap())
        .child(row().w_full().gap(6.0).center_y().child(spacer())
            .child(btn_sm(format!("d:{id}:resend-cancel"), "Abbrechen", None, Variant::Outline, None, t, Some(A::CancelResend)))
            .child(btn_sm(format!("d:{id}:resend-go"), "Erneut senden", None, Variant::Brand, None, t, Some(A::Resend(id.into())))))
}

fn ai_edit(d: &Design, t: &CardTheme) -> Vec<El> {
    let id = d.id.as_str();
    let k = |name: &str| format!("d:{id}:{name}");
    if d.editing.is_some() {
        let mut b = Badge::new(k("editing"), "Bearbeite…", BadgeLook::Outline, t);
        b.glyph = Some(&LOADER_CIRCLE);
        b.color = t.primary;
        b.border_color = Some(t.primary.alpha(0.4));
        let cancel = icon_btn(k("edit-cancel"), "Cancel AI edit", &X, 16.0, 12.0, t.muted_fg, t, Some(A::CancelEdit(id.into())));
        return vec![row().key(k("editing-row")).gap(2.0).center_y().child(b.build(t)).child(cancel)];
    }
    let mut out = vec![icon_btn(k("ai-edit"), "Edit with AI (changes this card in place)", &WAND_SPARKLES, 28.0, 14.0, t.fg, t,
        d.is_ready().then(|| A::AiEdit(id.into())))];
    if d.versions.as_ref().is_some_and(|v| !v.is_empty()) {
        out.push(icon_btn(k("undo-edit"), "Undo AI edit", &UNDO_2, 28.0, 14.0, t.fg, t, Some(A::UndoEdit(id.into()))));
    }
    if let Some(err) = &d.edit_error {
        let mut b = Badge::new(k("edit-error"), "Edit failed", BadgeLook::Outline, t);
        b.color = t.destructive;
        b.border_color = Some(t.destructive.alpha(0.4));
        b.font = XS_MED;
        let a = A::DismissEditError(id.into());
        out.push(b.build(t).role("button").label("Edit failed").help(format!("{err} (click to dismiss)"))
            .on_press(move |s: &mut crate::app::state::AppState| super::dispatch(s, a.clone())));
    }
    out
}
