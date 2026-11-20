//! The HTTP side of the notes service: every route under `/api/notes` (AN-CLI-101), the local-only and
//! no-web-page guards (AN-CLI-501/502), JSON-only bodies of at most 1 MiB (AN-CLI-102/153).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{Method, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use serde_json::{Value, json};

use super::model::{self, ApiError, respond};
use super::store::{Dest, Filter, NotesStore};
use super::watch::{self, WatchParams};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/notes", any(handle)).route("/api/notes/{*rest}", any(handle))
}

async fn handle(State(app): State<AppState>, req: Request) -> Response {
    match dispatch(&app.notes_store, req).await {
        Ok((status, body)) => respond(status, &body),
        Err(e) => e.into_response(),
    }
}

/// Host `127.0.0.1`, `localhost` or `[::1]` with any port.
fn local_host(host: &str) -> bool {
    let name = if let Some(rest) = host.strip_prefix('[') {
        match rest.split_once(']') {
            Some((inner, tail)) if tail.is_empty() || tail.strip_prefix(':').is_some_and(|p| p.bytes().all(|b| b.is_ascii_digit())) => {
                return inner == "::1";
            }
            _ => return false,
        }
    } else {
        match host.rsplit_once(':') {
            Some((n, p)) if p.bytes().all(|b| b.is_ascii_digit()) => n,
            Some(_) => return false,
            None => host,
        }
    };
    name == "127.0.0.1" || name.eq_ignore_ascii_case("localhost")
}

fn guard(req: &Request) -> Result<(), ApiError> {
    let not_local = || ApiError::new(403, "not_local", "The notes service only accepts requests from this computer");
    // In-process callers (router tests) have no peer; a real connection always does.
    if let Some(ConnectInfo(peer)) = req.extensions().get::<ConnectInfo<SocketAddr>>()
        && !peer.ip().is_loopback()
    {
        return Err(not_local());
    }
    let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).map(str::to_string).or_else(|| req.uri().authority().map(|a| a.to_string()));
    if !host.as_deref().is_some_and(local_host) {
        return Err(not_local());
    }
    if req.headers().contains_key(header::ORIGIN) || req.headers().contains_key("sec-fetch-mode") {
        return Err(ApiError::new(403, "browser_request", "The notes service does not accept requests from web pages"));
    }
    Ok(())
}

/// Percent-decode one path segment (invalid UTF-8 becomes U+FFFD, so it never matches a real id).
fn decode(seg: &str) -> String {
    let b = seg.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A JSON object body: `Content-Type: application/json`, at most 1 MiB.
async fn json_body(req: Request) -> Result<Value, ApiError> {
    let ct = req.headers().get(header::CONTENT_TYPE).and_then(|h| h.to_str().ok()).unwrap_or("");
    let mime = ct.split(';').next().unwrap_or("").trim();
    if !mime.eq_ignore_ascii_case("application/json") {
        return Err(ApiError::new(415, "json_required", "Send the body as application/json"));
    }
    let too_large = || ApiError::new(413, "too_large", "The request body is larger than 1 MiB");
    if let Some(len) = req.headers().get(header::CONTENT_LENGTH).and_then(|h| h.to_str().ok()).and_then(|s| s.parse::<usize>().ok())
        && len > model::MAX_BODY
    {
        return Err(too_large());
    }
    let bytes = axum::body::to_bytes(req.into_body(), model::MAX_BODY).await.map_err(|_| too_large())?;
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(v @ Value::Object(_)) => Ok(v),
        _ => Err(ApiError::invalid_json()),
    }
}

fn query(req: &Request) -> HashMap<String, String> {
    axum::extract::Query::<HashMap<String, String>>::try_from_uri(req.uri()).map(|q| q.0).unwrap_or_default()
}

type Answer = Result<(u16, Value), ApiError>;

