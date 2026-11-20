//! `NotesService` and its callers: spec-public/04 AN-IF-561…566, spec-public/03 AN-SY-151…156 and the native
//! client's requests (spec-public/05 AN-CLI-160, -618, -619).

use std::net::TcpListener;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{DEAD, Mock, Svc};
use crate::notes::{NewNote, NotesService, ReplyError, Status};

fn new_note() -> NewNote<'static> {
    NewNote { comment: "c", element: "e", element_path: "p" }
}

fn bare_ai(dir: &std::path::Path) -> (proto_core::Store, proto_ai::Ai) {
    let store = proto_core::Store::open(dir).unwrap();
    (store, proto_ai::Ai::new(dir.to_path_buf(), None, Arc::new(proto_ai::NoNotifier)))
}

#[tokio::test(flavor = "multi_thread")]
async fn if561_a_dead_service_is_nothing_within_the_time_limits() {
    let ns = NotesService::new(DEAD, crate::DEFAULT_APP_URL);
    let t0 = Instant::now();
    assert!(!ns.reachable().await);
    assert!(ns.open_notes().await.is_none());
    assert!(ns.locations().await.is_none());
    assert!(ns.notes_at("x").await.is_none());
    assert!(ns.note("x").await.is_none());
    assert!(ns.watchers().await.is_none());
    assert!(ns.create(new_note()).await.is_none());
    ns.close("x", proto_ai::NoteStatus::Resolved).await;
    assert_eq!(ns.reply("x", "agent", "hi").await, Err(ReplyError::Unreachable));
    use proto_ai::Notifier;
    assert!(ns.announce("c".into(), "e".into(), "p".into()).await.is_none());
    ns.set_status("x".into(), proto_ai::NoteStatus::Dismissed).await;
    assert!(t0.elapsed() < Duration::from_secs(5), "{:?}", t0.elapsed());
    let dir = tempfile::tempdir().unwrap();
    let (store, ai) = bare_ai(dir.path());
    let v = crate::tasks::list(&store, &ai, &ns, &crate::tasks::TaskCache::default()).await;
    assert_eq!(v["notesService"], false);
    assert!(v.get("notesWatchers").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn if562_create_close_and_reply_against_the_own_service() {
    let svc = Svc::start().await;
    let ns = svc.service();
    assert_eq!(ns.base(), svc.base, "a trailing / is dropped");
    let id = ns.create(NewNote { comment: "[prototypie job j#0] x", element: "agent-queue item j-0", element_path: "data/agent-queue/j-0.json" }).await.unwrap();
    let id2 = ns.create(NewNote { comment: "second", element: "e", element_path: "p" }).await.unwrap();
    let (_, list) = svc.get("/api/notes/collections").await;
    let list = list["collections"].as_array().unwrap().clone();
    assert_eq!(list.len(), 1, "an existing active collection is used");
    assert_eq!(list[0]["location"], crate::DEFAULT_APP_URL);
    let (_, n) = svc.get(&format!("/api/notes/{id}")).await;
    assert_eq!(n["note"]["kind"], "feedback");
    assert_eq!(n["note"]["location"], crate::DEFAULT_APP_URL);
    assert_eq!(n["note"]["status"], "pending");
    assert_eq!(n["note"]["comment"], "[prototypie job j#0] x", "the comment arrives unchanged");
    assert_eq!(n["note"]["target"]["position"], json!({ "x": 0, "y": 0 }));
    assert!(n["note"]["clientTimestamp"].is_i64());
    use proto_ai::Notifier;
    ns.set_status(id.clone(), proto_ai::NoteStatus::Resolved).await;
    let (_, n) = svc.get(&format!("/api/notes/{id}")).await;
    assert_eq!((n["note"]["status"].as_str(), n["note"]["closedBy"].as_str()), (Some("resolved"), Some("agent")));
    ns.close(&id2, proto_ai::NoteStatus::Dismissed).await;
    assert_eq!(ns.note(&id2).await.unwrap().status, Some(Status::Dismissed));
    let after = ns.reply(&id, "human", "thanks").await.unwrap();
    assert_eq!(after.thread.last().map(|m| (m.role.as_str(), m.content.as_str())), Some(("human", "thanks")));
    assert_eq!(ns.reply("a b/ü", "agent", "x").await, Err(ReplyError::Refused(404)));
    assert_eq!(ns.reply(&id, "robot", "x").await, Err(ReplyError::Refused(400)));
    // A closed app collection is not reused.
    let coll = list[0]["id"].as_str().unwrap();
    svc.req(reqwest::Method::PATCH, &format!("/api/notes/collections/{coll}"), Some(json!({ "state": "closed" }))).await;
    ns.create(new_note()).await.unwrap();
    assert_eq!(svc.get("/api/notes/collections").await.1["collections"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn if562_ids_with_any_characters_arrive_intact() {
    let mock = Mock::start(|_, _, _| (201, json!({ "note": { "id": "a b/ü", "comment": "" } }).to_string()));
    let ns = NotesService::new(mock.url(), crate::DEFAULT_APP_URL);
    let n = ns.reply("a b/ü", "agent", "x").await.unwrap();
    assert_eq!(n.id, "a b/ü");
    ns.close("a b/ü", proto_ai::NoteStatus::Resolved).await;
    ns.note("a b/ü").await;
    let seen = mock.requests();
    assert_eq!(seen[0].1, "/api/notes/a%20b%2F%C3%BC/thread");
    assert_eq!(seen[1].1, "/api/notes/a%20b%2F%C3%BC/status");
    assert_eq!(seen[2].1, "/api/notes/a%20b%2F%C3%BC");
    assert_eq!(serde_json::from_str::<Value>(&seen[1].2).unwrap(), json!({ "status": "resolved", "by": "agent" }));
}

#[tokio::test(flavor = "multi_thread")]
async fn if563_reads_tell_nothing_from_empty() {
    let svc = Svc::start().await;
    let ns = svc.service();
    assert_eq!(ns.open_notes().await, Some(vec![]));
    assert_eq!(ns.locations().await, Some(vec![]));
    assert_eq!(ns.notes_at(crate::DEFAULT_APP_URL).await, Some(vec![]));
    assert_eq!(ns.watchers().await, Some(vec![]));
    assert!(ns.note("nnope000000").await.is_none());
    let a = svc.note_at("prototypie-rune://zz", "a").await;
    let b = svc.note_at("http://localhost:3124/", "b").await;
    svc.post("/api/notes/collections", json!({ "location": "empty-location" })).await;
    svc.post(&format!("/api/notes/{}/thread", b["id"].as_str().unwrap()), json!({ "role": "agent", "content": "on it" })).await;
    assert_eq!(ns.open_notes().await.unwrap().len(), 2);
    svc.status(a["id"].as_str().unwrap(), json!({ "status": "acknowledged" })).await;
    assert_eq!(ns.open_notes().await.unwrap().len(), 1, "an acknowledged note is no longer open");
    let mut locs = ns.locations().await.unwrap();
    locs.sort();
    assert_eq!(locs, ["http://localhost:3124/", "prototypie-rune://zz"], "only locations with notes");
    let at = ns.notes_at("prototypie-rune://zz").await.unwrap();
    assert_eq!(at.len(), 1);
    assert_eq!(at[0].status, Some(Status::Acknowledged));
    let n = ns.note(b["id"].as_str().unwrap()).await.unwrap();
    let re = regex::Regex::new(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$").unwrap();
    assert!(re.is_match(n.created_at.as_deref().unwrap()));
    assert!(re.is_match(n.updated_at.as_deref().unwrap()));
    assert_eq!(n.element.as_deref(), Some(""));
    assert_eq!(n.url.as_deref(), Some("http://localhost:3124/"));
    let m = &n.thread[0];
    assert!(m.id.starts_with('m') && m.role == "agent" && m.content == "on it" && m.at_ms > 0);
    // The watcher register through the handle (AN-CLI-404).
    svc.get("/api/notes/watch?timeout=1&by=zz-lead").await;
    let w = ns.watchers().await.unwrap();
    assert_eq!(w.len(), 1);
    assert_eq!((w[0].name.as_str(), w[0].waiting), ("zz-lead", false));
}

#[tokio::test]
async fn bridge_survives_garbage_refusals_and_empty_answers() {
    // AN-SY-156: garbage, refusals, empty bodies, missing ids → nothing, never a panic.
    for answer in [(200, "not json".to_string()), (500, "{}".to_string()), (200, String::new()), (201, json!({ "note": {} }).to_string())] {
        let a = answer.clone();
        let mock = Mock::start(move |_, _, _| a.clone());
        let ns = NotesService::new(mock.url(), crate::DEFAULT_APP_URL);
        assert!(ns.open_notes().await.is_none(), "{answer:?}");
        assert!(ns.create(new_note()).await.is_none(), "{answer:?}");
        assert!(ns.note("x").await.is_none());
        assert!(!ns.reachable().await);
        ns.close("x", proto_ai::NoteStatus::Resolved).await;
        assert!(ns.reply("x", "agent", "y").await.is_err(), "{answer:?}");
    }
    // create needs exactly one request (AN-CLI-161, -618).
    let mock = Mock::start(|_, _, _| (201, json!({ "note": { "id": "n1" } }).to_string()));
    let ns = NotesService::new(mock.url(), "http://localhost:3124/");
    assert_eq!(ns.create(new_note()).await.as_deref(), Some("n1"));
    let seen = mock.requests();
    assert_eq!(seen.len(), 1);
    assert_eq!((seen[0].0.as_str(), seen[0].1.as_str()), ("POST", "/api/notes"));
    let body: Value = serde_json::from_str(&seen[0].2).unwrap();
    assert_eq!((body["location"].as_str(), body["kind"].as_str()), (Some("http://localhost:3124/"), Some("feedback")));
}

#[tokio::test]
async fn bridge_calls_time_out_and_can_be_dropped() {
    // A service that accepts but never answers: each call ends within its limit; a dropped call leaves nothing.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let mut keep = Vec::new();
        for c in listener.incoming().flatten() {
            keep.push(c);
        }
    });
    let ns = NotesService::new(format!("http://{addr}"), crate::DEFAULT_APP_URL);
    let t0 = Instant::now();
    assert!(ns.open_notes().await.is_none());
    let read = t0.elapsed();
    assert!(read >= Duration::from_millis(2400) && read < Duration::from_millis(3500), "{read:?}");
    let t0 = Instant::now();
    assert!(ns.create(new_note()).await.is_none());
    assert!(t0.elapsed() < Duration::from_millis(3600));
    let t0 = Instant::now();
    assert_eq!(ns.reply("x", "agent", "y").await, Err(ReplyError::Unreachable));
    assert!(t0.elapsed() < Duration::from_millis(4600));
    let dropped = tokio::time::timeout(Duration::from_millis(100), ns.create(new_note())).await;
    assert!(dropped.is_err());
}

fn mock_notes(open: Value) -> Mock {
    Mock::start(move |m, path, _| {
        if path.starts_with("/api/notes/info") {
            return (200, json!({ "service": "prototypie-notes", "watchers": [{ "name": "proto-lead", "waiting": true, "lastSeenAt": "2026-11-20T10:00:00.000Z" }] }).to_string());
        }
        if path.starts_with("/api/notes/collections") {
            return (200, json!({ "collections": [] }).to_string());
        }
        if m == "POST" && path.contains("/thread") {
            return if path.contains("refuse") { (500, "{}".into()) } else { (201, "not json".into()) };
        }
        if path.starts_with("/api/notes?") || path == "/api/notes" {
            return (200, json!({ "notes": open.clone() }).to_string());
        }
        (404, json!({ "error": "Note not found", "code": "note_not_found" }).to_string())
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn if564_the_task_list_shows_only_app_notes() {
    let long = "L".repeat(95);
    let mock = mock_notes(json!([
        { "id": "n-none", "comment": long, "status": "pending", "createdAt": "2026-11-20T10:00:00.000Z" },
        { "id": "n-rune", "comment": "rune", "location": "prototypie-rune://zz/x", "status": "pending" },
        { "id": "n-job", "comment": "[prototypie job j#0] x", "location": "http://localhost:3124/", "status": "pending" },
        { "id": "n-empty", "comment": "", "location": "http://localhost:3124/", "status": "pending" },
    ]));
    let ns = NotesService::new(mock.url(), crate::DEFAULT_APP_URL);
    let dir = tempfile::tempdir().unwrap();
    let (store, ai) = bare_ai(dir.path());
    let v = crate::tasks::list(&store, &ai, &ns, &crate::tasks::TaskCache::default()).await;
    assert_eq!(v["notesService"], true);
    let tasks = v["tasks"].as_array().unwrap();
    let ids: Vec<&str> = tasks.iter().map(|t| t["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"n-none") && ids.contains(&"n-empty"), "{ids:?}");
    assert!(!ids.contains(&"n-rune") && !ids.contains(&"n-job"), "{ids:?}");
    let title = |id: &str| tasks.iter().find(|t| t["id"] == id).unwrap()["title"].clone();
    assert_eq!(title("n-none"), json!(format!("{}…", "L".repeat(88))));
    assert_eq!(title("n-empty"), json!("Notiz"));
    // AN-CLI-404: the watchers come along.
    assert_eq!(v["notesWatchers"], json!([{ "name": "proto-lead", "waiting": true, "lastSeenAt": "2026-11-20T10:00:00.000Z" }]));
}

#[tokio::test(flavor = "multi_thread")]
async fn if564_queue_items_follow_their_note() {
    // A queue job announced through the own service: its note carries the job tag; the task follows the note
    // (acknowledged → working, resolved → resolved), and delivering closes the note (AN-SY-160).
    let svc = Svc::start().await;
    let http = &svc.http;
    let res = http.post(format!("{}/api/variations", svc.base))
        .json(&json!({ "instruction": "zz-cli make it pop", "count": 2, "agent": "claude", "spaceId": "zz-space" }))
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let job = res.headers()["x-job-id"].to_str().unwrap().to_string();
    let _stream = tokio::spawn(async move { res.text().await });
    let changes = json!([
        { "path": "spaces/zz-space", "value": { "id": "zz-space", "name": "zz", "createdAt": 1 } },
        { "path": format!("runs/zz-{job}"), "value": { "id": format!("zz-{job}"), "serverJobId": job, "spaceId": "zz-space", "designIds": ["zz-a", "zz-b"], "instruction": "zz", "createdAt": 1 } },
        { "path": "designs/zz-a", "value": { "id": "zz-a", "spaceId": "zz-space", "kind": "variation", "title": "Direction 1", "rationale": "", "html": "", "status": "generating", "position": { "x": 0, "y": 0 }, "createdAt": 1 } },
        { "path": "designs/zz-b", "value": { "id": "zz-b", "spaceId": "zz-space", "kind": "variation", "title": "Direction 2", "rationale": "", "html": "", "status": "generating", "position": { "x": 0, "y": 0 }, "createdAt": 2 } },
    ]);
    let (s, v) = svc.req(reqwest::Method::PUT, "/api/state", Some(json!({ "changes": changes, "clientId": "zz" }))).await;
    assert_eq!(s, 200, "{v}");
    let mut notes = Vec::new();
    for _ in 0..200 {
        notes = svc.get("/api/notes?tag=job").await.1["notes"].as_array().cloned().unwrap_or_default();
        let queued = svc.get("/api/agent-queue").await.1["items"].as_array().map_or(0, |i| i.iter().filter(|i| i["annotationId"].is_string()).count());
        if notes.len() == 2 && queued == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(notes.len(), 2, "{notes:?}");
    let note_of = |i: u64| notes.iter().find(|n| n["tag"]["index"] == i).unwrap()["id"].as_str().unwrap().to_string();
    let (n0, n1) = (note_of(0), note_of(1));
    assert!(notes[0]["comment"].as_str().unwrap().contains(&format!("prototypie queue show {job}#")));
    let status_of = |v: &Value, i: u64| v["tasks"].as_array().unwrap().iter().find(|t| t["jobId"] == job.as_str() && t["index"] == i).unwrap()["status"].clone();
    let tasks = svc.get("/api/agent-tasks").await.1;
    assert_eq!(status_of(&tasks, 0), "waiting", "{tasks}");
    svc.status(&n0, json!({ "status": "acknowledged" })).await;
    svc.post(&format!("/api/notes/{n1}/thread"), json!({ "role": "agent", "content": "proto-worker-8: on it" })).await;
    let tasks = svc.get("/api/agent-tasks").await.1;
    assert_eq!((status_of(&tasks, 0), status_of(&tasks, 1)), (json!("working"), json!("working")), "{tasks}");
    // Delivering item 0 closes its note (resolved by agent).
    let (s, v) = svc.post(&format!("/api/agent-queue/{job}/0"), json!({ "title": "Bolder", "rationale": "r", "html": "<main>x</main>" })).await;
    assert_eq!(s, 200, "{v}");
    for _ in 0..200 {
        if svc.get(&format!("/api/notes/{n0}")).await.1["note"]["status"] == "resolved" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let n = svc.get(&format!("/api/notes/{n0}")).await.1;
    assert_eq!((n["note"]["status"].as_str(), n["note"]["closedBy"].as_str()), (Some("resolved"), Some("agent")));
    let tasks = svc.get("/api/agent-tasks").await.1;
    assert_eq!(status_of(&tasks, 0), "resolved", "{tasks}");
    svc.status(&n1, json!({ "status": "dismissed" })).await;
    let tasks = svc.get("/api/agent-tasks").await.1;
    assert_eq!(status_of(&tasks, 1), "cancelled", "{tasks}");
}

#[tokio::test(flavor = "multi_thread")]
async fn if565_implement_dedupes_against_the_own_service() {
    let svc = Svc::start().await;
    let body = json!({ "designId": "zzimpl1", "title": "Header", "html": "<main>x</main>" });
    let (s, first) = svc.post("/api/implement", body.clone()).await;
    assert_eq!(s, 200, "{first}");
    let id = first["annotationId"].as_str().unwrap().to_string();
    let (_, n) = svc.get(&format!("/api/notes/{id}")).await;
    assert!(n["note"]["comment"].as_str().unwrap().starts_with("[prototypie implement zzimpl1] Implement the design \"Header\""));
    assert_eq!(n["note"]["tag"]["kind"], "implement");
    let (s, again) = svc.post("/api/implement", body.clone()).await;
    assert_eq!(s, 200);
    assert_eq!((again["duplicate"].as_bool(), again["annotationId"].as_str()), (Some(true), Some(id.as_str())));
    svc.status(&id, json!({ "status": "resolved" })).await;
    let (s, done) = svc.post("/api/implement", body.clone()).await;
    assert_eq!(s, 409, "{done}");
    let mut forced = body.clone();
    forced["force"] = json!(true);
    let (s, v) = svc.post("/api/implement", forced).await;
    assert_eq!(s, 200, "{v}");
    assert_ne!(v["annotationId"], json!(id));
}

#[tokio::test(flavor = "multi_thread")]
async fn if565_implement_without_a_service_says_so() {
    // The server's notes handle on a dead port: 503 with the hint to `prototypie notes`.
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = crate::ServerConfig::new(dir.path().to_path_buf());
    cfg.addr = "127.0.0.1:0".parse().unwrap();
    cfg.notes_url = DEAD.into();
    cfg.codex = false;
    let (store, ai) = bare_ai(dir.path());
    let handle = crate::serve(cfg, store, Some(ai)).await.unwrap();
    let res = reqwest::Client::builder().no_proxy().build().unwrap()
        .post(format!("http://{}/api/implement", handle.addr))
        .json(&json!({ "designId": "zzimpl-dead", "title": "T", "html": "<p>x</p>" }))
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 503);
    let v: Value = res.json().await.unwrap();
    assert!(v["error"].as_str().unwrap().contains("agents read notes with `prototypie notes`"), "{v}");
    handle.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn if566_panel_notes_reach_the_thread_or_say_why_not() {
    let svc = Svc::start().await;
    let id = svc.service().create(NewNote { comment: "Please check the footer", element: "e", element_path: "p" }).await.unwrap();
    let (s, v) = svc.post(&format!("/api/agent-tasks/{id}/notes"), json!({ "text": "  also the header  " })).await;
    assert_eq!(s, 200, "{v}");
    let last = v["thread"].as_array().unwrap().last().unwrap().clone();
    assert_eq!((last["role"].as_str(), last["content"].as_str()), (Some("human"), Some("also the header")));
    let (s, v) = svc.post("/api/agent-tasks/nnope000000/notes", json!({ "text": "x" })).await;
    assert_eq!((s, v["error"].as_str()), (404, Some("Unbekannte Aufgabe")));
    let mock = mock_notes(json!([
        { "id": "n-refuse", "comment": "a", "location": "http://localhost:3124/", "status": "pending" },
        { "id": "n-garbage", "comment": "b", "location": "http://localhost:3124/", "status": "pending" },
    ]));
    let ns = NotesService::new(mock.url(), crate::DEFAULT_APP_URL);
    let dir = tempfile::tempdir().unwrap();
    let (store, ai) = bare_ai(dir.path());
    let cache = crate::tasks::TaskCache::default();
    let e = crate::tasks::add_note(&store, &ai, &ns, &cache, "n-refuse", "x").await.unwrap_err();
    assert_eq!(e, (502, "Notizdienst antwortet 500".to_string()));
    let e = crate::tasks::add_note(&store, &ai, &ns, &cache, "n-garbage", "x").await.unwrap_err();
    assert_eq!(e, (502, "Notizdienst nicht erreichbar".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn project_info_files_its_note_with_the_cli_hint() {
    // The note texts point to the skill and the CLI (spec-public/05, decision 3), never to a script file.
    let svc = Svc::start().await;
    let (s, _) = svc.post("/api/projects/zz-unknown-project", json!({ "spaceId": "zzs1" })).await;
    assert_eq!(s, 404, "unknown project: no note");
    assert_eq!(svc.get("/api/notes").await.1["count"], 0);
    let src = include_str!("../../routes/misc.rs");
    assert!(src.contains("prototypie cards create --space") && !src.contains("scripts/agent-queue.md"));
}

/// The request sequence of the app's sync client (spec-public/05 AN-CLI-160; spec-public/03 AN-SY-191 cases 2, 3,
/// 6, 9, 12) against the own service. The client itself is another implementer's; this checks the service side
/// of each of its steps.
#[tokio::test(flavor = "multi_thread")]
async fn t619_the_native_clients_requests_work() {
    let svc = Arc::new(Svc::start().await);
    let loc = "prototypie-rune://zz";
    // Case 3: a new space makes its collection.
    let (_, found) = svc.get(&format!("/api/notes/collections?location={loc}")).await;
    assert!(found["collections"].as_array().unwrap().iter().all(|c| c["state"] == "closed"));
    let coll = svc.collection(loc).await;
    // Case 2: send into it with the local id; the service id comes back; content arrives unchanged.
    let note = json!({ "comment": "Make it smaller", "element": "button \"Review\"", "elementPath": "header-actions > review-dock",
        "location": "prototypie-rune://zz/review-dock", "kind": "feedback", "clientRef": "mwoo25q8-9vy5fw", "clientTimestamp": 1763632800123u64,
        "designId": "zzd1", "target": { "fullPath": "root > review-dock", "rect": { "x": 1, "y": 2, "width": 3, "height": 4 }, "sourceLocations": "a.rs:1 | b.rs:2" } });
    let (s, sent) = svc.post(&format!("/api/notes/collections/{coll}/notes"), note.clone()).await;
    assert_eq!(s, 201);
    let sid = sent["note"]["id"].as_str().unwrap().to_string();
    // Case 9: a resend with the same local id (slow service) makes no second note.
    let (s, again) = svc.post(&format!("/api/notes/collections/{coll}/notes"), note.clone()).await;
    assert_eq!((s, again["note"]["id"].as_str()), (200, Some(sid.as_str())));
    // Case 12: resolving while a resend is under way: both land, the note stays resolved.
    let (svc2, c2, n2) = (svc.clone(), coll.clone(), note.clone());
    let resend = tokio::spawn(async move { svc2.post(&format!("/api/notes/collections/{c2}/notes"), n2).await });
    let (s, _) = svc.status(&sid, json!({ "status": "resolved", "by": "human" })).await;
    assert_eq!(s, 200);
    assert_eq!(resend.await.unwrap().0, 200);
    // Case 6: refreshing reads status, thread, location, design id, rect, path, source and time.
    svc.post(&format!("/api/notes/{sid}/thread"), json!({ "role": "human", "content": "thanks" })).await;
    let (_, c) = svc.get(&format!("/api/notes/collections/{coll}")).await;
    assert_eq!(c["notes"].as_array().unwrap().len(), 1);
    let n = &c["notes"][0];
    assert_eq!((n["status"].as_str(), n["closedBy"].as_str()), (Some("resolved"), Some("human")));
    assert_eq!(n["thread"][0]["role"], "human");
    assert_eq!(n["location"], "prototypie-rune://zz/review-dock");
    assert_eq!(n["designId"], "zzd1");
    assert_eq!(n["target"]["rect"], json!({ "x": 1, "y": 2, "width": 3, "height": 4 }));
    assert_eq!(n["target"]["fullPath"], "root > review-dock");
    assert_eq!(n["target"]["sourceLocations"].as_str().unwrap().split(" | ").next(), Some("a.rs:1"));
    assert_eq!(n["clientTimestamp"], 1763632800123u64);
    // Another space's collection holds none of them.
    let other = svc.collection("prototypie-rune://elsewhere").await;
    assert_eq!(svc.get(&format!("/api/notes/collections/{other}")).await.1["notes"], json!([]));
}
