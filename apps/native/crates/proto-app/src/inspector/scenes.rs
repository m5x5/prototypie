//! `--snap` scenes for the inspector and composer, on a small in-memory store: a project with an
//! exploration holding a ready design (prompt, long rationale, two variations, one starred), a
//! promoted copy in the project, and a second exploration.

use proto_core::{Change, ChangeMeta, Store};
use serde_json::{Value, json};

use crate::app::state::AppState;
use crate::composer::{Menu, Target};

const PROJECT: &str = "zzproj";
const EXPLORATION: &str = "zzexpl";
const DESIGN: &str = "zzhero";

fn space(id: &str, name: &str, parent: &str, at: f64, extra: Value) -> Change {
    let mut v = json!({ "id": id, "name": name, "parentSpaceId": parent, "createdAt": at });
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    Change::set(format!("spaces/{id}"), v)
}

fn design(id: &str, space: &str, title: &str, parent: Option<&str>, extra: Value) -> Change {
    let mut v = json!({
        "id": id, "spaceId": space, "kind": if parent.is_some() { "variation" } else { "page" },
        "title": title, "rationale": "", "html": "<!doctype html><p>x</p>", "status": "ready",
        "position": { "x": 0, "y": 0 }, "createdAt": 1.0,
    });
    if let Some(p) = parent {
        v["parentId"] = json!(p);
    }
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    Change::set(format!("designs/{id}"), v)
}

fn fixture() -> Store {
    let store = Store::in_memory();
    let changes = vec![
        space(PROJECT, "Prototypie", "root", 1.0, json!({ "kind": "project", "localPath": "/Users/me/code/prototypie" })),
        space(EXPLORATION, "Header navbar", PROJECT, 2.0, json!({})),
        space("zzexpl2", "Inspector glass", PROJECT, 3.0, json!({})),
        space("zznest", "Pill variants", EXPLORATION, 4.0, json!({})),
        design(DESIGN, EXPLORATION, "Floating pill toolbar", None, json!({
            "instruction": "create multiple alternative designs for prototypie",
            "rationale": "Splits the header into two floating pills over the canvas: identity and location on the left, canvas actions on the right, so the full canvas stays visible and the controls read as tools rather than chrome.",
            "starred": true,
        })),
        design("zzv1", EXPLORATION, "Side dock", Some(DESIGN), json!({})),
        design("zzv2", EXPLORATION, "Command bar", Some(DESIGN), json!({ "starred": true })),
        design("zzcopy", PROJECT, "Floating pill toolbar", None, json!({})),
        Change::set("promotedFrom/zzcopy", json!({ "spaceId": EXPLORATION, "designId": DESIGN, "spaceName": "Header navbar", "at": 1.0 })),
    ];
    store.apply(changes, ChangeMeta::client("scenes")).expect("fixture applies");
    store
}

fn in_exploration(s: &mut AppState, select: bool) {
    s.use_store(fixture());
    s.enter_space(EXPLORATION, select.then(|| DESIGN.to_string()));
    crate::composer::poll(s, 0.0, &mut rune::Tx::default());
}