async fn dispatch(store: &NotesStore, req: Request<Body>) -> Answer {
    guard(&req)?;
    let path = req.uri().path().to_string();
    let rest = path.strip_prefix("/api/notes").unwrap_or("");
    let segs: Vec<String> = rest.split('/').skip(1).map(decode).collect();
    let segs: Vec<&str> = segs.iter().map(String::as_str).collect();
    let m = req.method().clone();
    let only = |allowed: &[Method]| if allowed.contains(&m) { Ok(()) } else { Err(ApiError::method_not_allowed()) };
    match segs.as_slice() {
        [] | [""] => {
            if m == Method::GET {
                list_notes(store, &query(&req))
            } else {
                only(&[Method::POST])?;
                let body = json_body(req).await?;
                let input = model::parse_new_note(&body)?;
                let Some(loc) = input.location.clone() else { return Err(ApiError::field("location", "1 to 2048 characters without control characters")) };
                let (note, created) = store.add_note(Dest::Location(loc), input).await?;
                Ok((if created { 201 } else { 200 }, json!({ "note": note })))
            }
        }
        ["info"] => {
            only(&[Method::GET])?;
            Ok((200, store.info()))
        }
        ["watch"] => {
            only(&[Method::GET])?;
            let p = watch_params(&query(&req))?;
            Ok((200, watch::watch(store, p).await?))
        }
        ["collections"] => {
            if m == Method::GET {
                let q = query(&req);
                let state = match q.get("state").map(String::as_str) {
                    None => None,
                    Some(s) if model::STATES.contains(&s) => Some(s),
                    Some(_) => return Err(ApiError::query("state", "active or closed")),
                };
                Ok((200, json!({ "collections": store.list_collections(q.get("location").map(String::as_str), state) })))
            } else {
                only(&[Method::POST])?;
                let body = json_body(req).await?;
                let location = model::check_location(&body["location"])?;
                let reuse = match &body["reuse"] {
                    Value::Null => true,
                    Value::Bool(b) => *b,
                    _ => return Err(ApiError::field("reuse", "true or false")),
                };
                let (c, created) = store.create_collection(location, reuse).await?;
                Ok((if created { 201 } else { 200 }, json!({ "collection": c })))
            }
        }
        ["collections", id] => {
            if m == Method::GET {
                let (c, notes) = store.get_collection(id).ok_or_else(ApiError::collection_not_found)?;
                Ok((200, json!({ "collection": c, "notes": notes })))
            } else {
                only(&[Method::PATCH])?;
                let id = id.to_string();
                let body = json_body(req).await?;
                let state = match body["state"].as_str() {
                    Some(s) if model::STATES.contains(&s) => s.to_string(),
                    _ => return Err(ApiError::field("state", "active or closed")),
                };
                Ok((200, json!({ "collection": store.set_collection_state(&id, &state).await? })))
            }
        }
        ["collections", id, "notes"] => {
            only(&[Method::POST])?;
            let id = id.to_string();
            let body = json_body(req).await?;
            let input = model::parse_new_note(&body)?;
            let (note, created) = store.add_note(Dest::Collection(&id), input).await?;
            Ok((if created { 201 } else { 200 }, json!({ "note": note })))
        }
        [id] => {
            only(&[Method::GET])?;
            let note = valid_id(id).then(|| store.note(id)).flatten().ok_or_else(ApiError::note_not_found)?;
            Ok((200, json!({ "note": note })))
        }
        [id, "status"] => {
            only(&[Method::POST])?;
            let id = id.to_string();
            let body = json_body(req).await?;
            let input = model::parse_status(&body)?;
            if !valid_id(&id) {
                return Err(ApiError::note_not_found());
            }
            Ok((200, json!({ "note": store.set_status(&id, input).await? })))
        }
        [id, "thread"] => {
            only(&[Method::POST])?;
            let id = id.to_string();
            let body = json_body(req).await?;
            let (role, content) = model::parse_thread(&body)?;
            if !valid_id(&id) {
                return Err(ApiError::note_not_found());
            }
            Ok((201, json!({ "note": store.add_message(&id, &role, &content).await? })))
        }
        _ => Err(ApiError::not_found()),
    }
}

