use proto_core::live_view::*;
use proto_core::*;
use serde_json::{json, Value};

fn design(id: &str, html: &str, parent: Option<&str>, main_at: Option<f64>) -> Value {
    let mut v = json!({"id": id, "spaceId": "s", "kind": "variation", "title": format!("T \"{id}\""), "rationale": "",
                       "html": html, "status": "ready", "position": {"x": 0, "y": 0}, "createdAt": 1});
    if let Some(p) = parent {
        v["parentId"] = json!(p);
    }
    if let Some(m) = main_at {
        v["mainAt"] = json!(m);
    }
    v
}

fn store(designs: Vec<Value>) -> Store {
    let store = Store::in_memory();
    let changes = designs.into_iter().map(|d| Change::set(format!("designs/{}", d["id"].as_str().unwrap()), d)).collect();
    store.apply(changes, ChangeMeta::default()).unwrap();
    store
}

#[test]
fn slots_and_ids() {
    assert!(has_slots("<PROTOTYPIE-SLOT data-design=a>"));
    assert!(!has_slots("<prototypie-slots>"));
    let html = r#"<prototypie-slot data-design="a"></prototypie-slot><x><prototypie-slot class="c" data-design='b-2'>  </prototypie-slot><prototypie-slot style="x"></prototypie-slot>"#;
    assert_eq!(slot_ids(html), ["a", "b-2"]);
}

#[test]
fn main_variant_rules() {
    let s = store(vec![
        design("c", "<p>c</p>", None, None),
        design("c1", "<p>c1</p>", Some("c"), Some(5.0)),
        design("c2", "<p>c2</p>", Some("c1"), Some(9.0)),
        // A live view derived from c, with a newer mark: skipped with its branch.
        design("lv", r#"<prototypie-slot data-design="c"></prototypie-slot>"#, Some("c"), Some(50.0)),
        design("lvc", "<p>under live view</p>", Some("lv"), Some(60.0)),
        design("z", "<p>z</p>", None, Some(0.0)),
    ])
    .snapshot();
    assert_eq!(s.main_variant("c").unwrap().id, "c2");
    assert_eq!(s.main_variant("c1").unwrap().id, "c2");
    assert_eq!(s.main_variant("z").unwrap().id, "z");
    assert!(s.main_variant("missing").is_none());
    let lineage = slot_lineage(&s);
    let mut l: Vec<_> = lineage.into_iter().collect();
    l.sort();
    assert_eq!(l, ["c", "c1", "c2"], "targets and their variations, not the live view branch");
}

#[test]
fn compose_replaces_slots() {
    let s = store(vec![
        design("a", "<html><body><b>A & \"q\"</b></body></html>", None, None),
        design("n", r#"<div><prototypie-slot data-design="a"></prototypie-slot></div>"#, None, None),
    ])
    .snapshot();
    let html = r#"<html><body><prototypie-slot class="row wide" data-design="a" data-height="120" style="flex:1"></prototypie-slot><prototypie-slot data-design="nope"></prototypie-slot></body></html>"#;
    let out = compose_live(html, &s, None);
    // Two spaces after the class: the class attribute is cut out of the attrs, like the reference does.
    let expected_frame = r#"<iframe title="T &quot;a&quot;" srcdoc="&lt;html&gt;&lt;body&gt;&lt;b&gt;A &amp; &quot;q&quot;&lt;/b&gt;&lt;/body&gt;&lt;/html&gt;" style="height:120px"></iframe>"#;
    assert!(
        out.starts_with(&format!(r#"<html><body><div class="pt-slot row wide"  data-design="a" data-height="120" style="flex:1">{expected_frame}</div>"#)),
        "{out}"
    );
    assert!(out.contains(r#"<div class="pt-slot" data-design="nope"></div>"#), "missing target → empty slot");
    assert!(out.ends_with(&format!("{SIZER}</body></html>")), "SIZER before the last </body>");
    assert_eq!(out.matches("data-prototypie-live").count(), 2);

    // Without slots: unchanged. Without </body>: SIZER appended.
    assert_eq!(compose_live("<p>plain</p>", &s, None), "<p>plain</p>");
    let bare = compose_live(r#"<prototypie-slot data-design="a">"#, &s, None);
    assert!(bare.ends_with(SIZER));

    // Nested slots are expanded inside the srcdoc, and wrap is applied to each component document.
    let wrap = |h: &str| format!("[{h}]");
    let nested = compose_live(r#"<prototypie-slot data-design="n"></prototypie-slot>"#, &s, Some(&wrap));
    assert!(nested.contains("pt-slot"));
    assert!(nested.contains("srcdoc=\"[&lt;div&gt;&lt;div class=&quot;pt-slot&quot;"), "{nested}");
    assert!(nested.contains("[&amp;lt;html&amp;gt;"), "inner component wrapped and escaped twice: {nested}");
}

#[test]
fn not_ready_component_renders_empty() {
    let mut a = design("a", "<p>a</p>", None, None);
    a["status"] = json!("generating");
    let s = store(vec![a]).snapshot();
    let out = compose_live(r#"<prototypie-slot data-design="a"></prototypie-slot>"#, &s, None);
    assert!(out.starts_with(r#"<div class="pt-slot" data-design="a"></div>"#), "{out}");
}

#[test]
fn depth_is_bounded() {
    // A slot that shows itself: expanded a few levels, then left as is.
    let s = store(vec![design("r", r#"<prototypie-slot data-design="r"></prototypie-slot>"#, None, None)]).snapshot();
    let out = compose_live(r#"<prototypie-slot data-design="r"></prototypie-slot>"#, &s, None);
    assert!(out.len() < 200_000);
    assert!(out.contains("pt-slot"));
}

#[test]
fn cache_reuses_until_a_component_changes() {
    let st = store(vec![design("a", "<p>a</p>", None, None)]);
    let card = r#"<body><prototypie-slot data-design="a"></prototypie-slot></body>"#;
    let mut cache = ComposeCache::new(2);
    let s1 = st.snapshot();
    let first = cache.compose("card", card, &s1, None);
    // Unrelated change: same output (same Arc).
    st.apply(vec![Change::set("portals/x", json!({"x": 1, "y": 1}))], ChangeMeta::default()).unwrap();
    let again = cache.compose("card", card, &st.snapshot(), None);
    assert!(std::sync::Arc::ptr_eq(&first, &again));
    assert_eq!(fingerprint(card, &s1), fingerprint(card, &st.snapshot()));
    // The component changes: recomposed, and the fingerprint moves.
    st.apply(vec![Change::set("designs/a", design("a", "<p>a2</p>", None, None))], ChangeMeta::default()).unwrap();
    let changed = cache.compose("card", card, &st.snapshot(), None);
    assert!(changed.contains("a2"));
    assert_ne!(fingerprint(card, &s1), fingerprint(card, &st.snapshot()));
    // Bounded, oldest evicted.
    cache.compose("b", card, &st.snapshot(), None);
    cache.compose("c", card, &st.snapshot(), None);
    assert_eq!(cache.len(), 2);
}

#[test]
fn default_html() {
    assert_eq!(
        default_live_html(&["a", "b"], 1280),
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=1280\"><style>html,body{margin:0;background:#fff}main{display:flex;flex-direction:column}</style></head><body><main>\n<prototypie-slot data-design=\"a\"></prototypie-slot>\n<prototypie-slot data-design=\"b\"></prototypie-slot>\n</main></body></html>"
    );
}
