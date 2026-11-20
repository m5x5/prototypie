use serde_json::json;

use super::*;
use crate::store::SharedStore;
use proto_core::ChangeMeta;

fn store_with_space() -> SharedStore {
    let s: SharedStore = proto_core::Store::in_memory();
    s.apply(
        vec![
            Change::set("spaces/p1", json!({ "id": "p1", "name": "Proj", "kind": "project", "createdAt": 1 })),
            Change::set(
                "designs/d1",
                json!({ "id": "d1", "spaceId": "p1", "kind": "page", "title": "A", "rationale": "", "html": "<p>a</p>", "status": "ready", "position": { "x": 0, "y": 0 }, "createdAt": 1 }),
            ),
        ],
        ChangeMeta::client("test"),
    )
    .unwrap();
    s
}

#[test]
fn place_row_steps_right_of_near_and_past_collisions() {
    let taken = [Box2 { x: 540.0, y: 0.0, w: NODE_W, h: NODE_H }];
    let spots = place_row(&taken, 2, NODE_W, NODE_H, Some((0.0, 0.0)));
    assert_eq!(spots, vec![(1080.0, 0.0), (1620.0, 0.0)]);
}

#[test]
fn place_row_goes_below_everything_without_near() {
    let taken = [Box2 { x: 100.0, y: 0.0, w: NODE_W, h: NODE_H }, Box2 { x: -50.0, y: 200.0, w: NODE_W, h: TALL_H }];
    assert_eq!(place_row(&taken, 1, NODE_W, NODE_H, None), vec![(-50.0, 920.0)]);
    assert_eq!(place_row(&[], 1, NODE_W, NODE_H, None), vec![(0.0, 0.0)]);
}

#[test]
fn cards_land_right_of_near_in_its_space() {
    let s = store_with_space();
    let body = parse_cards_body(&json!({ "nearId": "d1", "cards": [{ "title": " T ", "html": "<b>x</b>", "viewport": 400 }] })).unwrap();
    let out = create_cards(&s, body).unwrap();
    assert_eq!(out["spaceId"], "p1");
    let id = out["ids"][0].as_str().unwrap();
    let rec = s.read(0, None).records[&format!("designs/{id}")].value.as_deref().cloned().unwrap();
    assert_eq!(rec["title"], "T");
    assert_eq!(rec["kind"], "variation");
    assert_eq!(rec["position"], json!({ "x": 540.0, "y": 0.0 }));
    assert_eq!(rec["viewport"], 400);
    assert!(rec.get("component").is_none());
}

#[test]
fn root_and_unknown_targets_are_rejected() {
    let s = store_with_space();
    let root = create_cards(&s, parse_cards_body(&json!({ "cards": [{ "title": "t", "html": "h" }] })).unwrap()).unwrap_err();
    assert_eq!(root.0, StatusCode::BAD_REQUEST);
    let near = create_cards(&s, parse_cards_body(&json!({ "nearId": "nope", "cards": [{ "title": "t", "html": "h" }] })).unwrap()).unwrap_err();
    assert_eq!(near, CanvasError(StatusCode::NOT_FOUND, "Unknown design nope".into()));
    let parent =
        create_cards(&s, parse_cards_body(&json!({ "spaceId": "p1", "cards": [{ "title": "t", "html": "h", "parentId": "x" }] })).unwrap()).unwrap_err();
    assert_eq!(parent.1, "cards[0].parentId: unknown design");
}

#[test]
fn invalid_cards_report_issues() {
    let issues = parse_cards_body(&json!({ "cards": [{ "title": "", "html": "  ", "kind": "x" }] })).err().unwrap();
    let paths: Vec<String> = issues.iter().map(|i| serde_json::to_string(&i.path).unwrap()).collect();
    assert!(paths.contains(&r#"["cards",0,"title"]"#.to_string()));
    assert!(issues.iter().any(|i| i.message == "html is empty"));
    assert!(paths.contains(&r#"["cards",0,"kind"]"#.to_string()));
    assert!(parse_cards_body(&json!({ "cards": [] })).is_err());
    assert!(parse_cards_body(&serde_json::Value::Null).is_err());
}

#[test]
fn exploration_with_seed_is_one_batch() {
    let s = store_with_space();
    let before = s.cursor().version;
    let body = parse_exploration_body(&json!({ "name": "Explore A", "seedId": "d1", "cards": [{ "title": "c", "html": "h" }] })).unwrap();
    let out = create_exploration(&s, body).unwrap();
    assert_eq!(s.cursor().version, before + 1);
    let space = out["spaceId"].as_str().unwrap();
    let state = s.read(0, None);
    assert_eq!(state.records[&format!("spaces/{space}")].value.as_deref().unwrap()["parentSpaceId"], "p1");
    let seed = out["seedId"].as_str().unwrap();
    assert_eq!(state.records[&format!("seededFrom/{seed}")].value.as_deref(), Some(&json!("d1")));
    let card = out["cardIds"][0].as_str().unwrap();
    assert_eq!(state.records[&format!("designs/{card}")].value.as_deref().unwrap()["position"], json!({ "x": 540.0, "y": 0.0 }));
    // Portal right of the seed in the parent space.
    assert_eq!(out["portal"], json!({ "x": 540.0, "y": 0.0 }));
}

#[test]
fn describe_lists_root_first() {
    let s = store_with_space();
    let v = describe(&s);
    assert_eq!(v["spaces"][0]["id"], "root");
    assert_eq!(v["spaces"][1]["kind"], "project");
    assert_eq!(v["spaces"][1]["cards"][0]["id"], "d1");
}
