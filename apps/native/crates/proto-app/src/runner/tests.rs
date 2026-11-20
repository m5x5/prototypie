//! The runner end to end on an in-memory store: a scripted provider (no HTTP, no real Codex) or the agent queue
//! in a temp data dir.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use proto_ai::llm::{Completion, LlmStatus, Provider, ToolRequest};
use proto_ai::{AiError, CancellationToken};
use proto_core::{Change, DesignKind, DesignStatus, RecordKind, Store, VariationRequest};
use serde_json::{Value, json};

use super::changes;
use super::*;
use crate::app::state::AppState;

/// Answers by prompt: plans N directions, builds a design titled after its direction, edits by appending.
struct Scripted {
    prompts: Mutex<Vec<String>>,
}

impl Provider for Scripted {
    fn status(&self) -> LlmStatus {
        LlmStatus { provider: "scripted".into(), model: "m".into(), logged_in: true }
    }
    fn complete<'a>(&'a self, req: &'a ToolRequest, _c: &'a CancellationToken) -> BoxFuture<'a, Result<Completion, AiError>> {
        self.prompts.lock().unwrap().push(req.prompt.clone());
        let p = &req.prompt;
        let out = if p.contains("Propose exactly 2") {
            json!({"directions":[{"title":"Dock","brief":"b1"},{"title":"Rail","brief":"b2"}]})
        } else if let Some(i) = p.find("Title: ") {
            let title = p[i + 7..].lines().next().unwrap_or("").to_string();
            if title == "Rail" {
                return Box::pin(async { Err(AiError::Llm("rate limited".into())) });
            }
            json!({"title": title, "rationale": "r", "html": format!("<p>{title}</p>")})
        } else if p.contains("Edit instruction:") {
            json!({"rationale": "Made it blue", "html": "<p class=blue>"})
        } else {
            json!({})
        };
        Box::pin(async move { Ok(Completion::ToolCall(out)) })
    }
}

struct Fixture {
    s: AppState,
    _dir: tempfile::TempDir,
    provider: Arc<Scripted>,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let mut s = AppState::new();
    s.use_store(Store::in_memory());
    let provider = Arc::new(Scripted { prompts: Mutex::default() });
    s.runner.config = HostConfig { data_dir: Some(dir.path().to_path_buf()), provider: Some(provider.clone()), notifier: None };
    let space = json!({"id":"sp","name":"Header navbar","parentSpaceId":"root","createdAt":1});
    let base = json!({"id":"base","spaceId":"sp","kind":"page","title":"Base","rationale":"","html":"<p>base</p>","status":"ready",
        "position":{"x":0,"y":0},"createdAt":1,"viewport":400});
    s.apply(vec![Change::set("spaces/sp", space), Change::set("designs/base", base)], "test").unwrap();
    Fixture { s, _dir: dir, provider }
}

fn poll(s: &mut AppState) -> bool {
    super::poll(s, 0.0, &mut rune::Tx::default())
}

