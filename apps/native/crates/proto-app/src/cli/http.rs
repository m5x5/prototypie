//! A small blocking HTTP/1.1 client on `std::net::TcpStream` with its own time limits and the status code, and
//! the messages for an app that can't be reached (spec-public/05 AN-CLI-202, -204, -231).

use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::out::Fail;

pub const DEFAULT_SERVER: &str = "http://127.0.0.1:3124";
const CONNECT: Duration = Duration::from_secs(2);
/// Answer limits: reads and writes; delivering and creating cards.
pub const NORMAL: Duration = Duration::from_secs(10);
pub const LONG: Duration = Duration::from_secs(30);

/// The app's server: only this computer (AN-CLI-204).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub addr: SocketAddr,
    /// `host:port` for messages and the `Host` header.
    pub host: String,
}

impl Server {
    /// Normalize like the app's client: trim, drop trailing `/`, `http://` optional, port 80 when missing,
    /// `localhost` = `127.0.0.1`; only `localhost`, `127.0.0.0/8` and `[::1]`.
    pub fn parse(input: &str) -> Result<Server, Fail> {
        let bad = || Fail::usage(format!("Invalid server address \"{input}\" (use http://127.0.0.1:PORT; only this computer is allowed)"));
        let s = input.trim().trim_end_matches('/');
        let rest = match s.split_once("://") {
            Some((scheme, rest)) if scheme.eq_ignore_ascii_case("http") => rest,
            Some(_) => return Err(bad()),
            None => s,
        };
        if rest.is_empty() || rest.contains(['/', '?', '#', '@']) || rest.chars().any(char::is_whitespace) {
            return Err(bad());
        }
        let (host, port) = if let Some(v6) = rest.strip_prefix('[') {
            let (inner, tail) = v6.split_once(']').ok_or_else(bad)?;
            let port = match tail {
                "" => None,
                t => Some(t.strip_prefix(':').ok_or_else(bad)?),
            };
            (format!("[{inner}]"), port)
        } else {
            match rest.rsplit_once(':') {
                Some((h, p)) => (h.to_string(), Some(p)),
                None => (rest.to_string(), None),
            }
        };
        let port: u16 = match port {
            None => 80,
            Some(p) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => p.parse().ok().filter(|n| *n > 0).ok_or_else(bad)?,
            Some(_) => return Err(bad()),
        };
        let ip: IpAddr = if host.eq_ignore_ascii_case("localhost") {
            "127.0.0.1".parse().expect("ip")
        } else if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
            inner.parse().map_err(|_| bad())?
        } else {
            // Digits and dots only: no names other than localhost.
            if !host.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
                return Err(bad());
            }
            host.parse().map_err(|_| bad())?
        };
        if !ip.is_loopback() {
            return Err(bad());
        }
        let addr = SocketAddr::new(ip, port);
        let host = match ip {
            IpAddr::V4(v4) => format!("{v4}:{port}"),
            IpAddr::V6(v6) => format!("[{v6}]:{port}"),
        };
        Ok(Server { addr, host })
    }
}

/// Why no answer came.
#[derive(Debug)]
pub enum Gone {
    /// Nothing accepted the connection.
    Refused,
    /// Connected, but no complete answer in time.
    Timeout,
    /// The connection broke off (the app quit, or something that isn't an HTTP server).
    Broken,
}

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

/// Percent-encode a path segment or query value.
pub fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `?a=1&b=2` from the pairs that have a value.
pub fn query(pairs: &[(&str, Option<String>)]) -> String {
    let parts: Vec<String> = pairs.iter().filter_map(|(k, v)| v.as_ref().map(|v| format!("{k}={}", enc(v)))).collect();
    if parts.is_empty() { String::new() } else { format!("?{}", parts.join("&")) }
}

/// One exchange. `limit` bounds the whole answer after connecting.
pub fn request(server: &Server, method: &str, path: &str, body: Option<&[u8]>, limit: Duration) -> Result<Response, Gone> {
    let mut stream = TcpStream::connect_timeout(&server.addr, CONNECT).map_err(|_| Gone::Refused)?;
    let deadline = Instant::now() + limit;
    let _ = stream.set_nodelay(true);
    let body = body.unwrap_or(b"");
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nUser-Agent: prototypie-cli\r\nConnection: close\r\n{}Content-Length: {}\r\n\r\n",
        server.host,
        if method == "GET" { "" } else { "Content-Type: application/json\r\n" },
        body.len()
    );
    let _ = stream.set_write_timeout(Some(limit));
    let sent = stream.write_all(head.as_bytes()).and_then(|_| stream.write_all(body)).and_then(|_| stream.flush());
    if let Err(e) = sent {
        return Err(if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) { Gone::Timeout } else { Gone::Broken });
    }
    let mut raw = Vec::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Gone::Timeout);
        }
        let _ = stream.set_read_timeout(Some(left));
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if complete(&raw) {
                    break;
                }
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => return Err(Gone::Timeout),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(Gone::Broken),
        }
    }
    parse(&raw).ok_or(Gone::Broken)
}

