use std::sync::Arc;

use proto_core::ops::*;
use proto_core::*;
use serde_json::{json, Value};

fn design(id: &str, space: &str, parent: Option<&str>, x: f64) -> Value {
    let mut v = json!({"id": id, "spaceId": space, "kind": "variation", "title": format!("T {id}"), "rationale": "r",
                       "html": format!("<p>{id}</p>"), "status": "ready", "position": {"x": x, "y": 0}, "createdAt": 1,
                       "viewport": 390, "error": null});
    if let Some(p) = parent {
        v["parentId"] = json!(p);
    }
    v
}

/// root ─ P (project /code/p, portal at 0,0) ─ E (portal at 0,0 in P) ; designs a → b in E, x in P
fn fixture() -> Store {
    let store = Store::in_memory();
    store
        .apply(
            vec![
                Change::set("spaces/root", json!({"id": "root", "name": "Main canvas", "createdAt": 0})),
                Change::set("spaces/P", json!({"id": "P", "name": "P", "parentSpaceId": "root", "createdAt": 1, "kind": "project", "localPath": "/code/p"})),
                Change::set("spaces/E", json!({"id": "E", "name": "E", "parentSpaceId": "P", "createdAt": 2})),
                Change::set("portals/P", json!({"x": 0, "y": 0})),
                Change::set("portals/E", json!({"x": 0, "y": 0})),
                Change::set("designs/a", design("a", "E", None, 0.0)),
                Change::set("designs/b", design("b", "E", Some("a"), 500.0)),
                Change::set("designs/x", design("x", "P", None, 2000.0)),
            ],
            ChangeMeta::default(),
        )
        .unwrap();
    store
}

fn apply(store: &Store, changes: Vec<Change>) {
    store.apply(changes, ChangeMeta::client("test")).unwrap();
}

