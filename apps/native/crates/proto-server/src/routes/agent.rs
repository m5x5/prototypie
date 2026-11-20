//! `/api/agent-queue/**` and `/api/agent/**` (reference agent-queue + agent-canvas routes).

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{Value, json};

use crate::AppState;
use crate::canvas;
use crate::delivery::{self, Submit};
use proto_ai::Variation;
use crate::util::{self, Check, p};

/// GET /api/agent-queue → `{ items }`.
pub async fn queue_list(State(app): State<AppState>) -> Response {
    util::ok(json!({ "items": delivery::list_pending(&app.store, app.ai.queue()).await }))
}

const GONE: &str = "The placeholder card for this item was deleted in the app, so the item is cancelled and nothing was placed. If the design is still wanted, create a card with POST /api/agent/cards.";

/// Validate a delivery body `{ title, rationale, html, viewport?, viewportHeight? }`.
pub fn parse_result(v: &Value) -> Result<Variation, Vec<util::Issue>> {
    let mut c = Check::default();
    if !v.is_object() {
        c.push(&[], "invalid_type", "Expected object, received null");
        return Err(c.issues);
    }
    let title = c.string(&v["title"], &p(&[&"title"]), false, 1, usize::MAX);
    let rationale = c.string(&v["rationale"], &p(&[&"rationale"]), false, 0, usize::MAX);
    let html = c.string(&v["html"], &p(&[&"html"]), false, 1, usize::MAX);
    let viewport = c.opt_int(&v["viewport"], &p(&[&"viewport"]), 200, 4000);
    let viewport_height = c.opt_int(&v["viewportHeight"], &p(&[&"viewportHeight"]), 200, 4000);
    match (title, rationale, html) {
        (Some(title), Some(rationale), Some(html)) if c.ok() => Ok(Variation {
            title,
            rationale,
            html,
            viewport: viewport.map(|v| v as u32),
            viewport_height: viewport_height.map(|v| v as u32),
        }),
        _ => Err(c.issues),
    }
}

/// POST /api/agent-queue/:jobId/:index → 200 placed · 404 nobody waits · 409 placeholder deleted · 400 invalid.
pub async fn queue_deliver(State(app): State<AppState>, Path((job_id, index)): Path<(String, String)>, body: Bytes) -> Response {
    let mut v = match parse_result(&util::parse_body(&body)) {
        Ok(v) => v,
        Err(issues) => return util::issues_error(issues),
    };
    v.viewport = v.viewport.or_else(|| proto_ai::infer_viewport(&v.html));
    v.viewport_height = v.viewport_height.or_else(|| proto_ai::infer_viewport_height(&v.html));
    // Number(index) in the reference: a non-number never matches a waiting item.
    let Ok(index) = index.parse::<usize>() else { return not_waiting() };
    match delivery::submit(&app.store, app.ai.queue(), &job_id, index, v).await {
        Submit::Placed => util::ok(json!({ "ok": true })),
        Submit::Gone => util::error(StatusCode::CONFLICT, GONE),
        Submit::NotWaiting => not_waiting(),
    }
}

/// POST /api/agent-queue/:jobId/:index/claim { agent } → `{ ok, agent, claimedAt }` · 404 unknown item · 409 not pending.
/// Who works on the item (worker or session name); re-claiming overwrites. Claim before working on it.
pub async fn queue_claim(State(app): State<AppState>, Path((job_id, index)): Path<(String, String)>, body: Bytes) -> Response {
    let body = util::parse_body(&body);
    let mut c = Check::default();
    let agent = c.string(&body["agent"], &p(&[&"agent"]), true, 1, 100);
    let Some(agent) = agent.filter(|_| c.ok()) else { return util::issues_error(c.issues) };
    let Ok(index) = index.parse::<usize>() else { return util::error(StatusCode::NOT_FOUND, "No such queue item") };
    let queue = app.ai.queue();
    match queue.load(&job_id, index).await {
        None => return util::error(StatusCode::NOT_FOUND, "No such queue item"),
        Some(it) if it.status != proto_ai::QueueStatus::Pending => {
            return util::error(StatusCode::CONFLICT, "This item is no longer pending (done or cancelled)");
        }
        Some(_) => {}
    }
    match queue.claim(&job_id, index, &agent).await {
        Some(it) => util::ok(json!({ "ok": true, "agent": it.agent, "claimedAt": it.claimed_at })),
        None => util::error(StatusCode::INTERNAL_SERVER_ERROR, "Couldn't save the claim"),
    }
}

fn not_waiting() -> Response {
    util::error(StatusCode::NOT_FOUND, "No job is waiting for this item (done, cancelled or expired)")
}

/// GET /api/agent/canvas
pub async fn canvas(State(app): State<AppState>) -> Response {
    util::ok(canvas::describe(&app.store))
}

/// POST /api/agent/cards
pub async fn cards(State(app): State<AppState>, body: Bytes) -> Response {
    match canvas::parse_cards_body(&util::parse_body(&body)) {
        Err(issues) => util::invalid_body(issues),
        Ok(input) => match canvas::create_cards(&app.store, input) {
            Ok(v) => util::ok(v),
            Err(canvas::CanvasError(status, msg)) => util::error(status, msg),
        },
    }
}

/// POST /api/agent/explorations
pub async fn explorations(State(app): State<AppState>, body: Bytes) -> Response {
    match canvas::parse_exploration_body(&util::parse_body(&body)) {
        Err(issues) => util::invalid_body(issues),
        Ok(input) => match canvas::create_exploration(&app.store, input) {
            Ok(v) => util::ok(v),
            Err(canvas::CanvasError(status, msg)) => util::error(status, msg),
        },
    }
}
