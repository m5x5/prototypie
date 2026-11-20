//! `/api/state` and `/api/state/events` (reference `src/app/api/state/**`).

use std::collections::HashMap;
use std::convert::Infallible;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;

use crate::AppState;
use crate::store::{Change, ChangeMeta};
use proto_core::is_valid_path;
use crate::util;

/// GET /api/state?since=N&epoch=E
pub async fn get(State(app): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    let since = q.get("since").and_then(|s| s.parse::<f64>().ok()).filter(|n| n.is_finite() && *n > 0.0).map_or(0, |n| n as u64);
    let epoch = q.get("epoch").map(String::as_str);
    util::no_store(app.store.read(since, epoch))
}

/// PUT /api/state { clientId?, reason?, changes: [{ path, value } | { path, deleted: true }] }
pub async fn put(State(app): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    let Ok(body) = serde_json::from_slice::<Value>(&body) else {
        return util::error(StatusCode::BAD_REQUEST, "Body must be JSON.");
    };
    let Some(list) = body.get("changes").and_then(Value::as_array) else {
        return util::error(StatusCode::BAD_REQUEST, "`changes` must be an array.");
    };
    let mut changes = Vec::with_capacity(list.len());
    for c in list {
        let path = c.get("path").and_then(Value::as_str);
        let Some(path) = path.filter(|p| is_valid_path(p)) else {
            let shown = c.get("path").map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).unwrap_or("undefined".into());
            return util::error(StatusCode::BAD_REQUEST, format!("Invalid record path (not a shared store key): {shown}"));
        };
        let deleted = c.get("deleted").is_some_and(truthy);
        let value = c.get("value").cloned();
        if !deleted && value.is_none() {
            return util::error(StatusCode::BAD_REQUEST, format!("Missing value for {path}"));
        }
        changes.push(Change { path: path.to_string(), value: if deleted { None } else { value }, deleted });
    }
    let short = |k: &str| body.get(k).and_then(Value::as_str).map(|s| s.chars().take(200).collect::<String>());
    let meta = ChangeMeta {
        client_id: short("clientId"),
        reason: short("reason"),
        user_agent: headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).map(str::to_string),
    };
    let deleted_designs: Vec<String> =
        changes.iter().filter(|c| c.deleted).filter_map(|c| c.path.strip_prefix("designs/").map(str::to_string)).collect();
    let cursor = match app.store.apply(changes, meta) {
        Ok(c) => c,
        Err(e) => return util::error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    // A deleted placeholder card cancels the queue item that would have filled it.
    if !deleted_designs.is_empty() {
        app.cancel_for_deleted(&deleted_designs).await;
    }
    util::ok(cursor)
}

/// JavaScript truthiness for `deleted`.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// GET /api/state/events: one `data: {epoch, version, clientId?}` per change, plus one on connect; pings every 20 s.
pub async fn events(State(app): State<AppState>) -> Response {
    let hello = app.store.cursor();
    let rx = app.store.subscribe();
    let first = Bytes::from(format!("data: {}\n\n", serde_json::to_string(&hello).unwrap_or_default()));
    let mut ping = tokio::time::interval(Duration::from_secs(20));
    ping.reset(); // the first tick would fire at once
    let store = app.store.clone();
    let state = (rx, ping, Some(first));
    let body = futures_util::stream::unfold(state, move |(mut rx, mut ping, first)| {
        let store = store.clone();
        async move {
        if let Some(f) = first {
            return Some((Ok::<_, Infallible>(f), (rx, ping, None)));
        }
        let chunk = tokio::select! {
            ev = rx.recv() => match ev {
                Ok(e) => format!("data: {}\n\n", serde_json::to_string(&e).unwrap_or_default()),
                // Missed events: send the current cursor; the client refetches since its own.
                Err(RecvError::Lagged(_)) => format!("data: {}\n\n", serde_json::to_string(&store.cursor()).unwrap_or_default()),
                Err(RecvError::Closed) => return None,
            },
            _ = ping.tick() => ": ping\n\n".to_string(),
        };
        Some((Ok(Bytes::from(chunk)), (rx, ping, None)))
        }
    });
    let mut res = Response::new(Body::from_stream(body));
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream; charset=utf-8"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store, no-transform"));
    h.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
    h.insert("x-accel-buffering", HeaderValue::from_static("no"));
    res
}
