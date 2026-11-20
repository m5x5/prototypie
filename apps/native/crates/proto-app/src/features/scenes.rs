//! `--snap` scenes for the features, on a small in-memory store: a project with an exploration holding a
//! base design and a round of three variations made together.

use proto_core::{Change, ChangeMeta, Store};
use serde_json::json;

use crate::app::state::AppState;

pub fn fixture() -> Store {
    let store = Store::in_memory();
    let html = |c: &str| format!("<!doctype html><body style='margin:0;background:{c};height:900px'><h1 style='font:600 64px system-ui;padding:40px'>Hero</h1></body>");
    let design = |id: &str, title: &str, parent: Option<&str>, x: f64, at: f64, extra: serde_json::Value| {
        let mut v = json!({"id": id, "spaceId": "zzfx", "kind": if parent.is_some() { "variation" } else { "page" },
            "title": title, "rationale": "Bolder hierarchy so the primary action reads first.", "html": html("#e0e7ff"),
            "status": "ready", "position": {"x": x, "y": if parent.is_some() { 640.0 } else { 0.0 }}, "createdAt": at,
            "instruction": "more playful"});
        if let Some(p) = parent { v["parentId"] = json!(p); }
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) { o.extend(e.clone()); }
        Change::set(format!("designs/{id}"), v)
    };
    let changes = vec![
        Change::set("spaces/zzproj", json!({"id": "zzproj", "name": "Shop", "parentSpaceId": "root", "createdAt": 1, "kind": "project"})),
        Change::set("spaces/zzfx", json!({"id": "zzfx", "name": "Checkout", "parentSpaceId": "zzproj", "createdAt": 2})),
        design("zzbase", "Checkout page", None, 0.0, 1000.0, json!({})),
        design("zzv1", "Playful steps", Some("zzbase"), 0.0, 5000.0, json!({"starred": true})),
        design("zzv2", "Confetti summary", Some("zzbase"), 420.0, 5001.0, json!({})),
        design("zzv3", "Rounded cards", Some("zzbase"), 840.0, 5002.0, json!({})),
        Change::set("reviewedIds/zzbase", json!(true)),
    ];
    store.apply(changes, ChangeMeta::client("scene")).expect("fixture");
    store
}

fn base(s: &mut AppState) {
    s.use_store(fixture());
    s.enter_space("zzfx", None);
}

