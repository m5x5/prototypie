use proto_core::*;
use serde_json::{json, Value};

fn space(id: &str, parent: Option<&str>, created: f64) -> Value {
    let mut v = json!({"id": id, "name": format!("S {id}"), "createdAt": created});
    if let Some(p) = parent {
        v["parentSpaceId"] = json!(p);
    }
    v
}

fn design(id: &str, space: &str, kind: &str, parent: Option<&str>) -> Value {
    let mut v = json!({"id": id, "spaceId": space, "kind": kind, "title": id, "rationale": "", "html": format!("<p>{id}</p>"),
                       "status": "ready", "position": {"x": 0, "y": 0}, "createdAt": 1});
    if let Some(p) = parent {
        v["parentId"] = json!(p);
    }
    v
}

/// root ─ P (project, /code/p) ─ E ─ F
///      └ Q (project, no path)
fn fixture() -> Store {
    let store = Store::in_memory();
    let mut p = space("P", Some("root"), 2.0);
    p["kind"] = json!("project");
    p["localPath"] = json!("/code/p");
    let mut q = space("Q", Some("root"), 1.0);
    q["kind"] = json!("project");
    let mut g = design("g", "E", "page", None);
    g["guidelinesAt"] = json!(10);
    let mut g_old = design("g-old", "P", "page", None);
    g_old["guidelinesAt"] = json!(5);
    let changes = vec![
        Change::set("spaces/root", json!({"id": "root", "name": "Main canvas", "createdAt": 0})),
        Change::set("spaces/P", p),
        Change::set("spaces/Q", q),
        Change::set("spaces/E", space("E", Some("P"), 3.0)),
        Change::set("spaces/F", space("F", Some("E"), 4.0)),
        Change::set("spaces/E2", space("E2", Some("P"), 2.5)),
        // hub page h → spoke s → spoke variation sv; variation v of h
        Change::set("designs/h", design("h", "E", "page", None)),
        Change::set("designs/s", design("s", "E", "spoke", Some("h"))),
        Change::set("designs/sv", design("sv", "E", "spoke", Some("s"))),
        Change::set("designs/v", design("v", "E", "variation", Some("h"))),
        Change::set("designs/vv", design("vv", "E", "variation", Some("v"))),
        Change::set("designs/f1", design("f1", "F", "page", None)),
        Change::set("designs/g", g),
        Change::set("designs/g-old", g_old),
        // f1 promoted from F into E as copy c1
        Change::set("designs/c1", design("c1", "E", "page", None)),
        Change::set("promotedFrom/c1", json!({"spaceId": "F", "designId": "f1", "spaceName": "S F", "at": 9})),
        Change::set("reviewedIds/h", json!(true)),
        Change::set("reviewedIds/v", json!(false)),
    ];
    store.apply(changes, ChangeMeta::default()).unwrap();
    store
}

fn ids<T: AsRef<str>>(v: impl IntoIterator<Item = T>) -> Vec<String> {
    v.into_iter().map(|s| s.as_ref().to_string()).collect()
}

#[test]
fn spaces_and_projects() {
    let s = fixture().snapshot();
    assert_eq!(ids(s.child_spaces("P").iter().map(|x| &x.id)), ["E2", "E"], "oldest first");
    assert_eq!(ids(s.projects().iter().map(|x| &x.id)), ["Q", "P"]);
    let mut d = s.descendant_space_ids("P");
    d.sort();
    assert_eq!(d, ["E", "E2", "F"]);
    assert_eq!(ids(s.space_path("F").iter().map(|x| &x.id)), ["root", "P", "E", "F"]);
    assert_eq!(s.project_of("F").unwrap().id, "P");
    assert_eq!(s.project_of("P").unwrap().id, "P");
    assert!(s.project_of("root").is_none());
    assert_eq!(s.local_path_of("F").as_deref(), Some("/code/p"));
    assert_eq!(s.local_path_of("Q"), None);
    let mut sub = ids(s.subtree_designs("E").iter().map(|d| &d.id));
    sub.sort();
    assert_eq!(sub, ["c1", "f1", "g", "h", "s", "sv", "v", "vv"]);
    assert_eq!(s.designs_in("F").count(), 1);
}

#[test]
fn cycles_dont_hang() {
    let store = Store::in_memory();
    store
        .apply(vec![Change::set("spaces/a", space("a", Some("b"), 1.0)), Change::set("spaces/b", space("b", Some("a"), 2.0))], ChangeMeta::default())
        .unwrap();
    let s = store.snapshot();
    assert_eq!(s.space_path("a").len(), 2);
    assert!(s.project_of("a").is_none());
    let mut d = s.descendant_space_ids("a");
    d.sort();
    assert_eq!(d, ["b"]);
}

#[test]
fn hubs_children_promotion_review() {
    let s = fixture().snapshot();
    assert_eq!(s.hub_of("s").unwrap().id, "h");
    assert_eq!(s.hub_of("sv").unwrap().id, "h", "a spoke's hub is the first non-spoke ancestor");
    assert_eq!(s.hub_of("vv").unwrap().id, "h");
    assert!(s.hub_of("h").is_none());
    assert_eq!(ids(s.children_of("h").iter().map(|d| &d.id)).len(), 2);
    assert_eq!(s.promoted_copy_of("f1").unwrap().id, "c1");
    assert!(s.promoted_copy_of("h").is_none());
    assert!(s.promoted_design_ids().contains("f1"));
    assert_eq!(s.reviewed_ids().into_iter().collect::<Vec<_>>(), ["h"]);
}

#[test]
fn guidelines_inherit_and_newest_wins() {
    let store = fixture();
    let s = store.snapshot();
    assert_eq!(s.guidelines_for("F").unwrap().id, "g", "nearest ancestor's");
    assert_eq!(s.guidelines_for("E2").unwrap().id, "g-old");
    assert!(s.guidelines_for("Q").is_none());
    // A newer mark in the same space wins.
    let mut g2 = design("g2", "E", "page", None);
    g2["guidelinesAt"] = json!(20);
    store.apply(vec![Change::set("designs/g2", g2)], ChangeMeta::default()).unwrap();
    assert_eq!(store.snapshot().guidelines_for("E").unwrap().id, "g2");
}
