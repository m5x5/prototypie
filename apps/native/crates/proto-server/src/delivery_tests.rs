use std::sync::Arc;

use proto_core::{ChangeMeta, Store};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::*;

fn variation() -> Variation {
    Variation { title: "T".into(), rationale: "R".into(), html: "<p>x</p>".into(), viewport: Some(400), viewport_height: None }
}

fn setup() -> (tempfile::TempDir, SharedStore, AgentQueue) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::in_memory();
    store
        .apply(
            vec![
                Change::set("spaces/s1", json!({ "id": "s1", "name": "Space", "createdAt": 1 })),
                Change::set("runs/r1", json!({ "serverJobId": "job1", "spaceId": "s1", "parentId": "base", "designIds": ["ph0", "ph1"] })),
                Change::set("designs/ph0", json!({ "id": "ph0", "spaceId": "s1", "kind": "variation", "title": "Direction 1", "rationale": "", "html": "", "status": "generating", "position": { "x": 0, "y": 0 }, "createdAt": 1 })),
                Change::set("designs/ph1", json!({ "id": "ph1", "spaceId": "s1", "kind": "variation", "title": "Direction 2", "rationale": "", "html": "", "status": "generating", "position": { "x": 0, "y": 0 }, "createdAt": 2 })),
            ],
            ChangeMeta::client("test"),
        )
        .unwrap();
    let queue = AgentQueue::new(dir.path(), Arc::new(proto_ai::NoNotifier));
    (dir, store, queue)
}

fn item(index: usize) -> QueueItem {
    serde_json::from_value(json!({ "jobId": "job1", "index": index, "instruction": "do", "direction": { "title": "D", "brief": "b" }, "createdAt": "", "status": "pending" })).unwrap()
}

async fn wait_listed(queue: &AgentQueue, n: usize) {
    for _ in 0..100 {
        if queue.waiting().len() == n {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("items not queued");
}

#[tokio::test]
async fn delivery_fills_placeholder_and_resolves_waiter() {
    let (_d, store, queue) = setup();
    let q = queue.clone();
    let waiter = tokio::spawn(async move { q.enqueue_and_wait(item(0), CancellationToken::new()).await });
    wait_listed(&queue, 1).await;
    let listed = list_pending(&store, &queue).await;
    let ctx = listed[0].context.as_ref().unwrap();
    assert_eq!(ctx["placeholderId"], "ph0");
    assert_eq!(ctx["baseDesignId"], "base");
    assert_eq!(ctx["spacePath"], json!(["Main canvas", "Space"]));
    assert_eq!(submit(&store, &queue, "job1", 0, variation()).await, Submit::Placed);
    assert_eq!(waiter.await.unwrap().unwrap(), variation());
    let card = store::live_value(&store.read(0, None), "designs/ph0").unwrap();
    assert_eq!(card["status"], "ready");
    assert_eq!(card["title"], "T");
    assert_eq!(card["viewport"], 400);
    assert_eq!(submit(&store, &queue, "job1", 0, variation()).await, Submit::NotWaiting);
}

#[tokio::test]
async fn deleted_placeholder_cancels() {
    let (_d, store, queue) = setup();
    let (q0, q1) = (queue.clone(), queue.clone());
    let w0 = tokio::spawn(async move { q0.enqueue_and_wait(item(0), CancellationToken::new()).await });
    let w1 = tokio::spawn(async move { q1.enqueue_and_wait(item(1), CancellationToken::new()).await });
    wait_listed(&queue, 2).await;
    store.apply(vec![Change::delete("designs/ph0")], ChangeMeta::client("test")).unwrap();
    assert_eq!(submit(&store, &queue, "job1", 0, variation()).await, Submit::Gone);
    assert!(w0.await.unwrap().is_err());
    store.apply(vec![Change::delete("designs/ph1")], ChangeMeta::client("test")).unwrap();
    cancel_for_deleted(&store, &queue, &["ph1".to_string()]).await;
    assert!(w1.await.unwrap().is_err());
    assert!(queue.waiting().is_empty());
}
