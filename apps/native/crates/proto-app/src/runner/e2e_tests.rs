//! The app's real runner against the real embedded proto-server (the counterpart of proto-server's
//! tests/agent_queue_e2e.rs): composer outbox → runner → Claude agent queue → delivery over HTTP → cards in the
//! app's store. Temp data dir, port 0, the notes service on a dead port, no LLM call.

use std::time::Duration;

use proto_core::{Change, DesignKind, DesignStatus, Store, VariationRequest};
use serde_json::{Value, json};

use super::*;
use crate::app::state::AppState;

struct Env {
    s: AppState,
    _dir: tempfile::TempDir,
    _server: proto_server::ServerHandle,
    base: String,
    /// For the test's own HTTP calls (the app's runtime belongs to the runner).
    rt: tokio::runtime::Runtime,
    http: reqwest::Client,
}

impl Env {
    fn start() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut s = AppState::new();
        s.use_store(store.clone());
        let space = json!({"id":"sp","name":"zz-bench","parentSpaceId":"root","createdAt":1});
        s.apply(vec![Change::set("spaces/sp", space)], "test").unwrap();
        // The app's one Ai (as AppState::shared_ai does), served on the runner's runtime.
        let host = ensure_host(&mut s).expect("host");
        assert!(!host.is_scratch(), "a real store's data dir is used");
        let opts = proto_server::ServeOpts {
            addr: "127.0.0.1:0".parse().unwrap(),
            notes_url: "http://127.0.0.1:9".into(),
            runtime: Some(host.handle().clone()),
            ..Default::default()
        };
        let server = proto_server::start(store, host.ai().clone(), opts).unwrap();
        let base = format!("http://{}", server.addr);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        Env { s, _dir: dir, _server: server, base, rt, http: reqwest::Client::new() }
    }

    fn get(&self, path: &str) -> Value {
        self.rt.block_on(async { self.http.get(format!("{}{path}", self.base)).send().await.unwrap().json().await.unwrap() })
    }

    fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.rt.block_on(async {
            let res = self.http.post(format!("{}{path}", self.base)).json(&body).send().await.unwrap();
            (res.status().as_u16(), res.json().await.unwrap_or(Value::Null))
        })
    }

    fn poll_until(&mut self, what: &str, mut done: impl FnMut(&AppState) -> bool) {
        for _ in 0..1000 {
            super::poll(&mut self.s, 0.0, &mut rune::Tx::default());
            if done(&self.s) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for {what}");
    }
}

#[test]
fn composer_runner_server_delivery_round_trip() {
    let mut env = Env::start();
    let run_id = new_run_id();
    env.s.composer.outbox.push(Outgoing::Generate {
        space_id: "sp".into(),
        parent_id: None,
        kind: DesignKind::Page,
        label: None,
        request: VariationRequest { instruction: "zz-bench toolbar".into(), count: 2, agent: Some("claude".into()), ..Default::default() },
        origin: None,
        run_id: Some(run_id.clone()),
    });
    env.poll_until("placeholders", |s| s.snap().designs_in("sp").count() == 2);

    // The run record has the shape the server's queue reads: serverJobId + designIds by index.
    let run = env.s.snap().run(&run_id).expect("run record");
    let job = run.server_job_id.clone().expect("serverJobId");
    assert_eq!(run.design_ids.len(), 2);
    assert_eq!(run.space_id, "sp");

    // The server lists both items with the context it derives from that record.
    let mut items = Vec::new();
    for _ in 0..200 {
        items = env.get("/api/agent-queue")["items"].as_array().cloned().unwrap_or_default();
        if items.len() == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(items.len(), 2, "queue: {items:?}");
    for it in &items {
        let index = it["index"].as_u64().unwrap() as usize;
        assert_eq!(it["jobId"], job.as_str());
        assert_eq!(it["context"]["spaceId"], "sp");
        assert_eq!(it["context"]["placeholderId"], run.design_ids[index].as_str(), "placeholder from runs/<id>.designIds");
        assert_eq!(it["context"]["spacePath"], json!(["Main canvas", "zz-bench"]));
    }

    // Deliver both over HTTP, as a Claude Code session does.
    let html = r#"<!doctype html><meta name="viewport" content="width=400,height=800"><p>dock</p>"#;
    let (status, body) = env.post(&format!("/api/agent-queue/{job}/0"), json!({"title":"Bottom dock","rationale":"R","html":html}));
    assert_eq!(status, 200, "{body}");
    let (status, _) = env.post(&format!("/api/agent-queue/{job}/1"), json!({"title":"Side rail","rationale":"R2","html":"<p>rail</p>"}));
    assert_eq!(status, 200);
    let (status, _) = env.post(&format!("/api/agent-queue/{job}/1"), json!({"title":"x","rationale":"","html":"<p>"}));
    assert_eq!(status, 404, "a second delivery finds nobody waiting");

    env.poll_until("run to finish", |s| !s.runner.is_busy());
    let a = env.s.snap().design(&run.design_ids[0]).unwrap();
    assert_eq!((a.title.as_str(), a.status), ("Bottom dock", DesignStatus::Ready));
    assert_eq!((a.viewport, a.viewport_height), (Some(400.0), Some(800.0)), "viewport inferred by the server");
    let b = env.s.snap().design(&run.design_ids[1]).unwrap();
    assert_eq!((b.title.as_str(), b.html.as_str()), ("Side rail", "<p>rail</p>"));
    assert!(env.s.snap().run(&run_id).is_none(), "run record removed");
    let done = env.s.runner.take_finished_where(|f| f.run_id == run_id);
    assert_eq!(done[0].outcome, Outcome::Done);
    assert!(env.get("/api/agent-queue")["items"].as_array().unwrap().is_empty());
}
