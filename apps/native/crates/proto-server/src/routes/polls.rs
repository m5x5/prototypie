//! Polls: the API on the loopback server, and the stakeholder listener on the LAN (lead decision, see
//! docs/api-server.md "Stakeholder listener"): that one serves only the poll page and the read/vote endpoints of
//! published polls, rate-limited per client IP.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::watch;

use crate::AppState;
use crate::polls::{self, Poll};
use crate::util;

const PAGE: &str = include_str!("../../assets/poll.html");
pub const DEFAULT_SHARE_PORT: u16 = 3125;
const MAX_VOTE_BODY: usize = 16 * 1024;

// ---- shared handlers -------------------------------------------------------------------------------------------

async fn read_poll(app: &AppState, id: &str) -> Result<Poll, Response> {
    match app.polls.read(id).await {
        Ok(Some(p)) => Ok(p),
        Ok(None) => Err(util::error(StatusCode::NOT_FOUND, "Poll not found.")),
        Err(e) => Err(util::error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

/// GET /api/polls/:id
pub async fn get_poll(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    match read_poll(&app, &id).await {
        Ok(p) => util::no_store(p),
        Err(r) => r,
    }
}

/// GET /api/polls/:id/votes
pub async fn get_votes(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    match read_poll(&app, &id).await {
        Ok(p) => util::no_store(polls::results(&p)),
        Err(r) => r,
    }
}

/// POST /api/polls/:id/votes
pub async fn post_vote(State(app): State<AppState>, Path(id): Path<String>, body: Bytes) -> Response {
    let Ok(json) = serde_json::from_slice::<Value>(&body) else {
        return util::error(StatusCode::BAD_REQUEST, "Body must be JSON.");
    };
    let vote = match polls::parse_vote(&json) {
        Ok(v) => v,
        Err(issues) => return util::error(StatusCode::BAD_REQUEST, util::issue_message(&issues, "Invalid vote.")),
    };
    if !polls::is_poll_id(&id) {
        return util::error(StatusCode::NOT_FOUND, "Poll not found.");
    }
    match app.polls.update(&id, |p| polls::cast(p, vote, util::now_ms())).await {
        Ok(None) => util::error(StatusCode::NOT_FOUND, "Poll not found."),
        Ok(Some((_, Err(msg)))) => util::error(StatusCode::BAD_REQUEST, msg),
        Ok(Some((poll, Ok(())))) => util::ok(polls::results(&poll)),
        Err(e) => util::error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// POST /api/polls (loopback only) → 201 `{ id, path, lanOrigins }`.
pub async fn create(State(app): State<AppState>, body: Bytes) -> Response {
    let Ok(json) = serde_json::from_slice::<Value>(&body) else {
        return util::error(StatusCode::BAD_REQUEST, "Body must be JSON.");
    };
    let (title, intro, decisions) = match polls::parse_create(&json) {
        Ok(x) => x,
        Err(issues) => return util::error(StatusCode::BAD_REQUEST, util::issue_message(&issues, "Invalid poll.")),
    };
    let id = util::nanoid(10);
    let poll = Poll { id: id.clone(), title, intro, created_at: util::now_ms(), decisions, votes: Vec::new() };
    if let Err(e) = app.polls.write(&poll).await {
        return util::error(StatusCode::INTERNAL_SERVER_ERROR, format!("Could not save the poll: {e}"));
    }
    let origins = app.share.state().origins;
    (StatusCode::CREATED, axum::Json(json!({ "id": id, "path": format!("/poll/{id}"), "lanOrigins": origins }))).into_response()
}

/// GET /poll/:id — the stakeholder page (the poll itself is fetched by the page).
pub async fn page() -> Response {
    let mut res = Html(PAGE).into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    // No CSP header: srcdoc iframes inherit it, which would block the prototypes' Google Fonts. The prototypes are
    // isolated by `sandbox` without allow-same-origin instead (opaque origin: no access to this page or its API).
    res.headers_mut().insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    res
}

// ---- sharing state -----------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareState {
    pub sharing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// `http://<lan ip>:<port>` (empty when off or when no LAN interface was found).
    pub origins: Vec<String>,
}

/// The stakeholder listener: on/off, its origins, and a watch channel the app can observe.
#[derive(Clone)]
pub struct Share {
    tx: Arc<watch::Sender<ShareState>>,
    stop: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl Default for Share {
    fn default() -> Self {
        Self { tx: Arc::new(watch::channel(ShareState::default()).0), stop: Arc::default() }
    }
}

impl Share {
    pub fn state(&self) -> ShareState {
        self.tx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<ShareState> {
        self.tx.subscribe()
    }

    /// Start the LAN listener (idempotent: an existing one on the same port stays).
    pub async fn start(&self, app: AppState, port: u16) -> anyhow::Result<ShareState> {
        let cur = self.state();
        if cur.sharing && cur.port == Some(port) {
            return Ok(cur);
        }
        self.stop();
        let ip = lan_ip().ok_or_else(|| anyhow::anyhow!("No LAN interface found"))?;
        let listener = tokio::net::TcpListener::bind(SocketAddr::new(ip, port)).await?;
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        *self.stop.lock().unwrap() = Some(tx);
        let router = lan_router(app).into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = rx.await;
                })
                .await;
        });
        let state = ShareState { sharing: true, port: Some(port), origins: vec![format!("http://{ip}:{port}")] };
        self.tx.send_replace(state.clone());
        Ok(state)
    }

    pub fn stop(&self) {
        if let Some(tx) = self.stop.lock().unwrap().take() {
            let _ = tx.send(());
        }
        self.tx.send_replace(ShareState::default());
    }
}

/// This machine's address on the local network: `PROTOTYPIE_LAN_IP`, else the source address of the default
/// route (a UDP connect sends no packet). Only private IPv4 addresses count.
pub fn lan_ip() -> Option<IpAddr> {
    if let Some(ip) = std::env::var("PROTOTYPIE_LAN_IP").ok().and_then(|s| s.parse().ok()) {
        return Some(ip);
    }
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:9").ok()?; // TEST-NET-1: never routed, nothing sent
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(v4) if v4.is_private() || v4.is_link_local() => Some(IpAddr::V4(v4)),
        _ => None,
    }
}

/// POST /api/polls/share { port? } → `{ sharing, port, lanOrigins }` (loopback only).
pub async fn share_start(State(app): State<AppState>, body: Bytes) -> Response {
    let port = util::parse_body(&body)["port"].as_u64().and_then(|p| u16::try_from(p).ok()).filter(|p| *p >= 1024).unwrap_or(DEFAULT_SHARE_PORT);
    match app.share.clone().start(app.clone(), port).await {
        Ok(s) => util::ok(json!({ "sharing": s.sharing, "port": s.port, "lanOrigins": s.origins })),
        Err(e) => util::error(StatusCode::SERVICE_UNAVAILABLE, format!("Couldn't share on the LAN: {e}")),
    }
}

/// GET /api/polls/share → the current sharing state.
pub async fn share_get(State(app): State<AppState>) -> Response {
    let s = app.share.state();
    util::no_store(json!({ "sharing": s.sharing, "port": s.port, "lanOrigins": s.origins }))
}

/// DELETE /api/polls/share → 204.
pub async fn share_stop(State(app): State<AppState>) -> Response {
    app.share.stop();
    StatusCode::NO_CONTENT.into_response()
}

// ---- LAN router --------------------------------------------------------------------------------------------

/// Token buckets per client IP: `rate` per minute, up to `burst` at once.
#[derive(Clone)]
pub struct Limiter {
    buckets: Arc<Mutex<HashMap<IpAddr, (f64, Instant)>>>,
    per_sec: f64,
    burst: f64,
}

impl Limiter {
    pub fn new(per_minute: f64, burst: f64) -> Self {
        Self { buckets: Arc::default(), per_sec: per_minute / 60.0, burst }
    }

    pub fn allow(&self, ip: IpAddr) -> bool {
        let mut map = self.buckets.lock().unwrap();
        if map.len() > 10_000 {
            map.retain(|_, (_, at)| at.elapsed().as_secs() < 600);
        }
        let now = Instant::now();
        let (tokens, at) = map.entry(ip).or_insert((self.burst, now));
        *tokens = (*tokens + at.elapsed().as_secs_f64() * self.per_sec).min(self.burst);
        *at = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[derive(Clone)]
struct Lan {
    app: AppState,
    votes: Limiter,
    reads: Limiter,
}

fn too_many(msg: &str) -> Response {
    util::error(StatusCode::TOO_MANY_REQUESTS, msg)
}

async fn lan_page(State(lan): State<Lan>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Path(id): Path<String>) -> Response {
    if !lan.reads.allow(peer.ip()) {
        return too_many("Too many requests, please wait a moment.");
    }
    let _ = id; // the page shows "This poll doesn't exist" itself when the read 404s
    page().await
}

async fn lan_get_poll(State(lan): State<Lan>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Path(id): Path<String>) -> Response {
    if !lan.reads.allow(peer.ip()) {
        return too_many("Too many requests, please wait a moment.");
    }
    get_poll(State(lan.app), Path(id)).await
}

async fn lan_get_votes(State(lan): State<Lan>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Path(id): Path<String>) -> Response {
    if !lan.reads.allow(peer.ip()) {
        return too_many("Too many requests, please wait a moment.");
    }
    get_votes(State(lan.app), Path(id)).await
}

async fn lan_post_vote(State(lan): State<Lan>, ConnectInfo(peer): ConnectInfo<SocketAddr>, Path(id): Path<String>, body: Bytes) -> Response {
    if !lan.votes.allow(peer.ip()) {
        return too_many("Too many votes, please wait a moment.");
    }
    // Only published polls: a vote for an unknown id never creates anything.
    if !lan.app.polls.exists(&id).await {
        return util::error(StatusCode::NOT_FOUND, "Poll not found.");
    }
    post_vote(State(lan.app), Path(id), body).await
}

/// The stakeholder surface only; everything else is 404.
pub fn lan_router(app: AppState) -> Router {
    let lan = Lan { app, votes: Limiter::new(30.0, 10.0), reads: Limiter::new(120.0, 60.0) };
    Router::new()
        .route("/poll/{id}", get(lan_page))
        .route("/api/polls/{id}", get(lan_get_poll))
        .route("/api/polls/{id}/votes", get(lan_get_votes).post(lan_post_vote))
        .fallback(|| async { StatusCode::NOT_FOUND })
        .layer(axum::extract::DefaultBodyLimit::max(MAX_VOTE_BODY))
        .with_state(lan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_bursts_then_blocks() {
        let l = Limiter::new(30.0, 3.0);
        let ip: IpAddr = "192.168.1.9".parse().unwrap();
        assert!(l.allow(ip) && l.allow(ip) && l.allow(ip));
        assert!(!l.allow(ip));
        assert!(l.allow("192.168.1.10".parse().unwrap()));
    }
}
