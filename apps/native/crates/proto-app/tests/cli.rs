//! The `prototypie` command, run as the real binary against an in-process server on a free port (never :3124),
//! with a temporary HOME and data dir (spec-public/05 AN-CLI-630…647 and the CLI rows of AN-CLI-2xx).
//! Every test name starts with `cli_` (so `cargo test -p proto-app cli` runs them).

use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_prototypie");

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
    took: Duration,
}

impl Out {
    fn json(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {}", self.stdout))
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// A temporary HOME and an unlocked data dir for the CLI.
struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new() -> Home {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("home")).unwrap();
        std::fs::create_dir_all(dir.path().join("clidata")).unwrap();
        Home { dir }
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn run(&self, server: &str, args: &[&str], stdin: Option<&[u8]>, env: &[(&str, &str)]) -> Out {
        let mut cmd = Command::new(BIN);
        cmd.args(args)
            .env("PROTOTYPIE_SERVER", server)
            .env("HOME", self.home())
            .env("PROTOTYPIE_DATA", self.dir.path().join("clidata"))
            .env("PATH", "/usr/bin:/bin")
            .env_remove("PROTOTYPIE_AGENT")
            .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let t0 = Instant::now();
        let mut child = cmd.spawn().unwrap();
        if let Some(input) = stdin {
            let mut pipe = child.stdin.take().unwrap();
            let input = input.to_vec();
            std::thread::spawn(move || {
                let _ = pipe.write_all(&input);
            });
        }
        let o = child.wait_with_output().unwrap();
        Out {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            took: t0.elapsed(),
        }
    }
}

/// A server like the app's: own notes service, the queue announcing to it.
struct App {
    home: Home,
    data: tempfile::TempDir,
    store: proto_core::Store,
    handle: Option<proto_server::ServerHandle>,
    url: String,
    rt: tokio::runtime::Runtime,
    http: reqwest::Client,
}

impl App {
    fn start() -> App {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build().unwrap();
        let data = tempfile::tempdir().unwrap();
        let addr: std::net::SocketAddr = format!("127.0.0.1:{}", free_port()).parse().unwrap();
        let url = format!("http://{addr}");
        let store = proto_core::Store::open(data.path()).unwrap();
        let mut cfg = proto_server::ServerConfig::new(data.path().to_path_buf());
        cfg.addr = addr;
        cfg.notes_url = url.clone();
        cfg.codex = false;
        let ai = proto_ai::Ai::new(data.path().to_path_buf(), None, proto_server::notifier(&url, proto_server::DEFAULT_APP_URL));
        let handle = rt.block_on(proto_server::serve(cfg, store.clone(), Some(ai))).unwrap();
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        App { home: Home::new(), data, store, handle: Some(handle), url, rt, http }
    }

    fn cli(&self, args: &[&str]) -> Out {
        self.home.run(&self.url, args, None, &[])
    }

    fn cli_in(&self, args: &[&str], stdin: &[u8]) -> Out {
        self.home.run(&self.url, args, Some(stdin), &[])
    }

