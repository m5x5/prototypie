//! An in-memory store with one of every card state, for `--snap` scenes and tests. Never touches
//! disk. Spec states covered: docs/spec-cards.md §2.3 (ready desktop/phone/portrait, generating,
//! error), §2.5 rows (Compare, Promote/Promoted, Implement, Main, Guidelines, from-badge, Merge),
//! §3 (project, exploration, empty, designing).

use proto_core::{Change, ChangeMeta, Store};
use serde_json::{Value, json};

use crate::app::state::AppState;

pub const PROJECT: &str = "fx-project";
pub const EXPLORATION: &str = "fx-explore";
pub const SUB: &str = "fx-sub";

const HTML: &str = "<!doctype html><html><body style=\"margin:0;font:16px system-ui;background:#111;color:#eee\"><header style=\"padding:16px\">Header</header></body></html>";

fn space(id: &str, name: &str, parent: &str, extra: Value) -> Change {
    let mut v = json!({ "id": id, "name": name, "parentSpaceId": parent, "createdAt": 1.0e12 });
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    Change::set(format!("spaces/{id}"), v)
}

fn design(id: &str, space: &str, title: &str, x: f64, y: f64, extra: Value) -> Change {
    let mut v = json!({
        "id": id, "spaceId": space, "kind": "variation", "title": title,
        "rationale": "A compact header that keeps the space path first.",
        "html": HTML, "status": "ready", "position": { "x": x, "y": y }, "createdAt": 1.7e12,
    });
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    Change::set(format!("designs/{id}"), v)
}

/// The fixture store.
pub fn store() -> Store {
    let store = Store::in_memory();
    let now = super::now_ms();
    let changes = vec![
        space(PROJECT, "Prototypie", "root", json!({ "kind": "project", "localPath": "/Users/me/code/prototypie" })),
        space("fx-empty", "Empty one", "root", json!({ "kind": "project" })),
        space(EXPLORATION, "Header navbar", PROJECT, json!({})),
        space(SUB, "Pills", EXPLORATION, json!({})),
        // No portals/<id> record: must land right of everything, not at the origin.
        space("fx-noportal", "Pill variants", EXPLORATION, json!({})),
        space("fx-designing", "Composer", PROJECT, json!({})),
        Change::set(format!("portals/{PROJECT}"), json!({ "x": 0, "y": 0 })),
        Change::set("portals/fx-empty", json!({ "x": 380, "y": 0 })),
        Change::set(format!("portals/{EXPLORATION}"), json!({ "x": 0, "y": 0 })),
        Change::set("portals/fx-designing", json!({ "x": 380, "y": 0 })),
        Change::set(format!("portals/{SUB}"), json!({ "x": 1840, "y": 0 })),
        // Exploration designs: a base, its variations, every status.
        design("fx-base", EXPLORATION, "Floating pill toolbar", 0.0, 0.0, json!({ "kind": "page", "starred": true, "notes": "Try a darker rail" })),
        design("fx-v1", EXPLORATION, "Breadcrumb rail with a very long title that must truncate to the card width", 460.0, 0.0, json!({ "parentId": "fx-base", "mainAt": now })),
        design("fx-phone", EXPLORATION, "Phone header", 920.0, 0.0, json!({ "parentId": "fx-base", "viewport": 390 })),
        design("fx-portrait", EXPLORATION, "Fixed screen", 1380.0, 0.0, json!({ "viewport": 640, "viewportHeight": 960, "guidelinesAt": now })),
        design("fx-gen", EXPLORATION, "Command bar", 0.0, 620.0, json!({ "status": "generating", "html": "" })),
        design("fx-err", EXPLORATION, "Side dock", 460.0, 620.0, json!({ "status": "error", "error": "The agent stopped before finishing." })),
        design("fx-spoke", EXPLORATION, "Logo spoke", 920.0, 620.0, json!({ "kind": "spoke", "parentId": "fx-base" })),
        // AI edit running; AI edit failed with an undo version.
        design("fx-editing", EXPLORATION, "Editing now", 0.0, 1240.0, json!({ "editing": { "instruction": "bigger clock", "jobId": "j1" } })),
        design("fx-editerr", EXPLORATION, "Edit failed", 460.0, 1240.0, json!({
            "editError": "The agent gave up.",
            "versions": [{ "html": HTML, "title": "Before", "instruction": "softer" }]
        })),
        // Promotions go to the parent space: Pills → Header navbar, Header navbar → Prototypie.
        design("fx-src", SUB, "Pill switcher", 0.0, 0.0, json!({})),
        design("fx-copy", EXPLORATION, "Pill switcher", 1380.0, 620.0, json!({})),
        design("fx-pcopy", PROJECT, "Floating pill toolbar", 760.0, 0.0, json!({ "starred": true })),
        Change::set("promotedFrom/fx-pcopy", json!({ "spaceId": EXPLORATION, "designId": "fx-base", "spaceName": "Header navbar", "at": now })),
        Change::set("promotedFrom/fx-copy", json!({ "spaceId": SUB, "designId": "fx-src", "spaceName": "Pills", "at": now })),
        design("fx-gen2", "fx-designing", "Composer v2", 0.0, 0.0, json!({ "status": "generating", "html": "" })),
        // Fresh designs in the sub-space: its card shows "N new" (never left on this device).
        design("fx-new1", SUB, "Pill new A", 460.0, 0.0, json!({ "createdAt": now })),
        design("fx-new2", SUB, "Pill new B", 920.0, 0.0, json!({ "createdAt": now })),
        Change::set("notes/fx-v1", json!([{ "id": "n1", "designId": "fx-v1", "comment": "Tighter", "element": "header", "elementPath": "header", "timestamp": now }])),
    ];
    store.apply(changes, ChangeMeta::client("fixture")).expect("fixture applies");
    store
}