fn header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|w| w == b"\r\n\r\n")
}

fn content_length(head: &str) -> Option<usize> {
    head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case("content-length").then(|| v.trim().parse().ok()).flatten()
    })
}

fn chunked(head: &str) -> bool {
    head.lines().any(|l| l.split_once(':').is_some_and(|(k, v)| k.trim().eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked")))
}

/// Whether `raw` holds a whole answer (so we need not wait for the close).
fn complete(raw: &[u8]) -> bool {
    let Some(end) = header_end(raw) else { return false };
    let head = String::from_utf8_lossy(&raw[..end]);
    let body = &raw[end + 4..];
    if chunked(&head) {
        return body.ends_with(b"0\r\n\r\n");
    }
    content_length(&head).is_some_and(|n| body.len() >= n)
}

/// Status and (de-chunked) body.
pub fn parse(raw: &[u8]) -> Option<Response> {
    let end = header_end(raw)?;
    let head = String::from_utf8_lossy(&raw[..end]).into_owned();
    let status: u16 = head.split_whitespace().nth(1)?.parse().ok()?;
    let mut body = raw[end + 4..].to_vec();
    if chunked(&head) {
        body = dechunk(&body)?;
    } else if let Some(n) = content_length(&head) {
        if body.len() < n {
            return None;
        }
        body.truncate(n);
    }
    Some(Response { status, body })
}

fn dechunk(mut b: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = b.windows(2).position(|w| w == b"\r\n")?;
        let size = usize::from_str_radix(String::from_utf8_lossy(&b[..line_end]).split(';').next()?.trim(), 16).ok()?;
        b = &b[line_end + 2..];
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(b.get(..size)?);
        b = b.get(size + 2..).unwrap_or(&[]);
    }
}

/// The exit-3 message for an app that doesn't answer (AN-CLI-231). Only on connection failures does it look
/// at the data dir's lock to tell "not running" from "running, but its server doesn't answer".
pub fn unreachable(server: &Server, gone: &Gone) -> Fail {
    let host = &server.host;
    match gone {
        Gone::Refused => match proto_core::lock_holder(&proto_core::default_data_dir()) {
            None => Fail::new(3, "not_running", format!("Prototypie is not running (nothing answers on {host}). Start the app, then try again.")),
            Some(pid) => Fail::new(
                3,
                "server_not_answering",
                format!("Prototypie is running (pid {pid}) but its server does not answer on {host}. Another program may be using the port; the app shows a banner then."),
            ),
        },
        Gone::Timeout | Gone::Broken => Fail::new(3, "timeout", format!("The server on {host} did not answer in time.")),
    }
}

pub fn no_notes_service(server: &Server) -> Fail {
    Fail::new(3, "no_notes_service", format!("The server on {} has no notes service. Update Prototypie.", server.host))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_server_addresses() {
        let ok = |s: &str| Server::parse(s).map(|x| x.host).unwrap_or_else(|f| panic!("{s}: {}", f.msg));
        assert_eq!(ok("http://127.0.0.1:3124"), "127.0.0.1:3124");
        assert_eq!(ok("  localhost:3124/ "), "127.0.0.1:3124");
        assert_eq!(ok("127.0.0.1"), "127.0.0.1:80");
        assert_eq!(ok("127.1.2.3:9"), "127.1.2.3:9");
        assert_eq!(ok("http://[::1]:3124"), "[::1]:3124");
        for bad in ["http://192.168.1.5:3124", "https://127.0.0.1:3124", "127.0.0.1:3124/api", "example.com:80", "127.0.0.1:x", "127.0.0.1:70000", "ftp://127.0.0.1", ""] {
            let f = Server::parse(bad).unwrap_err();
            assert_eq!(f.exit, 2, "{bad}");
            assert_eq!(f.msg, format!("Invalid server address \"{bad}\" (use http://127.0.0.1:PORT; only this computer is allowed)"));
        }
    }

    #[test]
    fn cli_http_answers_plain_and_chunked() {
        let r = parse(b"HTTP/1.1 201 Created\r\ncontent-length: 2\r\n\r\n{}").unwrap();
        assert_eq!((r.status, r.body.as_slice()), (201, b"{}".as_slice()));
        let r = parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{\"\r\n1\r\n}\r\n0\r\n\r\n").unwrap();
        assert_eq!(r.body, b"{\"}");
        assert!(complete(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"));
        assert!(!complete(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nab"));
        assert_eq!(query(&[("a", Some("x y".into())), ("b", None), ("c", Some("é/".into()))]), "?a=x%20y&c=%C3%A9%2F");
    }
}
