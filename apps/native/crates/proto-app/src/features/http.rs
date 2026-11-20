//! The one way features reach proto-server: blocking HTTP/1.1 to its loopback API, called only on
//! `rune::Latest` workers. Base address `PROTOTYPIE_SERVER` (host:port), default proto-server on
//! 127.0.0.1:3124; the app never talks to the reference app.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Whether requests that change anything (POST/PUT/DELETE) may go out. False until the real store is loaded and
/// whenever it has no data dir (headless tooling runs work on an in-memory copy and must write nothing).
static WRITES: AtomicBool = AtomicBool::new(false);

/// Set from `features::poll` each frame: `s.store().data_dir().is_some()`.
pub fn allow_writes(on: bool) {
    WRITES.store(on, Ordering::Relaxed);
}

/// proto-server's address (`host:port`).
pub fn server_addr() -> String {
    std::env::var("PROTOTYPIE_SERVER").unwrap_or_else(|_| "127.0.0.1:3124".into())
}

/// A minimal HTTP/1.1 exchange with the loopback server: status 2xx → body (de-chunked).
pub fn request(addr: &str, method: &str, path: &str, json: Option<&str>) -> Result<Vec<u8>, String> {
    if method != "GET" && !writes_allowed() {
        return Err("Read-only run: changes are not sent (no data dir).".into());
    }
    send(addr, method, path, json)
}

/// Whether this run may send changes (see `WRITES`).
pub fn writes_allowed() -> bool {
    WRITES.load(Ordering::Relaxed)
}

/// `request` without the read-only gate: for callers that gate themselves (the Annotate client decides once,
/// when it starts, so its tests against a mock server don't race the global flag).
pub(crate) fn send(addr: &str, method: &str, path: &str, json: Option<&str>) -> Result<Vec<u8>, String> {
    let sock: SocketAddr = addr.parse().map_err(|e| format!("{addr}: {e}"))?;
    let mut stream = TcpStream::connect_timeout(&sock, Duration::from_secs(2)).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(Duration::from_secs(8))).ok();
    let body = json.unwrap_or("");
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAccept: application/json\r\nConnection: close\r\n{}Content-Length: {}\r\n\r\n",
        if json.is_some() { "Content-Type: application/json\r\n" } else { "" },
        body.len()
    );
    stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body.as_bytes())).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    parse_response(&raw)
}

pub(crate) fn parse_response(raw: &[u8]) -> Result<Vec<u8>, String> {
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("no HTTP header")?;
    let head = String::from_utf8_lossy(&raw[..split]);
    let body = &raw[split + 4..];
    let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or("no HTTP status")?;
    let chunked = head.lines().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    let body = if chunked { dechunk(body)? } else { body.to_vec() };
    if !(200..300).contains(&status) {
        let msg = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(msg);
    }
    Ok(body)
}

fn dechunk(mut b: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = b.windows(2).position(|w| w == b"\r\n").ok_or("bad chunk")?;
        let size_str = String::from_utf8_lossy(&b[..line_end]);
        let size = usize::from_str_radix(size_str.split(';').next().unwrap_or("").trim(), 16).map_err(|_| "bad chunk size")?;
        b = &b[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if b.len() < size {
            return Err("short chunk".into());
        }
        out.extend_from_slice(&b[..size]);
        b = b.get(size + 2..).unwrap_or(&[]);
    }
}


#[cfg(test)]
mod write_tests {
    #[test]
    fn writes_are_refused_without_a_data_dir() {
        super::allow_writes(false);
        let e = super::request("127.0.0.1:9", "POST", "/api/polls", Some("{}")).unwrap_err();
        assert!(e.starts_with("Read-only run"));
    }
}
