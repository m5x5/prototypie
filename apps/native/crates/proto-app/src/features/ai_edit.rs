//! In-place AI edit, the feature side (the reference's `features/ai-edit`): the card's undo / cancel / dismiss
//! actions and the fullscreen side panel's "Änderung anfragen" box. The edit itself runs through the runner
//! (`Outgoing::Edit`), which writes the result into the card and keeps the previous content in `versions`.

use std::collections::HashMap;

use proto_core::{Change, Design, RecordKind, VariationRequest};
use rune::color::hex;
use rune::{Color, El, TextEdit, TextStyle, col, row, text, text_input};
use serde_json::json;

use crate::app::state::AppState;
use crate::app::style::Palette;
use crate::composer::Outgoing;
use crate::features::nav;

const LABEL: TextStyle = TextStyle::new(12.5, 600.0);
const BODY: TextStyle = TextStyle::new(13.0, 400.0);
const SMALL: TextStyle = TextStyle::new(11.5, 400.0);

#[derive(Default)]
pub struct AiEditState {
    /// The box's text per design id.
    pub drafts: HashMap<String, TextEdit>,
    /// Designs whose edit was sent from the box (for its "done" line).
    pub sent: HashMap<String, bool>,
}

/// Start an edit of `design_id`: its open Annotate notes become the focus, like "Apply notes".
pub fn start(s: &mut AppState, design_id: &str, instruction: &str) {
    let Some(d) = s.snap().design(design_id).filter(|d| d.is_ready() && d.editing.is_none()) else { return };
    let mut request = VariationRequest {
        instruction: instruction.to_string(),
        count: 1,
        space_id: Some(d.space_id.clone()),
        agent: Some(match s.prefs.agent_choice {
            crate::app::prefs::AgentChoice::Claude => "claude".into(),
            crate::app::prefs::AgentChoice::Codex => "codex".into(),
        }),
        ..Default::default()
    };
    if let Some(a) = crate::features::review::open_annotations(s.snap(), design_id) {
        request.focus = crate::features::notes::notes_focus(&crate::features::notes::open_notes(s, design_id));
        request.extra.insert("annotations".into(), a);
    }
    s.prefs_mut().remember_prompt(instruction);
    s.composer.outbox.push(Outgoing::Edit { design_id: design_id.to_string(), request });
}

/// Restore the content the last AI edit replaced.
pub fn undo(s: &mut AppState, design_id: &str) {
    let Some(d) = s.snap().design(design_id).filter(|d| d.editing.is_none()) else { return };
    let Some(prev) = d.versions.as_ref().and_then(|v| v.last()).cloned() else { return };
    let mut v = serde_json::to_value(Design::clone(&d)).expect("design json");
    let prev_v = serde_json::to_value(&prev).expect("version json");
    for k in ["html", "title", "rationale", "viewport", "viewportHeight"] {
        match prev_v.get(k) {
            Some(x) => v[k] = x.clone(),
            None => {
                if let Some(o) = v.as_object_mut() {
                    o.remove(k);
                }
            }
        }
    }
    let mut versions = d.versions.clone().unwrap_or_default();
    versions.pop();
    v["versions"] = json!(versions);
    if let Some(o) = v.as_object_mut() {
        o.remove("editError");
    }
    nav::apply(s, vec![Change::set(RecordKind::Designs.path(design_id), v)], "undo ai edit");
}

pub fn cancel(s: &mut AppState, design_id: &str) {
    crate::runner::cancel_edit(s, design_id);
}

pub fn dismiss_error(s: &mut AppState, design_id: &str) {
    let Some(d) = s.snap().design(design_id) else { return };
    let mut d = Design::clone(&d);
    d.edit_error = None;
    if let Ok(c) = Change::put(RecordKind::Designs, design_id, &d) {
        nav::apply(s, vec![c], "dismiss edit error");
    }
}

fn send_box(s: &mut AppState, id: &str) {
    let text = s.features.ai_edit.drafts.get(id).map(|e| e.text.trim().to_string()).unwrap_or_default();
    if text.is_empty() {
        return;
    }
    start(s, id, &text);
    s.features.ai_edit.drafts.remove(id);
    s.features.ai_edit.sent.insert(id.to_string(), true);
}

