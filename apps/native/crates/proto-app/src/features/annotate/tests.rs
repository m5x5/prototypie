//! Annotate's acceptance tests (spec 01 AN-UI-203, spec 03 AN-SY-191, spec 04 §3.1–3.4), without a window and
//! without the real notes service: network tests talk to a small notes service of their own on port 0, or to a
//! dead port.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::{Key, KeyEvent, Mods, TextEdit};
use serde_json::{Value, json};

use super::client::{Addr, Client, Job, Outcome, parse_addr};
use super::logic::{self, Conn, Fact, Reach};
use super::model::{Message, Note, Status, Target};
use super::{place, source, wire};
use crate::app::state::AppState;

// ---- helpers ----

fn state() -> AppState {
    let mut s = AppState::new();
    s.use_store(crate::features::scenes::fixture());
    s.enter_space("zzfx", None);
    s
}

fn note(id: &str, space: &str, status: Status) -> Note {
    Note { id: id.into(), server_id: None, space: space.into(), comment: format!("c {id}"), targets: vec![], timestamp: 1.0, status, thread: vec![], window: [1280.0, 800.0] }
}

fn dock_target() -> Target {
    Target {
        element: "button \"Review\"".into(),
        element_path: "header-actions > review-dock".into(),
        full_path: "root/app/header-right/header-actions/review-dock".into(),
        key: Some("review-dock".into()),
        source: Some("crates/proto-app/src/features/review/dock.rs:33".into()),
        module: Some("features::review::dock".into()),
        rect: [100.0, 10.0, 80.0, 36.0],
        ..Target::default()
    }
}

fn key(k: Key, mods: Mods) -> KeyEvent {
    KeyEvent { key: k, mods }
}

fn esc() -> KeyEvent {
    key(Key::Escape, Mods::default())
}

fn cmd_shift_a() -> KeyEvent {
    key(Key::Command('a'), Mods { cmd: true, shift: true, ..Mods::default() })
}

fn wait_for(client: &Client, mut done: impl FnMut(&[Outcome]) -> bool, secs: f64) -> Vec<Outcome> {
    let start = Instant::now();
    let mut all = Vec::new();
    while start.elapsed().as_secs_f64() < secs {
        all.extend(client.take());
        if done(&all) {
            return all;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    all
}

// ---- a small notes service (spec 05 AN-CLI-1xx, the parts the app uses) ----

#[derive(Default)]
struct Mock {
    collections: Vec<Value>,
    notes: BTreeMap<String, Vec<Value>>,
    next: u32,
    /// Every request as `METHOD path`.
    log: Vec<String>,
    /// Refuse note creation with this text, this many times.
    refuse: Option<(String, usize)>,
    /// Hold note creation this long.
    delay_ms: u64,
    /// Bodies of created notes.
    bodies: Vec<Value>,
    /// Status changes: (id, status, by).
    statuses: Vec<(String, String, String)>,
    replies: Vec<(String, String, String)>,
    /// Answer status changes with an empty body.
    empty_status: bool,
}

struct Server {
    port: u16,
    mock: Arc<Mutex<Mock>>,
}

impl Server {
    fn start(m: Mock) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mock = Arc::new(Mutex::new(m));
        let shared = mock.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(mut conn) = conn else { continue };
                let shared = shared.clone();
                std::thread::spawn(move || {
                    let (method, path, body) = read_request(&mut conn);
                    let (status, out, delay) = route(&shared, &method, &path, &body);
                    if delay > 0 {
                        std::thread::sleep(Duration::from_millis(delay));
                    }
                    let text = out.map(|v| v.to_string()).unwrap_or_default();
                    let _ = write!(conn, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
                });
            }
        });
        Server { port, mock }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn count(&self, prefix: &str) -> usize {
        self.mock.lock().unwrap().log.iter().filter(|l| l.starts_with(prefix)).count()
    }
}

fn read_request(conn: &mut std::net::TcpStream) -> (String, String, Value) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = conn.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..at]).to_string();
            let len: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
            while buf.len() < at + 4 + len {
                let n = conn.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let mut words = head.split_whitespace();
            let (m, p) = (words.next().unwrap_or("").to_string(), words.next().unwrap_or("").to_string());
            let body = serde_json::from_slice(&buf[at + 4..]).unwrap_or(Value::Null);
            return (m, p, body);
        }
    }
    (String::new(), String::new(), Value::Null)
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap_or(b'?'));
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn find_note(m: &Mock, id: &str) -> Option<(String, usize)> {
    m.notes.iter().find_map(|(c, l)| l.iter().position(|n| n["id"] == id).map(|i| (c.clone(), i)))
}

fn route(shared: &Arc<Mutex<Mock>>, method: &str, path: &str, body: &Value) -> (u16, Option<Value>, u64) {
    let mut m = shared.lock().unwrap();
    m.log.push(format!("{method} {path}"));
    let (p, query) = path.split_once('?').unwrap_or((path, ""));
    let segs: Vec<String> = p.trim_start_matches('/').split('/').map(decode).collect();
    let segs: Vec<&str> = segs.iter().map(String::as_str).collect();
    match (method, segs.as_slice()) {
        ("GET", ["api", "notes", "collections"]) => {
            let loc = query.strip_prefix("location=").map(decode);
            let list: Vec<Value> = m.collections.iter().filter(|c| loc.as_deref().is_none_or(|l| c["location"] == l)).cloned().collect();
            (200, Some(json!({"collections": list})), 0)
        }
        ("POST", ["api", "notes", "collections"]) => {
            m.next += 1;
            let c = json!({"id": format!("c{}", m.next), "location": body["location"], "state": "active"});
            m.collections.push(c.clone());
            (201, Some(json!({"collection": c})), 0)
        }
        ("GET", ["api", "notes", "collections", cid]) => {
            if !m.collections.iter().any(|c| c["id"] == *cid) {
                return (404, Some(json!({"error": "Collection not found", "code": "collection_not_found"})), 0);
            }
            let notes = m.notes.get(*cid).cloned().unwrap_or_default();
            (200, Some(json!({"collection": {"id": cid}, "notes": notes})), 0)
        }
        ("POST", ["api", "notes", "collections", cid, "notes"]) => {
            if let Some((text, left)) = m.refuse.clone() {
                if left > 0 {
                    m.refuse = Some((text.clone(), left - 1));
                    return (400, Some(json!({"error": text, "code": "missing_fields"})), 0);
                }
            }
            if !m.collections.iter().any(|c| c["id"] == *cid) {
                return (404, Some(json!({"error": "Collection not found"})), 0);
            }
            m.next += 1;
            let mut n = body.clone();
            n["id"] = json!(format!("n{}", m.next));
            n["status"] = json!("pending");
            n["thread"] = json!([]);
            m.bodies.push(body.clone());
            m.notes.entry(cid.to_string()).or_default().push(n.clone());
            let delay = m.delay_ms;
            (201, Some(json!({"note": n})), delay)
        }
        ("POST", ["api", "notes", id, "status"]) => {
            let by = body["by"].as_str().unwrap_or("agent").to_string();
            let st = body["status"].as_str().unwrap_or("").to_string();
            m.statuses.push((id.to_string(), st.clone(), by));
            let Some((c, i)) = find_note(&m, id) else { return (404, Some(json!({"error": "Note not found"})), 0) };
            m.notes.get_mut(&c).unwrap()[i]["status"] = json!(st);
            if m.empty_status { (200, None, 0) } else { (200, Some(json!({"note": m.notes[&c][i]})), 0) }
        }
        ("POST", ["api", "notes", id, "thread"]) => {
            m.replies.push((id.to_string(), body["role"].as_str().unwrap_or("").into(), body["content"].as_str().unwrap_or("").into()));
            let Some((c, i)) = find_note(&m, id) else { return (404, Some(json!({"error": "Note not found"})), 0) };
            let entry = json!({"role": body["role"], "content": body["content"]});
            if let Some(t) = m.notes.get_mut(&c).unwrap()[i]["thread"].as_array_mut() {
                t.push(entry);
            }
            (201, Some(json!({"note": m.notes[&c][i]})), 0)
        }
        _ => (405, Some(json!({"error": "Method not allowed"})), 0),
    }
}