fn poll_until(s: &mut AppState, what: &str, mut done: impl FnMut(&AppState) -> bool) {
    for _ in 0..1000 {
        poll(s);
        if done(s) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for {what}");
}

fn generate(agent: &str, count: u32) -> Outgoing {
    Outgoing::Generate {
        space_id: "sp".into(),
        parent_id: Some("base".into()),
        kind: DesignKind::Variation,
        label: None,
        request: VariationRequest {
            instruction: "toolbar".into(),
            count,
            agent: Some(agent.into()),
            base_html: Some("<p>base</p>".into()),
            base_title: Some("Base".into()),
            ..VariationRequest::default()
        },
        origin: None,
        run_id: None,
    }
}

fn children(s: &AppState) -> Vec<Arc<proto_core::Design>> {
    let mut v: Vec<_> = s.snap().designs_in("sp").filter(|d| d.id != "base").collect();
    v.sort_by(|a, b| a.position.x.total_cmp(&b.position.x));
    v
}

#[test]
fn codex_generation_fills_placeholders_and_marks_failures() {
    let mut f = fixture();
    f.s.composer.outbox.push(generate("codex", 2));
    assert!(poll(&mut f.s));
    assert!(f.s.composer.outbox.is_empty());
    assert_eq!(f.s.snap().runs().count(), 1, "run record written at start");
    poll_until(&mut f.s, "run to finish", |s| !s.runner.is_busy());

    let kids = children(&f.s);
    assert_eq!(kids.len(), 2);
    let (dock, rail) = (&kids[0], &kids[1]);
    assert_eq!(dock.title, "Dock");
    assert_eq!(dock.status, DesignStatus::Ready);
    assert_eq!(dock.html, "<p>Dock</p>");
    assert_eq!(dock.parent_id.as_deref(), Some("base"));
    assert_eq!(dock.viewport, Some(400.0), "variations keep the base's frame");
    assert_eq!(rail.status, DesignStatus::Error);
    assert_eq!(rail.error.as_deref(), Some("rate limited"));
    // Row centred below the parent: 2 × 380 + 160 wide, 420 + 220 down.
    assert_eq!((dock.position.x, dock.position.y), (190.0 - 460.0, 640.0));
    assert_eq!(rail.position.x - dock.position.x, 540.0);
    let req = dock.request.as_ref().unwrap();
    assert_eq!(req.count, 1);
    assert!(req.base_html.is_none(), "stored request drops the base html");
    assert_eq!(req.direction.as_ref().unwrap().title, "Dock");
    assert_eq!(f.s.snap().runs().count(), 0, "run record removed at the end");
    assert!(f.provider.prompts.lock().unwrap()[0].contains("Base design (\"Base\")"));
}

#[test]
fn claude_generation_goes_through_the_queue() {
    let mut f = fixture();
    f.s.composer.outbox.push(generate("claude", 2));
    poll(&mut f.s);
    poll_until(&mut f.s, "placeholders", |s| children(s).len() == 2);
    let ai = f.s.runner.host().unwrap().ai().clone();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let job = f.s.runner.active_runs().next().unwrap().job_id.clone();
    rt.block_on(async {
        for _ in 0..400 {
            if ai.queue().list_pending().await.len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let items = ai.queue().list_pending().await;
        assert_eq!(items.len(), 2, "two queue items on disk");
        assert_eq!(items[0].base_html.as_deref(), Some("<p>base</p>"));
        let v = proto_ai::Variation { title: "Bottom dock".into(), rationale: "R".into(), html: "<p>dock".into(), viewport: Some(1280), viewport_height: None };
        assert!(ai.queue().submit(&job, 0, v).await);
        assert!(ai.queue().cancel_item(&job, 1, "Card deleted").await);
    });
    poll_until(&mut f.s, "run to finish", |s| !s.runner.is_busy());
    let kids = children(&f.s);
    assert_eq!(kids[0].title, "Bottom dock");
    assert_eq!(kids[0].status, DesignStatus::Ready);
    assert_eq!(kids[0].viewport, Some(1280.0));
    assert_eq!(kids[1].error.as_deref(), Some("Card deleted"));
    assert!(f.provider.prompts.lock().unwrap().is_empty(), "queue mode never calls the model");
}

#[test]
fn cancelling_a_run_fails_its_waiting_cards() {
    let mut f = fixture();
    f.s.composer.outbox.push(generate("claude", 1));
    poll(&mut f.s);
    poll_until(&mut f.s, "placeholder", |s| children(s).len() == 1);
    let run_id = f.s.runner.active_runs().next().unwrap().run.id.clone();
    cancel_run(&mut f.s, &run_id);
    poll_until(&mut f.s, "run to finish", |s| !s.runner.is_busy());
    assert_eq!(children(&f.s)[0].error.as_deref(), Some("Cancelled"));
    let done = f.s.runner.take_finished();
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].outcome, Outcome::Cancelled);
    assert_eq!(done[0].design_ids.len(), 1);
}

