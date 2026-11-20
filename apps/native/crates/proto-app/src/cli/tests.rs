//! Unit tests of the CLI: parsing, texts, snapshots of the human output, the embedded skill. Names start with
//! `cli_` so `cargo test -p proto-app cli` picks them up.

use serde_json::json;

use super::args::{self, JobRef};
use super::{help, is_cli, notes, out, queue, skill};

fn words(s: &[&str]) -> Vec<String> {
    s.iter().map(|x| x.to_string()).collect()
}

#[test]
fn cli_detects_its_words() {
    // AN-CLI-201, -646: the app keeps its own arguments and the empty command line.
    for a in ["notes", "queue", "cards", "status", "skill", "help", "--help", "-h", "--version", "-V", "--json", "--as", "--as=w1", "--server", "--server=x"] {
        assert!(is_cli(Some(a)), "{a}");
    }
    for a in ["--snap", "--tree", "--script", "annotate", "--asx", "-psn_0_123"] {
        assert!(!is_cli(Some(a)), "{a}");
    }
    assert!(!is_cli(None));
}

#[test]
fn cli_parses_options_anywhere() {
    let l = args::parse(&words(&["--as", "w1", "notes", "ack", "n1", "--message=on it", "n2", "--", "--force"])).unwrap();
    assert_eq!(l.words, ["notes", "ack", "n1", "n2", "--force"]);
    assert_eq!(l.value("as"), Some("w1"));
    assert_eq!(l.value("message"), Some("on it"));
    assert!(!l.has("force"));
    let l = args::parse(&words(&["skill", "install", "--dir", "a", "--dir", "b", "-h"])).unwrap();
    assert_eq!(l.values("dir"), ["a", "b"]);
    assert!(l.has("help"));
    assert_eq!(args::parse(&words(&["notes", "list", "--status"])).unwrap_err().exit, 2);
    assert_eq!(args::parse(&words(&["notes", "list", "--json=1"])).unwrap_err().exit, 2);
    let l = args::parse(&words(&["queue", "deliver", "J#0", "--html", "-"])).unwrap();
    assert_eq!(l.value("html"), Some("-"));
}

#[test]
fn cli_durations_and_refs() {
    assert_eq!(args::duration("90"), Some(90));
    assert_eq!(args::duration("90s"), Some(90));
    assert_eq!(args::duration("2m"), Some(120));
    assert_eq!(args::duration("1m30s"), Some(90));
    for bad in ["", "s", "1.5s", "-1", "1h", "m30s", "1m30"] {
        assert_eq!(args::duration(bad), None, "{bad}");
    }
    assert_eq!(args::show_duration(100), "100s");
    assert_eq!(args::show_duration(120), "2m");
    assert_eq!(JobRef::parse("J1abc#0").unwrap(), JobRef { job: "J1abc".into(), index: 0 });
    assert_eq!(JobRef::parse("J1abc/3").unwrap().shown(), "J1abc#3");
    for bad in ["J1abc", "#0", "J1#x", "J 1#0", "J1#-1"] {
        let f = JobRef::parse(bad).unwrap_err();
        assert_eq!(f.exit, 2);
        assert_eq!(f.msg, format!("\"{bad}\" is not a queue item (use JOB#INDEX, for example J1abc#0)."));
    }
}

fn ctx(name: Option<&str>) -> super::Ctx {
    let mut a = vec!["notes", "reply", "n1"];
    if let Some(n) = name {
        a.extend(["--as", n]);
    }
    let line = args::parse(&words(&a)).unwrap();
    super::Ctx::new(line, "notes reply".into(), 2, false).unwrap()
}

#[test]
fn cli_signs_agent_texts() {
    // AN-CLI-217
    let c = ctx(Some("proto-lead"));
    assert_eq!(c.sign("proto-lead: done"), "proto-lead: done");
    assert_eq!(c.sign("done"), "proto-lead: done");
    assert_eq!(ctx(Some("agent")).sign("done"), "done");
    assert_eq!(c.args(), ["n1"]);
}

#[test]
fn cli_safe_text_and_times() {
    // AN-CLI-506
    assert_eq!(out::safe("a\u{1b}[2Jb\n\tc\u{7}"), "a\\u{1b}[2Jb\n\tc\\u{7}");
    assert_eq!(out::time("2026-11-20T10:00:00.123Z"), "2026-11-20 10:00Z");
    assert_eq!(out::time("whenever"), "whenever");
    let long = "x".repeat(700);
    let s = out::shortened(&long, "  ", "prototypie notes show n1");
    assert_eq!(s, format!("  {}\n  … (prototypie notes show n1)\n", "x".repeat(600)));
    assert_eq!(out::shortened("a\nb", "  ", "m"), "  a\n  b\n");
}

fn native() -> serde_json::Value {
    json!({ "id": "n7k2x9q0bz4", "status": "pending", "createdAt": "2026-11-20T10:00:00.123Z", "location": "prototypie-rune://zzspace/review-dock",
        "element": "button \"Review\"", "elementPath": "header-actions > review-dock",
        "comment": "Make it smaller\n— Rune element `review-dock` at crates/proto-app/src/features/review/dock.rs:33 (features::review::dock)", "thread": [] })
}

