//! The embedded HTTP API (proto-server on 127.0.0.1:3124), so `scripts/agent-queue.md` workflows and
//! the worker agents reach this app by port. It runs on its own small tokio runtime and shares the
//! app's one `Store` (never a second Store on the same data dir: two writers would corrupt it).
//!
//! Started once the real store has loaded; never for fixtures (scenes, tests) or headless runs.
//! If the port is taken (another instance, a dev server), it fails in the log and in a banner
//! (`shell::banner`, with Retry) and the app keeps running without it; it never talks to someone
//! else's server.

use std::net::SocketAddr;

use proto_core::Store;

/// The server's lifecycle.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ServerState {
    #[default]
    Off,
    Starting,
    Running { addr: SocketAddr },
    Failed(String),
}

#[derive(Default)]
pub struct Server {
    pub state: ServerState,
    rt: Option<tokio::runtime::Runtime>,
    handle: Option<proto_server::ServerHandle>,
}

impl Server {
    /// Bind 127.0.0.1:3124 now (synchronous: a taken port fails here) and serve on a new runtime.
    /// `ai` is the app's generation service when it has one; else the server gets its own, which
    /// `ai()` hands out.
    /// `PROTOTYPIE_ADDR` (e.g. `127.0.0.1:3126`) overrides the address, so benchmark instances with a
    /// temp `PROTOTYPIE_DATA` never touch :3124; bad input is logged and falls back to the default.
    pub fn start(&mut self, store: Store, ai: Option<proto_ai::Ai>) {
        let default: SocketAddr = proto_server::DEFAULT_ADDR.parse().expect("addr");
        let addr = match std::env::var("PROTOTYPIE_ADDR") {
            Ok(v) if !v.trim().is_empty() => v.trim().parse().unwrap_or_else(|e| {
                eprintln!("prototypie: PROTOTYPIE_ADDR {v:?} isn't host:port ({e}); using {default}");
                default
            }),
            _ => default,
        };
        self.start_on(store, ai, addr);
    }

    fn start_on(&mut self, store: Store, ai: Option<proto_ai::Ai>, addr: SocketAddr) {
        if self.state != ServerState::Off {
            return;
        }
        let Some(dir) = store.data_dir().map(|d| d.to_path_buf()) else {
            self.state = ServerState::Failed("the store has no data dir".into());
            return;
        };
        let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("proto-server").enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                self.state = ServerState::Failed(format!("tokio runtime: {e}"));
                return;
            }
        };
        let opts = proto_server::ServeOpts { addr, runtime: Some(rt.handle().clone()), ..Default::default() };
        let ai = ai.unwrap_or_else(|| proto_ai::Ai::with_codex(dir, proto_server::notifier(&opts.notes_url, &opts.app_url)));
        match proto_server::start(store, ai, opts) {
            Ok(h) => {
                eprintln!("prototypie: API on http://{}", h.addr);
                self.state = ServerState::Running { addr: h.addr };
                self.handle = Some(h);
                self.rt = Some(rt);
            }
            Err(e) => {
                eprintln!("prototypie: API server not started on {addr}: {e:#} (another Prototypie or a proto-server dev instance on it?)");
                self.state = ServerState::Failed(format!("{addr}: {e:#}"));
            }
        }
    }

    /// Try again after a failed start (the banner's Retry, once the port is free).
    pub fn retry(&mut self, store: Store, ai: Option<proto_ai::Ai>) {
        if matches!(self.state, ServerState::Failed(_)) {
            self.state = ServerState::Off;
            self.start(store, ai);
        }
    }

    /// The generation service the server uses, once running.
    pub fn ai(&self) -> Option<&proto_ai::Ai> {
        self.handle.as_ref().map(|h| &h.ai)
    }

    /// Stop serving (on app exit, after the store was flushed).
    pub fn stop(&mut self) {
        if let (Some(h), Some(rt)) = (self.handle.take(), self.rt.as_ref()) {
            rt.block_on(h.shutdown());
        }
        if let Some(rt) = self.rt.take() {
            rt.shutdown_timeout(std::time::Duration::from_millis(500));
        }
        self.state = ServerState::Off;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn get(addr: SocketAddr, path: &str) -> String {
        let mut c = std::net::TcpStream::connect(addr).unwrap();
        write!(c, "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn serves_the_shared_store_and_stops() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let mut server = Server::default();
        server.start_on(store, None, "127.0.0.1:0".parse().unwrap());
        let ServerState::Running { addr } = server.state.clone() else { panic!("{:?}", server.state) };
        assert!(get(addr, "/api/state").starts_with("HTTP/1.1 200"));
        server.stop();
        assert_eq!(server.state, ServerState::Off);
        assert!(std::net::TcpStream::connect(addr).is_err(), "stopped");
    }

    #[test]
    fn a_taken_port_fails_clearly() {
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut server = Server::default();
        server.start_on(Store::open(dir.path()).unwrap(), None, taken.local_addr().unwrap());
        assert!(matches!(server.state, ServerState::Failed(ref e) if e.contains("127.0.0.1")), "{:?}", server.state);
    }

    #[test]
    fn an_in_memory_store_never_serves() {
        let mut server = Server::default();
        server.start_on(Store::in_memory(), None, "127.0.0.1:0".parse().unwrap());
        assert!(matches!(server.state, ServerState::Failed(_)));
    }
}