fn mock_with_collection(space: &str) -> Mock {
    Mock { collections: vec![json!({"id": "c0", "location": format!("prototypie-rune://{space}"), "state": "active"})], next: 10, ..Mock::default() }
}

// ---- AN-UI-031 / AN-IF-515: the free spot ----

#[test]
fn free_spot_keeps_the_wish_when_nothing_is_taken() {
    assert_eq!(place::free_spot([16.0, 60.0], [196.0, 40.0], &[], [1280.0, 800.0]), [16.0, 60.0]);
}

#[test]
fn free_spot_goes_right_of_the_preview_bottom_aligned() {
    let taken = [[16.0, 72.0, 944.0, 712.0]];
    assert_eq!(place::free_spot([16.0, 60.0], [196.0, 40.0], &taken, [1280.0, 800.0]), [972.0, 744.0]);
    let p = place::free_spot([16.0, 100.0], [300.0, 200.0], &taken, [1280.0, 800.0]);
    assert!(p[0] >= 960.0 && p[0] + 300.0 <= 1280.0, "{p:?}");
}

#[test]
fn toolbar_is_clamped_into_the_window() {
    // AN-UI-030: a saved spot far outside lands in the bottom-right corner, 4 from the edge.
    assert_eq!(place::free_spot([5000.0, 5000.0], place::TOOLBAR, &[], [1280.0, 800.0]), [1280.0 - 216.0 - 4.0, 800.0 - 40.0 - 4.0]);
}

#[test]
fn panels_go_below_or_above_the_toolbar_and_anchor() {
    assert_eq!(place::by_toolbar([16.0, 60.0], 130.0, &[], [1280.0, 800.0]), [16.0, 108.0]);
    assert_eq!(place::by_toolbar([16.0, 600.0], 130.0, &[], [1280.0, 800.0]), [16.0, 462.0]);
    // AN-UI-102: under the anchor when it fits, else above.
    assert_eq!(place::at_anchor([916.0, 6.0, 126.0, 36.0], 170.0, &[], [1280.0, 800.0]), [916.0, 52.0]);
    assert_eq!(place::at_anchor([100.0, 700.0, 50.0, 30.0], 170.0, &[], [1280.0, 800.0]), [100.0, 520.0]);
}

// ---- AN-UI-034 / AN-IF-516: connection state ----

#[test]
fn connection_states_colours_and_texts() {
    let ep = "http://127.0.0.1:3124";
    assert_eq!(logic::conn(Reach::Unknown, 0, 0.0, 0.0, false), Conn::Unknown);
    assert_eq!(logic::conn_text(Conn::Unknown, ep), "Connecting to the notes service (127.0.0.1:3124)");
    let c = logic::conn(Reach::Yes, 0, 10.0, 10.0, false);
    assert_eq!(c, Conn::Connected);
    assert_eq!(logic::conn_text(c, ep), "Connected to the notes service (127.0.0.1:3124)");
    assert_eq!(logic::conn_hex(c), "#10b981");
    assert_eq!(logic::conn(Reach::Yes, 0, 100.0, 10.0, false), Conn::Syncing(0));
    assert_eq!(logic::conn_text(Conn::Syncing(0), ep), "Reconnecting to the notes service (127.0.0.1:3124)");
    assert_eq!(logic::conn(Reach::Yes, 2, 10.0, 10.0, false), Conn::Syncing(2));
    assert_eq!(logic::conn_text(Conn::Syncing(1), ep), "Connected to the notes service (127.0.0.1:3124): sending 1 note");
    assert_eq!(logic::conn(Reach::No, 1, 0.0, 0.0, false), Conn::Offline(1));
    assert_eq!(logic::conn_text(Conn::Offline(2), ep), "Offline: 2 notes queued (127.0.0.1:3124 unreachable)");
    assert_eq!(logic::conn(Reach::No, 0, 0.0, 0.0, false), Conn::Unreachable);
    assert_eq!(logic::conn_text(Conn::Unreachable, "http://127.0.0.1:9//"), "Notes service unreachable (127.0.0.1:9)");
    assert_eq!(logic::conn_hex(Conn::Unreachable), "#ef4444");
    assert_eq!(logic::conn_hex(Conn::Unknown), "#a3a3a3");
    // Active: tick 4 s → limit 13 s.
    assert_eq!(logic::conn(Reach::Yes, 0, 24.0, 10.0, true), Conn::Syncing(0));
    assert_eq!(logic::list_status(Reach::No), "Offline, notes wait here");
    assert_eq!(logic::list_status(Reach::Yes), "Notes service connected");
    assert_eq!(logic::list_status(Reach::Unknown), "");
}

#[test]
fn waiting_counts_queued_notes_of_every_space() {
    let a = note("a", "x", Status::Queued);
    let b = note("b", "y", Status::Queued);
    let mut c = note("c", "x", Status::Queued);
    c.server_id = Some("s".into());
    assert_eq!(logic::waiting(&[a, b, c, note("d", "x", Status::Resolved)]), 2);
}

// ---- AN-UI-040 / AN-IF-517: the capture surface ----

#[derive(Default)]
struct Mini {
    button: u32,
    layer: u32,
}

fn mini_view(_s: &Mini, _cx: &mut rune::Cx<Mini>) -> rune::El {
    use rune::el;
    el().key("root").w_full().h_full()
        .child(el().key("app").w_full().h_full()
            .child(el().key("btn").pin(Some(0.0), Some(0.0), None, None).size(50.0, 50.0).fill(rune::Color::WHITE)
                .on_press(|s: &mut Mini| s.button += 1)))
        .child(el().key("capture").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(rune::Color::TRANSPARENT)
            .press_on_down().on_background_press(|s: &mut Mini| s.layer += 1))
}

