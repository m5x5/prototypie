//! Tests of the notes service (spec-public/05 AN-CLI-601…619 and the rows of AN-CLI-101/103) and of
//! `NotesService` and its callers (spec-public/04 AN-IF-561…566). Every network test runs against a server on
//! a free port with a temporary data dir, a small mock on port 0, or the dead port 9; never against :3124.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::model;
use super::{NotesService, Status};

mod bridge;
mod service;

const DEAD: &str = "http://127.0.0.1:9";

pub(super) struct Svc {
    pub dir: tempfile::TempDir,
    pub handle: Option<crate::ServerHandle>,
    pub addr: SocketAddr,
    pub base: String,
    pub http: reqwest::Client,
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

impl Svc {
    /// A server on a free port whose own notes handle points at itself (so the queue, the task list and the
    /// implement route use this notes service).
    pub async fn start() -> Svc {
        Svc::start_in(tempfile::tempdir().unwrap()).await
    }

    pub async fn start_in(dir: tempfile::TempDir) -> Svc {
        let port = free_port();
        let addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
        let cfg = crate::ServerConfig {
            addr,
            data_dir: dir.path().to_path_buf(),
            notes_url: format!("http://{addr}"),
            app_url: crate::DEFAULT_APP_URL.into(),
            codex: false,
        };
        let store = proto_core::Store::open(dir.path()).unwrap();
        // The queue announces its jobs to this same service.
        let ai = proto_ai::Ai::new(dir.path().to_path_buf(), None, crate::notifier(&cfg.notes_url, &cfg.app_url));
        let handle = crate::serve(cfg, store, Some(ai)).await.unwrap();
        let base = format!("http://{addr}");
        Svc { dir, handle: Some(handle), addr, base, http: reqwest::Client::builder().no_proxy().build().unwrap() }
    }

    pub fn service(&self) -> NotesService {
        NotesService::new(format!("{}/", self.base), crate::DEFAULT_APP_URL)
    }

    pub async fn stop(mut self) -> tempfile::TempDir {
        if let Some(h) = self.handle.take() {
            h.shutdown().await;
        }
        std::mem::replace(&mut self.dir, tempfile::tempdir().unwrap())
    }

    pub async fn req(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut r = self.http.request(method, format!("{}{path}", self.base));
        if let Some(b) = body {
            r = r.json(&b);
        }
        let res = r.send().await.unwrap();
        let status = res.status().as_u16();
        (status, res.json().await.unwrap_or(Value::Null))
    }

    pub async fn get(&self, path: &str) -> (u16, Value) {
        self.req(reqwest::Method::GET, path, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.req(reqwest::Method::POST, path, Some(body)).await
    }

    pub async fn collection(&self, location: &str) -> String {
        let (s, v) = self.post("/api/notes/collections", json!({ "location": location })).await;
        assert!(s == 201 || s == 200, "{v}");
        v["collection"]["id"].as_str().unwrap().to_string()
    }

    pub async fn note_in(&self, coll: &str, comment: &str) -> Value {
        let (s, v) = self.post(&format!("/api/notes/collections/{coll}/notes"), json!({ "comment": comment, "element": "el", "elementPath": "a > b" })).await;
        assert_eq!(s, 201, "{v}");
        v["note"].clone()
    }

    pub async fn note_at(&self, location: &str, comment: &str) -> Value {
        let (s, v) = self.post("/api/notes", json!({ "location": location, "comment": comment, "element": "", "elementPath": "" })).await;
        assert!(s == 201 || s == 200, "{v}");
        v["note"].clone()
    }

    pub async fn status(&self, id: &str, body: Value) -> (u16, Value) {
        self.post(&format!("/api/notes/{id}/status"), body).await
    }
}

impl Drop for Svc {
    fn drop(&mut self) {
        drop(self.handle.take());
    }
}

/// One raw HTTP/1.1 exchange (for headers reqwest would set itself): `(status, body)`.
pub(super) fn raw(addr: SocketAddr, head: &str, body: &[u8]) -> (u16, Value) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(head.as_bytes()).unwrap();
    let _ = s.write_all(body);
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).into_owned();
    let status = text.split_whitespace().nth(1).and_then(|x| x.parse().ok()).unwrap_or(0);
    let body = text.split_once("\r\n\r\n").map(|x| x.1).unwrap_or("");
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

pub(super) fn assert_err(got: &(u16, Value), status: u16, code: &str, error: &str) {
    assert_eq!(got.0, status, "{}", got.1);
    assert_eq!(got.1["code"], code, "{}", got.1);
    assert_eq!(got.1["error"], error, "{}", got.1);
}

pub(super) fn is_time(v: &Value) -> bool {
    let re = regex::Regex::new(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$").unwrap();
    v.as_str().is_some_and(|s| re.is_match(s))
}

type Answer = Arc<dyn Fn(&str, &str, &str) -> (u16, String) + Send + Sync>;

/// A tiny HTTP server on port 0 answering every request with `answer(method, path, body)`, recording them.
pub(super) struct Mock {
    pub addr: SocketAddr,
    seen: Arc<Mutex<Vec<(String, String, String)>>>,
}

impl Mock {
    pub fn start(answer: impl Fn(&str, &str, &str) -> (u16, String) + Send + Sync + 'static) -> Mock {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        let answer: Answer = Arc::new(answer);
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                let (seen, answer) = (seen2.clone(), answer.clone());
                std::thread::spawn(move || {
                    let mut conn = conn;
                    let Some((method, path, body)) = read_request(&mut conn) else { return };
                    seen.lock().unwrap().push((method.clone(), path.clone(), body.clone()));
                    let (status, text) = answer(&method, &path, &body);
                    let res = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
                    let _ = conn.write_all(res.as_bytes());
                });
            }
        });
        Mock { addr, seen }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn requests(&self) -> Vec<(String, String, String)> {
        self.seen.lock().unwrap().clone()
    }
}

