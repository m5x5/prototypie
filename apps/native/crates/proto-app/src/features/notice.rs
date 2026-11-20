//! A short notice at the bottom centre (the reference's sonner toasts): title, optional description, one
//! optional action. Replaced by the next one; leaves after 5 s. Placeholder until the shell has toasts.

use rune::{Color, El, TextStyle, Tx, col, el, row, text};

use crate::app::state::AppState;
use crate::app::style::{self, Palette};

const SHOW_FOR: f64 = 5.0;
const TITLE: TextStyle = TextStyle::new(13.0, 600.0);
const BODY: TextStyle = TextStyle::new(12.5, 400.0);

#[derive(Clone, Debug, PartialEq)]
pub enum NoticeAction {
    /// Enter `space` with `design` selected ("Show").
    Show { space: String, design: String },
    /// Undo a discard: put the design back as it was.
    Restore(Box<proto_core::Design>),
    /// Undo "mark all reviewed".
    Unreview(Vec<String>),
    /// Generic Undo: put these record states back (built with `undo_for` before the write).
    Undo(Vec<proto_core::Change>),
}

/// The current state of `paths` ("designs/<id>", "portals/<id>", …) as the changes that restore it: a set for
/// each live record, a delete for each one that doesn't exist yet. Call it BEFORE writing, then pass the
/// result as `NoticeAction::Undo`.
pub fn undo_for(snap: &proto_core::Snapshot, paths: &[String]) -> Vec<proto_core::Change> {
    paths
        .iter()
        .map(|p| match snap.value(p) {
            Some(v) => proto_core::Change::set(p.clone(), serde_json::Value::clone(v)),
            None => proto_core::Change::delete(p.clone()),
        })
        .collect()
}

impl NoticeAction {
    fn label(&self) -> &'static str {
        match self {
            NoticeAction::Show { .. } => "Show",
            NoticeAction::Restore(_) | NoticeAction::Unreview(_) | NoticeAction::Undo(_) => "Undo",
        }
    }

    fn run(&self, s: &mut AppState) {
        match self {
            NoticeAction::Show { space, design } => crate::features::nav::enter_space(s, space, Some(design)),
            NoticeAction::Restore(d) => {
                if let Ok(c) = proto_core::Change::put(proto_core::RecordKind::Designs, &d.id, d.as_ref()) {
                    crate::features::nav::apply(s, vec![c], "undo discard");
                }
            }
            NoticeAction::Undo(changes) => {
                crate::features::nav::apply(s, changes.clone(), "undo");
            }
            NoticeAction::Unreview(ids) => {
                let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
                crate::features::nav::apply(s, proto_core::ops::set_reviewed(&refs, false), "undo reviewed");
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub title: String,
    pub description: String,
    pub action: Option<NoticeAction>,
}

/// No notice, one waiting for its first poll (to stamp its time), or one showing until `until`.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum NoticeState {
    #[default]
    None,
    New(Notice),
    Showing { notice: Notice, until: f64 },
}

impl NoticeState {
    pub fn current(&self) -> Option<&Notice> {
        match self {
            NoticeState::None => None,
            NoticeState::New(n) | NoticeState::Showing { notice: n, .. } => Some(n),
        }
    }

    pub fn poll(&mut self, now: f64, tx: &mut Tx) -> bool {
        match std::mem::take(self) {
            NoticeState::New(notice) => {
                tx.wake_at(now + SHOW_FOR);
                *self = NoticeState::Showing { notice, until: now + SHOW_FOR };
                false
            }
            NoticeState::Showing { until, .. } if now >= until => true,
            other => {
                *self = other;
                false
            }
        }
    }
}

pub fn notify(s: &mut AppState, title: &str, description: &str) {
    notify_with(s, title, description, None);
}

pub fn notify_with(s: &mut AppState, title: &str, description: &str, action: Option<NoticeAction>) {
    s.features.notice = NoticeState::New(Notice { title: title.into(), description: description.into(), action });
}

pub fn view(s: &AppState, p: &Palette) -> Option<El> {
    let n = s.features.notice.current()?;
    let mut body = col().gap(2.0).min_w(0.0).child(text(n.title.clone()).font(TITLE).color(p.fg));
    if !n.description.is_empty() {
        body = body.child(text(n.description.clone()).font(BODY).color(p.muted_fg).wrap());
    }
    let mut card = row().key("notice").role("status").gap(12.0).px(14.0).py(10.0).radius(12.0).center_y().max_w(460.0).child(body);
    if let Some(action) = &n.action {
        let label = action.label();
        let action = action.clone();
        card = card.child(row().key("notice-action").role("button").label(label).h(26.0).px(10.0).radius(8.0).center_y()
            .fill(p.fg).press(Color::BLACK.alpha(0.15))
            .on_press(move |s: &mut AppState| {
                action.run(s);
                s.features.notice = NoticeState::None;
            })
            .child(text(label).font(TITLE).color(p.canvas_bg)));
    }
    Some(el().key("notice-anchor").pin(Some(0.0), None, Some(0.0), Some(24.0)).items_center().justify_center()
        .child(style::glass(card, p, 0.9)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_restores_a_deleted_design_and_removes_a_created_one() {
        let mut s = AppState::new();
        s.use_store(crate::features::scenes::fixture());
        let paths = vec!["designs/zzv1".to_string(), "designs/zznew".to_string()];
        let undo = undo_for(s.snap(), &paths);
        let mut v = serde_json::to_value(proto_core::Design::clone(&s.snap().design("zzv2").unwrap())).unwrap();
        v["id"] = serde_json::json!("zznew");
        s.apply(vec![proto_core::Change::delete("designs/zzv1"), proto_core::Change::set("designs/zznew", v)], "t").unwrap();
        NoticeAction::Undo(undo).run(&mut s);
        assert!(s.snap().design("zzv1").is_some() && s.snap().design("zznew").is_none());
    }
}