#[test]
fn ids_look_like_nanoid() {
    let a = new_id(10);
    assert_eq!(a.len(), 10);
    assert!(a.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'));
    assert_ne!(new_id(10), new_id(10));
}

#[test]
fn move_keeps_every_other_field() {
    let store = fixture();
    apply(&store, vec![move_design(&store.snapshot(), "a", Point { x: 12.5, y: -3.0 }).unwrap(), move_portal("E", Point { x: 7.0, y: 8.0 })]);
    let s = store.snapshot();
    let raw = s.value("designs/a").unwrap();
    assert_eq!(raw["position"], json!({"x": 12.5, "y": -3}));
    assert_eq!(raw["error"], Value::Null, "explicit null kept by raw patches");
    assert_eq!(raw["viewport"], json!(390));
    assert_eq!(s.portal("E"), Some(Point { x: 7.0, y: 8.0 }));
    assert!(move_design(&s, "missing", Point::default()).is_none());
}

#[test]
fn explore_creates_space_portal_and_seed() {
    let store = fixture();
    let c = create_exploration(&store.snapshot(), "E", None, Some("b"));
    apply(&store, c.changes.clone());
    let s = store.snapshot();
    let sp = s.space(&c.space_id).unwrap();
    assert_eq!(sp.name, "T b", "named after the seed");
    assert_eq!(sp.parent_space_id.as_deref(), Some("E"));
    let seed = s.design(c.seed_id.as_deref().unwrap()).unwrap();
    assert_eq!((seed.space_id.as_str(), seed.parent_id.as_deref(), seed.position), (c.space_id.as_str(), None, Point::default()));
    assert_eq!(seed.html, "<p>b</p>");
    assert_eq!(s.seeded_from(&seed.id).as_deref(), Some("b"));
    // Portal right of the seed, clear of the other cards.
    let p = s.portal(&c.space_id).unwrap();
    assert!(p.x >= 500.0 + NODE_W, "{p:?}");
    // Plain new exploration: "Exploration N".
    let c2 = create_exploration(&s, "E", None, None);
    apply(&store, c2.changes);
    assert_eq!(store.snapshot().space(&c2.space_id).unwrap().name, "Exploration 2");
}

#[test]
fn projects_are_unique_per_folder() {
    let store = fixture();
    let (same, created) = create_project(&store.snapshot(), "again", Some("/CODE/p"));
    assert!(!created && same.changes.is_empty());
    assert_eq!(same.space_id, "P");
    let (c, created) = create_project(&store.snapshot(), "Q", Some("/code/q"));
    assert!(created);
    apply(&store, c.changes);
    let s = store.snapshot();
    let q = s.space(&c.space_id).unwrap();
    assert!(q.is_project());
    assert_eq!(q.local_path.as_deref(), Some("/code/q"));
    assert_eq!(s.projects().len(), 2);
    assert_ne!(s.portal(&c.space_id), s.portal("P"), "placed apart");
}

#[test]
fn promote_copies_up_with_lineage_and_never_to_root() {
    let store = fixture();
    // Promote a first, then b (child of a): b's copy gets an edge to a's copy.
    let (ch, ids) = promote(&store.snapshot(), &["a"]);
    apply(&store, ch);
    let (ch2, ids2) = promote(&store.snapshot(), &["b", "a"]);
    assert_eq!(ids2.len(), 1, "a is already promoted");
    apply(&store, ch2);
    let s = store.snapshot();
    let ca = s.design(&ids[0]).unwrap();
    let cb = s.design(&ids2[0]).unwrap();
    assert_eq!(ca.space_id, "P");
    assert_eq!(cb.parent_id.as_deref(), Some(ca.id.as_str()));
    assert_eq!(s.promoted_copy_of("a").unwrap().id, ca.id);
    assert_eq!(s.promoted_from(&cb.id).unwrap().space_name, "E");
    assert!(ca.position.y >= PORTAL_H, "below the portal");
    assert_ne!(ca.position, cb.position);
    // From a project (parent = main canvas): nothing.
    let (none, _) = promote(&s, &["x"]);
    assert!(none.is_empty());
    assert!(!can_promote_to(Some("root")) && can_promote_to(Some("P")) && !can_promote_to(None));
}

#[test]
fn delete_space_removes_subtree() {
    let store = fixture();
    let c = create_exploration(&store.snapshot(), "E", Some("inner"), Some("a"));
    apply(&store, c.changes.clone());
    let (ch, ids) = promote(&store.snapshot(), &["a"]);
    apply(&store, ch);
    apply(&store, delete_space(&store.snapshot(), "E"));
    let s = store.snapshot();
    assert!(s.space("E").is_none() && s.space(&c.space_id).is_none());
    assert!(s.design("a").is_none() && s.design(c.seed_id.as_deref().unwrap()).is_none());
    assert!(s.portal("E").is_none() && s.portal(&c.space_id).is_none());
    assert!(s.seeded_from(c.seed_id.as_deref().unwrap()).is_none());
    assert!(s.design(&ids[0]).is_some(), "the promoted copy in P stays");
    assert!(s.promoted_from(&ids[0]).is_some());
    assert!(delete_space(&s, "root").is_empty());
}

#[test]
fn flags() {
    let store = fixture();
    apply(&store, set_starred(&store.snapshot(), "a", true).into_iter().collect());
    apply(&store, set_guidelines(&store.snapshot(), "a", true));
    apply(&store, set_guidelines(&store.snapshot(), "b", true));
    apply(&store, set_main(&store.snapshot(), "b", true).into_iter().collect());
    apply(&store, set_reviewed(&["a", "b"], true));
    apply(&store, set_reviewed(&["b"], false));
    let s = store.snapshot();
    assert!(s.design("a").unwrap().is_starred());
    assert!(s.design("a").unwrap().guidelines_at.is_none(), "one guidelines card per space");
    assert_eq!(s.guidelines_for("E").unwrap().id, "b");
    assert_eq!(s.main_variant("a").unwrap().id, "b");
    assert_eq!(s.reviewed_ids().into_iter().collect::<Vec<_>>(), ["a"]);
}

#[test]
fn preview_inputs_ignore_moves_and_follow_components() {
    let store = fixture();
    apply(&store, vec![Change::set("designs/lv", {
        let mut v = design("lv", "E", None, 900.0);
        v["html"] = json!(r#"<body><prototypie-slot data-design="a"></prototypie-slot></body>"#);
        v
    })]);
    let wrap: preview::Wrap = Arc::new(|h: &str| format!("<!--w-->{h}"));
    let mut p = PreviewInputs::new(Some(wrap), 100);
    let a1 = p.get(&store.snapshot(), "a").unwrap();
    let lv1 = p.get(&store.snapshot(), "lv").unwrap();
    assert_eq!(&*a1.html, "<p>a</p>");
    assert!(lv1.html.contains("srcdoc=\"&lt;!--w--&gt;&lt;p&gt;a&lt;/p&gt;\""), "{}", lv1.html);

    // Moving cards: same keys, same Arcs.
    apply(&store, vec![move_design(&store.snapshot(), "a", Point { x: 1.0, y: 1.0 }).unwrap(), move_design(&store.snapshot(), "lv", Point { x: 2.0, y: 2.0 }).unwrap()]);
    let a2 = p.get(&store.snapshot(), "a").unwrap();
    let lv2 = p.get(&store.snapshot(), "lv").unwrap();
    assert_eq!(a2.key, a1.key);
    assert!(Arc::ptr_eq(&a2.html, &a1.html));
    assert_eq!(lv2.key, lv1.key);
    assert!(Arc::ptr_eq(&lv2.html, &lv1.html));

    // The component's html changes: both keys move, the live view is recomposed.
    let mut a = design("a", "E", None, 1.0);
    a["html"] = json!("<p>a2</p>");
    apply(&store, vec![Change::set("designs/a", a)]);
    let a3 = p.get(&store.snapshot(), "a").unwrap();
    let lv3 = p.get(&store.snapshot(), "lv").unwrap();
    assert_ne!(a3.key, a1.key);
    assert_ne!(lv3.key, lv1.key);
    assert!(lv3.html.contains("a2"));
    assert!(p.get(&store.snapshot(), "missing").is_none());
    p.invalidate("a");
    assert_eq!(p.len(), 1);
}

#[test]
fn spokes_lanes_polls() {
    let store = fixture();
    apply(&store, set_spoke_focus(&["a", "b"], "Header"));
    let lane = |id: &str, t: f64| Lane { job_id: id.into(), lane_type: LaneType::Spoke, name: id.into(), focus: None, hub_id: None, count: 1, started_at: t, extra: Default::default() };
    let s0 = store.snapshot();
    let first: Vec<Change> = (0..MAX_LANES).flat_map(|i| put_lane(&s0, &lane(&format!("l{i}"), i as f64))).collect();
    apply(&store, first);
    // One more, newest: the oldest (l0) is deleted in the same batch.
    let ch = put_lane(&store.snapshot(), &lane("new", 1000.0));
    assert_eq!(ch.len(), 2);
    apply(&store, ch);
    // Re-putting an existing lane doesn't evict anything.
    assert_eq!(put_lane(&store.snapshot(), &lane("l5", 5.0)).len(), 1);
    apply(&store, vec![set_poll_winner("poll1", "d1", Some("o2"))]);
    let s = store.snapshot();
    assert_eq!(s.spoke_focus("b").as_deref(), Some("Header"));
    assert_eq!(s.lanes().count(), MAX_LANES);
    assert!(s.lane("l0").is_none() && s.lane("new").is_some());
    assert_eq!(s.poll_winner("poll1/d1").as_deref(), Some("o2"));
    apply(&store, vec![set_poll_winner("poll1", "d1", None)]);
    assert!(store.snapshot().poll_winner("poll1/d1").is_none());
}
