//! The notes service over HTTP: spec-public/05 AN-CLI-601…617 and the rows of AN-CLI-101/103/153.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{Svc, assert_err, is_time, raw};
use crate::notes::model;

// ---- collections and notes ---------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn t601_a_new_collection_is_listed_once_active_and_empty() {
    let svc = Svc::start().await;
    let (s, v) = svc.post("/api/notes/collections", json!({ "location": "prototypie-rune://zz" })).await;
    assert_eq!(s, 201, "{v}");
    let (s, list) = svc.get("/api/notes/collections").await;
    assert_eq!(s, 200);
    let list = list["collections"].as_array().unwrap().clone();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["location"], "prototypie-rune://zz");
    assert_eq!(list[0]["state"], "active");
    assert_eq!(list[0]["noteCount"], 0);
    assert_eq!(list[0]["pendingCount"], 0);
    assert!(list[0].get("updatedAt").is_none());
    assert!(is_time(&list[0]["createdAt"]));
    let (s, again) = svc.post("/api/notes/collections", json!({ "location": "prototypie-rune://zz" })).await;
    assert_eq!(s, 200, "reuse answers with the active one");
    assert_eq!(again["collection"]["id"], v["collection"]["id"]);
    let (s, _) = svc.post("/api/notes/collections", json!({ "location": "prototypie-rune://zz", "reuse": false })).await;
    assert_eq!(s, 201);
    let (_, bad) = svc.post("/api/notes/collections", json!({ "location": "a\u{7}b" })).await;
    assert_eq!((bad["code"].as_str(), bad["field"].as_str()), (Some("invalid_field"), Some("location")));
}

#[tokio::test(flavor = "multi_thread")]
async fn t602_required_fields_and_an_unchanged_round_trip() {
    let svc = Svc::start().await;
    let coll = svc.collection("prototypie-rune://zzspace").await;
    let path = format!("/api/notes/collections/{coll}/notes");
    let got = svc.post(&path, json!({ "comment": "x", "element": "e" })).await;
    assert_err(&got, 400, "missing_fields", "comment, element and elementPath are required");
    assert_eq!(svc.post(&path, json!({ "comment": 1, "element": "e", "elementPath": "p" })).await.1["code"], "missing_fields");
    let full = json!({
        "comment": "Make it smaller\n— Rune element `review-dock` at crates/proto-app/src/features/review/dock.rs:33 (features::review::dock)",
        "element": "button \"Review\"",
        "elementPath": "header-actions > review-dock",
        "location": "prototypie-rune://zzspace/review-dock",
        "kind": "feedback",
        "designId": "zzd1",
        "clientRef": "mwoo25q8-9vy5fw",
        "clientTimestamp": 1763632800123u64,
        "status": "pending",
        "unknownField": "dropped",
        "target": {
            "fullPath": "root > app > header-right > header-actions > review-dock",
            "position": { "x": 14.25, "y": 28 },
            "rect": { "x": 100, "y": 10, "width": 80.5, "height": 36 },
            "rects": [{ "x": 1, "y": 2, "width": 3, "height": 4 }],
            "fixed": true,
            "multiSelect": false,
            "sourceLocations": "crates/proto-app/src/features/review/dock.rs:33",
            "componentChain": "Rune: header-actions > review-dock",
            "nearbyText": "Review · New", "nearbyElements": "button, button", "cssClasses": "a b",
            "computedStyles": "color: red", "accessibility": "role=button", "selectedText": "Rev",
            "attributes": { "data-x": "1", "aria-label": "Review" },
            "bogus": 1
        }
    });
    let (s, v) = svc.post(&path, full.clone()).await;
    assert_eq!(s, 201, "{v}");
    let id = v["note"]["id"].as_str().unwrap().to_string();
    let (s, read) = svc.get(&format!("/api/notes/{id}")).await;
    assert_eq!(s, 200);
    let n = &read["note"];
    for k in ["comment", "element", "elementPath", "location", "kind", "designId", "clientRef", "clientTimestamp"] {
        assert_eq!(n[k], full[k], "{k}");
    }
    let mut target = full["target"].clone();
    target.as_object_mut().unwrap().remove("bogus");
    assert_eq!(n["target"], target);
    assert_eq!(n["target"]["position"]["x"].as_f64(), Some(14.25));
    assert_eq!(n["status"], "pending");
    assert_eq!(n["collectionId"], coll.as_str());
    assert!(n.get("updatedAt").is_none() && n.get("unknownField").is_none() && n.get("closedBy").is_none());
    assert_eq!(n["thread"], json!([]));
    assert!(n["seq"].as_u64().unwrap() >= 1);
    assert!(is_time(&n["createdAt"]));
    // Wrong types inside target name the field.
    let mut bad = full.clone();
    bad["clientRef"] = json!("other");
    bad["target"]["rect"] = json!({ "x": "1" });
    let (_, e) = svc.post(&path, bad).await;
    assert_eq!((e["code"].as_str(), e["field"].as_str()), (Some("invalid_field"), Some("target.rect")));
    let (_, e) = svc.post(&path, json!({ "comment": "", "element": "", "elementPath": "", "status": "resolved" })).await;
    assert_eq!(e["error"], "status must be pending for a new note");
    let (_, e) = svc.post(&path, json!({ "comment": "", "element": "", "elementPath": "", "kind": "Bad Kind" })).await;
    assert_eq!(e["field"], "kind");
    let got = svc.post("/api/notes/collections/cnope000000/notes", json!({ "comment": "", "element": "", "elementPath": "" })).await;
    assert_err(&got, 404, "collection_not_found", "Collection not found");
}

