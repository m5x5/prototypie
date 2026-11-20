//! The app's client of the notes service (spec 03 §B, spec 05 AN-CLI-160): one background thread that takes
//! jobs in order, keeps each space's collection id, and leaves results in a shared inbox, waking the UI after
//! each. Nothing here runs on the UI thread except `submit` and `take`.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};

use serde_json::{Value, json};

use super::wire::collection_location;

/// The text for an address the app can't use (AN-SY-024).
pub const INVALID: &str = "Invalid notes service URL (use http://host:port)";
/// A local address only (AN-SY-181).
pub const NOT_LOCAL: &str = "The notes service must run on this computer (use http://127.0.0.1:port)";

/// What a configured address comes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Addr {
    /// `ip:port` to connect to.
    Ok(String),
    /// A host name other than `localhost`: fails at request time, without a name lookup.
    Named(String),
    /// A numeric address that isn't this machine.
    Remote(String),
    Invalid,
}

/// Normalise and check an address (AN-SY-023): trim, drop trailing `/`, optional `http://`, port 80 by default,
/// `localhost` = `127.0.0.1`, no path, no other scheme.
pub fn parse_addr(raw: &str) -> Addr {
    let s = raw.trim().trim_end_matches('/');
    let s = match s.split_once("://") {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("http") => rest,
        Some(_) => return Addr::Invalid,
        None => s,
    };
    if s.is_empty() || s.contains('/') || s.contains('?') || s.contains('#') || s.contains('@') {
        return Addr::Invalid;
    }
    let (host, port) = if let Some(rest) = s.strip_prefix('[') {
        let Some((h, after)) = rest.split_once(']') else { return Addr::Invalid };
        match after {
            "" => (h.to_string(), None),
            p => match p.strip_prefix(':') {
                Some(p) => (h.to_string(), Some(p)),
                None => return Addr::Invalid,
            },
        }
    } else {
        match s.rsplit_once(':') {
            Some((h, p)) if !h.contains(':') => (h.to_string(), Some(p)),
            Some(_) => return Addr::Invalid,
            None => (s.to_string(), None),
        }
    };
    let port: u16 = match port {
        None => 80,
        Some(p) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => match p.parse() {
            Ok(n) => n,
            Err(_) => return Addr::Invalid,
        },
        Some(_) => return Addr::Invalid,
    };
    if host.is_empty() {
        return Addr::Invalid;
    }
    let ip: std::net::IpAddr = if host.eq_ignore_ascii_case("localhost") {
        std::net::IpAddr::from([127, 0, 0, 1])
    } else {
        match host.parse() {
            Ok(ip) => ip,
            Err(_) => return Addr::Named(host),
        }
    };
    let sock = std::net::SocketAddr::new(ip, port).to_string();
    if ip.is_loopback() { Addr::Ok(sock) } else { Addr::Remote(sock) }
}

/// Percent-encoding for a path segment or query value.
pub fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A job for the worker (AN-SY-031).
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    /// Create a note (local id, space, body).
    Send { local: String, space: String, body: Value },
    /// Set a note's status, closed by `by`.
    Status { id: String, status: String, by: String },
    /// Append a human message.
    Reply { id: String, text: String },
    /// Read the space's collection.
    Refresh { space: String },
    /// Use another address; forgets every collection id.
    Endpoint(String),
}

/// What the worker found.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Sent { local: String, id: String },
    SendFailed { local: String, error: String },
    Content { space: String, notes: Vec<Value> },
    Reach(bool),
}

/// The text a create that the service took without an id gives (AN-SY-055).
pub const NO_NOTE_ID: &str = "no note id";

struct Shared {
    jobs: Mutex<VecDeque<Job>>,
    ready: Condvar,
    inbox: Mutex<Vec<Outcome>>,
}

/// Handle to the running worker. Dropping it lets the thread end after its queue.
pub struct Client {
    shared: Arc<Shared>,
}

impl Client {
    /// Start the worker for `endpoint`. None when the thread can't start.
    pub fn start(endpoint: &str, wake: Option<rune::Waker>) -> Option<Client> {
        let shared = Arc::new(Shared { jobs: Mutex::new(VecDeque::new()), ready: Condvar::new(), inbox: Mutex::new(Vec::new()) });
        let weak = Arc::downgrade(&shared);
        let mut worker = Worker { addr: parse_addr(endpoint), cache: HashMap::new() };
        std::thread::Builder::new().name("annotate-notes-client".into()).spawn(move || {
            loop {
                let Some(shared) = weak.upgrade() else { return };
                let batch: Vec<Job> = {
                    let mut q = shared.jobs.lock().expect("jobs");
                    while q.is_empty() {
                        // Wait with a timeout so a dropped client lets the thread end.
                        let (g, _) = shared.ready.wait_timeout(q, std::time::Duration::from_millis(500)).expect("jobs");
                        q = g;
                        if q.is_empty() && Arc::strong_count(&shared) == 1 {
                            return;
                        }
                    }
                    q.drain(..).collect()
                };
                // Within one batch a later refresh of a space already refreshed is skipped (AN-SY-032).
                let mut refreshed: Vec<String> = Vec::new();
                for job in batch {
                    if let Job::Refresh { space } = &job {
                        if refreshed.contains(space) {
                            continue;
                        }
                        refreshed.push(space.clone());
                    }
                    for out in worker.run(job) {
                        shared.inbox.lock().expect("inbox").push(out);
                        if let Some(w) = &wake {
                            w();
                        }
                    }
                }
            }
        }).ok()?;
        Some(Client { shared })
    }