pub const SCENES: &[(&str, fn(&mut AppState))] = &[
    ("review", |s| {
        base(s);
        super::review::open(s, Some("zzv2".into()));
        s.features.review.feedback = rune::TextEdit::new("1 and 3, more contrast");
    }),
    ("review-dock", |s| {
        base(s);
        s.features.review.dock_open = true;
    }),
    ("spokes", |s| {
        base(s);
        s.features.spokes.cache.insert("zzbase".into(), vec![
            super::spokes::SpokeComponent { name: "Order summary".into(), description: "The sticky right column with line items and totals.".into(), selector: Some("aside.summary".into()) },
            super::spokes::SpokeComponent { name: "Step indicator".into(), description: "Progress through shipping, payment and review.".into(), selector: Some("nav.steps".into()) },
            super::spokes::SpokeComponent { name: "Payment form".into(), description: "Card fields and wallet buttons.".into(), selector: None },
            super::spokes::SpokeComponent { name: "Trust strip".into(), description: "Badges and return policy under the button.".into(), selector: None },
        ]);
        super::spokes::open_picker(s, "zzbase");
    }),
    ("clay", |s| {
        base(s);
        s.features.spokes.clay = Some(rune::TextEdit::new("Landing page for a climbing gym"));
    }),
    ("multi", |s| {
        base(s);
        s.route.selected = vec!["zzv1".into(), "zzv2".into(), "zzv3".into()];
    }),
    ("multi-open", |s| {
        base(s);
        s.route.selected = vec!["zzv1".into(), "zzv2".into(), "zzv3".into()];
        s.features.multi.open = true;
        s.features.multi.mode = super::multi::Mode::Combine;
    }),
    ("decisions", |s| {
        base(s);
        for id in ["zzv1", "zzv2", "zzv3"] {
            super::polls::toggle_candidate(s, id);
        }
        s.features.polls.title = rune::TextEdit::new("Checkout - pick a direction");
        super::polls::add_decision(s);
        let d = &mut s.features.polls.decisions[0];
        d.question = rune::TextEdit::new("How playful should the steps feel?");
        d.why = rune::TextEdit::new("Sets the tone for the whole checkout.");
        d.options = vec![
            super::polls::DraftOption { design_id: "zzv1".into(), label: rune::TextEdit::new("Playful") },
            super::polls::DraftOption { design_id: "zzv3".into(), label: rune::TextEdit::new("Calm") },
        ];
        super::polls::open_panel(s, super::polls::Tab::Build);
    }),
    ("decisions-dock", |s| {
        base(s);
        super::polls::toggle_candidate(s, "zzv1");
        s.features.polls.dock_open = true;
    }),
    ("mention", |s| {
        base(s);
        s.route.selected = vec!["zzbase".into()];
        let mut e = rune::TextEdit::new("reuse #");
        e.anchor = 7;
        e.focus = 7;
        e.focused = true;
        *crate::composer::edit_mut(s) = e;
        s.features.mention.open = Some(super::mention::Open { trigger: '#', start: 6, query: String::new(), active: 1 });
        let k = crate::composer::draft_key(s);
        s.features.mention.by_draft.entry(k).or_default().chips.push(super::mention::Chip::Component("zzv1".into()));
    }),
    ("layouts", |s| {
        base(s);
        s.features.mention.catalog = Some(serde_json::from_value(json!({"pods": [{"id": "home", "label": "Home", "display": "my-pod"}], "entries": [
            {"id": "sh-task", "kind": "shape", "pod": "home", "label": "ical:Vtodo", "path": "tasks", "access": "ref", "children": ["t1", "t2", "t3"]},
            {"id": "t1", "kind": "resource", "pod": "home", "label": "Essay", "path": "tasks/t1.ttl", "access": "401",
             "fields": [{"p": "ical:due", "v": "2026-11-20T10:00 (Europe/Berlin)"}], "relations": [{"p": "schema:about", "target": "c1"}]},
            {"id": "t2", "kind": "resource", "pod": "home", "label": "Quiz", "path": "tasks/t2.ttl", "access": "live",
             "fields": [{"p": "ical:due", "v": "2026-11-21T09:00 (Europe/Berlin)"}], "relations": [{"p": "schema:about", "target": "c2"}]},
            {"id": "t3", "kind": "resource", "pod": "home", "label": "Lab report", "path": "tasks/t3.ttl", "access": "live",
             "fields": [{"p": "ical:due", "v": "2026-11-22T14:00 (Europe/Berlin)"}], "relations": [{"p": "schema:about", "target": "c1"}]},
            {"id": "sh-course", "kind": "shape", "pod": "home", "label": "schema:Course", "path": "courses", "access": "live", "children": ["c1", "c2"]},
            {"id": "c1", "kind": "resource", "pod": "home", "label": "Biology", "path": "courses/c1.ttl", "access": "live"},
            {"id": "c2", "kind": "resource", "pod": "home", "label": "History", "path": "courses/c2.ttl", "access": "live"}
        ]})).expect("catalog"));
        s.features.layouts = super::pod_layouts::LayoutFinder { open: true, as_space: true, ..Default::default() };
    }),
    ("compare", |s| {
        base(s);
        super::compare::open(s, "zzbase");
    }),
    ("annotate", |s| annotate(s, false)),
    ("annotate-light", |s| annotate(s, true)),
    ("annotate-thread", |s| {
        annotate(s, false);
        s.features.annotate.draft = None;
        s.features.annotate.selection.clear();
        s.features.annotate.open_note = s.features.annotate.notes.first().map(|n| n.id.clone());
    }),
];

/// Annotate mode on: a pending note pinned on the Review button (with an agent reply), an acknowledged one on the
/// `+` button, the note popover open on the Agents pill. Nothing is sent (tooling runs have no client).
fn annotate(s: &mut AppState, light: bool) {
    use super::annotate::model::{Message, Note, Status, Target};
    base(s);
    if light {
        s.theme = crate::app::state::ThemePref::Light;
    }
    let target = |key: &str, element: &str| Target {
        element: element.into(),
        element_path: format!("header-actions > {key}"),
        key: Some(key.into()),
        source: Some("crates/proto-app/src/features/review/dock.rs:33".into()),
        ..Target::default()
    };
    let note = |id: &str, comment: &str, t: Target, status: Status, thread: Vec<Message>| Note {
        id: id.into(), server_id: Some(id.into()), space: s.route.space.clone(), comment: comment.into(), targets: vec![t],
        timestamp: 1.0, status, thread, window: [1280.0, 800.0],
    };
    let a = vec![
        note("zz-a1", "Make the waiting count easier to read", target("review-dock", "button \"Review\""), Status::Pending,
            vec![Message { role: "agent".into(), content: "Bumping it to 11.5 pt in dock.rs.".into() }]),
        note("zz-a2", "Plus should say New on hover", target("new", "button \"New\""), Status::Acknowledged, vec![]),
    ];
    let ag = &mut s.features.annotate;
    ag.notes = a;
    ag.on = true;
    ag.selection = vec![Target { element: "button \"Agents\"".into(), element_path: "agent-capacity".into(), key: Some("agent-capacity".into()),
        source: Some("crates/proto-app/src/features/agent_tasks/view.rs:40".into()), rect: [916.0, 6.0, 126.0, 36.0], ..Target::default() }];
    ag.draft = Some(rune::TextEdit::new("Too many dots, show a number"));
}
