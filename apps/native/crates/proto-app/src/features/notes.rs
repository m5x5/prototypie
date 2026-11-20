//! Annotate notes pinned on designs (the reference's design notes feature, the parts that don't need the
//! toolbar): the open-notes badge count, the notes list with dismiss, and "Apply notes as a new round". Pinning
//! notes from the app waits for the user's decision on pin mode (spec-features §10.1).

use proto_core::{Change, DesignKind, DesignNote, RecordKind, VariationRequest};
use rune::{El, TextStyle, col, row, text};

use crate::app::state::AppState;
use crate::app::style::Palette;
use crate::features::launch::{self, Launch};
use crate::features::{icons, nav};

const LABEL: TextStyle = TextStyle::new(12.5, 600.0);
const SMALL: TextStyle = TextStyle::new(11.5, 400.0);
const BODY: TextStyle = TextStyle::new(12.5, 400.0);

/// Open (unresolved) notes on a design, oldest first.
pub fn open_notes(s: &AppState, design_id: &str) -> Vec<DesignNote> {
    s.snap().notes(design_id).map(|ns| ns.iter().filter(|n| n.resolved != Some(true)).cloned().collect()).unwrap_or_default()
}

/// Number for the card's notes badge (worker-5).
pub fn open_count(s: &AppState, design_id: &str) -> usize {
    open_notes(s, design_id).len()
}

/// "the annotated element(s) A; B" for `request.focus`.
pub fn notes_focus(notes: &[DesignNote]) -> Option<String> {
    let els: Vec<&str> = notes.iter().map(|n| n.element.as_str()).filter(|e| !e.is_empty()).collect();
    (!els.is_empty()).then(|| format!("the annotated element{} {}", if els.len() > 1 { "s" } else { "" }, els.join("; ")))
}

fn write_notes(s: &mut AppState, design_id: &str, notes: Vec<DesignNote>, reason: &str) {
    let change = if notes.is_empty() {
        Change::delete(RecordKind::Notes.path(design_id))
    } else {
        Change::put(RecordKind::Notes, design_id, &notes).expect("notes serialise")
    };
    nav::apply(s, vec![change], reason);
}

/// × on a note: drop it from the design's list.
pub fn dismiss(s: &mut AppState, design_id: &str, note_id: &str) {
    let Some(all) = s.snap().notes(design_id) else { return };
    let rest: Vec<DesignNote> = all.iter().filter(|n| n.id != note_id).cloned().collect();
    write_notes(s, design_id, rest, "dismiss note");
}

/// "Apply notes as a new round": variations that redesign only the annotated elements; the notes count as
/// resolved once queued.
pub fn apply(s: &mut AppState, design_id: &str) {
    let Some(d) = s.snap().design(design_id).filter(|d| d.is_ready()) else { return };
    let open = open_notes(s, design_id);
    let Some(annotations) = crate::features::review::open_annotations(s.snap(), design_id) else { return };
    let n = open.len();
    let mut request = VariationRequest {
        base_html: Some(d.html.clone()),
        base_title: Some(d.title.clone()),
        instruction: "Redesign ONLY the annotated elements to address their pinned notes. Leave everything else in the design exactly as it is.".into(),
        count: 2,
        focus: notes_focus(&open),
        scope: Some("page".into()),
        space_id: Some(d.space_id.clone()),
        agent: Some(match s.prefs.agent_choice {
            crate::app::prefs::AgentChoice::Claude => "claude".into(),
            crate::app::prefs::AgentChoice::Codex => "codex".into(),
        }),
        ..Default::default()
    };
    request.extra.insert("annotations".into(), annotations);
    launch::launch(s, Launch {
        space_id: d.space_id.clone(),
        parent_id: Some(d.id.clone()),
        kind: if d.kind == DesignKind::Spoke { DesignKind::Spoke } else { DesignKind::Variation },
        origin: None,
        label: format!("Apply {n} note{}: {}", if n > 1 { "s" } else { "" }, d.title),
        request,
        lane: None,
    });
    let resolved: Vec<DesignNote> = s.snap().notes(design_id).map(|ns| ns.iter().cloned().map(|mut x| { x.resolved = Some(true); x }).collect()).unwrap_or_default();
    write_notes(s, design_id, resolved, "notes applied");
}

/// The notes list for the inspector (and fullscreen); None when there are no open notes.
pub fn panel(s: &AppState, p: &Palette, design_id: &str) -> Option<El> {
    let open = open_notes(s, design_id);
    if open.is_empty() {
        return None;
    }
    let mut c = col().key("notes-panel").w_full().gap(8.0).child(text(format!("Notes · {}", open.len())).font(LABEL).color(p.fg));
    for (i, n) in open.iter().enumerate() {
        let (did, nid) = (design_id.to_string(), n.id.clone());
        c = c.child(row().key(format!("note-{}", n.id)).w_full().gap(8.0).padding(8.0).radius(8.0).border(1.0, p.border).items_start()
            .child(row().size(18.0, 18.0).radius(9.0).center().fill(p.fg).child(text((i + 1).to_string()).font(SMALL).color(p.canvas_bg)))
            .child(col().grow(1.0).min_w(0.0).gap(2.0)
                .child(text(n.element.clone()).font(SMALL).color(p.muted_fg).truncate().selectable(true))
                .child(text(n.comment.clone()).font(BODY).color(p.fg).wrap().selectable(true)))
            .child(row().key(format!("note-x-{}", n.id)).role("button").label("Dismiss note").size(20.0, 20.0).center()
                .on_press(move |s: &mut AppState| dismiss(s, &did, &nid)).child(icons::x(11.0, p.muted_fg))));
    }
    let id = design_id.to_string();
    c = c.child(row().key("notes-apply").role("button").label("Apply notes as a new round").w_full().h(30.0).radius(8.0).center()
        .border(1.0, p.border).hover(p.hover).press(p.press)
        .on_press(move |s: &mut AppState| apply(s, &id))
        .child(text("Apply notes as a new round").font(LABEL).color(p.fg)));
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composer::Outgoing;
    use serde_json::json;

    #[test]
    fn apply_sends_annotations_and_resolves() {
        let mut s = AppState::new();
        s.use_store(crate::features::scenes::fixture());
        let notes = json!([{"id": "n1", "designId": "zzv1", "comment": "bigger", "element": "button \"Pay\"", "elementPath": "main > button", "timestamp": 1.0}]);
        s.apply(vec![Change::set("notes/zzv1", notes)], "test").unwrap();
        assert_eq!(open_count(&s, "zzv1"), 1);
        apply(&mut s, "zzv1");
        let [Outgoing::Generate { request, .. }] = s.composer.outbox.as_slice() else { panic!("one generate") };
        assert_eq!(request.focus.as_deref(), Some("the annotated element button \"Pay\""));
        assert_eq!(request.extra["annotations"][0]["element"], "button \"Pay\" (at `main > button`)");
        assert_eq!(open_count(&s, "zzv1"), 0);
    }
}
