//! M3 end to end over HTTP, no LLM: a Claude-agent (queue) generate run, delivery into the placeholders,
//! an in-place edit, and cancels (job, edit, deleted card). The Ai has no provider, so any LLM path would fail.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

struct Env {
    _dir: tempfile::TempDir,
    store: proto_core::Store,
    handle: Option<proto_server::ServerHandle>,
    base: String,
    http: reqwest::Client,
}

impl Env {
    fn start() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let store = proto_core::Store::open(dir.path()).unwrap();
        let ai = proto_ai::Ai::new(dir.path().to_path_buf(), None, Arc::new(proto_ai::NoNotifier));
        // Port 0 (never :3124); the notes service on a dead port so nothing reaches the real one.
        let opts = proto_server::ServeOpts {
            addr: "127.0.0.1:0".parse().unwrap(),
            notes_url: "http://127.0.0.1:9".into(),
            ..Default::default()
        };
        let handle = proto_server::start(store.clone(), ai, opts).unwrap();
        let base = format!("http://{}", handle.addr);
        Env { _dir: dir, store, handle: Some(handle), base, http: reqwest::Client::new() }
    }

    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let res = self.http.post(format!("{}{path}", self.base)).json(&body).send().await.unwrap();
        let status = res.status().as_u16();
        (status, res.json().await.unwrap_or(Value::Null))
    }

    async fn put_state(&self, changes: Value) {
        let (status, body) = self.http_put("/api/state", json!({ "changes": changes, "clientId": "zz-bench" })).await;
        assert_eq!(status, 200, "{body}");
    }

    async fn http_put(&self, path: &str, body: Value) -> (u16, Value) {
        let res = self.http.put(format!("{}{path}", self.base)).json(&body).send().await.unwrap();
        let status = res.status().as_u16();
        (status, res.json().await.unwrap_or(Value::Null))
    }

    async fn delete(&self, path: &str) -> u16 {
        self.http.delete(format!("{}{path}", self.base)).send().await.unwrap().status().as_u16()
    }

    /// POST /api/variations: the job id (header) and a task collecting the NDJSON stream until it ends.
    async fn generate(&self, body: Value) -> (String, tokio::task::JoinHandle<Vec<Value>>) {
        let res = self.http.post(format!("{}/api/variations", self.base)).json(&body).send().await.unwrap();
        assert_eq!(res.status().as_u16(), 200);
        let job = res.headers()["x-job-id"].to_str().unwrap().to_string();
        let lines = tokio::spawn(async move {
            let text = tokio::time::timeout(Duration::from_secs(10), res.text()).await.expect("stream ended").unwrap();
            text.lines().map(|l| serde_json::from_str(l).unwrap()).collect()
        });
        (job, lines)
    }

    async fn queue(&self) -> Vec<Value> {
        let res = self.http.get(format!("{}/api/agent-queue", self.base)).send().await.unwrap();
        res.json::<Value>().await.unwrap()["items"].as_array().cloned().unwrap_or_default()
    }

    /// Wait until the queue lists `n` pending items of `job`.
    async fn queued(&self, job: &str, n: usize) -> Vec<Value> {
        for _ in 0..200 {
            let items: Vec<Value> = self.queue().await.into_iter().filter(|i| i["jobId"] == job).collect();
            if items.len() == n {
                return items;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("job {job}: expected {n} queued items, have {:?}", self.queue().await);
    }

    fn design(&self, id: &str) -> Value {
        let state = self.store.read(0, None);
        let v = serde_json::to_value(&state).unwrap();
        v["records"][format!("designs/{id}")]["value"].clone()
    }

    /// Poll the store until `pred` holds for the card.
    async fn until(&self, id: &str, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..200 {
            let d = self.design(id);
            if pred(&d) {
                return d;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("design {id}: {what}; is {}", self.design(id));
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        drop(self.handle.take());
    }
}

fn placeholder(id: &str, n: u64) -> Value {
    json!({ "id": id, "spaceId": "zz-bench-space", "kind": "variation", "title": format!("Direction {n}"), "rationale": "",
            "html": "", "status": "generating", "position": { "x": 0, "y": 0 }, "createdAt": n })
}

fn design_body(title: &str) -> Value {
    json!({ "title": title, "rationale": "zz-bench delivery", "html": format!("<meta name=\"viewport\" content=\"width=390\"><main><h1>{title}</h1></main>") })
}

/// The app's part of starting a run: the run record pointing at the job, plus its placeholder cards.
async fn place_run(env: &Env, job: &str, ids: &[&str]) {
    let mut changes = vec![
        json!({ "path": "spaces/zz-bench-space", "value": { "id": "zz-bench-space", "name": "zz-bench", "createdAt": 1 } }),
        json!({ "path": format!("runs/zz-bench-{job}"), "value": { "id": format!("zz-bench-{job}"), "serverJobId": job,
            "spaceId": "zz-bench-space", "designIds": ids, "instruction": "zz-bench", "createdAt": 1 } }),
    ];
    for (i, id) in ids.iter().enumerate() {
        changes.push(json!({ "path": format!("designs/{id}"), "value": placeholder(id, i as u64 + 1) }));
    }
    env.put_state(Value::Array(changes)).await;
}

fn types(lines: &[Value]) -> Vec<String> {
    lines.iter().map(|l| l["type"].as_str().unwrap_or("?").to_string()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn queue_generate_deliver_and_edit() {
    let env = Env::start();
    let (job, stream) =
        env.generate(json!({ "instruction": "zz-bench login screen", "count": 2, "agent": "claude", "spaceId": "zz-bench-space" })).await;
    place_run(&env, &job, &["zz-bench-a", "zz-bench-b"]).await;

    let items = env.queued(&job, 2).await;
    let mut placeholders: Vec<&str> = items.iter().map(|i| i["context"]["placeholderId"].as_str().unwrap_or("-")).collect();
    placeholders.sort();
    assert_eq!(placeholders, ["zz-bench-a", "zz-bench-b"], "queue context: {items:?}");

    // Claim: stored on the item, listed, overwritten by a re-claim; validation and unknown items.
    let (s, body) = env.post(&format!("/api/agent-queue/{job}/0/claim"), json!({ "agent": "proto-worker-6" })).await;
    assert_eq!(s, 200, "{body}");
    assert_eq!(body["agent"], "proto-worker-6");
    let (s, _) = env.post(&format!("/api/agent-queue/{job}/0/claim"), json!({ "agent": "prototypie-68" })).await;
    assert_eq!(s, 200);
    let item0 = env.queued(&job, 2).await.into_iter().find(|i| i["index"] == 0).unwrap();
    assert_eq!(item0["agent"], "prototypie-68");
    assert!(item0["claimedAt"].is_string());
    let tasks: Value = env.http.get(format!("{}/api/agent-tasks", env.base)).send().await.unwrap().json().await.unwrap();
    assert!(tasks.to_string().contains("\"agent\":\"prototypie-68\""), "{tasks}");
    assert_eq!(env.post(&format!("/api/agent-queue/{job}/0/claim"), json!({ "agent": "" })).await.0, 400);
    assert_eq!(env.post(&format!("/api/agent-queue/{job}/9/claim"), json!({ "agent": "x" })).await.0, 404);

    // Invalid body → 400 with issues; a non-waiting index → 404.
    let (s, _) = env.post(&format!("/api/agent-queue/{job}/0"), json!({ "title": "" })).await;
    assert_eq!(s, 400);
    let (s, _) = env.post(&format!("/api/agent-queue/{job}/7"), design_body("X")).await;
    assert_eq!(s, 404);

    for (i, id) in ["zz-bench-a", "zz-bench-b"].iter().enumerate() {
        let (s, body) = env.post(&format!("/api/agent-queue/{job}/{i}"), design_body(&format!("Delivered {i}"))).await;
        assert_eq!(s, 200, "{body}");
        let d = env.until(id, "filled", |d| d["status"] == "ready").await;
        assert_eq!(d["title"], format!("Delivered {i}"));
        assert_eq!(d["viewport"], 390, "viewport inferred from the meta tag");
    }
    let lines = stream.await.unwrap();
    let t = types(&lines);
    assert_eq!(t.first().map(String::as_str), Some("job"));
    assert_eq!(t.iter().filter(|x| *x == "variation").count(), 2, "{lines:?}");
    assert_eq!(t.last().map(String::as_str), Some("done"), "{lines:?}");
    let (s, _) = env.post(&format!("/api/agent-queue/{job}/0"), design_body("Again")).await;
    assert_eq!(s, 404, "a delivered item is no longer waiting");
    assert_eq!(env.post(&format!("/api/agent-queue/{job}/0/claim"), json!({ "agent": "x" })).await.0, 409, "done items can't be claimed");

    // Edit in place through the queue.
    let (s, body) = env.post("/api/edit", json!({ "designId": "zz-bench-a", "instruction": "zz-bench darker", "agent": "claude" })).await;
    assert_eq!(s, 200, "{body}");
    let edit_job = body["jobId"].as_str().unwrap().to_string();
    assert_eq!(env.design("zz-bench-a")["editing"]["jobId"], edit_job);
    let (s, _) = env.post("/api/edit", json!({ "designId": "zz-bench-a", "instruction": "again", "agent": "claude" })).await;
    assert_eq!(s, 409, "already being edited");
    let items = env.queued(&edit_job, 1).await;
    assert_eq!(items[0]["context"]["placeholderId"], "zz-bench-a", "{items:?}");
    let (s, body) = env.post(&format!("/api/agent-queue/{edit_job}/0"), design_body("Edited")).await;
    assert_eq!(s, 200, "{body}");
    let d = env.until("zz-bench-a", "edited", |d| d["editing"].is_null() && d["html"].as_str().is_some_and(|h| h.contains("Edited"))).await;
    let versions = d["versions"].as_array().expect("versions");
    assert_eq!(versions.len(), 1);
    assert!(versions[0]["html"].as_str().unwrap().contains("Delivered 0"));
    assert_eq!(versions[0]["instruction"], "zz-bench darker");
    assert_eq!(d["title"], "Delivered 0", "an edit keeps the title");
}

#[tokio::test(flavor = "multi_thread")]
async fn queue_cancels() {
    let env = Env::start();

    // Cancel a whole job: its items leave the queue, the stream ends, a late delivery is refused.
    let (job, stream) = env.generate(json!({ "instruction": "zz-bench cancel me", "count": 1, "agent": "claude", "spaceId": "zz-bench-space" })).await;
    place_run(&env, &job, &["zz-bench-c"]).await;
    env.queued(&job, 1).await;
    assert_eq!(env.delete(&format!("/api/variations/{job}")).await, 204);
    env.queued(&job, 0).await;
    let lines = stream.await.unwrap();
    assert_eq!(types(&lines).last().map(String::as_str), Some("done"), "{lines:?}");
    let (s, _) = env.post(&format!("/api/agent-queue/{job}/0"), design_body("Late")).await;
    assert_eq!(s, 404);
    assert_eq!(env.design("zz-bench-c")["status"], "generating", "nothing placed after a cancel");

    // Deleting the placeholder card cancels its item.
    let (job, stream) = env.generate(json!({ "instruction": "zz-bench delete card", "count": 1, "agent": "claude", "spaceId": "zz-bench-space" })).await;
    place_run(&env, &job, &["zz-bench-d"]).await;
    env.queued(&job, 1).await;
    env.put_state(json!([{ "path": "designs/zz-bench-d", "deleted": true }])).await;
    env.queued(&job, 0).await;
    let (s, _) = env.post(&format!("/api/agent-queue/{job}/0"), design_body("Late")).await;
    assert!(s == 404 || s == 409, "status {s}");
    let lines = stream.await.unwrap();
    assert_eq!(types(&lines).last().map(String::as_str), Some("done"), "{lines:?}");

    // Cancel an edit: busy flag cleared, no error recorded, the item is gone, content unchanged.
    env.put_state(json!([{ "path": "designs/zz-bench-e", "value": {
        "id": "zz-bench-e", "spaceId": "zz-bench-space", "kind": "variation", "title": "E", "rationale": "",
        "html": "<p>original</p>", "status": "ready", "position": { "x": 0, "y": 0 }, "createdAt": 5 } }]))
    .await;
    let (s, body) = env.post("/api/edit", json!({ "designId": "zz-bench-e", "instruction": "zz-bench x", "agent": "claude" })).await;
    assert_eq!(s, 200, "{body}");
    let edit_job = body["jobId"].as_str().unwrap().to_string();
    env.queued(&edit_job, 1).await;
    assert_eq!(env.delete("/api/edit/zz-bench-e").await, 204);
    env.queued(&edit_job, 0).await;
    let (s, _) = env.post(&format!("/api/agent-queue/{edit_job}/0"), design_body("Late edit")).await;
    assert_eq!(s, 404);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let d = env.design("zz-bench-e");
    assert!(d["editing"].is_null() && d["editError"].is_null(), "{d}");
    assert_eq!(d["html"], "<p>original</p>");
    assert_eq!(env.delete("/api/edit/zz-bench-e").await, 404, "no longer being edited");
}