#[test]
fn a_transparent_layer_takes_clicks_and_app_picks_see_through_it() {
    let mut rt = rune::Runtime::new(Mini::default(), mini_view, (200.0, 200.0), rune::Env::new(true, 1.0));
    for _ in 0..5 {
        rt.frame(1.0 / 60.0);
    }
    rt.pointer_move(10.0, 10.0);
    rt.pointer_down(10.0, 10.0);
    rt.pointer_up(10.0, 10.0);
    for _ in 0..5 {
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(rt.state.button, 0);
    assert_eq!(rt.state.layer, 1);
    assert_eq!(rt.element_at_in("app", 10.0, 10.0).and_then(|p| p.key().map(str::to_string)).as_deref(), Some("btn"));
}

// ---- AN-UI-042 / AN-IF-511 ----

#[test]
fn nearest_key_and_key_path() {
    let p = "root/app/header-right/header-actions/review-dock/#0";
    assert_eq!(logic::nearest_key(p).as_deref(), Some("review-dock"));
    assert_eq!(logic::key_path(p), "header-right > header-actions > review-dock");
    assert_eq!(logic::nearest_key("root/app/#3"), None);
}

#[test]
fn chip_text_shows_the_last_two_keys_and_the_file() {
    assert_eq!(logic::chip_text("header-right > header-actions > review-dock", Some("crates/proto-app/src/features/review/dock.rs:33")),
        "header-actions › review-dock  ·  dock.rs:33");
    assert_eq!(logic::chip_text("", None), "element");
    assert_eq!(logic::chip_text("one", None), "one");
}

// ---- AN-UI-050 / AN-UI-055 / AN-IF-512: selection and saving ----

fn hover(s: &mut AppState, path: &str) {
    super::set_hover(&mut s.features.annotate, Some(path.into()));
}

#[test]
fn shift_clicks_collect_and_a_plain_click_opens_the_editor() {
    let mut s = state();
    super::toggle(&mut s);
    for p in ["root/app/a", "root/app/b", "root/app/a"] {
        hover(&mut s, p);
        super::click(&mut s, true);
        assert!(s.features.annotate.draft.is_none());
    }
    assert_eq!(s.features.annotate.selection.len(), 1);
    hover(&mut s, "root/app/c");
    super::click(&mut s, false);
    let a = &s.features.annotate;
    assert!(a.draft.as_ref().is_some_and(|d| d.focused));
    let keys: Vec<_> = a.selection.iter().map(|t| t.key.clone().unwrap()).collect();
    assert_eq!(keys, ["b", "c"]);
    // Without analysis the role is `element`, named by the key.
    assert_eq!(a.selection[0].element, "element `b`");
}

#[test]
fn clicking_without_hover_does_nothing_and_a_click_closes_the_thread() {
    let mut s = state();
    super::toggle(&mut s);
    super::click(&mut s, false);
    assert!(s.features.annotate.selection.is_empty() && s.features.annotate.draft.is_none());
    s.features.annotate.notes.push(note("n1", "zzfx", Status::Pending));
    s.features.annotate.open_note = Some("n1".into());
    hover(&mut s, "root/app/x");
    super::click(&mut s, false);
    assert!(s.features.annotate.open_note.is_none());
}

#[test]
fn saving_trims_and_creates_one_queued_note() {
    let mut s = state();
    s.features.annotate.sync.writes = Some(false);
    super::toggle(&mut s);
    hover(&mut s, "root/app/a");
    super::click(&mut s, true);
    hover(&mut s, "root/app/b");
    super::click(&mut s, false);
    s.features.annotate.draft = Some(TextEdit::new("  Tighter  "));
    super::save_draft(&mut s);
    let a = &s.features.annotate;
    assert_eq!(a.notes.len(), 1);
    let n = &a.notes[0];
    assert_eq!((n.comment.as_str(), n.targets.len(), n.status, n.space.as_str()), ("Tighter", 2, Status::Queued, "zzfx"));
    assert!(a.selection.is_empty() && a.draft.is_none() && a.on);
    assert!(id_form(&n.id), "{}", n.id);
    // AN-UI-165: no client in a run that may not write.
    assert!(a.sync.client.is_none());
}

#[test]
fn an_empty_draft_saves_nothing() {
    let mut s = state();
    super::toggle(&mut s);
    hover(&mut s, "root/app/a");
    super::click(&mut s, false);
    s.features.annotate.draft = Some(TextEdit::new("   "));
    super::save_draft(&mut s);
    assert!(s.features.annotate.notes.is_empty());
    assert!(s.features.annotate.draft.is_some());
}

#[test]
fn an_analysis_names_the_target() {
    let facts = vec![
        Fact { path: "root/app/bar".into(), key: Some("bar".into()), role: "group".into(), ..Fact::default() },
        Fact { path: "root/app/bar/go".into(), key: Some("go".into()), role: "button".into(), label: Some("Go".into()), rect: [10.0, 20.0, 80.4, 36.0],
            fill: [0x14 as f32 / 255.0, 0x47 as f32 / 255.0, 0xe6 as f32 / 255.0, 1.0], radius: 8.0, text_color: Some([1.0; 4]), ..Fact::default() },
        Fact { path: "root/app/bar/go/#0".into(), role: "text".into(), text: Some("Go".into()), ..Fact::default() },
        Fact { path: "root/app/bar/other".into(), role: "text".into(), text: Some("  ".into()), label: Some("hidden".into()), ..Fact::default() },
        Fact { path: "root/app/bar/third".into(), role: "button".into(), label: Some("Third".into()), ..Fact::default() },
    ];
    let a = logic::analyse(&facts, "root/app/bar/go").unwrap();
    assert_eq!(a.styles, "size: 80×36; fill: #1447e6; radius: 8; color: #ffffff");
    assert_eq!(a.nearby, "Go · Third");
    let t = logic::target_from_hover("root/app/bar/go", Some(&a), None, None);
    assert_eq!(t.element, "button \"Go\"");
    assert_eq!(t.accessibility, "role=button, label=\"Go\"");
    assert_eq!(t.rect, [10.0, 20.0, 80.4, 36.0]);
    assert_eq!(logic::num(7.5), "7.5");
    assert_eq!(logic::hex([1.0, 0.0, 0.0, 0.5]), "#ff000080");
    assert!(logic::analyse(&facts, "root/app/missing").is_none());
    // Long names are cut at 60 characters; an empty label counts as the label.
    let long = logic::Analysis { role: "button".into(), label: Some("x".repeat(80)), ..Default::default() };
    assert_eq!(logic::element_label(Some(&long), None), format!("button \"{}\"", "x".repeat(60)));
    let empty = logic::Analysis { role: "button".into(), label: Some(String::new()), text: Some("t".into()), ..Default::default() };
    assert_eq!(logic::element_label(Some(&empty), Some("k")), "button `k`");
}

// ---- AN-UI-180/181 / AN-IF-513: keys ----

#[test]
fn escape_closes_the_innermost_and_cmd_shift_a_toggles() {
    let mut s = state();
    super::toggle(&mut s);
    hover(&mut s, "root/app/a");
    super::click(&mut s, false);
    super::save_draft(&mut s);
    assert!(s.features.annotate.notes.is_empty());
    assert!(super::on_key(&mut s, &esc()));
    assert!(s.features.annotate.draft.is_none() && s.features.annotate.on);
    assert!(super::on_key(&mut s, &esc()));
    assert!(!s.features.annotate.on);
    assert!(!super::on_key(&mut s, &esc()));
    assert!(super::on_key(&mut s, &cmd_shift_a()));
    assert!(s.features.annotate.on);
    // As text with ⌘⇧ too; never with ⌥.
    assert!(super::on_key(&mut s, &key(Key::Text("A".into()), Mods { cmd: true, shift: true, ..Mods::default() })));
    assert!(!s.features.annotate.on);
    assert!(!super::on_key(&mut s, &key(Key::Command('a'), Mods { cmd: true, shift: true, alt: true, ..Mods::default() })));
}

#[test]
fn escape_order_thread_settings_list() {
    let mut s = state();
    let a = &mut s.features.annotate;
    a.open_note = Some("x".into());
    a.settings_open = true;
    a.list_open = true;
    assert!(super::on_key(&mut s, &esc()));
    assert!(s.features.annotate.open_note.is_none() && s.features.annotate.settings_open);
    assert!(super::on_key(&mut s, &key(Key::Escape, Mods { shift: true, ..Mods::default() })));
    assert!(!s.features.annotate.settings_open && s.features.annotate.list_open);
    assert!(super::on_key(&mut s, &esc()));
    assert!(!super::on_key(&mut s, &esc()));
}

#[test]
fn toggling_resets_the_picking_state_but_keeps_the_list() {
    let mut s = state();
    s.features.annotate.list_open = true;
    super::toggle(&mut s);
    hover(&mut s, "root/app/a");
    super::click(&mut s, false);
    super::toggle(&mut s);
    super::toggle(&mut s);
    let a = &s.features.annotate;
    assert!(a.on && a.draft.is_none() && a.selection.is_empty() && a.hover.is_none() && a.list_open && a.sync.now);
}

// ---- AN-UI-110…113 / AN-IF-531…533: source locations ----

#[test]
fn key_sites_are_found_with_the_longest_pattern_winning() {
    let text = [
        "    col().key(\"lanes\")",
        "        .child(text(n).key(format!(\"lane-name-{}\", i)))",
        "    row().key(format!(\"lane-{}\", id)).set_shown_key(\"no\")",
        "    e.set_key(\"set\");",
    ].join("\n");
    let mut entries = Vec::new();
    source::scan_text("crates/proto-app/src/features/lanes.rs", &text, &mut entries);
    assert_eq!(entries.len(), 4);
    let idx = source::Index { entries };
    let f = "crates/proto-app/src/features/lanes.rs";
    assert_eq!(idx.lookup("lanes").unwrap().0, format!("{f}:1"));
    assert_eq!(idx.lookup("lane-name-zz1").unwrap().0, format!("{f}:2"));
    assert_eq!(idx.lookup("lane-zz1").unwrap().0, format!("{f}:3"));
    assert_eq!(idx.lookup("set").unwrap(), (format!("{f}:4"), "features::lanes".to_string()));
    assert_eq!(idx.lookup("no"), None);
}

#[test]
fn hotkeys_raw_strings_and_variables_are_not_keys() {
    let mut e = Vec::new();
    source::scan_text("x.rs", "a.hotkey(\"h\"); b.key(r\"raw\"); c.key(name); d.key( \"spaced\")", &mut e);
    assert_eq!(e.len(), 1);
}

#[test]
fn module_names_follow_the_path_under_src() {
    assert_eq!(source::module_of("crates/proto-app/src/features/review/dock.rs"), "features::review::dock");
    assert_eq!(source::module_of("crates/proto-app/src/features/annotate/mod.rs"), "features::annotate");
    assert_eq!(source::module_of("a/src/b/src/c.rs"), "b");
    assert_eq!(source::module_of("plain.rs"), "plain");
}

#[test]
fn the_real_crate_knows_review_dock_and_skips_test_files() {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir.parent().unwrap().parent().unwrap();
    let idx = source::build(&crate_dir.join("src"), root);
    let (src, module) = idx.lookup("review-dock").unwrap();
    assert!(src.starts_with("crates/proto-app/src/features/review/dock.rs:"), "{src}");
    assert_eq!(module, "features::review::dock");
    assert!(idx.entries.iter().all(|e| !e.file.ends_with("_tests.rs")));
    assert!(source::build(std::path::Path::new("/nonexistent/src"), root).entries.is_empty());
}

// ---- AN-UI-151: markdown ----

#[test]
fn markdown_export_is_byte_exact() {
    let mut a = note("a", "zz", Status::Pending);
    a.comment = "Make it smaller".into();
    a.targets = vec![dock_target()];
    let mut b = note("b", "zz", Status::Queued);
    b.comment = " Bigger headline ".into();
    b.targets = vec![Target { element: "h1".into(), element_path: "main > h1".into(), design_id: Some("zzd1".into()), ..Target::default() }];
    let closed = note("c", "zz", Status::Resolved);
    let other = note("d", "elsewhere", Status::Pending);
    let md = logic::markdown(&[a, closed, b, other], "zz");
    assert_eq!(md, "## Prototypie Rune notes\n\n### 1. button \"Review\"\n**Location:** `header-actions > review-dock`\n**Source:** crates/proto-app/src/features/review/dock.rs:33 (features::review::dock)\n**Feedback:** Make it smaller\n\n### 2. h1\n**Location:** `main > h1`\n**Design:** zzd1\n**Feedback:** Bigger headline\n");
    assert_eq!(logic::markdown(&[], "zz"), "## Prototypie Rune notes\n");
}

#[test]
fn numbering_skips_closed_notes_and_other_spaces() {
    let ns = [note("a1", "s", Status::Pending), note("a2", "s", Status::Resolved), note("x", "t", Status::Pending), note("a3", "s", Status::Queued)];
    let nums: Vec<_> = logic::numbered(&ns, "s").into_iter().map(|(i, n)| (i, n.id.clone())).collect();
    assert_eq!(nums, [(1, "a1".to_string()), (2, "a3".to_string())]);
    assert_eq!(logic::number_of(&ns, "a2", "s"), 0);
}

// ---- spec 03 §C / AN-IF-501/502, AN-UI-210/211 ----

fn rune_note() -> Note {
    Note { id: "l1".into(), server_id: None, space: "zzspace".into(), comment: "Make it smaller ".into(), targets: vec![dock_target()],
        timestamp: 5.0, status: Status::Queued, thread: vec![], window: [1000.0, 800.0] }
}

#[test]
fn the_body_of_a_rune_note() {
    let b = wire::note_body(&rune_note());
    assert_eq!(b["location"], "prototypie-rune://zzspace/review-dock");
    assert!(b["comment"].as_str().unwrap().starts_with("Make it smaller\n— Rune element `review-dock` at crates/proto-app/src/features/review/dock.rs:33 (features::review::dock)"));
    assert_eq!(b["target"]["position"]["x"].as_f64(), Some(14.0));
    assert_eq!(b["target"]["position"]["y"].as_f64(), Some(28.0));
    assert_eq!(b["target"]["componentChain"], "Rune: header-actions > review-dock");
    assert_eq!(b["target"]["sourceLocations"], "crates/proto-app/src/features/review/dock.rs:33");
    assert_eq!(b["target"]["fixed"], true);
    assert_eq!(b["target"]["multiSelect"], false);
    assert!(b.get("designId").is_none());
    assert_eq!(b["clientRef"], "l1");
    assert_eq!(b["kind"], "feedback");
    assert_eq!(b["element"], "button \"Review\"");
}

#[test]
fn the_body_of_a_design_note_with_two_targets() {
    let t = |p: &str| Target { element: format!("div \"{p}\""), element_path: p.into(), design_id: Some("zzd1".into()), rect: [1.0, 2.0, 3.0, 4.0], ..Target::default() };
    let mut n = rune_note();
    n.targets = vec![t(".a"), t(".b")];
    let b = wire::note_body(&n);
    assert_eq!(b["designId"], "zzd1");
    assert!(b["location"].as_str().unwrap().ends_with("/zzd1"));
    assert_eq!(b["target"]["multiSelect"], true);
    assert_eq!(b["target"]["rects"].as_array().unwrap().len(), 2);
    assert!(b["comment"].as_str().unwrap().ends_with("\n— In design zzd1 (HTML, Prototypie Rune)"));
    assert_eq!(b["elementPath"], ".a | .b");
    assert!(b["target"].get("componentChain").is_none());
}

#[test]
fn a_key_without_source_says_so_and_no_window_means_x_zero() {
    let mut n = rune_note();
    n.targets[0].source = None;
    n.window = [0.0, 0.0];
    let b = wire::note_body(&n);
    assert_eq!(b["comment"], "Make it smaller\n— Rune element `review-dock` (source not found)");
    assert_eq!(b["target"]["position"]["x"].as_f64(), Some(0.0));
    let mut empty = rune_note();
    empty.targets.clear();
    assert_eq!(wire::note_location(&empty), "prototypie-rune://zzspace/");
}

// ---- spec 03 §D / AN-IF-503/504, AN-UI-170 / AN-IF-514 ----

#[test]
fn reading_status_and_thread() {
    let v = json!({"id": "s", "location": "prototypie-rune://zz/k", "status": "acknowledged", "thread": [{"id": "m", "role": "agent", "content": "on it", "createdAt": "x"}, {"role": 3}]});
    let n = wire::rebuild(&v, "zz").unwrap();
    assert_eq!(n.status, Status::Acknowledged);
    assert_eq!(n.thread, vec![Message { role: "agent".into(), content: "on it".into() }]);
    assert_eq!(Status::from_service(Some("weird")), Status::Pending);
    assert_eq!(Status::from_service(None), Status::Pending);
}

#[test]
fn a_sent_note_comes_back_after_a_restart() {
    let mut v = wire::note_body(&rune_note());
    v["id"] = json!("srv-1");
    let n = wire::rebuild(&v, "zzspace").unwrap();
    assert_eq!((n.id.as_str(), n.server_id.as_deref(), n.comment.as_str()), ("srv-1", Some("srv-1"), "Make it smaller"));
    assert_eq!(n.targets[0].key.as_deref(), Some("review-dock"));
    assert_eq!(n.targets[0].rect, [100.0, 10.0, 80.0, 36.0]);
    assert_eq!(n.targets[0].source.as_deref(), Some("crates/proto-app/src/features/review/dock.rs:33"));
    assert_eq!(n.window, [0.0, 0.0]);
    assert!(wire::rebuild(&v, "other").is_none());
    // Junk never panics.
    assert!(wire::rebuild(&json!({"id": 3}), "zz").is_none());
    assert!(wire::rebuild(&json!({"id": "x", "location": "prototypie-rune://zz/", "target": 5, "thread": "no"}), "zz").is_some());
}

#[test]
fn merging_takes_status_and_thread_and_rebuilds_unknown_notes() {
    let mut l1 = note("l1", "zz", Status::Pending);
    l1.server_id = Some("s1".into());
    l1.timestamp = 2.0;
    let mut notes = vec![l1];
    let service = vec![
        json!({"id": "s1", "status": "resolved", "thread": [{"role": "agent", "content": "done"}]}),
        json!({"id": "s0", "status": "acknowledged", "location": "prototypie-rune://zz/review-dock", "comment": "old\n— Rune element", "clientTimestamp": 1}),
        json!({"status": "pending"}),
    ];
    assert!(wire::merge(&mut notes, "zz", &service));
    assert_eq!(notes[0].id, "s0");
    assert_eq!(notes[0].comment, "old");
    assert_eq!(notes[0].targets[0].key.as_deref(), Some("review-dock"));
    assert_eq!(notes[1].status, Status::Resolved);
    assert_eq!(notes[1].thread[0].content, "done");
    assert!(!wire::merge(&mut notes, "zz", &service), "nothing new the second time");
}

#[test]
fn local_closing_wins_and_threads_only_grow() {
    let mut n = note("l", "zz", Status::Resolved);
    n.server_id = Some("s".into());
    n.thread = vec![Message { role: "human".into(), content: "a".into() }, Message { role: "human".into(), content: "b".into() }];
    let mut notes = vec![n];
    let changed = wire::merge(&mut notes, "zz", &[json!({"id": "s", "status": "pending", "thread": [{"role": "human", "content": "a"}]})]);
    assert!(!changed);
    assert_eq!(notes[0].status, Status::Resolved);
    assert_eq!(notes[0].thread.len(), 2);
    // Notes of another space only come back for their own space; an empty collection deletes nothing.
    assert!(!wire::merge(&mut notes, "zz", &[json!({"id": "f", "location": "prototypie-rune://other/x"})]));
    assert!(!wire::merge(&mut notes, "zz", &[]));
    assert_eq!(notes.len(), 1);
}

// ---- spec 03 AN-SY-023/024, AN-IF-521 ----

#[test]
fn addresses_are_normalised_and_checked() {
    for a in ["http://localhost:3124", "http://127.0.0.1:3124/", "localhost:3124", "  http://127.0.0.1:3124//  "] {
        assert_eq!(parse_addr(a), Addr::Ok("127.0.0.1:3124".into()), "{a}");
    }
    assert_eq!(parse_addr("http://127.0.0.1:4800/"), Addr::Ok("127.0.0.1:4800".into()));
    assert_eq!(parse_addr("http://localhost"), Addr::Ok("127.0.0.1:80".into()));
    assert_eq!(parse_addr("http://[::1]:5"), Addr::Ok("[::1]:5".into()));
    for bad in ["http://x:1/api", "https://x:1", "http://localhost:99999", "", "http://:3", "http://a:b"] {
        assert_eq!(parse_addr(bad), Addr::Invalid, "{bad}");
    }
    assert_eq!(parse_addr("http://beispiel:1"), Addr::Named("beispiel".into()));
    assert!(matches!(parse_addr("http://192.168.1.2:3124"), Addr::Remote(_)));
}

#[test]
fn bad_addresses_report_unreachable_without_a_connection() {
    for a in ["https://x:1", "http://beispiel:1", "http://10.1.2.3:1"] {
        let c = Client::start(a, None).unwrap();
        c.submit(Job::Refresh { space: "zz".into() });
        let out = wait_for(&c, |o| !o.is_empty(), 3.0);
        assert_eq!(out, vec![Outcome::Reach(false)], "{a}");
    }
}

fn id_form(s: &str) -> bool {
    let Some((t, r)) = s.split_once('-') else { return false };
    let ok = |x: &str| x.chars().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase());
    !t.is_empty() && ok(t) && (1..=6).contains(&r.len()) && ok(r)
}