#[test]
fn feature_runs_take_origin_run_id_and_bases_and_report_when_done() {
    let mut f = fixture();
    let other = json!({"id":"other","spaceId":"sp","kind":"page","title":"Other","rationale":"","html":"<p>o</p>","status":"ready",
        "position":{"x":600,"y":0},"createdAt":1});
    f.s.apply(vec![Change::set("designs/other", other)], "t").unwrap();
    let mut request = VariationRequest { instruction: "Combine them into one design.".into(), count: 2, agent: Some("codex".into()), ..Default::default() };
    request.extra.insert(
        "bases".into(),
        json!([{"id":"base","title":"Base","html":"<p>base</p>"},{"id":"other","title":"Other","html":"<p>o</p>"}]),
    );
    let run_id = new_run_id();
    f.s.composer.outbox.push(Outgoing::Generate {
        space_id: "sp".into(),
        parent_id: Some("base".into()),
        kind: DesignKind::Variation,
        label: Some("Mix: Base + Other".into()),
        request,
        origin: Some(proto_core::Point { x: 1000.0, y: 2000.0 }),
        run_id: Some(run_id.clone()),
    });
    poll(&mut f.s);
    assert!(f.s.runner.is_running(&run_id));
    assert_eq!(f.s.snap().run(&run_id).unwrap().label, "Mix: Base + Other");
    poll_until(&mut f.s, "run to finish", |s| !s.runner.is_busy());
    assert!(f.s.runner.take_finished_where(|d| d.run_id == "someone-else").is_empty());
    let done = f.s.runner.take_finished_where(|d| d.run_id == run_id);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].run_id, run_id);
    assert_eq!(done[0].outcome, Outcome::Done);
    let cards: Vec<_> = done[0].design_ids.iter().map(|id| f.s.snap().design(id).unwrap()).collect();
    assert_eq!((cards[0].position.x, cards[0].position.y), (1000.0, 2000.0));
    assert_eq!(cards[1].position.x, 1540.0);
    let stored = serde_json::to_value(cards[0].request.as_ref().unwrap()).unwrap();
    assert_eq!(stored["bases"][1], json!({"id":"other","title":"Other","html":""}), "bases keep id/title only");
    assert!(f.provider.prompts.lock().unwrap()[0].starts_with("Combine these 2 designs into ONE new design."));
    assert!(f.s.runner.take_finished().is_empty(), "taken once");
}

#[test]
fn a_job_level_failure_is_reported() {
    let mut f = fixture();
    f.s.runner.config.provider = Some(Arc::new(Scripted { prompts: Mutex::default() }));
    // count 3: the scripted planner only answers "Propose exactly 2", so planning returns no directions object.
    f.s.composer.outbox.push(generate("codex", 3));
    poll(&mut f.s);
    poll_until(&mut f.s, "run to finish", |s| !s.runner.is_busy());
    let done = f.s.runner.take_finished();
    assert!(matches!(&done[0].outcome, Outcome::Failed(m) if m == "The model did not return a valid result."), "{done:?}");
    assert!(done[0].design_ids.is_empty());
}

#[test]
fn in_place_edit_keeps_a_version_and_rejects_busy_cards() {
    let mut f = fixture();
    f.s.composer.outbox.push(Outgoing::Edit {
        design_id: "base".into(),
        request: VariationRequest { instruction: "blue".into(), count: 1, agent: Some("codex".into()), ..Default::default() },
    });
    poll(&mut f.s);
    let d = f.s.snap().design("base").unwrap();
    assert_eq!(d.editing.as_ref().unwrap()["instruction"], "blue");
    // A second edit while the first runs is refused with the reference's message.
    f.s.composer.outbox.push(Outgoing::Edit { design_id: "base".into(), request: VariationRequest::default() });
    poll(&mut f.s);
    assert_eq!(f.s.composer.edit_error.as_deref(), Some("This design is already being edited"));
    poll_until(&mut f.s, "edit to finish", |s| !s.runner.is_busy());
    let d = f.s.snap().design("base").unwrap();
    assert_eq!(d.html, "<p class=blue>");
    assert_eq!(d.rationale, "Made it blue");
    assert!(d.editing.is_none() && d.edit_error.is_none());
    let versions = d.versions.as_ref().unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].html, "<p>base</p>");
    assert_eq!(versions[0].extra["instruction"], "blue");
    assert_eq!(versions[0].extra["viewport"], json!(400.0));

    f.s.composer.outbox.push(Outgoing::Edit { design_id: "nope".into(), request: VariationRequest::default() });
    poll(&mut f.s);
    assert_eq!(f.s.composer.edit_error.as_deref(), Some("No such design"));
}

#[test]
fn queued_edit_can_be_cancelled() {
    let mut f = fixture();
    f.s.composer.outbox.push(Outgoing::Edit {
        design_id: "base".into(),
        request: VariationRequest { instruction: "blue".into(), agent: Some("claude".into()), ..Default::default() },
    });
    poll(&mut f.s);
    assert!(cancel_edit(&mut f.s, "base"));
    poll_until(&mut f.s, "edit to finish", |s| !s.runner.is_busy());
    let d = f.s.snap().design("base").unwrap();
    assert!(d.editing.is_none());
    assert!(d.edit_error.is_none(), "a cancel leaves no error");
    assert_eq!(d.html, "<p>base</p>");
}

