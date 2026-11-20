//! `/api/variations` and `/api/variations/:id` (reference `src/app/api/variations/**`). Generation itself is
//! `proto_ai::Ai::start_variations`; this module adds the space's guidelines and the NDJSON framing.

use std::collections::HashMap;
use std::convert::Infallible;

use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use serde_json::json;

use crate::AppState;
use crate::context;
use crate::util;

/// POST /api/variations → NDJSON job stream.
pub async fn start(State(app): State<AppState>, body: Bytes) -> Response {
    let req: proto_ai::VariationRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return util::error(StatusCode::BAD_REQUEST, format!("Invalid request: {e}")),
    };
    let guidelines = context::guidelines_for(&app.store, req.space_id.as_deref());
    let id = app.ai.start_variations(req, guidelines);
    stream(&app, &id, 0)
}

/// `{"type":"job","id"}` then one JSON line per event from `from`, following live until Done.
pub fn stream(app: &AppState, id: &str, from: usize) -> Response {
    let Some(events) = app.ai.jobs().subscribe(id, from) else {
        return util::error(StatusCode::NOT_FOUND, "Unknown or expired job");
    };
    let first = futures_util::stream::once(std::future::ready(json!({ "type": "job", "id": id }).to_string()));
    let lines = first
        .chain(events.map(|e| serde_json::to_string(&e).unwrap_or_default()))
        .map(|mut line| {
            line.push('\n');
            Ok::<_, Infallible>(Bytes::from(line))
        });
    let mut res = Response::new(Body::from_stream(lines));
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/x-ndjson; charset=utf-8"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    if let Ok(v) = HeaderValue::from_str(id) {
        h.insert("x-job-id", v);
    }
    res
}

/// GET /api/variations/:id?from=N
pub async fn follow(State(app): State<AppState>, Path(id): Path<String>, Query(q): Query<HashMap<String, String>>) -> Response {
    let from = q.get("from").and_then(|s| s.parse::<f64>().ok()).filter(|n| n.is_finite() && *n > 0.0).map_or(0, |n| n as usize);
    stream(&app, &id, from)
}

/// HEAD /api/variations/:id
pub async fn exists(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    if app.ai.jobs().exists(&id) { StatusCode::OK } else { StatusCode::NOT_FOUND }.into_response()
}

/// DELETE /api/variations/:id
pub async fn cancel(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    app.ai.jobs().cancel(&id);
    StatusCode::NO_CONTENT.into_response()
}