    fn req(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> (u16, Value) {
        self.rt.block_on(async {
            let mut r = self.http.request(method, format!("{}{path}", self.url));
            if let Some(b) = body {
                r = r.json(&b);
            }
            let res = r.send().await.unwrap();
            let s = res.status().as_u16();
            (s, res.json().await.unwrap_or(Value::Null))
        })
    }

    fn get(&self, path: &str) -> Value {
        self.req(reqwest::Method::GET, path, None).1
    }

    fn post(&self, path: &str, body: Value) -> (u16, Value) {
        self.req(reqwest::Method::POST, path, Some(body))
    }

    /// A pending note in a collection at `location`; its JSON.
    fn note(&self, location: &str, comment: &str) -> Value {
        let (s, v) = self.post("/api/notes", json!({ "location": location, "comment": comment, "element": "button \"Review\"", "elementPath": "header-actions > review-dock" }));
        assert_eq!(s, 201, "{v}");
        v["note"].clone()
    }

    fn cursor(&self) -> u64 {
        self.get("/api/notes/info")["cursor"].as_u64().unwrap()
    }

    fn put_state(&self, changes: Value) {
        let (s, v) = self.req(reqwest::Method::PUT, "/api/state", Some(json!({ "changes": changes, "clientId": "zz-cli" })));
        assert_eq!(s, 200, "{v}");
    }

    /// A design job of `count` items with placeholders in the project space `zz-proj`; the job id.
    fn job(&self, count: usize) -> String {
        let res = self.rt.block_on(
            self.http.post(format!("{}/api/variations", self.url))
                .json(&json!({ "instruction": "zz-cli make it pop", "count": count, "agent": "claude", "spaceId": "zz-proj" }))
                .send(),
        ).unwrap();
        assert_eq!(res.status().as_u16(), 200);
        let job = res.headers()["x-job-id"].to_str().unwrap().to_string();
        self.rt.spawn(async move {
            let _ = res.text().await;
        });
        self.spaces();
        let ids: Vec<String> = (0..count).map(|i| format!("zz-ph-{job}-{i}")).collect();
        let mut changes = vec![json!({ "path": format!("runs/zz-{job}"), "value": { "id": format!("zz-{job}"), "serverJobId": job, "spaceId": "zz-proj", "designIds": ids, "instruction": "zz", "createdAt": 1 } })];
        for (i, id) in ids.iter().enumerate() {
            changes.push(json!({ "path": format!("designs/{id}"), "value": { "id": id, "spaceId": "zz-proj", "kind": "variation", "title": format!("Direction {}", i + 1), "rationale": "", "html": "", "status": "generating", "position": { "x": 0, "y": 0 }, "createdAt": i + 1 } }));
        }
        self.put_state(Value::Array(changes));
        for _ in 0..200 {
            let items = self.get("/api/agent-queue")["items"].as_array().cloned().unwrap_or_default();
            if items.iter().filter(|i| i["jobId"] == job.as_str() && i["annotationId"].is_string()).count() == count {
                return job;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("job {job} not queued");
    }

    fn spaces(&self) {
        self.put_state(json!([
            { "path": "spaces/zz-proj", "value": { "id": "zz-proj", "name": "Shop", "parentSpaceId": "root", "kind": "project", "createdAt": 1 } },
            { "path": "spaces/zz-expl", "value": { "id": "zz-expl", "name": "Checkout", "parentSpaceId": "zz-proj", "createdAt": 2 } },
        ]));
    }

    fn design(&self, id: &str) -> Value {
        let state = serde_json::to_value(self.store.read(0, None)).unwrap();
        state["records"][format!("designs/{id}")]["value"].clone()
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(h) = self.handle.take() {
            self.rt.block_on(h.shutdown());
        }
        let _ = &self.data;
    }
}

fn dead_url() -> String {
    format!("http://127.0.0.1:{}", free_port())
}

// ---- reachability (AN-CLI-630, -231, -642) -----------------------------------------------------------------

#[test]
fn cli_630_nothing_answers() {
    let home = Home::new();
    let url = dead_url();
    let port = url.rsplit(':').next().unwrap().to_string();
    for args in [&["notes", "list"][..], &["notes", "watch"], &["queue", "list"], &["cards", "list"], &["status"], &["notes", "show", "n1"]] {
        let o = home.run(&url, args, None, &[]);
        assert_eq!(o.code, 3, "{args:?}: {}", o.stderr);
        assert!(o.took < Duration::from_secs(3), "{:?}", o.took);
        assert_eq!(o.stderr.trim_end(), format!("prototypie: Prototypie is not running (nothing answers on 127.0.0.1:{port}). Start the app, then try again."));
    }
    // A process holding the data dir's lock: the message names its pid.
    let _store = proto_core::Store::open(&home.dir.path().join("clidata")).unwrap();
    let o = home.run(&url, &["notes", "list"], None, &[]);
    assert_eq!(o.code, 3);
    assert_eq!(
        o.stderr.trim_end(),
        format!("prototypie: Prototypie is running (pid {}) but its server does not answer on 127.0.0.1:{port}. Another program may be using the port; the app shows a banner then.", std::process::id())
    );
    // --json: the error as JSON on stderr; status also says running: false on stdout.
    let o = Home::new().run(&url, &["status", "--json"], None, &[]);
    assert_eq!(o.code, 3);
    let err: Value = serde_json::from_str(&o.stderr).unwrap();
    assert_eq!((err["code"].as_str(), err["exitCode"].as_i64()), (Some("not_running"), Some(3)));
    assert_eq!(o.json()["running"], false);
}

#[test]
fn cli_231_a_server_that_never_answers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut keep = Vec::new();
        for c in listener.incoming().flatten() {
            keep.push(c);
        }
    });
    let home = Home::new();
    home.run(&format!("http://127.0.0.1:{port}"), &["--version"], None, &[]);
    let o = home.run(&format!("http://127.0.0.1:{port}"), &["notes", "list"], None, &[]);
    assert_eq!(o.code, 3);
    assert_eq!(o.stderr.trim_end(), format!("prototypie: The server on 127.0.0.1:{port} did not answer in time."));
    // 10 s (± 1 s); a little more slack for a busy test machine.
    assert!(o.took >= Duration::from_secs(9) && o.took <= Duration::from_millis(11_500), "{:?}", o.took);
}

#[test]
fn cli_231_a_server_without_the_notes_service() {
    // Any HTTP server whose /api/notes routes are missing (404 without the service's code).
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut c in listener.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let _ = std::io::Read::read(&mut c, &mut buf);
            let _ = c.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    let home = Home::new();
    for args in [&["notes", "list"][..], &["status"]] {
        let o = home.run(&format!("http://127.0.0.1:{port}"), args, None, &[]);
        assert_eq!(o.code, 3, "{}", o.stderr);
        assert_eq!(o.stderr.trim_end(), format!("prototypie: The server on 127.0.0.1:{port} has no notes service. Update Prototypie."));
    }
}

#[test]
fn cli_642_only_this_computer() {
    let home = Home::new();
    for bad in ["http://192.168.1.5:3124", "https://127.0.0.1:3124"] {
        let o = home.run(&dead_url(), &["notes", "list", "--server", bad], None, &[]);
        assert_eq!(o.code, 2);
        assert!(o.took < Duration::from_secs(1));
        assert_eq!(o.stderr.trim_end(), format!("prototypie: Invalid server address \"{bad}\" (use http://127.0.0.1:PORT; only this computer is allowed)"));
    }
    let o = home.run("http://10.0.0.1:1", &["status"], None, &[]);
    assert_eq!(o.code, 2, "the environment variable is checked the same way");
}

// ---- notes (AN-CLI-631…637, -647) --------------------------------------------------------------------------

#[test]
fn cli_631_list_and_json() {
    let app = App::start();
    let a = app.note("prototypie-rune://zzspace/review-dock", "Make it smaller\n— Rune element `review-dock` at crates/x.rs:33 (x)");
    let b = app.note("http://localhost:3124/", "[prototypie job J1abc#0] make it pop · COMBINE …");
    let o = app.cli(&["notes", "list"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.starts_with("2 pending notes\n\n"), "{}", o.stdout);
    for n in [&a, &b] {
        assert!(o.stdout.contains(n["id"].as_str().unwrap()));
    }
    assert!(o.stdout.contains("  on: button \"Review\" (header-actions > review-dock)\n"), "{}", o.stdout);
    assert!(o.stdout.contains("  job J1abc#0: prototypie queue show J1abc#0\n"), "{}", o.stdout);
    let o = app.cli(&["notes", "list", "--json"]);
    assert_eq!(o.json(), app.get("/api/notes?status=pending&limit=100"));
    assert!(o.stdout.contains("\n  \"notes\""), "2-space indent");
    // AN-CLI-003: show --json equals the service's answer.
    let id = a["id"].as_str().unwrap();
    assert_eq!(app.cli(&["notes", "show", id, "--json"]).json(), app.get(&format!("/api/notes/{id}")));
    // Empty and other states.
    assert_eq!(app.cli(&["notes", "list", "--status", "resolved"]).stdout, "No resolved notes.\n");
    assert!(app.cli(&["notes", "list", "--all"]).stdout.starts_with("2 notes\n"));
    assert!(app.cli(&["notes", "list", "--status", "active", "--tag", "job"]).stdout.starts_with("1 open note\n"));
    let o = app.cli(&["notes", "list", "--limit", "1"]);
    assert!(o.stdout.ends_with("(more notes: raise --limit)\n"), "{}", o.stdout);
}

#[test]
fn cli_632_watch_without_new_notes() {
    let app = App::start();
    app.note("prototypie-rune://zz", "waiting");
    let o = app.cli(&["notes", "watch"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.took < Duration::from_secs(1));
    let c = app.cursor();
    assert!(o.stdout.starts_with(&format!("1 new note (cursor {c})\n\n")), "{}", o.stdout);
    assert!(o.stdout.ends_with(&format!("Next: prototypie notes watch --after {c}\n")), "{}", o.stdout);
    let o = app.cli(&["notes", "watch", "--after", &c.to_string(), "--timeout", "2s"]);
    assert_eq!(o.code, 0);
    assert!(o.took >= Duration::from_millis(1900) && o.took <= Duration::from_millis(2600), "{:?}", o.took);
    assert_eq!(o.stdout, format!("No new notes in 2s (cursor {c}).\nNext: prototypie notes watch --after {c} --timeout 2s\n"));
    // The next command carries the filters, --ack, non-default times and --as.
    let o = app.cli(&["--as", "w9", "notes", "watch", "--after", &c.to_string(), "--timeout", "1", "--batch", "0", "--tag", "job", "--ack"]);
    assert_eq!(o.stdout.lines().last().unwrap(), format!("Next: prototypie notes watch --after {c} --tag job --ack --timeout 1s --batch 0s --as w9"));
    let o = app.cli(&["notes", "watch", "--after", &c.to_string(), "--timeout", "1s", "--json"]);
    let v = o.json();
    assert_eq!((v["count"].as_u64(), v["timedOut"].as_bool()), (Some(0), Some(true)));
    assert_eq!(v["next"], format!("prototypie notes watch --after {c} --timeout 1s"));
    let o = app.cli(&["notes", "watch", "--timeout", "301s"]);
    assert_eq!((o.code, o.stderr.trim_end()), (2, "prototypie: --timeout must be a duration from 1s to 300s (for example 90s or 2m)."));
    let o = app.cli(&["notes", "watch", "--batch", "2m"]);
    assert_eq!((o.code, o.stderr.trim_end()), (2, "prototypie: --batch must be a duration from 0s to 60s (for example 5s or 1m)."));
}

#[test]
fn cli_633_two_ack_watchers_one_note() {
    let app = App::start();
    let c = app.cursor().to_string();
    let spawn = |name: &str| {
        let (url, home, data) = (app.url.clone(), app.home.home(), app.home.dir.path().join("clidata"));
        let args: Vec<String> = ["--as", name, "notes", "watch", "--ack", "--after", &c, "--batch", "1s", "--timeout", "20s"].iter().map(|s| s.to_string()).collect();
        std::thread::spawn(move || {
            let o = Command::new(BIN).args(&args).env("PROTOTYPIE_SERVER", url).env("HOME", home).env("PROTOTYPIE_DATA", data).output().unwrap();
            (o.status.code(), String::from_utf8_lossy(&o.stdout).into_owned())
        })
    };
    let (w1, w2) = (spawn("w1"), spawn("w2"));
    for _ in 0..100 {
        if app.get("/api/notes/info")["watchers"].as_array().unwrap().iter().filter(|w| w["waiting"] == true).count() == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let n = app.note("prototypie-rune://zz", "only one of you");
    let id = n["id"].as_str().unwrap();
    let (r1, r2) = (w1.join().unwrap(), w2.join().unwrap());
    assert_eq!((r1.0, r2.0), (Some(0), Some(0)));
    let shown = [&r1.1, &r2.1].iter().filter(|o| o.contains(id)).count();
    assert_eq!(shown, 1, "{}\n---\n{}", r1.1, r2.1);
    let note = app.get(&format!("/api/notes/{id}"))["note"].clone();
    assert_eq!(note["status"], "acknowledged");
}

#[test]
fn cli_634_ack_twice_and_force() {
    let app = App::start();
    let id = app.note("prototypie-rune://zz", "a")["id"].as_str().unwrap().to_string();
    let o = app.cli(&["notes", "ack", &id]);
    assert_eq!((o.code, o.stdout.as_str()), (0, format!("Acknowledged {id}\n").as_str()));
    let o = app.cli(&["notes", "ack", &id]);
    assert_eq!(o.code, 5);
    assert_eq!(o.stderr.trim_end(), format!("prototypie: Note {id} is already acknowledged (another agent may have taken it; --force takes it anyway)."));
    assert_eq!(app.cli(&["notes", "ack", &id, "--force"]).code, 0);
    // Several ids: the largest code; the others are still handled; JSON lists both.
    let other = app.note("prototypie-rune://zz", "b")["id"].as_str().unwrap().to_string();
    let o = app.cli(&["notes", "ack", &id, "nnope000000", &other, "--json"]);
    assert_eq!(o.code, 5);
    let v = o.json();
    assert_eq!(v["notes"].as_array().unwrap().len(), 1);
    assert_eq!(v["errors"].as_array().unwrap().len(), 2);
    assert_eq!(app.get(&format!("/api/notes/{other}"))["note"]["status"], "acknowledged");
    app.post(&format!("/api/notes/{other}/status"), json!({ "status": "resolved" }));
    let o = app.cli(&["notes", "ack", &other]);
    assert_eq!((o.code, o.stderr.trim_end()), (5, format!("prototypie: Note {other} is already resolved.").as_str()));
}

#[test]
fn cli_635_replies_are_signed() {
    let app = App::start();
    let id = app.note("prototypie-rune://zz", "a")["id"].as_str().unwrap().to_string();
    let o = app.cli(&["--as", "proto-worker-8", "notes", "reply", &id, "on", "it"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, format!("Replied to {id} (1 message in thread)\n"));
    let t = app.get(&format!("/api/notes/{id}"))["note"]["thread"].clone();
    let last = t.as_array().unwrap().last().unwrap();
    assert_eq!((last["role"].as_str(), last["content"].as_str()), (Some("agent"), Some("proto-worker-8: on it")));
    // Already signed: unchanged; from the environment; default name: no signature; human role: never.
    app.home.run(&app.url, &["notes", "reply", &id, "proto-lead: done"], None, &[("PROTOTYPIE_AGENT", "proto-lead")]);
    app.cli(&["notes", "reply", &id, "plain"]);
    app.cli(&["--as", "x", "notes", "reply", &id, "--role", "human", "from me"]);
    let o = app.cli_in(&["--as", "w2", "notes", "reply", &id, "--file", "-"], b"from stdin\n");
    assert_eq!(o.code, 0, "{}", o.stderr);
    let t = app.get(&format!("/api/notes/{id}"))["note"]["thread"].clone();
    let contents: Vec<&str> = t.as_array().unwrap().iter().map(|m| m["content"].as_str().unwrap()).collect();
    assert_eq!(contents, ["proto-worker-8: on it", "proto-lead: done", "plain", "from me", "w2: from stdin\n"]);
    let o = app.cli(&["notes", "reply", &id, "   "]);
    assert_eq!((o.code, o.stderr.trim_end()), (2, "prototypie: The text is empty."));
    let long = "x".repeat(20_001);
    let o = app.cli(&["notes", "reply", &id, &long]);
    assert_eq!((o.code, o.stderr.trim_end()), (2, "prototypie: The text is longer than 20000 characters."));
}

#[test]
fn cli_636_dismiss_needs_a_reason() {
    // A dead port: had the command sent anything, it would end with 3, not 2.
    let o = Home::new().run(&dead_url(), &["notes", "dismiss", "n1"], None, &[]);
    assert_eq!(o.code, 2);
    assert_eq!(o.stderr.trim_end(), "prototypie: dismiss needs --reason TEXT (the reason goes into the note's thread).");
    let app = App::start();
    let id = app.note("prototypie-rune://zz", "a")["id"].as_str().unwrap().to_string();
    let o = app.cli(&["--as", "w3", "notes", "dismiss", &id, "--reason", "Removed last week."]);
    assert_eq!((o.code, o.stdout.as_str()), (0, format!("Dismissed {id} (reason added to the thread)\n").as_str()));
    let n = app.get(&format!("/api/notes/{id}"))["note"].clone();
    assert_eq!((n["status"].as_str(), n["closedBy"].as_str()), (Some("dismissed"), Some("w3")));
    assert_eq!(n["thread"][0]["content"], "w3: Removed last week.");
}

#[test]
fn cli_637_show() {
    let app = App::start();
    let o = app.cli(&["notes", "show", "nnope000000"]);
    assert_eq!((o.code, o.stderr.trim_end()), (4, "prototypie: No note with id nnope000000."));
    let (_, v) = app.post("/api/notes", json!({ "location": "prototypie-rune://zzspace/review-dock", "comment": "Make it smaller", "element": "button \"Review\"",
        "elementPath": "header-actions > review-dock", "designId": "zzd1", "target": { "sourceLocations": "crates/x.rs:33" } }));
    let id = v["note"]["id"].as_str().unwrap().to_string();
    app.cli(&["--as", "proto-worker-3", "notes", "reply", &id, "on it"]);
    app.cli(&["--as", "proto-worker-3", "notes", "resolve", &id]);
    let o = app.cli(&["notes", "show", &id]);
    assert_eq!(o.code, 0);
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines[0], format!("Note {id} · resolved · feedback"));
    assert_eq!(lines[1], "Location   prototypie-rune://zzspace/review-dock");
    assert_eq!(lines[2], "Element    button \"Review\"");
    assert_eq!(lines[3], "Path       header-actions > review-dock");
    assert_eq!(lines[4], "Source     crates/x.rs:33");
    assert_eq!(lines[5], "Design     zzd1");
    assert!(lines[6].starts_with("Created    ") && lines[6].contains("   Updated "), "{}", lines[6]);
    assert!(lines[7].starts_with("Closed     resolved by proto-worker-3, "), "{}", lines[7]);
    assert_eq!(&lines[8..11], ["", "Make it smaller", ""]);
    assert_eq!(lines[11], "Thread (1)");
    assert!(lines[12].starts_with("  agent · "));
    assert_eq!(lines[13], "    proto-worker-3: on it");
}

#[test]
fn cli_215_resolve_with_a_summary() {
    let app = App::start();
    let id = app.note("prototypie-rune://zz", "a")["id"].as_str().unwrap().to_string();
    let o = app.cli(&["--as", "proto-lead", "notes", "resolve", &id, "--summary", "Made it 20% smaller."]);
    assert_eq!((o.code, o.stdout.as_str()), (0, format!("Resolved {id}\n").as_str()));
    let n = app.get(&format!("/api/notes/{id}"))["note"].clone();
    assert_eq!((n["status"].as_str(), n["closedBy"].as_str()), (Some("resolved"), Some("proto-lead")));
    assert_eq!(n["thread"].as_array().unwrap().last().unwrap()["content"], "proto-lead: Made it 20% smaller.");
    let o = app.cli_in(&["--as", "proto-lead", "notes", "resolve", &id, "--summary-file", "-"], b"and the label");
    assert_eq!((o.code, o.stdout.as_str()), (0, format!("Note {id} was already resolved.\n").as_str()));
    let n = app.get(&format!("/api/notes/{id}"))["note"].clone();
    assert_eq!(n["thread"].as_array().unwrap().len(), 2, "the summary is still added");
}

#[test]
fn cli_647_control_characters_are_shown_escaped() {
    let app = App::start();
    let id = app.note("prototypie-rune://zz", "clear \u{1b}[2J screen")["id"].as_str().unwrap().to_string();
    let o = app.cli(&["notes", "list"]);
    assert!(o.stdout.contains("clear \\u{1b}[2J screen"), "{}", o.stdout);
    assert!(!o.stdout.contains('\u{1b}'));
    let v = app.cli(&["notes", "show", &id, "--json"]).json();
    assert_eq!(v["note"]["comment"], "clear \u{1b}[2J screen");
}

#[test]
fn cli_218_collections_and_long_comments() {
    let app = App::start();
    app.note("prototypie-rune://a", "one");
    let long: String = (0..10).map(|i| format!("line {i}\n")).collect();
    let n = app.note("http://localhost:3124/", &long);
    let o = app.cli(&["notes", "collections"]);
    assert_eq!(o.stdout.lines().count(), 2, "{}", o.stdout);
    assert!(o.stdout.lines().any(|l| l.ends_with("1 pending / 1 notes  prototypie-rune://a") && l.contains("  active  ")), "{}", o.stdout);
    let o = app.cli(&["notes", "list"]);
    let id = n["id"].as_str().unwrap();
    assert!(o.stdout.contains(&format!("  line 5\n  … (prototypie notes show {id})\n")), "{}", o.stdout);
    assert!(!o.stdout.contains("line 6"));
    assert!(app.cli(&["notes", "show", id]).stdout.contains("line 9"), "show never shortens");
}

#[test]
fn cli_watch_replies_and_follow() {
    let app = App::start();
    // A pending task note (the panel lists open notes); a person answers it from the panel.
    let id = app.note("http://localhost:3124/", "task")["id"].as_str().unwrap().to_string();
    let c = app.cursor().to_string();
    let (url, home, data) = (app.url.clone(), app.home.home(), app.home.dir.path().join("clidata"));
    let c2 = c.clone();
    let waiter = std::thread::spawn(move || {
        Command::new(BIN).args(["notes", "watch", "--replies", "--after", &c2, "--batch", "0s", "--timeout", "10s"]).env("PROTOTYPIE_SERVER", url).env("HOME", home).env("PROTOTYPIE_DATA", data).output().unwrap()
    });
    std::thread::sleep(Duration::from_millis(500));
    let (s, v) = app.post(&format!("/api/agent-tasks/{id}/notes"), json!({ "text": "also the footer" }));
    assert_eq!(s, 200, "{v}");
    let o = waiter.join().unwrap();
    let text = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(text.contains(&id) && text.contains("new reply from a person in the thread"), "{text}");
    assert!(text.lines().last().unwrap().contains(" --replies"), "{text}");
    // --follow --json: one compact line per answer until stopped.
    let mut child = Command::new(BIN)
        .args(["notes", "watch", "--follow", "--json", "--timeout", "1s", "--after", &app.cursor().to_string()])
        .env("PROTOTYPIE_SERVER", &app.url).env("HOME", app.home.home()).env("PROTOTYPIE_DATA", app.home.dir.path().join("clidata"))
        .stdout(Stdio::piped()).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(2600));
    child.kill().unwrap();
    let o = child.wait_with_output().unwrap();
    let lines: Vec<Value> = String::from_utf8_lossy(&o.stdout).lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert!(lines.len() >= 2, "{lines:?}");
    assert!(lines.iter().all(|l| l["timedOut"] == true && l["next"].is_string()));
}

#[test]
fn cli_watch_follow_ends_when_the_app_quits() {
    let mut app = App::start();
    let child = Command::new(BIN)
        .args(["notes", "watch", "--follow", "--timeout", "30s", "--after", "0"])
        .env("PROTOTYPIE_SERVER", &app.url).env("HOME", app.home.home()).env("PROTOTYPIE_DATA", app.home.dir.path().join("clidata"))
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(700));
    let h = app.handle.take().unwrap();
    app.rt.block_on(h.shutdown());
    let t0 = Instant::now();
    let o = child.wait_with_output().unwrap();
    assert!(t0.elapsed() < Duration::from_secs(5));
    assert_eq!(o.status.code(), Some(3));
    assert_eq!(String::from_utf8_lossy(&o.stderr).trim_end(), "prototypie: Prototypie quit while waiting for notes.");
}

// ---- queue (AN-CLI-638…640, -221, -222) --------------------------------------------------------------------

#[test]
fn cli_638_claim() {
    let app = App::start();
    let job = app.job(1);
    let r = format!("{job}#0");
    let o = app.cli(&["queue", "claim", &r, "--as", "w1"]);
    assert_eq!((o.code, o.stdout.as_str()), (0, format!("Claimed {r} as w1\n").as_str()), "{}", o.stderr);
    let items = app.get("/api/agent-queue")["items"].clone();
    assert_eq!(items[0]["agent"], "w1");
    let o = app.cli(&["queue", "deliver", &format!("{job}/0"), "--title", "T", "--html", "-"]);
    assert_eq!(o.code, 2, "--html - with an empty stdin: {}", o.stderr);
    assert_eq!(o.stderr.trim_end(), "prototypie: The HTML is empty.");
    let o = app.cli_in(&["queue", "deliver", &r, "--title", "T", "--html", "-"], b"<main>x</main>");
    assert_eq!(o.code, 0, "{}", o.stderr);
    let o = app.cli(&["queue", "claim", &r, "--as", "w1"]);
    assert_eq!(o.code, 5, "{}", o.stderr);
    assert_eq!(o.stderr.trim_end(), format!("prototypie: Queue item {r} is no longer pending (done or cancelled)."));
    let o = app.cli(&["queue", "claim", "nojob#3"]);
    assert_eq!((o.code, o.stderr.trim_end()), (4, "prototypie: No queue item nojob#3 is waiting (done, cancelled or expired)."));
    let o = app.cli(&["queue", "claim", "J1abc"]);
    assert_eq!((o.code, o.stderr.trim_end()), (2, "prototypie: \"J1abc\" is not a queue item (use JOB#INDEX, for example J1abc#0)."));
}

#[test]
fn cli_639_deliver_5_mib_from_stdin() {
    let app = App::start();
    let job = app.job(2);
    let r = format!("{job}#1");
    let mut html = String::from("<!doctype html><meta name=\"viewport\" content=\"width=390\"><main>");
    while html.len() < 5 * 1024 * 1024 {
        html.push_str("<p>Größer · bolder price block</p>\n");
    }
    html.push_str("</main>");
    let o = app.cli_in(&["--as", "proto-worker-2", "queue", "deliver", &r, "--title", "Bolder price block", "--rationale", "Price first.", "--html", "-"], html.as_bytes());
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, format!("Delivered {r}: \"Bolder price block\"\n"));
    let d = app.design(&format!("zz-ph-{job}-1"));
    assert_eq!(d["html"].as_str().unwrap(), html, "byte-identical");
    assert_eq!(d["title"], "Bolder price block");
    let note_id = app.get("/api/notes?tag=job&status=all")["notes"].as_array().unwrap().iter().find(|n| n["tag"]["index"] == 1).unwrap()["id"].clone();
    let mut status = Value::Null;
    for _ in 0..100 {
        status = app.get(&format!("/api/notes/{}", note_id.as_str().unwrap()))["note"].clone();
        if status["status"] == "resolved" {
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!((status["status"].as_str(), status["closedBy"].as_str()), (Some("resolved"), Some("agent")));
    let o = app.cli_in(&["queue", "deliver", &r, "--title", "Again", "--html", "-"], b"<p>x</p>");
    assert_eq!(o.code, 4, "{}", o.stderr);
    // Missing HTML / title, invalid viewport: exit 2 before any request.
    let o = app.cli(&["queue", "deliver", &format!("{job}#0"), "--title", "T"]);
    assert_eq!((o.code, o.stderr.trim_end()), (2, "prototypie: Pass the HTML with --html FILE (or --html - to read stdin)."));
    let o = app.cli(&["queue", "deliver", &format!("{job}#0"), "--html", "-"]);
    assert_eq!(o.code, 2);
    assert!(o.stderr.contains("needs --title TEXT"), "{}", o.stderr);
    let dir = app.home.dir.path().to_string_lossy().into_owned();
    let o = app.cli(&["queue", "deliver", &format!("{job}#0"), "--title", "T", "--html", &dir]);
    assert_eq!((o.code, o.stderr.trim_end()), (2, format!("prototypie: Can't read {dir}: is a directory.").as_str()));
    let o = app.cli(&["queue", "deliver", &format!("{job}#0"), "--title", "T", "--html", "/nonexistent/zz.html"]);
    assert_eq!(o.code, 2);
    assert!(o.stderr.starts_with("prototypie: Can't read /nonexistent/zz.html: "), "{}", o.stderr);
    let o = app.cli_in(&["queue", "deliver", &format!("{job}#0"), "--title", "T", "--html", "-"], &[0xff, 0xfe, b'x']);
    assert_eq!((o.code, o.stderr.trim_end()), (2, "prototypie: Can't read -: not UTF-8 text."));
    // A refused body (400 with issues) → exit 2 with the first problem.
    let o = app.cli_in(&["queue", "deliver", &format!("{job}#0"), "--title", "", "--html", "-"], b"<p>x</p>");
    assert_eq!(o.code, 2);
    assert!(o.stderr.starts_with("prototypie: title: "), "{}", o.stderr);
}

#[test]
fn cli_640_deleted_placeholder() {
    let app = App::start();
    let job = app.job(1);
    // Delete the card in the store (as the app would), without the route's own cancel.
    app.store.apply(vec![proto_core::Change::delete(format!("designs/zz-ph-{job}-0"))], proto_core::ChangeMeta::client("zz-cli")).unwrap();
    let o = app.cli_in(&["queue", "deliver", &format!("{job}#0"), "--title", "T", "--html", "-"], b"<p>x</p>");
    assert_eq!(o.code, 5, "{}", o.stderr);
    assert_eq!(
        o.stderr.trim_end(),
        format!("prototypie: The card for {job}#0 was deleted in the app, so nothing was placed. Create a card with \"prototypie cards create\" only if the design is still wanted.")
    );
}

#[test]
fn cli_221_queue_list_and_show() {
    let app = App::start();
    assert_eq!(app.cli(&["queue", "list"]).stdout, "No queue items are waiting.\n");
    let job = app.job(2);
    app.cli(&["queue", "claim", &format!("{job}#0"), "--as", "proto-worker-2"]);
    let o = app.cli(&["queue", "list"]);
    assert_eq!(o.code, 0);
    assert!(o.stdout.starts_with("2 queue items waiting\n"), "{}", o.stdout);
    assert!(o.stdout.contains(&format!("{job}#0  claimed by proto-worker-2  ")), "{}", o.stdout);
    assert!(o.stdout.contains(&format!("{job}#1  unclaimed  ")), "{}", o.stdout);
    assert!(o.stdout.contains("  space: Main canvas › Shop"), "{}", o.stdout);
    assert!(o.stdout.contains("  zz-cli make it pop\n"), "{}", o.stdout);
    assert_eq!(app.cli(&["queue", "list", "--json"]).json(), app.get("/api/agent-queue"));
    let o = app.cli(&["queue", "show", &format!("{job}#1")]);
    assert!(o.stdout.lines().any(|l| l.starts_with("jobId") && l.ends_with(&job)), "{}", o.stdout);
    let o = app.cli(&["queue", "show", &format!("{job}#1"), "--base-html"]);
    assert_eq!((o.code, o.stderr.trim_end()), (4, format!("prototypie: Queue item {job}#1 has no base HTML.").as_str()));
    assert_eq!(app.cli(&["queue", "show", "nojob#0"]).code, 4);
    // An edit job carries the card's HTML as baseHtml: printed byte for byte.
    let base = "<!doctype html>\n<main>Base · design ünïcode</main>\n\n";
    app.put_state(json!([{ "path": "designs/zz-edit", "value": { "id": "zz-edit", "spaceId": "zz-proj", "kind": "variation", "title": "E", "rationale": "", "html": base, "status": "ready", "position": { "x": 0, "y": 0 }, "createdAt": 9 } }]));
    let (s, v) = app.post("/api/edit", json!({ "designId": "zz-edit", "instruction": "darker", "agent": "claude" }));
    assert_eq!(s, 200, "{v}");
    let edit = v["jobId"].as_str().unwrap().to_string();
    for _ in 0..200 {
        if app.get("/api/agent-queue")["items"].as_array().unwrap().iter().any(|i| i["jobId"] == edit.as_str()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let o = app.cli(&["queue", "show", &format!("{edit}#0"), "--base-html"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, base);
    let o = app.cli(&["queue", "show", &format!("{edit}#0")]);
    assert!(o.stdout.contains(&format!("{} characters (prototypie queue show {edit}#0 --base-html)", base.chars().count())), "{}", o.stdout);
    assert!(!o.stdout.contains("ünïcode"));
}

// ---- cards (AN-CLI-641, -225, -226) ------------------------------------------------------------------------

#[test]
fn cli_641_cards_create() {
    let app = App::start();
    app.spaces();
    let f = app.home.dir.path().join("f.html");
    std::fs::write(&f, "<!doctype html><main>Compact header</main>").unwrap();
    let fs = f.to_string_lossy().into_owned();
    let o = app.cli(&["cards", "create", "--space", "zz-proj", "--title", "Compact header", "--html", &fs]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let state = serde_json::to_value(app.store.read(0, None)).unwrap();
    let id = o.stdout.trim_end().rsplit(": ").next().unwrap().to_string();
    assert!(o.stdout.starts_with("Created 1 card in space zz-proj: "), "{}", o.stdout);
    assert_eq!(state["records"][format!("designs/{id}")]["value"]["title"], "Compact header");
    // Without --space/--near: exit 2, no request (a dead port would give 3).
    let o = Home::new().run(&dead_url(), &["cards", "create", "--title", "T", "--html", &fs], None, &[]);
    assert_eq!((o.code, o.stderr.trim_end()), (2, "prototypie: Pass --space ID or --near ID (prototypie cards list shows the spaces)."));
    let o = app.cli(&["cards", "create", "--space", "zz-nope", "--title", "T", "--html", &fs]);
    assert_eq!((o.code, o.stderr.trim_end()), (4, "prototypie: Unknown space zz-nope."));
    let o = app.cli(&["cards", "create", "--space", "root", "--title", "T", "--html", &fs]);
    assert_eq!(o.code, 2, "{}", o.stderr);
    // Several cards from a file; a new exploration.
    let body = json!({ "cards": [{ "title": "One", "html": "<p>1</p>" }, { "title": "Two", "html": "<p>2</p>" }] });
    let o = app.cli_in(&["cards", "create", "--from", "-", "--space", "zz-expl"], body.to_string().as_bytes());
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.starts_with("Created 2 cards in space zz-expl: "), "{}", o.stdout);
    let o = app.cli(&["cards", "create", "--new-exploration", "Checkout ideas", "--parent-space", "zz-proj", "--title", "Idea", "--html", &fs]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.starts_with("Created exploration ") && o.stdout.contains(" with 1 card: "), "{}", o.stdout);
}

#[test]
fn cli_225_cards_list() {
    let app = App::start();
    app.spaces();
    app.put_state(json!([{ "path": "designs/zz-c1", "value": { "id": "zz-c1", "spaceId": "zz-expl", "kind": "page", "title": "Checkout page", "rationale": "", "html": "<p/>", "status": "ready", "position": { "x": 0, "y": 0 }, "createdAt": 3 } }]));
    let o = app.cli(&["cards", "list"]);
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines[0], "root  -  0 cards  Main canvas");
    assert!(lines.contains(&"zz-proj  project  0 cards  Main canvas › Shop"), "{}", o.stdout);
    assert!(lines.contains(&"zz-expl  -  1 card  Main canvas › Shop › Checkout"), "{}", o.stdout);
    let o = app.cli(&["cards", "list", "--space", "zz-expl"]);
    assert_eq!(o.stdout, "zz-c1  ready  page  Checkout page\n");
    let o = app.cli(&["cards", "list", "--space", "zz-x"]);
    assert_eq!((o.code, o.stderr.trim_end()), (4, "prototypie: Unknown space zz-x."));
}

// ---- status, skill, help, exit codes (AN-CLI-643, -644, -646, -229, -230, -202, -203) ---------------------

#[test]
fn cli_643_status() {
    let app = App::start();
    app.note("prototypie-rune://zz", "a");
    app.get("/api/notes/watch?timeout=1&by=proto-lead");
    app.cli(&["--version"]);
    let o = app.cli(&["status"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.took < Duration::from_secs(1), "AN-CLI-202 (generous limit): {:?}", o.took);
    let port = app.url.rsplit(':').next().unwrap();
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines[0], format!("Prototypie is running: 127.0.0.1:{port}, pid {}, version {}", std::process::id(), env!("CARGO_PKG_VERSION")));
    assert_eq!(lines[1], format!("Notes: 1 pending, 0 acknowledged (cursor {})", app.cursor()));
    assert_eq!(lines[2], "Queue: nothing waiting");
    assert_eq!(lines[3], "Agents waiting for notes: proto-lead (just now)");
    assert_eq!(lines[4], "Skill: not installed (run \"prototypie skill install\")");
    let v = app.cli(&["status", "--json"]).json();
    assert_eq!((v["running"].as_bool(), v["queueWaiting"].as_u64()), (Some(true), Some(0)));
    assert_eq!(v["info"]["service"], "prototypie-notes");
    // Option before environment (AN-CLI-203): a dead PROTOTYPIE_SERVER, the live --server.
    let o = app.home.run(&dead_url(), &["status", "--server", &app.url], None, &[]);
    assert_eq!(o.code, 0);
    let o = app.home.run(&dead_url(), &["status", &format!("--server={}", app.url.trim_start_matches("http://"))], None, &[]);
    assert_eq!(o.code, 0);
    // Installed, then outdated.
    std::fs::create_dir_all(app.home.home().join(".claude")).unwrap();
    assert_eq!(app.cli(&["skill", "install", "--no-link"]).code, 0);
    assert!(app.cli(&["status"]).stdout.contains("Skill: installed (~/.agents/skills/prototypie/SKILL.md, ~/.claude/skills/prototypie/SKILL.md)"));
    std::fs::write(app.home.home().join(".claude/skills/prototypie/SKILL.md"), "---\nname: prototypie\n---\nold").unwrap();
    assert!(app.cli(&["status"]).stdout.contains("Skill: outdated, run \"prototypie skill install\" ("));
}

#[test]
fn cli_644_skill_install() {
    let home = Home::new();
    let h = home.home();
    std::fs::create_dir_all(h.join(".claude")).unwrap();
    let o = home.run(&dead_url(), &["skill", "install"], None, &[]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let skill = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../skills/prototypie/SKILL.md")).unwrap();
    let a = h.join(".agents/skills/prototypie/SKILL.md");
    let c = h.join(".claude/skills/prototypie/SKILL.md");
    assert_eq!(std::fs::read_to_string(&a).unwrap(), skill);
    assert_eq!(std::fs::read_to_string(&c).unwrap(), skill);
    assert!(!h.join(".codex").exists());
    let link = h.join(".local/bin/prototypie");
    assert_eq!(std::fs::read_link(&link).unwrap(), std::fs::canonicalize(BIN).unwrap());
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines[0], "Installed the Prototypie skill:");
    assert_eq!(lines[1], format!("  {}", a.display()));
    assert_eq!(lines[2], format!("  {}", c.display()));
    assert_eq!(lines[3], format!("Linked {} -> {}", link.display(), std::fs::canonicalize(BIN).unwrap().display()));
    assert!(lines[4].starts_with("Note: ~/.local/bin is not on PATH; call "));
    assert_eq!(*lines.last().unwrap(), format!("Read the skill now: {}", a.display()));
    // The second run overwrites our own file; a foreign SKILL.md stays, exit 1 with only that target.
    std::fs::write(&a, "---\nname: prototypie\ndescription: old\n---\n").unwrap();
    assert_eq!(home.run(&dead_url(), &["skill", "install"], None, &[]).code, 0);
    assert_eq!(std::fs::read_to_string(&a).unwrap(), skill);
    let other = home.dir.path().join("other");
    std::fs::create_dir_all(other.join("prototypie")).unwrap();
    std::fs::write(other.join("prototypie/SKILL.md"), "---\nname: something-else\n---\n").unwrap();
    let od = other.to_string_lossy().into_owned();
    let o = home.run(&dead_url(), &["skill", "install", "--dir", &od], None, &[]);
    assert_eq!(o.code, 1, "{}", o.stderr);
    assert_eq!(std::fs::read_to_string(other.join("prototypie/SKILL.md")).unwrap(), "---\nname: something-else\n---\n");
    assert_eq!(home.run(&dead_url(), &["skill", "install", "--dir", &od, "--force"], None, &[]).code, 0);
    // AN-CLI-505: a target whose prototypie folder is a symlink gets nothing.
    let linked = home.dir.path().join("linked");
    std::fs::create_dir_all(&linked).unwrap();
    std::os::unix::fs::symlink(home.dir.path().join("elsewhere"), linked.join("prototypie")).unwrap();
    let o = home.run(&dead_url(), &["skill", "install", "--dir", &linked.to_string_lossy(), "--json"], None, &[]);
    assert_eq!(o.code, 1);
    assert_eq!(o.json()["installed"], json!([]));
    assert!(!home.dir.path().join("elsewhere").exists());
    // skill show prints the embedded text unchanged.
    assert_eq!(home.run(&dead_url(), &["skill", "show"], None, &[]).stdout, skill);
}

#[test]
fn cli_229_help_and_usage() {
    let home = Home::new();
    home.run(&dead_url(), &["--version"], None, &[]);
    let main = home.run(&dead_url(), &["help"], None, &[]);
    assert_eq!(main.code, 0);
    assert!(main.took < Duration::from_secs(1), "AN-CLI-202 (generous limit): {:?}", main.took);
    assert!(main.stdout.starts_with("prototypie: work with the Prototypie app from a terminal or a coding agent.\n"));
    assert_eq!(home.run(&dead_url(), &["--help"], None, &[]).stdout, main.stdout);
    assert_eq!(home.run(&dead_url(), &["-h"], None, &[]).stdout, main.stdout);
    assert_eq!(home.run(&dead_url(), &["--version"], None, &[]).stdout, format!("prototypie {}\n", env!("CARGO_PKG_VERSION")));
    let g = home.run(&dead_url(), &["notes"], None, &[]);
    assert_eq!(g.code, 0);
    assert_eq!(g.stdout, home.run(&dead_url(), &["notes", "--help"], None, &[]).stdout);
    assert_eq!(g.stdout, home.run(&dead_url(), &["help", "notes"], None, &[]).stdout);
    let c = home.run(&dead_url(), &["notes", "watch", "--help"], None, &[]);
    assert_eq!(c.stdout, home.run(&dead_url(), &["help", "notes", "watch"], None, &[]).stdout);
    assert!(c.stdout.starts_with("Usage: prototypie notes watch"));
    for (args, msg) in [
        // (A bare unknown first word is the app's, AN-CLI-201; behind a CLI option it is the CLI's.)
        (&["--as", "w1", "frobnicate"][..], "prototypie: Unknown command \"frobnicate\". Run \"prototypie help\" for the commands."),
        (&["help", "frobnicate"], "prototypie: Unknown command \"frobnicate\". Run \"prototypie help\" for the commands."),
        (&["notes", "list", "--bogus"], "prototypie: Unknown option \"--bogus\" for \"prototypie notes list\". Run \"prototypie notes list --help\"."),
        (&["notes", "show"], "prototypie: \"prototypie notes show\" needs a note ID. Run \"prototypie notes show --help\"."),
        (&["notes", "list", "--status"], "prototypie: --status needs a value."),
    ] {
        let o = home.run(&dead_url(), args, None, &[]);
        assert_eq!((o.code, o.stderr.trim_end()), (2, msg), "{args:?}");
    }
    // --json: errors as JSON on stderr.
    let o = home.run(&dead_url(), &["notes", "show", "--json"], None, &[]);
    let e: Value = serde_json::from_str(&o.stderr).unwrap();
    assert_eq!((e["code"].as_str(), e["exitCode"].as_i64()), (Some("usage"), Some(2)));
}

#[test]
fn cli_230_exit_code_one() {
    // The server refuses (here: it can't save) → 1; a local write failure (skill) → 1.
    use std::os::unix::fs::PermissionsExt;
    let app = App::start();
    let id = app.note("prototypie-rune://zz", "a")["id"].as_str().unwrap().to_string();
    let notes = app.data.path().join("notes");
    std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o555)).unwrap();
    let o = app.cli(&["notes", "ack", &id]);
    std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(o.code, 1, "{}", o.stderr);
    assert!(o.stderr.starts_with("prototypie: The server refused the request (500): Couldn't save the notes: "), "{}", o.stderr);
    let ro = app.home.dir.path().join("ro");
    std::fs::create_dir_all(&ro).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    let o = app.cli(&["skill", "install", "--dir", &ro.to_string_lossy(), "--no-link"]);
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(o.code, 1, "{}", o.stderr);
}