#[test]
fn an_in_memory_store_never_writes_the_users_data_dir() {
    // Headless tooling and scenes run on an in-memory store: no data dir, no config override.
    let mut s = AppState::new();
    s.use_store(Store::in_memory());
    assert!(s.store().data_dir().is_none());
    s.runner.config.provider = Some(Arc::new(Scripted { prompts: Mutex::default() }));
    let queue_dir = {
        let host = ensure_host(&mut s).expect("host starts");
        assert!(host.is_scratch());
        host.ai().queue().dir().to_path_buf()
    };
    let user_dir = proto_core::default_data_dir();
    assert!(!queue_dir.starts_with(&user_dir), "{queue_dir:?} is inside the user's data dir {user_dir:?}");
    // A queued generation writes its item into the scratch dir only.
    let space = json!({"id":"sp","name":"S","parentSpaceId":"root","createdAt":1});
    s.apply(vec![Change::set("spaces/sp", space)], "t").unwrap();
    s.composer.outbox.push(Outgoing::Generate {
        space_id: "sp".into(),
        parent_id: None,
        kind: DesignKind::Page,
        label: None,
        request: VariationRequest { instruction: "x".into(), count: 1, agent: Some("claude".into()), ..Default::default() },
        origin: None,
        run_id: None,
    });
    poll(&mut s);
    let scratch = queue_dir.parent().unwrap().to_path_buf();
    for _ in 0..400 {
        if std::fs::read_dir(&queue_dir).map(|d| d.count() > 0).unwrap_or(false) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(std::fs::read_dir(&queue_dir).unwrap().count() > 0, "the item went to the scratch dir");
    drop(s);
    assert!(!scratch.exists(), "the scratch dir is deleted with the host");
}

#[test]
fn reap_cleans_up_an_earlier_process() {
    let mut f = fixture();
    let run = json!({"id":"r1","serverJobId":"gone","spaceId":"sp","kind":"variation","designIds":["g1"]});
    let g1 = json!({"id":"g1","spaceId":"sp","kind":"variation","status":"generating","position":{"x":0,"y":900},"createdAt":1});
    let mut base: Value = serde_json::to_value(&*f.s.snap().design("base").unwrap()).unwrap();
    base["editing"] = json!({"jobId":"old"});
    f.s.apply(vec![Change::set("runs/r1", run), Change::set("designs/g1", g1), Change::set("designs/base", base)], "t").unwrap();
    let c = changes::reap(f.s.snap(), |_| false);
    f.s.apply(c, "reap").unwrap();
    assert!(f.s.snap().run("r1").is_none());
    assert_eq!(f.s.snap().design("g1").unwrap().error.as_deref(), Some("Interrupted"));
    let b = f.s.snap().design("base").unwrap();
    assert!(b.editing.is_none());
    assert_eq!(b.edit_error.as_deref(), Some("Interrupted"));
    assert!(changes::reap(f.s.snap(), |_| false).is_empty(), "idempotent");
}

#[test]
fn stored_request_strips_heavy_fields_and_origin_avoids_overlap() {
    let r: VariationRequest = serde_json::from_value(json!({
        "instruction":"x","count":2,"baseHtml":"<big>",
        "importFrom":{"element":"Nav","source":"/r","screenshot":"data:image/png;base64,AAAA"},
        "components":[{"id":"c","title":"C","html":"<c>"}],
        "bases":[{"id":"b","title":"B","html":"<b>"}]
    }))
    .unwrap();
    let st = changes::stored_request(&r);
    let v = serde_json::to_value(&st).unwrap();
    assert!(v.get("baseHtml").is_none());
    assert_eq!(v["importFrom"], json!({"element":"Nav","source":"/r"}));
    assert_eq!(v["components"][0]["html"], "");
    assert_eq!(v["bases"][0]["html"], "");
    let ai = changes::to_ai_request(&r, "sp").unwrap();
    assert_eq!(ai.space_id.as_deref(), Some("sp"));
    assert_eq!(ai.components.as_ref().unwrap()[0].html, "<c>", "the job itself gets the full request");

    let f = fixture();
    let base = f.s.snap().design("base").unwrap();
    let first = changes::default_origin(f.s.snap(), "sp", Some(&base), 1);
    assert_eq!((first.x, first.y), (0.0, 640.0));
    // Empty space: origin; otherwise right of everything.
    assert_eq!(changes::default_origin(f.s.snap(), "elsewhere", None, 1), proto_core::Point::default());
    let right = changes::default_origin(f.s.snap(), "sp", None, 1);
    assert_eq!((right.x, right.y), (540.0, 0.0));
    let _ = RecordKind::Runs;
}