/// Ids in paths: any text of 1–200 characters (AN-CLI-104).
fn valid_id(id: &str) -> bool {
    (1..=200).contains(&id.chars().count())
}

fn filter_from(q: &HashMap<String, String>) -> Result<Filter, ApiError> {
    let tag = match q.get("tag").map(String::as_str) {
        None => None,
        Some(t @ ("job" | "implement" | "project-info" | "none")) => Some(t.to_string()),
        Some(_) => return Err(ApiError::query("tag", "job, implement, project-info or none")),
    };
    Ok(Filter {
        statuses: Vec::new(),
        location: q.get("location").cloned(),
        location_prefix: q.get("locationPrefix").cloned(),
        collection: q.get("collection").cloned(),
        tag,
    })
}

fn whole(q: &HashMap<String, String>, name: &str, min: u64, max: u64, default: u64, what: &str) -> Result<u64, ApiError> {
    match q.get(name) {
        None => Ok(default),
        Some(s) => match s.parse::<u64>() {
            Ok(n) if (min..=max).contains(&n) && !s.starts_with('+') => Ok(n),
            _ => Err(ApiError::query(name, what)),
        },
    }
}

/// `GET /api/notes` (AN-CLI-130).
fn list_notes(store: &NotesStore, q: &HashMap<String, String>) -> Answer {
    let mut f = filter_from(q)?;
    f.statuses = match q.get("status").map(String::as_str).unwrap_or("pending") {
        "all" => Vec::new(),
        "active" => vec!["pending", "acknowledged"],
        s => match model::STATUSES.iter().find(|x| **x == s) {
            Some(x) => vec![*x],
            None => return Err(ApiError::query("status", "one of pending, acknowledged, resolved, dismissed, active, all")),
        },
    };
    let limit = whole(q, "limit", 1, 5000, 1000, "a whole number from 1 to 5000")? as usize;
    let (notes, more) = store.query(&f, limit);
    Ok((200, json!({ "count": notes.len(), "notes": notes, "more": more })))
}

fn watch_params(q: &HashMap<String, String>) -> Result<WatchParams, ApiError> {
    let after = whole(q, "after", 0, u64::MAX, 0, "a whole number of 0 or more")?;
    let timeout = whole(q, "timeout", 1, 300, 120, "a whole number of seconds from 1 to 300")?;
    let batch = whole(q, "batchWindow", 0, 60, 10, "a whole number of seconds from 0 to 60")?;
    let by = match q.get("by") {
        None => "agent".to_string(),
        Some(b) if (1..=100).contains(&b.chars().count()) => b.clone(),
        Some(_) => return Err(ApiError::query("by", "1 to 100 characters")),
    };
    let replies = match q.get("replies").map(String::as_str) {
        None | Some("0" | "false") => false,
        Some("1" | "true") => true,
        Some(_) => return Err(ApiError::query("replies", "1 or 0")),
    };
    Ok(WatchParams {
        after,
        timeout: Duration::from_secs(timeout),
        batch: Duration::from_secs(batch),
        filter: filter_from(q)?,
        by,
        replies,
    })
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn hosts_and_segments() {
        assert!(local_host("127.0.0.1:3124"));
        assert!(local_host("localhost"));
        assert!(local_host("LOCALHOST:80"));
        assert!(local_host("[::1]:3124"));
        assert!(!local_host("evil.example"));
        assert!(!local_host("127.0.0.1.evil.example:80"));
        assert!(!local_host("[::2]:1"));
        assert_eq!(decode("a%20b%2F%C3%BC"), "a b/ü");
        assert_eq!(decode("100%"), "100%");
    }
}