/// `(name, setup)` pairs for `AppSpec::scenes`.
pub const SCENES: &[(&str, fn(&mut AppState))] = &[
    ("inspector-root", |s| {
        s.use_store(fixture());
        s.enter_space("root", None);
    }),
    ("inspector-exploration", |s| in_exploration(s, false)),
    ("inspector-design", |s| {
        in_exploration(s, true);
        s.inspector.details.rationale_open = Some(DESIGN.into());
    }),
    ("composer-kids", |s| {
        in_exploration(s, true);
        s.composer.target = Target::Kids;
        crate::composer::set_text(s, "use Lucide icons everywhere");
    }),
    ("composer-agent-menu", |s| {
        in_exploration(s, true);
        s.composer.menu = Menu::Agent;
    }),
    ("composer-history", |s| {
        in_exploration(s, false);
        s.prefs.remember_prompt("a landing page for a climbing gym");
        s.prefs.remember_prompt("bolder hierarchy, try a dark theme");
        s.composer.menu = Menu::History;
    }),
    ("more-menu", |s| {
        in_exploration(s, false);
        s.shell.more_open = true;
    }),
    ("inspector-made-from", |s| {
        in_exploration(s, true);
        s.inspector.details.made_from_open = Some(DESIGN.into());
    }),
];

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    fn render(name: &str) -> Runtime<AppState> {
        let setup = SCENES.iter().find(|(n, _)| *n == name).expect("scene").1;
        let mut s = AppState::new();
        setup(&mut s);
        let mut rt = Runtime::new(s, crate::shell::view, (1280.0, 800.0), Env::new(true, 2.0));
        rt.frame(0.0);
        rt
    }

    #[test]
    fn root_shows_the_note_and_projects() {
        let rt = render("inspector-root");
        assert!(rt.find("insp-root-note").is_some());
        assert!(rt.find("composer").is_none());
        assert!(rt.find(&format!("insp-space-{PROJECT}")).is_some());
    }

    #[test]
    fn exploration_shows_stats_composer_and_nested() {
        let rt = render("inspector-exploration");
        assert!(rt.find("insp-exploration").is_some());
        assert!(rt.find("composer-count").is_some(), "new-design mode shows the count");
        assert!(rt.find("composer-tabs").is_none());
        assert!(rt.find("insp-space-zznest").is_some());
    }

    #[test]
    fn starred_list_selects_and_centres_the_card() {
        let mut rt = render("inspector-exploration");
        assert!(rt.find("insp-expl-stats").is_none(), "no stat tiles");
        assert!(rt.find("insp-starred-zzv1").is_none(), "only starred designs");
        let (x, y) = rt.find("insp-starred-zzv2").expect("starred row");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.route.selected, vec!["zzv2".to_string()]);
        let (_, r) = rt.state.canvas.nodes.rects["zzv2"];
        let cam = &rt.state.canvas.camera;
        let mid = [cam.pan[0] + (r[0] + r[2] / 2.0) * cam.zoom, cam.pan[1] + (r[1] + r[3] / 2.0) * cam.zoom];
        let [l, t, rr, b] = crate::app::insets::canvas_insets(true);
        let size = rt.state.canvas.size;
        assert!((mid[0] - (l + (size[0] - l - rr) / 2.0)).abs() < 1.0 && (mid[1] - (t + (size[1] - t - b) / 2.0)).abs() < 1.0);
    }

    #[test]
    fn exploration_without_stars_hides_the_section() {
        let mut s = AppState::new();
        let store = fixture();
        let snap = store.snapshot();
        let unstar: Vec<_> = snap.designs_in(EXPLORATION).filter(|d| d.starred == Some(true))
            .filter_map(|d| proto_core::ops::set_starred(&snap, &d.id, false)).collect();
        drop(snap);
        store.apply(unstar, proto_core::ChangeMeta::client("test")).unwrap();
        s.use_store(store);
        s.enter_space(EXPLORATION, None);
        let mut rt = Runtime::new(s, crate::shell::view, (1280.0, 800.0), Env::new(true, 2.0));
        rt.frame(0.0);
        assert!(rt.find("insp-exploration").is_none());
        assert!(rt.find("insp-promote-starred").is_none());
    }

    #[test]
    fn a_selected_design_shows_details_tabs_and_promoted_state() {
        let rt = render("inspector-design");
        assert!(rt.find("insp-title").is_some());
        assert!(rt.find("insp-made-from").is_some());
        assert!(rt.find("insp-rationale-toggle").is_some());
        assert!(rt.find("composer-tabs").is_some());
        assert!(rt.find("insp-promoted").is_some(), "the design has a promoted copy");
    }

    #[test]
    fn menus_and_popovers_open() {
        assert!(render("composer-agent-menu").find("composer-agent-menu").is_some());
        assert!(render("composer-history").find("composer-history-menu").is_some());
        assert!(render("inspector-made-from").find("insp-made-from-pop").is_some());
        assert!(render("more-menu").find("more-decisions").is_some());
    }

    #[test]
    fn kids_mode_sends_one_revision_per_variation() {
        let mut rt = render("composer-kids");
        assert!(rt.find("composer-count").is_none());
        let (x, y) = rt.find("composer-send").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.composer.outbox.len(), 2);
        assert!(rt.state.composer.edit.text.is_empty());
        assert_eq!(rt.state.prefs.prompt_history[0], "use Lucide icons everywhere");
    }

    #[test]
    fn promote_starred_copies_up_to_the_project() {
        let mut rt = render("inspector-exploration");
        let before = rt.state.store().snapshot().designs_in(PROJECT).count();
        let (x, y) = rt.find("insp-promote-starred").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        // zzhero is already promoted; zzv2 (starred, ready) is the one left.
        assert_eq!(rt.state.store().snapshot().designs_in(PROJECT).count(), before + 1);
    }

    #[test]
    fn outside_edits_keep_the_selection_draft() {
        let mut s = AppState::new();
        s.use_store(fixture());
        s.enter_space(EXPLORATION, Some(DESIGN.to_string()));
        // An edit from outside (a mention pick) before the composer's poll noticed the selection.
        crate::composer::edit_mut(&mut s).text = "reuse #".into();
        crate::composer::poll(&mut s, 0.0, &mut rune::Tx::default());
        assert_eq!(s.composer.edit.text, "reuse #");
        assert_eq!(s.prefs.draft(DESIGN), "reuse #");
    }

    /// Enter sends, Shift+Enter is a newline (in process: the outbox is only queued, never run).
    #[test]
    fn enter_sends_and_shift_enter_breaks_the_line() {
        use rune::{Key, KeyEvent, Mods};
        let mut rt = render("inspector-exploration");
        let (x, y) = rt.find("composer").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        let key = |key: Key, shift: bool| KeyEvent { key, mods: Mods { shift, ..Mods::default() } };
        rt.key(key(Key::Text("one".into()), false));
        rt.key(key(Key::Enter, true));
        rt.key(key(Key::Text("two".into()), false));
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.composer.edit.text, "one\ntwo");
        assert!(rt.state.composer.outbox.is_empty());
        rt.key(key(Key::Enter, false));
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.composer.outbox.len(), 1);
        assert!(rt.state.composer.edit.text.is_empty());
    }
}