    pub fn submit(&self, job: Job) {
        self.submit_all(vec![job]);
    }

    /// Queue several jobs at once (the worker sees them in one batch if it's idle).
    pub fn submit_all(&self, jobs: Vec<Job>) {
        self.shared.jobs.lock().expect("jobs").extend(jobs);
        self.shared.ready.notify_one();
    }

    /// Every result since the last call, in arrival order.
    pub fn take(&self) -> Vec<Outcome> {
        std::mem::take(&mut *self.shared.inbox.lock().expect("inbox"))
    }
}

struct Worker {
    addr: Addr,
    cache: HashMap<String, String>,
}

impl Worker {
    fn http(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Option<Value>, String> {
        let sock = match &self.addr {
            Addr::Ok(s) => s,
            Addr::Invalid => return Err(INVALID.into()),
            Addr::Remote(_) => return Err(NOT_LOCAL.into()),
            Addr::Named(h) => return Err(format!("{h}: only localhost or a numeric address can be used")),
        };
        let text = body.map(Value::to_string);
        let raw = crate::features::http::send(sock, method, path, text.as_deref())?;
        if raw.iter().all(u8::is_ascii_whitespace) {
            return Ok(None);
        }
        serde_json::from_slice(&raw).map(Some).map_err(|e| format!("unreadable answer: {e}"))
    }

    /// The space's collection id: cached, else the first open one with the exact location, else a new one.
    fn collection(&mut self, space: &str) -> Result<String, String> {
        if let Some(id) = self.cache.get(space) {
            return Ok(id.clone());
        }
        let loc = collection_location(space);
        let list = self.http("GET", &format!("/api/notes/collections?location={}", enc(&loc)), None)?;
        let found = list.as_ref().and_then(|v| v.get("collections")).and_then(Value::as_array).and_then(|a| {
            a.iter().find(|c| {
                c.get("location").and_then(Value::as_str) == Some(loc.as_str())
                    && c.get("state").and_then(Value::as_str) != Some("closed")
                    && c.get("id").and_then(Value::as_str).is_some()
            })
        }).and_then(|c| c.get("id").and_then(Value::as_str)).map(str::to_string);
        let id = match found {
            Some(id) => id,
            None => {
                let made = self.http("POST", "/api/notes/collections", Some(&json!({"location": loc})))?;
                made.as_ref().and_then(|v| v.pointer("/collection/id")).and_then(Value::as_str).map(str::to_string)
                    .ok_or_else(|| "no collection id".to_string())?
            }
        };
        self.cache.insert(space.to_string(), id.clone());
        Ok(id)
    }

    fn run(&mut self, job: Job) -> Vec<Outcome> {
        match job {
            Job::Endpoint(e) => {
                self.addr = parse_addr(&e);
                self.cache.clear();
                Vec::new()
            }
            Job::Send { local, space, body } => {
                let res = self.collection(&space).and_then(|cid| {
                    self.http("POST", &format!("/api/notes/collections/{}/notes", enc(&cid)), Some(&body))
                });
                match res {
                    Ok(v) => match v.as_ref().and_then(|v| v.pointer("/note/id")).and_then(Value::as_str) {
                        Some(id) => vec![Outcome::Sent { local, id: id.to_string() }, Outcome::Reach(true)],
                        // Taken without an id: the cache stays (AN-SY-042), but it's no success (AN-SY-033).
                        None => vec![Outcome::SendFailed { local, error: NO_NOTE_ID.into() }, Outcome::Reach(false)],
                    },
                    Err(error) => {
                        self.cache.remove(&space);
                        vec![Outcome::SendFailed { local, error }, Outcome::Reach(false)]
                    }
                }
            }
            Job::Status { id, status, by } => {
                let ok = self.http("POST", &format!("/api/notes/{}/status", enc(&id)), Some(&json!({"status": status, "by": by}))).is_ok();
                vec![Outcome::Reach(ok)]
            }
            Job::Reply { id, text } => {
                let ok = self.http("POST", &format!("/api/notes/{}/thread", enc(&id)), Some(&json!({"role": "human", "content": text}))).is_ok();
                vec![Outcome::Reach(ok)]
            }
            Job::Refresh { space } => {
                let res = self.collection(&space).and_then(|cid| self.http("GET", &format!("/api/notes/collections/{}", enc(&cid)), None));
                match res {
                    Ok(v) => {
                        let notes = v.as_ref().and_then(|v| v.get("notes")).and_then(Value::as_array).cloned().unwrap_or_default();
                        vec![Outcome::Content { space, notes }, Outcome::Reach(true)]
                    }
                    Err(_) => {
                        self.cache.remove(&space);
                        vec![Outcome::Reach(false)]
                    }
                }
            }
        }
    }
}