fn scene_space(s: &mut AppState, space: &str, selected: &[&str]) {
    s.use_store(store());
    if space != crate::app::state::ROOT_SPACE {
        s.enter_space(space, None);
    }
    s.route.selected = selected.iter().map(|x| x.to_string()).collect();
}

/// `--snap` scenes for cards and fullscreen; the shell lists them in `AppSpec::scenes`.
pub const SCENES: &[(&str, fn(&mut AppState))] = &[
    ("cards", |s| scene_space(s, EXPLORATION, &["fx-v1"])),
    ("cards-light", |s| {
        scene_space(s, EXPLORATION, &["fx-v1"]);
        s.theme = crate::app::state::ThemePref::Light;
    }),
    ("projects", |s| scene_space(s, crate::app::state::ROOT_SPACE, &[PROJECT])),
    ("projects-light", |s| {
        scene_space(s, crate::app::state::ROOT_SPACE, &[PROJECT]);
        s.theme = crate::app::state::ThemePref::Light;
    }),
    ("explorations-light", |s| {
        scene_space(s, PROJECT, &[]);
        s.theme = crate::app::state::ThemePref::Light;
    }),
    ("fullscreen-light", |s| {
        scene_space(s, EXPLORATION, &[]);
        s.theme = crate::app::state::ThemePref::Light;
        crate::fullscreen::open(s, "fx-base", &[]);
    }),
    ("explorations", |s| scene_space(s, PROJECT, &[])),
    ("fullscreen", |s| {
        scene_space(s, EXPLORATION, &[]);
        crate::fullscreen::open(s, "fx-base", &[]);
    }),
    ("fullscreen-set", |s| {
        scene_space(s, EXPLORATION, &["fx-base", "fx-v1", "fx-phone"]);
        let sel = s.route.selected.clone();
        crate::fullscreen::open(s, "fx-v1", &sel);
    }),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::facts::{design_facts, portal_facts};

    #[test]
    fn facts_match_the_fixture() {
        let store = store();
        let snap = store.snapshot();
        let lineage = proto_core::live_view::slot_lineage(&snap);
        let base = design_facts(&snap, &snap.design("fx-base").unwrap(), &lineage);
        assert_eq!(base.ready_children, 3, "v1, phone and the spoke are ready children");
        assert_eq!(base.promote_to.as_deref(), Some("Prototypie"));
        assert_eq!(base.promoted_copy.as_deref(), Some("fx-pcopy"));
        let v1 = design_facts(&snap, &snap.design("fx-v1").unwrap(), &lineage);
        assert_eq!(v1.open_notes, 1);
        let copy = design_facts(&snap, &snap.design("fx-copy").unwrap(), &lineage);
        assert_eq!(copy.source.as_ref().map(|s| s.space_name.as_str()), Some("Pills"));
        assert_eq!(copy.promote_to.as_deref(), Some("Prototypie"));
        let pcopy = design_facts(&snap, &snap.design("fx-pcopy").unwrap(), &lineage);
        assert_eq!(pcopy.promote_to, None, "a project's parent is the main canvas");
        let src = design_facts(&snap, &snap.design("fx-src").unwrap(), &lineage);
        assert_eq!(src.promoted_copy.as_deref(), Some("fx-copy"));

        let promoted = snap.promoted_design_ids();
        let proj = portal_facts(&snap, &snap.space(PROJECT).unwrap(), &promoted, None, super::super::now_ms());
        assert!(proj.project && proj.local_path.as_deref() == Some("~/code/prototypie"));
        assert_eq!(proj.hero.as_deref(), Some("fx-pcopy"), "a project's hero is its newest promoted copy");
        assert!(proj.hero_starred);
        assert_eq!(proj.nested, 4);
        let empty = portal_facts(&snap, &snap.space("fx-empty").unwrap(), &promoted, None, 0.0);
        assert!(empty.hero.is_none() && !empty.designing && empty.designs == 0);
        let designing = portal_facts(&snap, &snap.space("fx-designing").unwrap(), &promoted, None, 0.0);
        assert!(designing.designing && designing.generating == 1);
    }

    #[test]
    fn canvas_order_reads_rows_then_columns() {
        let store = store();
        let snap = store.snapshot();
        let ids: Vec<String> = ["fx-spoke", "fx-v1", "fx-base", "fx-gen"].iter().map(|x| x.to_string()).collect();
        assert_eq!(crate::fullscreen::canvas_order(&snap, &ids), vec!["fx-base", "fx-v1", "fx-spoke"], "generating is skipped");
    }
}