#[tokio::test(flavor = "multi_thread")]
async fn t603_the_same_client_ref_makes_one_note() {
    let svc = Svc::start().await;
    let coll = svc.collection("prototypie-rune://zz").await;
    let body = json!({ "comment": "c", "element": "e", "elementPath": "p", "clientRef": "l1" });
    let (s1, a) = svc.post(&format!("/api/notes/collections/{coll}/notes"), body.clone()).await;
    let (s2, b) = svc.post(&format!("/api/notes/collections/{coll}/notes"), body).await;
    assert_eq!((s1, s2), (201, 200));
    assert_eq!(a["note"]["id"], b["note"]["id"]);
    assert_eq!(svc.get(&format!("/api/notes/collections/{coll}")).await.1["notes"].as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn t604_concurrent_posts_make_one_collection() {
    let svc = Arc::new(Svc::start().await);
    let mut tasks = Vec::new();
    for i in 0..20 {
        let svc = svc.clone();
        tasks.push(tokio::spawn(async move { svc.note_at("http://localhost:3124/zz-same", &format!("note {i}")).await }));
    }
    for t in tasks {
        t.await.unwrap();
    }
    let (_, list) = svc.get("/api/notes/collections?location=http://localhost:3124/zz-same").await;
    let list = list["collections"].as_array().unwrap().clone();
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["noteCount"], 20);
    // AN-CLI-121: 20 concurrent collection creations give one id.
    let mut tasks = Vec::new();
    for _ in 0..20 {
        let svc = svc.clone();
        tasks.push(tokio::spawn(async move { svc.collection("prototypie-rune://zz-once").await }));
    }
    let mut ids = std::collections::HashSet::new();
    for t in tasks {
        ids.insert(t.await.unwrap());
    }
    assert_eq!(ids.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn t605_status_cycle_raises_update_time_and_seq() {
    let svc = Svc::start().await;
    let coll = svc.collection("prototypie-rune://zz").await;
    let n = svc.note_in(&coll, "c").await;
    let id = n["id"].as_str().unwrap();
    let first_seq = n["seq"].as_u64().unwrap();
    let mut last = String::new();
    for st in ["acknowledged", "resolved", "pending"] {
        let (s, v) = svc.status(id, json!({ "status": st })).await;
        assert_eq!(s, 200, "{v}");
        let n = &v["note"];
        assert_eq!(n["status"], st);
        let at = n["updatedAt"].as_str().unwrap().to_string();
        assert!(at > last, "{at} after {last}");
        last = at;
        assert_eq!(n.get("closedBy").is_some(), st == "resolved", "{n}");
        assert_eq!(n.get("closedAt").is_some(), st == "resolved", "{n}");
        if st == "resolved" {
            assert_eq!(n["closedBy"], "agent");
        }
        if st == "pending" {
            assert!(n["seq"].as_u64().unwrap() > first_seq);
        }
    }
    // Two quick changes: still strictly rising (same millisecond → +1 ms).
    let (_, a) = svc.status(id, json!({ "status": "acknowledged" })).await;
    let (_, b) = svc.status(id, json!({ "status": "pending" })).await;
    assert!(b["note"]["updatedAt"].as_str().unwrap() > a["note"]["updatedAt"].as_str().unwrap());
    // The same state again changes nothing.
    let (_, c) = svc.status(id, json!({ "status": "pending" })).await;
    assert_eq!(c["note"]["updatedAt"], b["note"]["updatedAt"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn t606_unknown_status_and_conditional_change() {
    let svc = Svc::start().await;
    let coll = svc.collection("prototypie-rune://zz").await;
    let id = svc.note_in(&coll, "c").await["id"].as_str().unwrap().to_string();
    let got = svc.status(&id, json!({ "status": "done" })).await;
    assert_err(&got, 400, "invalid_field", "status must be one of pending, acknowledged, resolved, dismissed");
    svc.status(&id, json!({ "status": "acknowledged" })).await;
    let before = svc.get(&format!("/api/notes/{id}")).await.1;
    let got = svc.status(&id, json!({ "status": "acknowledged", "by": "w2", "ifStatus": "pending" })).await;
    assert_err(&got, 409, "status_conflict", "The note is acknowledged, not pending");
    assert_eq!(got.1["note"], before["note"]);
    assert_eq!(svc.get(&format!("/api/notes/{id}")).await.1, before);
    let (s, _) = svc.status(&id, json!({ "status": "resolved", "ifStatus": ["pending", "acknowledged"] })).await;
    assert_eq!(s, 200);
    let (_, e) = svc.status(&id, json!({ "status": "resolved", "ifStatus": "maybe" })).await;
    assert_eq!(e["field"], "ifStatus");
    let got = svc.status("nnope000000", json!({ "status": "resolved" })).await;
    assert_err(&got, 404, "note_not_found", "Note not found");
}

#[tokio::test(flavor = "multi_thread")]
async fn t607_resolving_with_a_message_adds_it_to_the_thread() {
    let svc = Svc::start().await;
    let coll = svc.collection("prototypie-rune://zz").await;
    let id = svc.note_in(&coll, "c").await["id"].as_str().unwrap().to_string();
    let (_, v) = svc.status(&id, json!({ "status": "resolved", "by": "proto-lead", "message": "proto-lead: done" })).await;
    let n = &v["note"];
    assert_eq!(n["status"], "resolved");
    assert_eq!(n["closedBy"], "proto-lead");
    let last = n["thread"].as_array().unwrap().last().unwrap().clone();
    assert_eq!((last["role"].as_str(), last["content"].as_str()), (Some("agent"), Some("proto-lead: done")));
    let id2 = svc.note_in(&coll, "d").await["id"].as_str().unwrap().to_string();
    let (_, v) = svc.status(&id2, json!({ "status": "dismissed", "by": "human", "message": "not wanted" })).await;
    assert_eq!(v["note"]["thread"][0]["role"], "human");
    assert_eq!(v["note"]["closedBy"], "human");
    // The same state with a message: the message is still added, closedBy stays.
    let (_, w) = svc.status(&id2, json!({ "status": "dismissed", "message": "again" })).await;
    assert_eq!(w["note"]["thread"].as_array().unwrap().len(), 2);
    assert_eq!(w["note"]["closedBy"], "human");
    assert!(w["note"]["updatedAt"].as_str() > v["note"]["updatedAt"].as_str());
}

#[tokio::test(flavor = "multi_thread")]
async fn t608_thread_messages_in_order_with_ids_and_times() {
    let svc = Svc::start().await;
    let coll = svc.collection("prototypie-rune://zz").await;
    let id = svc.note_in(&coll, "c").await["id"].as_str().unwrap().to_string();
    let (s, _) = svc.post(&format!("/api/notes/{id}/thread"), json!({ "role": "agent", "content": "  first  " })).await;
    assert_eq!(s, 201);
    let (_, v) = svc.post(&format!("/api/notes/{id}/thread"), json!({ "role": "human", "content": "second" })).await;
    let t = v["note"]["thread"].as_array().unwrap();
    assert_eq!(t.len(), 2);
    assert_eq!(t[0]["content"], "  first  ", "stored as sent");
    assert_eq!(t[1]["role"], "human");
    for m in t {
        assert!(m["id"].as_str().unwrap().starts_with('m'));
        assert!(is_time(&m["createdAt"]), "{m}");
        assert!(m["createdAtMs"].as_i64().unwrap() > 1_700_000_000_000);
    }
    assert!(is_time(&v["note"]["updatedAt"]));
    let got = svc.post(&format!("/api/notes/{id}/thread"), json!({ "content": "x" })).await;
    assert_err(&got, 400, "missing_thread_fields", "role and content are required");
    assert_eq!(svc.post(&format!("/api/notes/{id}/thread"), json!({ "role": "agent", "content": "   " })).await.1["code"], "missing_thread_fields");
    let got = svc.post(&format!("/api/notes/{id}/thread"), json!({ "role": "bot", "content": "x" })).await;
    assert_err(&got, 400, "invalid_field", "role must be human or agent");
    let got = svc.post("/api/notes/a%20b%2F%C3%BC/thread", json!({ "role": "agent", "content": "x" })).await;
    assert_err(&got, 404, "note_not_found", "Note not found");
    let got = svc.get("/api/notes/a%20b%2F%C3%BC").await;
    assert_err(&got, 404, "note_not_found", "Note not found");
}

#[tokio::test(flavor = "multi_thread")]
async fn t609_queries_and_filters() {
    let svc = Svc::start().await;
    let a = svc.collection("prototypie-rune://zza").await;
    let b = svc.collection("http://localhost:3124/").await;
    let n1 = svc.note_in(&a, "first").await;
    let n2 = svc.note_in(&b, "[prototypie job J1#3] make it pop").await;
    let n3 = svc.note_in(&b, "[prototypie implement d1] Implement").await;
    svc.status(n3["id"].as_str().unwrap(), json!({ "status": "acknowledged" })).await;
    let (_, v) = svc.get("/api/notes").await;
    assert_eq!(v["count"], 2);
    assert_eq!(v["more"], false);
    let seqs: Vec<u64> = v["notes"].as_array().unwrap().iter().map(|n| n["seq"].as_u64().unwrap()).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(v["notes"][0]["id"], n1["id"]);
    assert_eq!(svc.get("/api/notes?status=active").await.1["count"], 3);
    assert_eq!(svc.get("/api/notes?status=all").await.1["count"], 3);
    assert_eq!(svc.get("/api/notes?status=acknowledged").await.1["count"], 1);
    let (_, jobs) = svc.get("/api/notes?tag=job").await;
    assert_eq!(jobs["count"], 1);
    assert_eq!(jobs["notes"][0]["id"], n2["id"]);
    assert_eq!(jobs["notes"][0]["tag"], json!({ "kind": "job", "jobId": "J1", "index": 3 }));
    assert_eq!(svc.get("/api/notes?tag=none").await.1["count"], 1);
    assert_eq!(svc.get(&format!("/api/notes?collection={a}")).await.1["count"], 1);
    assert_eq!(svc.get("/api/notes?locationPrefix=prototypie-rune://").await.1["count"], 1);
    assert_eq!(svc.get("/api/notes?status=all&location=http://localhost:3124/").await.1["count"], 2);
    let (_, lim) = svc.get("/api/notes?status=all&limit=1").await;
    assert_eq!((lim["count"].as_u64(), lim["more"].as_bool()), (Some(1), Some(true)));
    let got = svc.get("/api/notes?status=done").await;
    assert_eq!((got.0, got.1["code"].as_str()), (400, Some("invalid_query")));
    assert_eq!(svc.get("/api/notes?limit=0").await.1["code"], "invalid_query");
    assert_eq!(svc.get("/api/notes?tag=x").await.1["code"], "invalid_query");
    // AN-CLI-120: two collections of one location, the first closed → only the second is active.
    let c1 = svc.collection("prototypie-rune://twice").await;
    let (s, v) = svc.req(reqwest::Method::PATCH, &format!("/api/notes/collections/{c1}"), Some(json!({ "state": "closed" }))).await;
    assert_eq!(s, 200);
    assert_eq!(v["collection"]["state"], "closed");
    assert!(is_time(&v["collection"]["updatedAt"]));
    let c2 = svc.collection("prototypie-rune://twice").await;
    assert_ne!(c1, c2);
    let (_, list) = svc.get("/api/notes/collections?location=prototypie-rune://twice&state=active").await;
    assert_eq!(list["collections"].as_array().unwrap().len(), 1);
    assert_eq!(list["collections"][0]["id"], c2.as_str());
    let (_, all) = svc.get("/api/notes/collections?location=prototypie-rune://twice").await;
    assert_eq!(all["collections"][0]["id"], c1.as_str(), "oldest first");
    assert_eq!(svc.get("/api/notes/collections?state=open").await.1["code"], "invalid_query");
    let got = svc.get("/api/notes/collections/cnope000000").await;
    assert_err(&got, 404, "collection_not_found", "Collection not found");
    let (_, one) = svc.get(&format!("/api/notes/collections/{b}")).await;
    assert_eq!(one["notes"].as_array().unwrap().len(), 2);
    assert_eq!(one["collection"]["pendingCount"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn info_of_an_empty_service() {
    // AN-CLI-133
    let svc = Svc::start().await;
    let (s, v) = svc.get("/api/notes/info").await;
    assert_eq!(s, 200);
    assert_eq!(v["service"], "prototypie-notes");
    assert_eq!(v["apiVersion"], 1);
    assert_eq!(v["appVersion"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["cursor"], 0);
    assert_eq!(v["counts"], json!({ "pending": 0, "acknowledged": 0, "resolved": 0, "dismissed": 0 }));
    assert_eq!(v["collections"], 0);
    assert_eq!(v["watchers"], json!([]));
    assert_eq!(v["pid"], std::process::id());
    assert!(svc.service().reachable().await);
}

// ---- watching ----------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn t610_the_batch_window_collects_two_notes() {
    let svc = Arc::new(Svc::start().await);
    let coll = svc.collection("prototypie-rune://zz").await;
    let t0 = Instant::now();
    let watcher = {
        let svc = svc.clone();
        tokio::spawn(async move { svc.get("/api/notes/watch?batchWindow=10&timeout=60&by=zz-w").await })
    };
    tokio::time::sleep_until(tokio::time::Instant::from_std(t0 + Duration::from_secs(2))).await;
    let a = svc.note_in(&coll, "a").await;
    tokio::time::sleep_until(tokio::time::Instant::from_std(t0 + Duration::from_secs(5))).await;
    let b = svc.note_in(&coll, "b").await;
    let (s, v) = watcher.await.unwrap();
    let took = t0.elapsed().as_secs_f64();
    assert_eq!(s, 200, "{v}");
    assert!((12.0..13.0).contains(&took), "took {took}");
    assert_eq!(v["count"], 2);
    assert_eq!(v["timedOut"], false);
    let cursor = v["cursor"].as_u64().unwrap();
    assert!(cursor >= a["seq"].as_u64().unwrap() && cursor >= b["seq"].as_u64().unwrap());
    let t1 = Instant::now();
    let (_, v) = svc.get(&format!("/api/notes/watch?after={cursor}&timeout=2")).await;
    let took = t1.elapsed().as_secs_f64();
    assert!((1.9..2.6).contains(&took), "took {took}");
    assert_eq!((v["count"].as_u64(), v["timedOut"].as_bool()), (Some(0), Some(true)));
    // The watcher stays listed in the info (AN-CLI-142).
    let (_, info) = svc.get("/api/notes/info").await;
    assert!(info["watchers"].as_array().unwrap().iter().any(|w| w["name"] == "zz-w" && w["waiting"] == false && is_time(&w["lastSeenAt"])), "{info}");
}

#[tokio::test(flavor = "multi_thread")]
async fn t611_waiting_notes_come_at_once() {
    let svc = Svc::start().await;
    let coll = svc.collection("prototypie-rune://zz").await;
    svc.note_in(&coll, "a").await;
    svc.note_in(&coll, "b").await;
    let t0 = Instant::now();
    let (_, v) = svc.get("/api/notes/watch?after=0").await;
    assert!(t0.elapsed() < Duration::from_millis(200), "{:?}", t0.elapsed());
    assert_eq!(v["count"], 2);
    // A note made while nobody waits is not lost (AN-CLI-140).
    let cursor = v["cursor"].as_u64().unwrap();
    let c = svc.note_in(&coll, "c").await;
    let (_, v) = svc.get(&format!("/api/notes/watch?after={cursor}")).await;
    assert_eq!(v["count"], 1);
    assert_eq!(v["notes"][0]["id"], c["id"]);
    // `after` past the highest number counts as 0.
    assert_eq!(svc.get("/api/notes/watch?after=999999").await.1["count"], 3);
    let got = svc.get("/api/notes/watch?timeout=301").await;
    assert_err(&got, 400, "invalid_query", "timeout must be a whole number of seconds from 1 to 300");
    assert_eq!(svc.get("/api/notes/watch?batchWindow=61").await.1["code"], "invalid_query");
    assert_eq!(svc.get("/api/notes/watch?after=-1").await.1["code"], "invalid_query");
}

#[tokio::test(flavor = "multi_thread")]
async fn t612_a_reopened_note_reaches_a_waiting_agent() {
    let svc = Arc::new(Svc::start().await);
    let coll = svc.collection("prototypie-rune://zz").await;
    let id = svc.note_in(&coll, "a").await["id"].as_str().unwrap().to_string();
    svc.status(&id, json!({ "status": "resolved" })).await;
    let cursor = svc.get("/api/notes/info").await.1["cursor"].as_u64().unwrap();
    let watcher = {
        let svc = svc.clone();
        tokio::spawn(async move { svc.get(&format!("/api/notes/watch?after={cursor}&batchWindow=0&timeout=20")).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    svc.status(&id, json!({ "status": "pending", "by": "human" })).await;
    let (_, v) = tokio::time::timeout(Duration::from_secs(5), watcher).await.unwrap().unwrap();
    assert_eq!(v["count"], 1);
    assert_eq!(v["notes"][0]["id"], id.as_str());
}

#[tokio::test(flavor = "multi_thread")]
async fn replies_wake_only_watchers_that_ask_for_them() {
    // Decision 4 of spec-public/05: `watch --replies` wakes on a person's reply to an open note.
    let svc = Arc::new(Svc::start().await);
    let coll = svc.collection("prototypie-rune://zz").await;
    let id = svc.note_in(&coll, "task").await["id"].as_str().unwrap().to_string();
    svc.status(&id, json!({ "status": "acknowledged" })).await;
    let cursor = svc.get("/api/notes/info").await.1["cursor"].as_u64().unwrap();
    let with = {
        let svc = svc.clone();
        tokio::spawn(async move { svc.get(&format!("/api/notes/watch?after={cursor}&batchWindow=0&timeout=4&replies=1")).await })
    };
    let without = {
        let svc = svc.clone();
        tokio::spawn(async move { svc.get(&format!("/api/notes/watch?after={cursor}&batchWindow=0&timeout=3")).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    svc.post(&format!("/api/notes/{id}/thread"), json!({ "role": "agent", "content": "on it" })).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!with.is_finished(), "an agent's own message wakes nobody");
    svc.post(&format!("/api/notes/{id}/thread"), json!({ "role": "human", "content": "also the footer" })).await;
    let (_, v) = tokio::time::timeout(Duration::from_secs(3), with).await.unwrap().unwrap();
    assert_eq!(v["count"], 1);
    assert_eq!(v["replies"], json!([id]));
    assert_eq!(v["notes"][0]["status"], "acknowledged");
    let (_, w) = without.await.unwrap();
    assert_eq!(w["timedOut"], true);
    let c = v["cursor"].as_u64().unwrap();
    let (_, again) = svc.get(&format!("/api/notes/watch?after={c}&timeout=1&replies=1")).await;
    assert_eq!(again["count"], 0, "the same reply is not reported twice");
}

#[tokio::test(flavor = "multi_thread")]
async fn t613_watcher_limit_and_shutdown() {
    let mut svc = Svc::start().await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut waits = Vec::new();
    for i in 0..32 {
        let (c, url) = (client.clone(), format!("{}/api/notes/watch?timeout=60&by=w{i}", svc.base));
        waits.push(tokio::spawn(async move { c.get(url).send().await.map(|r| r.status().as_u16()) }));
    }
    for _ in 0..100 {
        let (_, info) = svc.get("/api/notes/info").await;
        if info["watchers"].as_array().unwrap().iter().filter(|w| w["waiting"] == true).count() == 32 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let got = svc.get("/api/notes/watch?timeout=5").await;
    assert_err(&got, 429, "too_many_watchers", "Too many agents are waiting at once (32)");
    let t0 = Instant::now();
    svc.handle.take().unwrap().shutdown().await;
    let took = t0.elapsed();
    for w in waits {
        let status = tokio::time::timeout(Duration::from_secs(1), w).await.expect("ended").unwrap();
        assert_eq!(status.ok(), Some(503));
    }
    assert!(took < Duration::from_secs(1), "shutdown took {took:?}");
}

// ---- storage -----------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn t614_notes_survive_a_restart() {
    let svc = Svc::start().await;
    let coll = svc.collection("prototypie-rune://zz").await;
    let a = svc.note_in(&coll, "a").await;
    let b = svc.note_in(&coll, "b").await;
    svc.status(a["id"].as_str().unwrap(), json!({ "status": "resolved", "message": "done" })).await;
    svc.post(&format!("/api/notes/{}/thread", b["id"].as_str().unwrap()), json!({ "role": "human", "content": "hi" })).await;
    let before = svc.get("/api/notes?status=all").await.1;
    let cursor = svc.get("/api/notes/info").await.1["cursor"].as_u64().unwrap();
    // AN-CLI-150: one file per collection, holding the note with its seq.
    let notes_dir = svc.dir.path().join("notes");
    let files: Vec<String> = std::fs::read_dir(&notes_dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(files, [format!("{coll}.json")]);
    let file: Value = serde_json::from_slice(&std::fs::read(notes_dir.join(&files[0])).unwrap()).unwrap();
    assert_eq!(file["format"], 1);
    assert_eq!(file["collection"]["id"], coll.as_str());
    assert!(file["notes"][0]["seq"].is_u64() && file["notes"][0].get("collectionId").is_none() && file["notes"][0].get("tag").is_none());
    let dir = svc.stop().await;
    let svc = Svc::start_in(dir).await;
    assert_eq!(svc.get("/api/notes?status=all").await.1, before);
    let c = svc.note_in(&coll, "c").await;
    assert!(c["seq"].as_u64().unwrap() > cursor);
}

#[tokio::test(flavor = "multi_thread")]
async fn t615_odd_files_do_not_stop_the_service() {
    let dir = tempfile::tempdir().unwrap();
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(notes.join("cbroken0000.json"), "{ not json").unwrap();
    std::fs::write(notes.join("x.json"), "{}").unwrap();
    std::fs::write(notes.join("cleft000000.json.tmp"), "half").unwrap();
    let future = json!({ "format": 2, "collection": { "id": "cfuture0000", "location": "l", "state": "active", "createdAt": "2026-11-20T10:00:00.000Z" }, "notes": [] });
    std::fs::write(notes.join("cfuture0000.json"), future.to_string()).unwrap();
    let good = json!({ "format": 1, "collection": { "id": "cgood000000", "location": "l", "state": "active", "createdAt": "2026-11-20T10:00:00.000Z" }, "notes": [] });
    std::fs::write(dir.path().join("elsewhere.json"), good.to_string()).unwrap();
    // AN-CLI-505: a symlink with a good name is not loaded.
    std::os::unix::fs::symlink(dir.path().join("elsewhere.json"), notes.join("cgood000000.json")).unwrap();
    let svc = Svc::start_in(dir).await;
    let names: Vec<String> = std::fs::read_dir(&notes).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(names.iter().any(|n| n.starts_with("cbroken0000.json.corrupt-")), "{names:?}");
    assert!(!names.iter().any(|n| n == "cbroken0000.json"));
    assert!(!names.iter().any(|n| n.ends_with(".tmp")), "{names:?}");
    assert_eq!(std::fs::read_to_string(notes.join("x.json")).unwrap(), "{}");
    assert_eq!(std::fs::read_to_string(notes.join("cfuture0000.json")).unwrap(), future.to_string());
    assert_eq!(svc.get("/api/notes/info").await.1["collections"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn t616_a_failed_write_keeps_the_old_state() {
    use std::os::unix::fs::PermissionsExt;
    let svc = Svc::start().await;
    let coll = svc.collection("prototypie-rune://zz").await;
    let n = svc.note_in(&coll, "a").await;
    let before = svc.get("/api/notes?status=all").await.1;
    let notes = svc.dir.path().join("notes");
    std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o555)).unwrap();
    let got = svc.status(n["id"].as_str().unwrap(), json!({ "status": "resolved" })).await;
    let got2 = svc.post(&format!("/api/notes/collections/{coll}/notes"), json!({ "comment": "b", "element": "", "elementPath": "" })).await;
    std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(got.0, 500, "{}", got.1);
    assert_eq!(got.1["code"], "storage_failed");
    assert!(got.1["error"].as_str().unwrap().starts_with("Couldn't save the notes: "));
    assert_eq!(got2.0, 500);
    assert_eq!(svc.get("/api/notes?status=all").await.1, before);
    let c = svc.note_in(&coll, "c").await;
    assert_eq!(c["seq"].as_u64().unwrap(), n["seq"].as_u64().unwrap() + 1, "nothing half-committed");
}

#[tokio::test(flavor = "multi_thread")]
async fn limits_of_the_service() {
    // AN-CLI-153: the 5001st note of a collection and the 501st message (filled in memory, then one request).
    let svc = Svc::start().await;
    let coll = svc.collection("prototypie-rune://full").await;
    let n = svc.note_in(&coll, "seed").await;
    let id = n["id"].as_str().unwrap().to_string();
    {
        let store = svc.handle.as_ref().unwrap().state.notes_store.clone();
        let mut d = store.0.data_for_test();
        let c = d.colls.get_mut(&coll).unwrap();
        let seed = c.notes[0].clone();
        for i in 1..model::MAX_NOTES_PER_COLLECTION {
            let mut x = seed.clone();
            x.id = format!("nfill{i:06}");
            c.notes.push(x);
        }
        let m = model::StoredMessage { id: "m".into(), role: "agent".into(), content: "x".into(), created_at: crate::util::iso_now(), created_at_ms: 0, seq: None };
        c.notes[0].thread = vec![m; model::MAX_MESSAGES_PER_NOTE];
    }
    let got = svc.post(&format!("/api/notes/collections/{coll}/notes"), json!({ "comment": "x", "element": "", "elementPath": "" })).await;
    assert_err(&got, 409, "limit_reached", "This collection is full (5000 notes)");
    let got = svc.post(&format!("/api/notes/{id}/thread"), json!({ "role": "agent", "content": "x" })).await;
    assert_err(&got, 409, "limit_reached", "This note's thread is full (500 messages)");
    let got = svc.status(&id, json!({ "status": "resolved", "message": "x".repeat(model::MAX_MESSAGE + 1) })).await;
    assert_eq!((got.0, got.1["field"].as_str()), (400, Some("message")));
    let coll2 = svc.collection("prototypie-rune://other").await;
    let p2 = format!("/api/notes/collections/{coll2}/notes");
    let got = svc.post(&p2, json!({ "comment": "x".repeat(model::MAX_COMMENT + 1), "element": "", "elementPath": "" })).await;
    assert_eq!(got.1["field"], "comment");
    let rects = vec![json!({ "x": 0, "y": 0, "width": 1, "height": 1 }); 201];
    let got = svc.post(&p2, json!({ "comment": "", "element": "", "elementPath": "", "target": { "rects": rects } })).await;
    assert_eq!(got.1["field"], "target.rects");
    let attrs: serde_json::Map<String, Value> = (0..201).map(|i| (format!("a{i}"), json!("v"))).collect();
    let got = svc.post(&p2, json!({ "comment": "", "element": "", "elementPath": "", "target": { "attributes": attrs } })).await;
    assert_eq!(got.1["field"], "target.attributes");
    let got = svc.post("/api/notes/collections", json!({ "location": "l".repeat(2049) })).await;
    assert_eq!(got.1["field"], "location");
}

// ---- HTTP rules --------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn t617_size_type_and_origin_guards() {
    let svc = Svc::start().await;
    let host = format!("127.0.0.1:{}", svc.addr.port());
    let post = |extra: &str, len: usize| format!("POST /api/notes/collections HTTP/1.1\r\nHost: {host}\r\n{extra}Content-Length: {len}\r\nConnection: close\r\n\r\n");
    let big = vec![b' '; 1024 * 1024 + 512 * 1024];
    let got = raw(svc.addr, &post("Content-Type: application/json\r\n", big.len()), &big);
    assert_err(&got, 413, "too_large", "The request body is larger than 1 MiB");
    let body = br#"{"location":"x"}"#;
    let got = raw(svc.addr, &post("", body.len()), body);
    assert_err(&got, 415, "json_required", "Send the body as application/json");
    let got = raw(svc.addr, &post("Content-Type: application/json; charset=utf-8\r\n", 3), b"[1]");
    assert_err(&got, 400, "invalid_json", "The request body must be a JSON object");
    let got = raw(svc.addr, &post("Origin: http://127.0.0.1:3124\r\nContent-Type: application/json\r\n", body.len()), body);
    assert_err(&got, 403, "browser_request", "The notes service does not accept requests from web pages");
    let got = raw(svc.addr, &format!("GET /api/notes HTTP/1.1\r\nHost: {host}\r\nSec-Fetch-Mode: cors\r\nConnection: close\r\n\r\n"), b"");
    assert_eq!(got.1["code"], "browser_request");
    let got = raw(svc.addr, "GET /api/notes HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n", b"");
    assert_err(&got, 403, "not_local", "The notes service only accepts requests from this computer");
    let got = raw(svc.addr, &format!("GET /api/notes HTTP/1.1\r\nHost: localhost:{}\r\nConnection: close\r\n\r\n", svc.addr.port()), b"");
    assert_eq!(got.0, 200);
    let got = raw(svc.addr, &post("Content-Type: application/json\r\n", body.len()), body);
    assert_eq!(got.0, 201, "without those headers the same POST is taken");
    let res = svc.http.get(format!("{}/api/notes/info", svc.base)).send().await.unwrap();
    assert_eq!(res.headers()["cache-control"], "no-store");
    assert!(res.headers()["content-type"].to_str().unwrap().starts_with("application/json"));
    assert!(res.headers().get("access-control-allow-origin").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn route_table_and_wrong_methods() {
    // AN-CLI-101: one request per row; other methods → 405.
    let svc = Svc::start().await;
    let coll = svc.collection("prototypie-rune://zz").await;
    let id = svc.note_in(&coll, "a").await["id"].as_str().unwrap().to_string();
    use reqwest::Method as M;
    let rows: Vec<(M, String, Option<Value>, u16)> = vec![
        (M::GET, "/api/notes/info".into(), None, 200),
        (M::GET, "/api/notes/collections".into(), None, 200),
        (M::POST, "/api/notes/collections".into(), Some(json!({ "location": "x" })), 201),
        (M::GET, format!("/api/notes/collections/{coll}"), None, 200),
        (M::PATCH, format!("/api/notes/collections/{coll}"), Some(json!({ "state": "active" })), 200),
        (M::POST, format!("/api/notes/collections/{coll}/notes"), Some(json!({ "comment": "", "element": "", "elementPath": "" })), 201),
        (M::POST, "/api/notes".into(), Some(json!({ "location": "y", "comment": "", "element": "", "elementPath": "" })), 201),
        (M::GET, "/api/notes".into(), None, 200),
        (M::GET, "/api/notes/watch?timeout=1".into(), None, 200),
        (M::GET, format!("/api/notes/{id}"), None, 200),
        (M::POST, format!("/api/notes/{id}/status"), Some(json!({ "status": "acknowledged" })), 200),
        (M::POST, format!("/api/notes/{id}/thread"), Some(json!({ "role": "agent", "content": "x" })), 201),
    ];
    for (m, path, body, want) in rows {
        let (s, v) = svc.req(m.clone(), &path, body).await;
        assert_eq!(s, want, "{m} {path}: {v}");
    }
    for (m, path) in [
        (M::DELETE, "/api/notes/x".to_string()),
        (M::OPTIONS, "/api/notes".to_string()),
        (M::PUT, "/api/notes/info".to_string()),
        (M::POST, "/api/notes/watch".to_string()),
        (M::DELETE, format!("/api/notes/collections/{coll}")),
    ] {
        let got = svc.req(m.clone(), &path, None).await;
        assert_err(&got, 405, "method_not_allowed", "Method not allowed");
    }
    let (_, e) = svc.post("/api/notes", json!({ "comment": "", "element": "", "elementPath": "" })).await;
    assert_eq!(e["field"], "location");
}

#[tokio::test(flavor = "multi_thread")]
async fn watch_during_shutdown_and_after() {
    // AN-CLI-103 row 503: a watch arriving after shutdown began is refused at once.
    let svc = Svc::start().await;
    svc.handle.as_ref().unwrap().state.notes_store.shut_down();
    let got = svc.get("/api/notes/watch?timeout=5").await;
    assert_err(&got, 503, "shutting_down", "Prototypie is quitting");
}
