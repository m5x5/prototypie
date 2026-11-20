//! The store side of the agent queue (reference agent-queue.ts `submitResult`, `fillPlaceholder`,
//! `placeholderOf`, `cancelForDeletedDesigns`, `listPending` + `withContext`). The file/waiter part is
//! `proto_ai::AgentQueue`.

use proto_ai::{AgentQueue, QueueItem, Variation};
use serde_json::{Map, Value, json};

use crate::context;
use crate::store::{self, Change, SharedStore, StateFile};

/// Outcome of a delivery.
#[derive(Debug, PartialEq)]
pub enum Submit {
    Placed,
    /// Nobody waits for this item (done, cancelled, expired, or another process).
    NotWaiting,
    /// Its placeholder card was deleted: the item is cancelled and nothing is placed.
    Gone,
}

/// The placeholder card an item fills: from the run that drives the job, else the id remembered on disk.
async fn placeholder_of(queue: &AgentQueue, job_id: &str, index: usize, state: &StateFile) -> Option<String> {
    if let Some(id) = context::run_for_job(state, job_id).and_then(|r| r.design_ids.get(index).cloned()) {
        return Some(id);
    }
    queue.load(job_id, index).await?.placeholder_id
}

/// Deliver a finished design: deleted-card check, then resolve the waiter, then fill the placeholder.
pub async fn submit(store: &SharedStore, queue: &AgentQueue, job_id: &str, index: usize, v: Variation) -> Submit {
    if !queue.is_waiting(job_id, index) {
        return Submit::NotWaiting;
    }
    let state = store.read(0, None);
    if let Some(id) = placeholder_of(queue, job_id, index, &state).await
        && store::is_deleted(&state, &format!("designs/{id}"))
    {
        queue.cancel_item(job_id, index, "Card deleted").await;
        return Submit::Gone;
    }
    if !queue.submit(job_id, index, v.clone()).await {
        return Submit::NotWaiting;
    }
    fill_placeholder(store, queue, job_id, index, &v).await;
    Submit::Placed
}

/// Write a delivered result into its placeholder card when it is still generating (or failed without html).
/// Best effort and idempotent: the following browser tab writes the same fields.
async fn fill_placeholder(store: &SharedStore, queue: &AgentQueue, job_id: &str, index: usize, v: &Variation) {
    let state = store.read(0, None);
    let Some(id) = placeholder_of(queue, job_id, index, &state).await else { return };
    let path = format!("designs/{id}");
    let Some(Value::Object(current)) = store::live_value(&state, &path) else { return };
    let status = current.get("status").and_then(Value::as_str).unwrap_or("");
    let has_html = current.get("html").and_then(Value::as_str).is_some_and(|h| !h.is_empty());
    if !(status == "generating" || (status == "error" && !has_html)) {
        return;
    }
    let mut next: Map<String, Value> = current;
    next.insert("title".into(), json!(v.title));
    next.insert("rationale".into(), json!(v.rationale));
    next.insert("html".into(), json!(v.html));
    if let Some(w) = v.viewport {
        next.insert("viewport".into(), json!(w));
    }
    if let Some(h) = v.viewport_height {
        next.insert("viewportHeight".into(), json!(h));
    }
    next.insert("status".into(), json!("ready"));
    next.remove("error");
    let meta = store::meta("server:agent-queue", format!("queue delivery {}", proto_ai::queue::key(job_id, index)));
    let _ = store.apply(vec![Change::set(path, Value::Object(next))], meta);
}

/// Cancel waiting items whose placeholder card is among `design_ids` (deleted in the app).
pub async fn cancel_for_deleted(store: &SharedStore, queue: &AgentQueue, design_ids: &[String]) {
    let waiting = queue.waiting();
    if waiting.is_empty() {
        return;
    }
    let state = store.read(0, None);
    for (job, idx) in waiting {
        if let Some(id) = placeholder_of(queue, &job, idx, &state).await
            && design_ids.contains(&id)
        {
            queue.cancel_item(&job, idx, "Card deleted").await;
        }
    }
}

/// Pending items with their canvas context; remembers each placeholder id on disk.
pub async fn list_pending(store: &SharedStore, queue: &AgentQueue) -> Vec<QueueItem> {
    let mut items = queue.list_pending().await;
    let state = store.read(0, None);
    for it in &mut items {
        it.context = context::queue_context(&state, it);
        let ctx_id = it.context.as_ref().and_then(|c| c.get("placeholderId")).and_then(Value::as_str).map(str::to_string);
        if let Some(id) = ctx_id
            && it.placeholder_id.as_deref() != Some(&id)
        {
            queue.remember_placeholder(&it.job_id, it.index, &id).await;
        }
    }
    items
}

#[cfg(test)]
#[path = "delivery_tests.rs"]
mod tests;
