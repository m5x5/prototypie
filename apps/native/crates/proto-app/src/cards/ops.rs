//! Card actions that are plain record writes or navigation (the reference store's `updateDesign`,
//! `removeDesign`, `enterSpace`), plus the nested-space actions features owns (explore, promote, guidelines). Everything with more behaviour is queued for its
//! owner module instead (see `CardAction`).

use proto_core::{Change, ChangeMeta, RecordKind, ops};

use super::CardAction;
use crate::app::state::AppState;
use crate::features::nested;

fn meta(reason: &str) -> ChangeMeta {
    ChangeMeta::client("proto-app").reason(reason)
}

fn write(s: &AppState, changes: Vec<Change>, reason: &str) {
    if changes.is_empty() {
        return;
    }
    if let Err(e) = s.store().apply(changes, meta(reason)) {
        eprintln!("cards: {reason} failed: {e}");
    }
}

/// Handles `a` if it's one of these; returns it back otherwise.
pub fn apply(s: &mut AppState, a: CardAction) -> Option<CardAction> {
    match a {
        CardAction::ToggleStar(id) => {
            let on = !s.snap().design(&id).is_some_and(|d| d.is_starred());
            write(s, ops::set_starred(s.snap(), &id, on).into_iter().collect(), "star");
        }
        CardAction::Delete(id) => {
            let path = RecordKind::Designs.path(&id);
            let title = s.snap().design(&id).map(|d| d.title.clone()).unwrap_or_default();
            // The undo is captured before the write: it restores the record exactly.
            let undo = crate::features::undo_for(s.snap(), &[path.clone()]);
            write(s, vec![Change::delete(path)], "delete design");
            s.route.selected.retain(|x| *x != id);
            crate::features::notify_with(s, &format!("Deleted “{title}”"), "", Some(crate::features::NoticeAction::Undo(undo)));
        }
        CardAction::AskResend(id) => s.cards.resend_confirm = Some(id),
        CardAction::CancelResend => s.cards.resend_confirm = None,
        CardAction::Resend(id) => {
            s.cards.resend_confirm = None;
            crate::features::agent_tasks::implement::send(s, &id, true);
        }
        CardAction::ToggleMain(id) => {
            let on = !s.snap().design(&id).is_some_and(|d| d.main_at.is_some());
            write(s, ops::set_main(s.snap(), &id, on).into_iter().collect(), "main variant");
        }
        // Nested-space behaviour is features' (worker-8): seeded explorations, promotion notices.
        CardAction::SetGuidelines(id, on) => nested::set_guidelines(s, &id, on),
        CardAction::Explore(id) => nested::explore(s, &id),
        CardAction::Promote(id) => {
            nested::promote(s, &[id]);
        }
        CardAction::ShowPromoted(id) => nested::show_promoted_copy(s, &id),
        CardAction::EnterSpace(space) => s.enter_space(&space, None),
        CardAction::JumpToSource { space_id, design_id } => s.enter_space(&space_id, Some(design_id)),
        other => return Some(other),
    }
    None
}