fn read_request(conn: &mut TcpStream) -> Option<(String, String, String)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        let n = conn.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let len: usize = head
        .lines()
        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0)))
        .unwrap_or(0);
    while buf.len() < head_end + 4 + len {
        let n = conn.read(&mut tmp).ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let mut parts = head.split_whitespace();
    let (method, path) = (parts.next()?.to_string(), parts.next()?.to_string());
    Some((method, path, String::from_utf8_lossy(&buf[head_end + 4..]).into_owned()))
}

// ---- model -------------------------------------------------------------------------------------------------

#[test]
fn ids_have_the_service_form_and_differ() {
    // AN-CLI-104
    let re = regex::Regex::new(r"^[ncm][0-9a-z]{10}$").unwrap();
    let mut seen = std::collections::HashSet::new();
    for i in 0..10_000 {
        let id = model::new_id(['n', 'c', 'm'][i % 3]);
        assert!(re.is_match(&id), "{id}");
        assert!(seen.insert(id));
    }
    assert!(model::is_collection_id("c0123456789"));
    assert!(!model::is_collection_id("x0123456789"));
    assert!(!model::is_collection_id("c012345678A"));
}

#[test]
fn tags_come_from_the_prefix() {
    // AN-CLI-112
    assert_eq!(model::tag_of("[prototypie job J1#0] make it pop · …"), Some(json!({ "kind": "job", "jobId": "J1", "index": 0 })));
    assert_eq!(model::tag_of("[prototypie job J1#x] …"), None);
    assert_eq!(model::tag_of("[prototypie implement d1] Implement"), Some(json!({ "kind": "implement", "designId": "d1" })));
    assert_eq!(model::tag_of("[prototypie project-info s1] Projekt"), Some(json!({ "kind": "project-info", "spaceId": "s1" })));
    assert_eq!(model::tag_of("Make it smaller"), None);
    assert_eq!(model::tag_of("[prototypie other x] y"), None);
}

#[test]
fn change_times_always_rise() {
    // AN-CLI-105: a previous time at or after now still gives a later one.
    let future = crate::util::iso_from_ms(crate::util::now_ms() + 60_000);
    let (next, ms) = model::next_time(Some(&future));
    assert!(next > future);
    assert_eq!(ms, model::ms_of(&future).unwrap() + 1);
    let (a, _) = model::next_time(None);
    let (b, _) = model::next_time(Some(&a));
    assert!(b > a);
}

#[test]
fn service_json_from_foreign_data_never_panics() {
    // AN-SY-183: wrong types, no thread, no location.
    let n = super::note_from(&json!({ "id": "x", "comment": 5, "thread": "no", "status": "weird" })).unwrap();
    assert_eq!(n.comment, "");
    assert!(n.thread.is_empty() && n.url.is_none() && n.status.is_none());
    assert!(super::note_from(&json!({ "comment": "no id" })).is_none());
    let n = super::note_from(&json!({ "id": "y", "element": "", "elementPath": "", "location": "",
        "thread": [{ "id": "m1", "role": "agent", "content": "c", "createdAt": "2026-11-20T10:00:00.123Z" }, 7] }))
    .unwrap();
    assert_eq!((n.element.as_deref(), n.element_path.as_deref(), n.url.as_deref()), (Some(""), Some(""), Some("")));
    assert_eq!(n.thread.len(), 1);
    assert_eq!(n.thread[0].at_ms, model::ms_of("2026-11-20T10:00:00.123Z").unwrap());
    assert_eq!(super::encode_segment("a b/ü"), "a%20b%2F%C3%BC");
    assert_eq!(Status::parse("acknowledged"), Some(Status::Acknowledged));
    assert_eq!(Status::parse("done"), None);
}
