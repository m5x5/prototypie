//! The designer's own note on a design (spec-cards §4.2.1, the reference's `DesignNotes`): free
//! text, saved into `design.notes` 600 ms after the last keystroke, right away when stepping to
//! another design or closing. A change from elsewhere replaces the text only while nothing is
//! waiting to be saved. Blank removes the note.

use proto_core::{Change, Snapshot};
use rune::{El, TextEdit, Tx, col, icon, row, text, text_input};

use crate::app::state::AppState;
use crate::cards::CardTheme;
use crate::cards::icons::{CHECK, LOADER_CIRCLE};
use crate::cards::theme::{SM, XS_MED};

const SAVE_DELAY_S: f64 = 0.6;

/// Whether typed text waits to be saved.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Clean,
    /// Typed; saves at `due` (poll clock, s).
    Dirty { due: f64 },
}

#[derive(Default)]
pub struct NoteBox {
    /// The design the box holds the note of.
    design: Option<String>,
    pub edit: TextEdit,
    /// The text as last seen in the box (to notice keystrokes in `poll`).
    seen: String,
    phase: Option<Phase>,
}

fn stored(snap: &Snapshot, id: &str) -> String {
    snap.design(id).and_then(|d| d.notes.clone()).unwrap_or_default()
}

impl NoteBox {
    fn dirty(&self) -> bool {
        matches!(self.phase, Some(Phase::Dirty { .. }))
    }

    /// Takes the pending text to save, if any (`(design id, text)`), and marks the box clean.
    fn take_pending(&mut self) -> Option<(String, String)> {
        if !self.dirty() {
            return None;
        }
        self.phase = Some(Phase::Clean);
        self.design.clone().map(|id| (id, self.edit.text.clone()))
    }
}

/// Writes `text` as `id`'s note (raw patch: other fields stay exactly as stored).
fn save(s: &AppState, id: &str, text: &str) {
    let path = format!("designs/{id}");
    let Some(mut v) = s.snap().value(&path).map(|v| (**v).clone()) else { return };
    let Some(o) = v.as_object_mut() else { return };
    if text.trim().is_empty() {
        o.remove("notes");
    } else {
        o.insert("notes".into(), serde_json::Value::String(text.to_string()));
    }
    if let Err(e) = s.store().apply(vec![Change::set(path, v)], proto_core::ChangeMeta::client("proto-app").reason("design note")) {
        eprintln!("fullscreen: saving the note failed: {e}");
    }
}

/// Saves what's waiting now (stepping away, closing).
pub fn flush(s: &mut AppState) {
    if let Some((id, text)) = s.fullscreen.note.take_pending() {
        save(s, &id, &text);
    }
}

/// Follows the open design and the typed text; saves when due. True when the box changed visibly.
pub fn poll(s: &mut AppState, open: Option<&str>, clock: f64, tx: &mut Tx) -> bool {
    let snap = s.store().snapshot();
    let note = &mut s.fullscreen.note;
    // Another design (or closed): save the old one's pending text, load the new one's.
    if note.design.as_deref() != open {
        let pending = note.take_pending();
        note.design = open.map(str::to_string);
        let text = open.map_or_else(String::new, |id| stored(&snap, id));
        note.edit = TextEdit::default();
        note.edit.text = text.clone();
        note.seen = text;
        note.phase = Some(Phase::Clean);
        if let Some((id, text)) = pending {
            save(s, &id, &text);
        }
        return true;
    }
    let Some(id) = note.design.clone() else { return false };
    if note.edit.text != note.seen {
        // A keystroke: (re)start the delay.
        note.seen = note.edit.text.clone();
        let was_dirty = note.dirty();
        note.phase = Some(Phase::Dirty { due: clock + SAVE_DELAY_S });
        tx.wake_after(std::time::Duration::from_secs_f64(SAVE_DELAY_S));
        return !was_dirty;
    }
    match note.phase {
        Some(Phase::Dirty { due }) if clock >= due => {
            flush(s);
            true
        }
        Some(Phase::Dirty { due }) => {
            tx.wake_after(std::time::Duration::from_secs_f64((due - clock).max(0.01)));
            false
        }
        _ => {
            // Clean: follow changes from elsewhere.
            let remote = stored(&snap, &id);
            if remote != note.edit.text {
                note.edit.text = remote.clone();
                note.seen = remote;
                return true;
            }
            false
        }
    }
}

/// The box: "Notiz" with its save status, and the text field.
pub fn view(s: &AppState, d: &proto_core::Design, t: &CardTheme) -> El {
    let note = &s.fullscreen.note;
    let saving = note.dirty() && note.design.as_deref() == Some(d.id.as_str());
    let has_note = d.notes.as_ref().is_some_and(|n| !n.trim().is_empty());
    let status = if saving {
        Some(row().gap(4.0).center_y().child(icon(&LOADER_CIRCLE, 12.0).color(t.muted_fg)).child(text("Speichert…").font(crate::cards::theme::T11).color(t.muted_fg)))
    } else if has_note {
        Some(row().gap(4.0).center_y().child(icon(&CHECK, 12.0).color(t.muted_fg)).child(text("Gespeichert").font(crate::cards::theme::T11).color(t.muted_fg)))
    } else {
        None
    };
    let input = text_input(&note.edit, SM).color(t.fg).key(format!("fs:note:{}", d.id)).label("Notiz")
        .placeholder("Gedanken zu diesem Design…", t.muted_fg).w_full().min_h(96.0 - 16.0).max_h(288.0)
        .on_edit(|s: &mut AppState| &mut s.fullscreen.note.edit);
    col().key(format!("fs:note-box:{}", d.id)).w_full().gap(6.0)
        .child(row().w_full().center_y().child(text("Notiz").font(XS_MED).color(t.fg)).child(rune::spacer()).children(status))
        .child(rune::el().w_full().padding(8.0).radius(8.0).border(1.0, t.input).child(input))
}
