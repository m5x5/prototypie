//! What the CLI prints: failures with exit codes, safe text (no terminal control codes), times, shortened
//! comments and the note blocks shared by `list` and `watch` (spec-public/05 AN-CLI-205, -210, -212, -230, -506).

use std::io::Write;

use serde_json::{Value, json};

/// A failed command: exit code, a short machine code (for `--json`), the sentence after `prototypie: `.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fail {
    pub exit: i32,
    pub code: String,
    pub msg: String,
}

impl Fail {
    pub fn new(exit: i32, code: &str, msg: impl Into<String>) -> Fail {
        Fail { exit, code: code.into(), msg: msg.into() }
    }

    pub fn usage(msg: impl Into<String>) -> Fail {
        Fail::new(2, "usage", msg)
    }

    pub fn not_found(msg: impl Into<String>) -> Fail {
        Fail::new(4, "not_found", msg)
    }

    pub fn conflict(msg: impl Into<String>) -> Fail {
        Fail::new(5, "conflict", msg)
    }

    /// Report on stderr: `prototypie: …`, or the JSON error object.
    pub fn report(&self, json: bool) {
        let mut err = std::io::stderr().lock();
        if json {
            let v = json!({ "error": self.msg, "code": self.code, "exitCode": self.exit });
            let _ = writeln!(err, "{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        } else {
            let _ = writeln!(err, "prototypie: {}", safe(&self.msg));
        }
    }
}

/// Print to stdout (a closed pipe is not an error worth a panic).
pub fn print(s: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(s.as_bytes());
    let _ = out.flush();
}

pub fn println(s: &str) {
    print(&format!("{s}\n"));
}

/// Exactly one JSON document, 2-space indented.
pub fn print_json(v: &Value) {
    println(&serde_json::to_string_pretty(v).unwrap_or_default());
}

/// Control characters other than `\n` and `\t` shown as `\u{…}` (AN-CLI-506).
pub fn safe(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_control() && c != '\n' && c != '\t' {
            out.push_str(&format!("\\u{{{:x}}}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}

/// One line: newlines and tabs become spaces too.
pub fn safe_line(s: &str) -> String {
    safe(s).replace(['\n', '\t'], " ")
}

/// `2026-11-20T10:00:00.123Z` → `2026-11-20 10:00Z`; anything else as it is.
pub fn time(iso: &str) -> String {
    let b = iso.as_bytes();
    if b.len() >= 16 && b[4] == b'-' && b[7] == b'-' && b[10] == b'T' && b[13] == b':' {
        format!("{} {}Z", &iso[..10], &iso[11..16])
    } else {
        safe_line(iso)
    }
}

pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Indent every line by `pad`, at most 6 lines / 600 characters, then the hint to the full text.
pub fn shortened(text: &str, pad: &str, more: &str) -> String {
    let text = safe(text);
    let mut out = String::new();
    let mut chars = 0;
    let mut cut = false;
    for (i, line) in text.split('\n').enumerate() {
        if i >= 6 {
            cut = true;
            break;
        }
        let left = 600usize.saturating_sub(chars);
        let n = line.chars().count();
        if n > left {
            out.push_str(&format!("{pad}{}\n", line.chars().take(left).collect::<String>()));
            cut = true;
            break;
        }
        chars += n;
        out.push_str(&format!("{pad}{line}\n"));
    }
    if cut {
        out.push_str(&format!("{pad}… ({more})\n"));
    }
    out
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

/// The hint line for a note's tag (or its design), if any.
pub fn tag_line(n: &Value) -> Option<String> {
    let t = &n["tag"];
    match t["kind"].as_str() {
        Some("job") => {
            let r = format!("{}#{}", s(t, "jobId"), t["index"].as_u64().unwrap_or(0));
            Some(format!("job {r}: prototypie queue show {}", safe_line(&r)))
        }
        Some("implement") => Some(format!("implement design {}", safe_line(s(t, "designId")))),
        Some("project-info") => Some(format!("project-info space {}", safe_line(s(t, "spaceId")))),
        _ => n["designId"].as_str().map(|d| format!("design {}", safe_line(d))),
    }
}

/// A note in a list (`notes list`, `notes watch`).
pub fn note_block(n: &Value, reply: bool) -> String {
    let id = s(n, "id");
    let mut out = format!("{}  {}  {}  {}\n", safe_line(id), safe_line(s(n, "status")), time(s(n, "createdAt")), safe_line(s(n, "location")));
    let element = s(n, "element");
    if !element.is_empty() {
        let path = s(n, "elementPath");
        if path.is_empty() {
            out.push_str(&format!("  on: {}\n", safe_line(element)));
        } else {
            out.push_str(&format!("  on: {} ({})\n", safe_line(element), safe_line(path)));
        }
    }
    if let Some(t) = tag_line(n) {
        out.push_str(&format!("  {t}\n"));
    }
    if reply {
        out.push_str("  new reply from a person in the thread\n");
    }
    out.push_str(&shortened(s(n, "comment"), "  ", &format!("prototypie notes show {}", safe_line(id))));
    let t = n["thread"].as_array().map_or(0, Vec::len);
    if t > 0 {
        out.push_str(&format!("  {} in thread\n", plural(t, "message", "messages")));
    }
    out
}

/// Notes one after another, a blank line between them.
pub fn note_blocks(notes: &[Value], replies: &[String]) -> String {
    notes
        .iter()
        .map(|n| note_block(n, n["id"].as_str().is_some_and(|id| replies.iter().any(|r| r == id))))
        .collect::<Vec<_>>()
        .join("\n")
}

fn row(label: &str, value: &str) -> String {
    format!("{label:<11}{value}\n")
}

/// `notes show` (AN-CLI-212).
pub fn note_detail(n: &Value) -> String {
    let mut out = format!("Note {} · {} · {}\n", safe_line(s(n, "id")), safe_line(s(n, "status")), safe_line(s(n, "kind")));
    for (label, v) in [("Location", s(n, "location")), ("Element", s(n, "element")), ("Path", s(n, "elementPath")), ("Source", s(&n["target"], "sourceLocations"))] {
        if !v.is_empty() {
            out.push_str(&row(label, &safe_line(v)));
        }
    }
    if let Some(d) = n["designId"].as_str().filter(|d| !d.is_empty()) {
        out.push_str(&row("Design", &safe_line(d)));
    }
    let t = &n["tag"];
    match t["kind"].as_str() {
        Some("job") => {
            let r = safe_line(&format!("{}#{}", s(t, "jobId"), t["index"].as_u64().unwrap_or(0)));
            out.push_str(&row("Job", &format!("{r} (prototypie queue show {r})")));
        }
        Some("implement") => out.push_str(&row("Implement", &safe_line(s(t, "designId")))),
        Some("project-info") => out.push_str(&row("Space", &safe_line(s(t, "spaceId")))),
        _ => {}
    }
    let mut created = time(s(n, "createdAt"));
    if let Some(u) = n["updatedAt"].as_str() {
        created.push_str(&format!("   Updated {}", time(u)));
    }
    out.push_str(&row("Created", &created));
    if let Some(by) = n["closedBy"].as_str() {
        let at = n["closedAt"].as_str().map(|a| format!(", {}", time(a))).unwrap_or_default();
        out.push_str(&row("Closed", &format!("{} by {}{at}", safe_line(s(n, "status")), safe_line(by))));
    }
    out.push('\n');
    out.push_str(&safe(s(n, "comment")));
    out.push('\n');
    if let Some(thread) = n["thread"].as_array().filter(|t| !t.is_empty()) {
        out.push_str(&format!("\nThread ({})\n", thread.len()));
        for m in thread {
            out.push_str(&format!("  {} · {}\n", safe_line(s(m, "role")), time(s(m, "createdAt"))));
            for line in safe(s(m, "content")).split('\n') {
                out.push_str(&format!("    {line}\n"));
            }
        }
    }
    out
}