#[test]
fn cli_list_snapshot() {
    // AN-CLI-210: a native note, a job note and a design note.
    let job = json!({ "id": "n1a2b3c4d5e", "status": "pending", "createdAt": "2026-11-20T10:02:00.000Z", "location": "http://localhost:3124/",
        "element": "", "elementPath": "", "tag": { "kind": "job", "jobId": "J1abc", "index": 0 }, "comment": "[prototypie job J1abc#0] make it pop · COMBINE …", "thread": [] });
    let design = json!({ "id": "n9z8y7x6w5v", "status": "pending", "createdAt": "2026-11-20T10:03:00.000Z", "location": "prototypie-rune://zzspace/design",
        "element": "h1", "elementPath": "", "designId": "zzd1", "comment": "Bigger", "thread": [{ "role": "human", "content": "x" }] });
    let v = json!({ "notes": [native(), job, design], "count": 3, "more": false });
    let expected = "\
3 pending notes

n7k2x9q0bz4  pending  2026-11-20 10:00Z  prototypie-rune://zzspace/review-dock
  on: button \"Review\" (header-actions > review-dock)
  Make it smaller
  — Rune element `review-dock` at crates/proto-app/src/features/review/dock.rs:33 (features::review::dock)

n1a2b3c4d5e  pending  2026-11-20 10:02Z  http://localhost:3124/
  job J1abc#0: prototypie queue show J1abc#0
  [prototypie job J1abc#0] make it pop · COMBINE …

n9z8y7x6w5v  pending  2026-11-20 10:03Z  prototypie-rune://zzspace/design
  on: h1
  design zzd1
  Bigger
  1 message in thread
";
    assert_eq!(notes::list_text(&v, "pending"), expected);
    assert_eq!(notes::list_text(&json!({ "notes": [] }), "pending"), "No pending notes.\n");
    assert_eq!(notes::list_text(&json!({ "notes": [] }), "all"), "No notes.\n");
    assert!(notes::list_text(&json!({ "notes": [native()] }), "active").starts_with("1 open note\n"));
}

#[test]
fn cli_watch_snapshot() {
    let v = json!({ "notes": [native()], "count": 1, "cursor": 57, "timedOut": false });
    let t = notes::watch_text(&v, 100, 57, "prototypie notes watch --after 57");
    assert!(t.starts_with("1 new note (cursor 57)\n\nn7k2x9q0bz4  pending"), "{t}");
    assert!(t.ends_with("\nNext: prototypie notes watch --after 57\n"), "{t}");
    let none = json!({ "notes": [], "count": 0, "cursor": 57, "timedOut": true });
    assert_eq!(notes::watch_text(&none, 3, 57, "prototypie notes watch --after 57 --timeout 3s"), "No new notes in 3s (cursor 57).\nNext: prototypie notes watch --after 57 --timeout 3s\n");
}

#[test]
fn cli_show_snapshot() {
    // AN-CLI-212
    let mut n = native();
    n["status"] = json!("resolved");
    n["kind"] = json!("feedback");
    n["designId"] = json!("zzd1");
    n["target"] = json!({ "sourceLocations": "crates/proto-app/src/features/review/dock.rs:33" });
    n["updatedAt"] = json!("2026-11-20T10:01:30.500Z");
    n["closedBy"] = json!("agent");
    n["closedAt"] = json!("2026-11-20T10:30:00.000Z");
    n["thread"] = json!([{ "role": "agent", "content": "proto-worker-3: on it", "createdAt": "2026-11-20T10:01:30.500Z" }]);
    let expected = "\
Note n7k2x9q0bz4 · resolved · feedback
Location   prototypie-rune://zzspace/review-dock
Element    button \"Review\"
Path       header-actions > review-dock
Source     crates/proto-app/src/features/review/dock.rs:33
Design     zzd1
Created    2026-11-20 10:00Z   Updated 2026-11-20 10:01Z
Closed     resolved by agent, 2026-11-20 10:30Z

Make it smaller
— Rune element `review-dock` at crates/proto-app/src/features/review/dock.rs:33 (features::review::dock)

Thread (1)
  agent · 2026-11-20 10:01Z
    proto-worker-3: on it
";
    assert_eq!(out::note_detail(&n), expected);
    let job = json!({ "id": "n1", "status": "pending", "kind": "feedback", "createdAt": "x", "comment": "c", "tag": { "kind": "job", "jobId": "J1abc", "index": 0 } });
    assert!(out::note_detail(&job).contains("\nJob        J1abc#0 (prototypie queue show J1abc#0)\n"));
}

#[test]
fn cli_queue_snapshot() {
    // AN-CLI-221: two items.
    let v = json!({ "items": [
        { "jobId": "J1abc", "index": 0, "agent": "proto-worker-2", "createdAt": "2026-11-20T10:02:00.000Z", "mode": "edit", "scope": "component",
          "direction": { "title": "Bolder price block", "brief": "b" }, "instruction": "make it pop",
          "context": { "spaceName": "Checkout", "spacePath": ["Main canvas", "Shop", "Checkout"] } },
        { "jobId": "J1abc", "index": 1, "createdAt": "2026-11-20T10:02:00.000Z", "direction": { "title": "", "brief": "calmer" }, "instruction": "" }
    ] });
    let expected = "\
2 queue items waiting

J1abc#0  claimed by proto-worker-2  2026-11-20 10:02Z
  space: Main canvas › Shop › Checkout   mode: edit   scope: component
  direction: Bolder price block
  make it pop

J1abc#1  unclaimed  2026-11-20 10:02Z
  calmer
";
    assert_eq!(queue::list_text(&v), expected);
    let item = json!({ "jobId": "J1abc", "index": 0, "baseHtml": "<p>héllo</p>", "bases": [{ "title": "A", "html": "<p>a</p>" }], "guidelines": { "title": "G", "text": "long" } });
    let t = queue::show_text(&item, &JobRef { job: "J1abc".into(), index: 0 });
    assert!(t.contains("baseHtml    12 characters (prototypie queue show J1abc#0 --base-html)\n"), "{t}");
    assert!(t.contains("\"html\":\"8 characters\""), "{t}");
    assert!(t.contains("\"text\":\"4 characters\""), "{t}");
}

/// The main help as spec-public/05 AN-CLI-229 prints it (word for word).
const SPEC_HELP: &str = r#"prototypie: work with the Prototypie app from a terminal or a coding agent.
The app must be running; this command talks to it on 127.0.0.1:3124.

Notes (feedback people pin on the app and its designs):
  prototypie notes watch [--ack] [--after CURSOR]   wait for new notes (prints the next command)
  prototypie notes list [--status S] [--all]        list notes (default: pending)
  prototypie notes show ID                          one note with its thread
  prototypie notes ack ID...                        take notes on ("I'm on it")
  prototypie notes reply ID TEXT                    answer in the note's thread
  prototypie notes resolve ID [--summary TEXT]      mark done
  prototypie notes dismiss ID --reason TEXT         drop, with a reason

Design jobs (the agent queue):
  prototypie queue list | show REF | claim REF
  prototypie queue deliver REF --title T --html FILE

Cards on the canvas:
  prototypie cards list [--space ID]
  prototypie cards create --space ID --title T --html FILE

Setup:
  prototypie status                                 is the app running? what is waiting?
  prototypie skill install                          install the agent skill (SKILL.md)

Options: --json  --as NAME (or PROTOTYPIE_AGENT)  --server URL (or PROTOTYPIE_SERVER)
Exit codes: 0 ok, 2 usage, 3 app not reachable, 4 not found, 5 conflict, 1 other error.
More: prototypie help notes | queue | cards | skill
"#;

#[test]
fn cli_main_help_is_word_for_word() {
    assert_eq!(help::MAIN, SPEC_HELP);
}

#[test]
fn cli_every_help_is_short_and_has_a_usage_line() {
    let pairs = [
        ("notes", ["watch", "list", "show", "ack", "reply", "resolve", "dismiss", "collections"].as_slice()),
        ("queue", &["list", "show", "claim", "deliver"]),
        ("cards", &["list", "create"]),
        ("skill", &["install", "show"]),
    ];
    for (g, cmds) in pairs {
        assert!(help::group(g).unwrap().lines().count() <= 30, "{g}");
        for c in cmds {
            let t = help::command(g, c).unwrap_or_else(|| panic!("{g} {c}"));
            assert!(t.lines().count() <= 30, "{g} {c}");
            assert!(t.starts_with(&format!("Usage: prototypie {g} {c}")), "{g} {c}");
        }
    }
}

#[test]
fn cli_skill_is_the_repository_file() {
    // AN-CLI-301/303: one source; front matter with name and description.
    let file = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../skills/prototypie/SKILL.md")).unwrap();
    assert_eq!(skill::SKILL, file);
    let mut lines = skill::SKILL.lines();
    assert_eq!(lines.next(), Some("---"));
    let front: Vec<&str> = lines.by_ref().take_while(|l| *l != "---").collect();
    assert!(front.contains(&"name: prototypie"));
    let desc = front.iter().find_map(|l| l.strip_prefix("description: ")).unwrap();
    assert!(!desc.is_empty() && desc.chars().count() <= 1024);
    assert!(skill::SKILL.contains("## The loop: wait, take, work, close"));
    // When the spec sits next to the workspace (clean-room layout), the text equals its appendix A.
    let spec = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../spec-public/05-cli-service-skill.md");
    if let Ok(spec) = std::fs::read_to_string(spec) {
        let a = spec.find("\n<!-- skill begin -->\n").unwrap();
        let block = &spec[a..];
        let start = block.find("````markdown\n").unwrap() + "````markdown\n".len();
        let end = block.find("\n````\n<!-- skill end -->").unwrap() + 1;
        assert_eq!(&block[start..end], skill::SKILL);
    }
}