/// "Änderung anfragen" for the fullscreen side panel (worker-5 places it): request an in-place edit of the
/// design on display; shows running / failed / done and "Rückgängig" for the last edit.
pub fn request_box(s: &AppState, p: &Palette, d: &Design) -> El {
    let running = d.editing.is_some();
    let empty = TextEdit::default();
    let edit = s.features.ai_edit.drafts.get(&d.id).unwrap_or(&empty);
    let (bind, key_id, send_id) = (d.id.clone(), d.id.clone(), d.id.clone());
    let input = text_input(edit, BODY).color(p.fg).key(format!("edit-req-{}", d.id)).label("Änderung anfragen")
        .placeholder("z. B. Uhr größer, Softkeys höher setzen", p.muted_fg).w_full().min_h(64.0)
        .on_edit(move |s: &mut AppState| s.features.ai_edit.drafts.entry(bind.clone()).or_default())
        .on_input_key(move |s: &mut AppState, ev, _| {
            if ev.key == rune::Key::Enter && !ev.mods.shift {
                send_box(s, &key_id);
                return true;
            }
            false
        });
    let can = !running && d.is_ready() && !edit.text.trim().is_empty();
    // The send button sits inside the box, bottom right (the reference's icon button).
    let send = row().key(format!("edit-send-{}", d.id)).role("button").label("Änderung senden").disabled(!can).size(24.0, 24.0).radius(6.0).center()
        .fill(p.hover).opacity(if can { 1.0 } else { 0.5 }).child(crate::features::icons::send(13.0, p.fg));
    let send = if can { send.press(Color::BLACK.alpha(0.15)).on_press(move |s: &mut AppState| send_box(s, &send_id)) } else { send };
    // Pointing at things on the screen: the Annotate picker, whose notes pinned on this design go along with the edit.
    let picking = s.features.annotate.on;
    let pinned = crate::features::open_notes_count(s, &d.id);
    let point = row().key(format!("edit-point-{}", d.id)).role("button").label("Auf Elemente zeigen").expanded(picking).h(28.0).px(10.0).gap(6.0).radius(14.0).center_y()
        .border(1.0, if picking { p.fg } else { p.border }).fill(if picking { p.hover } else { Color::TRANSPARENT })
        .hover(p.hover).press(p.press)
        .help("Elemente im Design anklicken und kommentieren: die Notizen gehen mit der Änderung an den Agenten")
        .on_press(|s: &mut AppState| crate::features::annotate::toggle(s))
        .child(crate::features::icons::locate(14.0, p.fg))
        .child(text(if picking { "Zeigen beenden" } else { "Auf Elemente zeigen" }).font(SMALL).color(p.fg));
    let mut c = col().key(format!("edit-box-{}", d.id)).w_full().gap(8.0)
        .child(text("Änderung anfragen").font(LABEL).color(p.fg))
        .child(row().w_full().gap(8.0).center_y().child(point)
            .children((pinned > 0).then(|| text(format!("{pinned} markiert")).font(SMALL).color(p.muted_fg))))
        .child(col().w_full().padding(8.0).gap(4.0).radius(8.0).border(1.0, p.border).opacity(if running { 0.6 } else { 1.0 })
            .child(input)
            .child(row().w_full().justify_end().child(send)));
    if let Some(e) = d.editing.as_ref() {
        let instr = e.get("instruction").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let id = d.id.clone();
        c = c.child(row().w_full().gap(6.0).center_y()
            .child(text(format!("Bearbeite… {instr}")).font(SMALL).color(p.muted_fg).truncate().grow(1.0).min_w(0.0))
            .child(row().key(format!("edit-cancel-{}", d.id)).role("button").label("Abbrechen").h(22.0).px(8.0).radius(5.0).center().border(1.0, p.border)
                .on_press(move |s: &mut AppState| cancel(s, &id)).child(text("Abbrechen").font(SMALL).color(p.fg))));
    } else if let Some(err) = &d.edit_error {
        let id = d.id.clone();
        c = c.child(row().w_full().gap(6.0).center_y()
            .child(text(err.clone()).font(SMALL).color(hex("#ef4444").expect("hex")).wrap().grow(1.0).min_w(0.0))
            .child(row().key(format!("edit-err-x-{}", d.id)).role("button").label("Dismiss").size(20.0, 20.0).center()
                .on_press(move |s: &mut AppState| dismiss_error(s, &id)).child(crate::features::icons::x(10.0, p.muted_fg))));
    } else if s.features.ai_edit.sent.contains_key(&d.id) {
        c = c.child(text("Fertig: die Änderung ist eingebaut.").font(SMALL).color(hex("#10b981").expect("hex")));
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_restores_the_last_version() {
        let mut s = AppState::new();
        s.use_store(crate::features::scenes::fixture());
        let mut d = Design::clone(&s.snap().design("zzv1").unwrap());
        let mut v = serde_json::to_value(&d).unwrap();
        v["html"] = json!("<p>new</p>");
        v["editError"] = json!("x");
        v["versions"] = json!([{"html": d.html, "title": "Old title", "rationale": "old", "replacedAt": 1, "instruction": "bigger"}]);
        s.apply(vec![Change::set("designs/zzv1", v)], "t").unwrap();
        undo(&mut s, "zzv1");
        d = Design::clone(&s.snap().design("zzv1").unwrap());
        assert_eq!(d.title, "Old title");
        assert!(d.html.contains("Hero"));
        assert!(d.versions.unwrap_or_default().is_empty() && d.edit_error.is_none());
    }

    #[test]
    fn start_queues_an_edit() {
        let mut s = AppState::new();
        s.use_store(crate::features::scenes::fixture());
        start(&mut s, "zzv1", "bigger clock");
        assert!(matches!(s.composer.outbox.as_slice(), [Outgoing::Edit { design_id, request }] if design_id == "zzv1" && request.instruction == "bigger clock"));
    }
}