#[test]
fn local_ids_have_their_form_and_differ() {
    let (a, b) = (logic::local_id(1_763_632_800_123.0), logic::local_id(1_763_632_800_123.0));
    assert!(id_form(&a) && id_form(&b), "{a} {b}");
    assert_ne!(a, b);
    assert!(a.starts_with('m') && a.split('-').next().unwrap().len() == 8, "{a}");
}

// ---- spec 03 AN-SY-191: the client against a test service ----

#[test]
fn send_into_an_existing_collection_then_refresh() {
    let srv = Server::start(mock_with_collection("zz"));
    let c = Client::start(&srv.url(), None).unwrap();
    let n = Note { space: "zz".into(), ..rune_note() };
    let body = wire::note_body(&n);
    c.submit(Job::Send { local: "l1".into(), space: "zz".into(), body: body.clone() });
    let out = wait_for(&c, |o| o.len() >= 2, 5.0);
    assert_eq!(out, vec![Outcome::Sent { local: "l1".into(), id: "n11".into() }, Outcome::Reach(true)]);
    srv.mock.lock().unwrap().notes.get_mut("c0").unwrap()[0]["status"] = json!("resolved");
    c.submit(Job::Refresh { space: "zz".into() });
    let out = wait_for(&c, |o| o.len() >= 2, 5.0);
    let Outcome::Content { space, notes } = &out[0] else { panic!("{out:?}") };
    assert_eq!(space, "zz");
    assert_eq!(notes[0]["status"], "resolved");
    assert_eq!(out[1], Outcome::Reach(true));
    assert_eq!(srv.count("GET /api/notes/collections?"), 1, "the collection is looked up once");
    assert_eq!(srv.mock.lock().unwrap().bodies[0], body, "the body arrives unchanged");
}

#[test]
fn a_new_space_gets_a_collection_and_a_refusal_is_reported() {
    let srv = Server::start(Mock { refuse: Some(("boom".into(), 1)), ..Mock::default() });
    let c = Client::start(&srv.url(), None).unwrap();
    c.submit(Job::Send { local: "l1".into(), space: "zz".into(), body: wire::note_body(&rune_note()) });
    let out = wait_for(&c, |o| o.len() >= 2, 5.0);
    assert_eq!(out, vec![Outcome::SendFailed { local: "l1".into(), error: "boom".into() }, Outcome::Reach(false)]);
    assert_eq!(srv.mock.lock().unwrap().collections[0]["location"], "prototypie-rune://zz");
    // After the failure the cached id is gone: the next send looks again (and finds the collection).
    c.submit(Job::Send { local: "l1".into(), space: "zz".into(), body: wire::note_body(&rune_note()) });
    let out = wait_for(&c, |o| o.len() >= 2, 5.0);
    assert!(matches!(&out[0], Outcome::Sent { .. }), "{out:?}");
    assert_eq!(srv.count("GET /api/notes/collections?"), 2);
    assert_eq!(srv.mock.lock().unwrap().collections.len(), 1);
}

#[test]
fn a_dead_port_gives_one_unreachable_quickly() {
    let c = Client::start("http://127.0.0.1:9", None).unwrap();
    let start = Instant::now();
    c.submit(Job::Refresh { space: "zz".into() });
    let out = wait_for(&c, |o| !o.is_empty(), 5.0);
    assert_eq!(out, vec![Outcome::Reach(false)]);
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn one_batch_refreshes_a_space_once() {
    let srv = Server::start(mock_with_collection("A"));
    let c = Client::start(&srv.url(), None).unwrap();
    let r = || Job::Refresh { space: "A".into() };
    c.submit_all(vec![r(), r(), Job::Send { local: "l".into(), space: "A".into(), body: json!({"comment": "x", "element": "", "elementPath": ""}) }, r()]);
    let out = wait_for(&c, |o| o.len() >= 4, 5.0);
    assert!(matches!(out[0], Outcome::Content { .. }) && matches!(out[2], Outcome::Sent { .. }), "{out:?}");
    let log = srv.mock.lock().unwrap().log.clone();
    assert_eq!(log.len(), 3, "{log:?}");
    assert!(log[0].starts_with("GET /api/notes/collections?") && log[1] == "GET /api/notes/collections/c0" && log[2].starts_with("POST"));
}

#[test]
fn status_and_reply_with_empty_answers_count_as_reachable() {
    let srv = Server::start(Mock { empty_status: true, ..mock_with_collection("zz") });
    srv.mock.lock().unwrap().notes.insert("c0".into(), vec![json!({"id": "a b/ü", "status": "pending", "thread": []})]);
    let c = Client::start(&srv.url(), None).unwrap();
    c.submit(Job::Status { id: "a b/ü".into(), status: "resolved".into(), by: "human".into() });
    c.submit(Job::Reply { id: "a b/ü".into(), text: "ok".into() });
    let out = wait_for(&c, |o| o.len() >= 2, 5.0);
    assert_eq!(out, vec![Outcome::Reach(true), Outcome::Reach(true)]);
    let m = srv.mock.lock().unwrap();
    assert_eq!(m.statuses, vec![("a b/ü".to_string(), "resolved".to_string(), "human".to_string())]);
    assert_eq!(m.replies, vec![("a b/ü".to_string(), "human".to_string(), "ok".to_string())]);
}

#[test]
fn changing_the_address_forgets_collections() {
    let one = Server::start(mock_with_collection("zz"));
    let two = Server::start(mock_with_collection("zz"));
    let c = Client::start(&one.url(), None).unwrap();
    c.submit(Job::Refresh { space: "zz".into() });
    wait_for(&c, |o| o.len() >= 2, 5.0);
    c.submit(Job::Endpoint(two.url()));
    c.submit(Job::Refresh { space: "zz".into() });
    let out = wait_for(&c, |o| o.len() >= 2, 5.0);
    assert_eq!(out.last(), Some(&Outcome::Reach(true)));
    assert_eq!(two.mock.lock().unwrap().log[0].split('?').next(), Some("GET /api/notes/collections"));
}

// ---- the app's sync loop (AN-UI-160…166, AN-SY-050…061) ----

fn synced_state(srv: &Server) -> AppState {
    let mut s = state();
    s.prefs.features.insert("annotate".into(), json!({"endpoint": srv.url()}));
    s.features.annotate.sync.writes = Some(true);
    s
}

/// Poll until `f` holds (up to `secs`), with the clock at `clock`.
fn poll_until(s: &mut AppState, clock: f64, secs: f64, mut f: impl FnMut(&AppState) -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed().as_secs_f64() < secs {
        super::poll(s, clock, &mut rune::Tx::default());
        if f(s) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn end_to_end_a_saved_note_reaches_the_service_and_turns_pending() {
    let srv = Server::start(Mock::default());
    let mut s = synced_state(&srv);
    super::toggle(&mut s);
    hover(&mut s, "root/app/header-right/header-actions/review-dock");
    super::click(&mut s, false);
    s.features.annotate.draft = Some(TextEdit::new("Make it smaller "));
    super::save_draft(&mut s);
    assert!(poll_until(&mut s, 0.0, 5.0, |s| s.features.annotate.notes[0].status == Status::Pending));
    let a = &s.features.annotate;
    assert!(a.notes[0].server_id.is_some());
    assert_eq!(a.reach, Reach::Yes);
    let body = srv.mock.lock().unwrap().bodies[0].clone();
    assert!(body["comment"].as_str().unwrap().starts_with("Make it smaller\n— Rune element `review-dock`"));
    assert_eq!(body["location"], "prototypie-rune://zzfx/review-dock");
    assert!(body["target"]["fullPath"].as_str().unwrap().ends_with("review-dock"));
}

#[test]
fn the_first_tick_is_immediate_then_every_interval() {
    let srv = Server::start(mock_with_collection("zzfx"));
    let mut s = synced_state(&srv);
    let mut tx = rune::Tx::default();
    for clock in [0.0, 10.0, 29.9] {
        super::tick(&mut s, clock, &mut tx);
    }
    assert_eq!(s.features.annotate.sync.last_tick, Some(0.0));
    super::tick(&mut s, 30.0, &mut tx);
    assert_eq!(s.features.annotate.sync.last_tick, Some(30.0));
    s.features.annotate.list_open = true;
    super::tick(&mut s, 33.0, &mut tx);
    assert_eq!(s.features.annotate.sync.last_tick, Some(30.0));
    super::tick(&mut s, 34.0, &mut tx);
    assert_eq!(s.features.annotate.sync.last_tick, Some(34.0));
    // Switching the mode on asks for a tick at once.
    super::toggle(&mut s);
    super::tick(&mut s, 35.0, &mut tx);
    assert_eq!(s.features.annotate.sync.last_tick, Some(35.0));
}

#[test]
fn a_tooling_run_never_starts_a_client() {
    let mut s = state();
    s.features.annotate.sync.writes = Some(false);
    s.features.annotate.notes.push(note("q", "zzfx", Status::Queued));
    for clock in [0.0, 30.0, 60.0] {
        super::poll(&mut s, clock, &mut rune::Tx::default());
    }
    let a = &s.features.annotate;
    assert!(a.sync.client.is_none());
    assert_eq!(a.reach, Reach::Unknown);
    assert_eq!(a.notes[0].status, Status::Queued);
}

#[test]
fn offline_notes_are_sent_when_the_service_comes() {
    let srv = Server::start(Mock { refuse: Some(("down".into(), 2)), ..Mock::default() });
    let mut s = synced_state(&srv);
    let mut a = note("la", "zzfx", Status::Queued);
    a.targets = vec![dock_target()];
    let b = note("lb", "other", Status::Queued);
    s.features.annotate.notes = vec![a, b];
    assert!(poll_until(&mut s, 0.0, 5.0, |s| s.features.annotate.sync.last_tick.is_some() && s.features.annotate.sync.in_flight.is_empty() && srv.count("POST") >= 2));
    assert!(s.features.annotate.notes.iter().all(|n| n.server_id.is_none()));
    assert!(poll_until(&mut s, 30.0, 5.0, |s| s.features.annotate.notes.iter().all(|n| n.server_id.is_some())), "after the next tick");
    let m = srv.mock.lock().unwrap();
    let locs: Vec<_> = m.collections.iter().map(|c| c["location"].as_str().unwrap().to_string()).collect();
    assert!(locs.contains(&"prototypie-rune://zzfx".to_string()) && locs.contains(&"prototypie-rune://other".to_string()), "{locs:?}");
}

#[test]
fn a_slow_service_gets_each_note_once() {
    let srv = Server::start(Mock { delay_ms: 1500, ..mock_with_collection("zzfx") });
    let mut s = synced_state(&srv);
    super::toggle(&mut s);
    super::create_note(&mut s, "x", vec![dock_target()], [1000.0, 800.0]);
    for clock in [0.0, 4.0, 8.0] {
        super::poll(&mut s, clock, &mut rune::Tx::default());
    }
    assert!(poll_until(&mut s, 8.0, 5.0, |s| s.features.annotate.notes[0].server_id.is_some()));
    assert_eq!(srv.mock.lock().unwrap().bodies.len(), 1);
}

#[test]
fn closing_while_sending_is_told_once_the_id_arrives() {
    let srv = Server::start(Mock { delay_ms: 800, ..mock_with_collection("zzfx") });
    let mut s = synced_state(&srv);
    super::create_note(&mut s, "x", vec![dock_target()], [1000.0, 800.0]);
    let id = s.features.annotate.notes[0].id.clone();
    super::reply(&mut s, &id, "  later  ");
    super::set_status(&mut s, &id, Status::Resolved);
    assert!(poll_until(&mut s, 0.0, 5.0, |s| s.features.annotate.notes[0].server_id.is_some()));
    assert_eq!(s.features.annotate.notes[0].status, Status::Resolved);
    let start = Instant::now();
    while srv.mock.lock().unwrap().replies.is_empty() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(200));
    let m = srv.mock.lock().unwrap();
    assert_eq!(m.statuses, vec![("n11".to_string(), "resolved".to_string(), "human".to_string())]);
    assert_eq!(m.replies.len(), 1);
    assert_eq!(m.replies[0].2, "later");
}

#[test]
fn resolving_a_waiting_note_sends_nothing_ever() {
    let srv = Server::start(mock_with_collection("zzfx"));
    let mut s = synced_state(&srv);
    s.features.annotate.notes.push(note("w", "zzfx", Status::Queued));
    s.features.annotate.open_note = Some("w".into());
    super::set_status(&mut s, "w", Status::Dismissed);
    assert!(s.features.annotate.open_note.is_none());
    assert!(poll_until(&mut s, 0.0, 3.0, |s| s.features.annotate.reach == Reach::Yes));
    let m = srv.mock.lock().unwrap();
    assert!(m.bodies.is_empty() && m.statuses.is_empty());
}

#[test]
fn resolving_a_sent_note_tells_the_service_as_human() {
    let srv = Server::start(mock_with_collection("zzfx"));
    srv.mock.lock().unwrap().notes.insert("c0".into(), vec![json!({"id": "s1", "status": "pending", "thread": []})]);
    let mut s = synced_state(&srv);
    let mut n = note("l", "zzfx", Status::Pending);
    n.server_id = Some("s1".into());
    s.features.annotate.notes.push(n);
    super::reply(&mut s, "l", "  ok  ");
    super::set_status(&mut s, "l", Status::Resolved);
    let start = Instant::now();
    while srv.mock.lock().unwrap().statuses.is_empty() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let m = srv.mock.lock().unwrap();
    assert_eq!(m.statuses, vec![("s1".to_string(), "resolved".to_string(), "human".to_string())]);
    assert_eq!(m.replies, vec![("s1".to_string(), "human".to_string(), "ok".to_string())]);
}

#[test]
fn notes_of_earlier_runs_come_back_on_the_first_tick() {
    let srv = Server::start(mock_with_collection("zzfx"));
    srv.mock.lock().unwrap().notes.insert("c0".into(), vec![json!({"id": "old", "status": "acknowledged", "location": "prototypie-rune://zzfx/review-dock", "comment": "earlier\n— Rune element", "clientTimestamp": 1})]);
    let mut s = synced_state(&srv);
    assert!(poll_until(&mut s, 0.0, 5.0, |s| !s.features.annotate.notes.is_empty()));
    assert_eq!(s.features.annotate.notes[0].comment, "earlier");
}

#[test]
fn saving_an_address_resets_the_state_and_writes_prefs() {
    let mut s = state();
    s.features.annotate.reach = Reach::Yes;
    s.features.annotate.settings_open = true;
    s.prefs.features.insert("annotate".into(), json!("junk"));
    s.features.annotate.endpoint_edit = TextEdit::new("  http://127.0.0.1:3124  ");
    super::save_endpoint(&mut s);
    assert_eq!(s.prefs.features["annotate"], json!({"endpoint": null}));
    assert_eq!(super::endpoint(&s), "http://127.0.0.1:3124");
    let a = &s.features.annotate;
    assert!(a.reach == Reach::Unknown && !a.settings_open && a.sync.now);
    assert_eq!(a.endpoint_edit.text, "  http://127.0.0.1:3124  ");
    s.features.annotate.endpoint_edit = TextEdit::new("http://127.0.0.1:4800");
    super::save_endpoint(&mut s);
    assert_eq!(super::endpoint(&s), "http://127.0.0.1:4800");
}

#[test]
fn prefs_keep_other_fields_and_read_positions_loosely() {
    let mut s = state();
    assert_eq!(super::saved_pos(&s), None);
    s.prefs.features.insert("annotate".into(), json!({"pos": [10, 20.5, 30], "other": 1, "endpoint": ""}));
    assert_eq!(super::saved_pos(&s), Some([10.0, 20.5]));
    assert_eq!(super::endpoint(&s), "http://127.0.0.1:3124");
    super::save_pos(&mut s, [100.4, 50.6]);
    assert_eq!(s.prefs.features["annotate"]["pos"], json!([100, 51]));
    assert_eq!(s.prefs.features["annotate"]["other"], 1);
    s.prefs.features.insert("annotate".into(), json!({"pos": ["a", 1]}));
    assert_eq!(super::saved_pos(&s), None);
    // The first poll fills the address field from the prefs.
    s.prefs.features.insert("annotate".into(), json!({"endpoint": "http://127.0.0.1:4800"}));
    s.features.annotate.sync.writes = Some(false);
    super::poll(&mut s, 0.0, &mut rune::Tx::default());
    assert_eq!(s.features.annotate.endpoint_edit.text, "http://127.0.0.1:4800");
}

// ---- spec 02 §N: the shown page ----

#[test]
fn page_pins_keep_the_list_numbers() {
    let mut notes = vec![note("r", "s", Status::Pending)];
    for (id, d) in [("a1", "A"), ("b1", "B"), ("a2", "A")] {
        let mut n = note(id, "s", Status::Queued);
        n.targets = vec![Target { element_path: format!("#{id}"), design_id: Some(d.into()), ..Target::default() }];
        notes.push(n);
    }
    let pins = super::page::pins_for(&notes, "s", "A");
    assert_eq!(pins.iter().map(|p| p.number).collect::<Vec<_>>(), [2, 4]);
    assert_eq!(pins[0].status, "pending");
}

#[test]
fn design_notes_are_mirrored_and_marked_resolved() {
    let mut s = state();
    s.features.annotate.sync.writes = Some(false);
    let t = |el: &str, p: &str| Target { element: el.into(), element_path: p.into(), design_id: Some("zzv1".into()), ..Target::default() };
    super::create_note(&mut s, " Pop it ", vec![t("button \"Go\"", ".a"), t("h1 \"Title\"", ".b")], [800.0, 600.0]);
    let id = s.features.annotate.notes[0].id.clone();
    let stored = s.snap().notes("zzv1").unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!((stored[0].element.as_str(), stored[0].element_path.as_str(), stored[0].comment.as_str()), ("button \"Go\", h1 \"Title\"", ".a | .b", "Pop it"));
    assert_eq!(stored[0].resolved, None);
    super::set_status(&mut s, &id, Status::Dismissed);
    assert_eq!(s.snap().notes("zzv1").unwrap()[0].resolved, Some(true));
}

#[test]
fn web_picks_become_targets_with_window_rects() {
    let mut p = proto_web::WebPick { element: "button \"Go\"".into(), element_path: "main > .cta".into(), rect: [1.0, 2.0, 3.0, 4.0], window_rect: [60.0, 45.0, 40.0, 10.0], ..Default::default() };
    p.attributes.insert("id".into(), "go".into());
    p.attributes.insert("class".into(), "cta".into());
    let t = super::page::target_of(&p, "zzd1");
    assert_eq!(t.rect, [60.0, 45.0, 40.0, 10.0]);
    assert_eq!(t.design_id.as_deref(), Some("zzd1"));
    assert_eq!(t.attributes, vec![("class".into(), "cta".into()), ("id".into(), "go".into())]);
    assert!(t.source.is_none() && t.key.is_none());
}

// ---- the scenes (AN-IF-604, AN-UI-200…202) ----

#[test]
fn the_annotate_scenes_build_their_layer() {
    for name in ["annotate", "annotate-light", "annotate-thread"] {
        let scene = crate::features::scenes::SCENES.iter().find(|(n, _)| *n == name).unwrap().1;
        let mut s = AppState::new();
        scene(&mut s);
        let mut rt = rune::Runtime::new(s, crate::shell::view, (1600.0, 1000.0), rune::Env::new(true, 1.0));
        for _ in 0..10 {
            rt.frame(1.0 / 60.0);
        }
        let els = rt.elements();
        let label = |l: &str| els.iter().any(|e| e.label.as_deref() == Some(l));
        assert!(label("Notes: 2 open") && label("Annotate") && label("Stop annotating (⌘⇧A)"), "{name}");
        assert!(label("Connecting to the notes service (127.0.0.1:3124)"), "{name}");
        assert!(label("Note 1: Make the waiting count easier to read"), "{name}: pin 1 on review-dock");
        let tb = els.iter().find(|e| e.key.as_deref() == Some("annotate-ui-toolbar")).unwrap();
        assert_eq!([tb.rect[0], tb.rect[1], tb.rect[3]], [16.0, 60.0, 40.0]);
        if name == "annotate-thread" {
            assert!(label("Note 1") && label("Reply") && label("Resolve") && label("Delete"), "{name}");
        } else {
            assert!(label("New note") && label("Note") && label("Save") && label("Cancel"), "{name}");
        }
        assert!(rt.state.features.annotate.sync.client.is_none(), "scenes send nothing");
    }
}

#[test]
fn resolving_in_the_thread_scene_closes_it_and_drops_the_pin() {
    let scene = crate::features::scenes::SCENES.iter().find(|(n, _)| *n == "annotate-thread").unwrap().1;
    let mut s = AppState::new();
    scene(&mut s);
    super::set_status(&mut s, "zz-a1", Status::Resolved);
    assert!(s.features.annotate.open_note.is_none());
    assert_eq!(logic::numbered(&s.features.annotate.notes, &s.route.space).len(), 1);
}
