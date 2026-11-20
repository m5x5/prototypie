//! `prototypie status`: is the app running, what waits, is the skill installed (spec-public/05 AN-CLI-227).

use serde_json::{Value, json};

use super::http;
use super::out::{self, Fail};
use super::skill;
use super::{Ctx, VERSION};

fn skill_json() -> (String, Value) {
    let (state, paths) = skill::state();
    let shown: Vec<String> = paths.iter().map(|p| skill::tilde(p)).collect();
    let line = match state {
        "installed" => format!("installed ({})", shown.join(", ")),
        "outdated" => format!("outdated, run \"prototypie skill install\" ({})", shown.join(", ")),
        _ => "not installed (run \"prototypie skill install\")".to_string(),
    };
    (line, json!({ "state": state, "paths": paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>() }))
}

/// `(now)`, `(4 min ago)` from a watcher entry.
fn seen(w: &Value) -> String {
    if w["waiting"] == true {
        return "now".into();
    }
    let at = w["lastSeenAt"].as_str().and_then(|t| chrono_ms(t));
    match at {
        Some(ms) => {
            let mins = (now_ms() - ms).max(0) / 60_000;
            if mins == 0 { "just now".into() } else { format!("{mins} min ago") }
        }
        None => "recently".into(),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Unix ms of `YYYY-MM-DDTHH:MM:SS.mmmZ` (no time crate in this binary: civil-from-days by hand).
fn chrono_ms(t: &str) -> Option<i64> {
    let b = t.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| t.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let ms = if b.len() >= 23 && b[19] == b'.' { num(20..23)? } else { 0 };
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + hh) * 60 + mm) * 60_000 + ss * 1000 + ms)
}

pub fn status_text(server: &str, info: &Value, queue: Option<usize>, skill_line: &str) -> String {
    let c = &info["counts"];
    let mut s = format!(
        "Prototypie is running: {server}, pid {}, version {}\n",
        info["pid"].as_u64().unwrap_or(0),
        out::safe_line(info["appVersion"].as_str().unwrap_or("?"))
    );
    s.push_str(&format!(
        "Notes: {} pending, {} acknowledged (cursor {})\n",
        c["pending"].as_u64().unwrap_or(0),
        c["acknowledged"].as_u64().unwrap_or(0),
        info["cursor"].as_u64().unwrap_or(0)
    ));
    s.push_str(&match queue {
        Some(0) => "Queue: nothing waiting\n".to_string(),
        Some(n) => format!("Queue: {} waiting\n", out::plural(n, "item", "items")),
        None => "Queue: unknown\n".to_string(),
    });
    let watchers: Vec<String> = info["watchers"]
        .as_array()
        .map(|l| l.iter().map(|w| format!("{} ({})", out::safe_line(w["name"].as_str().unwrap_or("?")), seen(w))).collect())
        .unwrap_or_default();
    s.push_str(&format!("Agents waiting for notes: {}\n", if watchers.is_empty() { "none".to_string() } else { watchers.join(", ") }));
    s.push_str(&format!("Skill: {skill_line}\n"));
    if let Some(app) = info["appVersion"].as_str()
        && app != VERSION
    {
        s.push_str(&format!("Note: this command is version {VERSION}, the app {}.\n", out::safe_line(app)));
    }
    s
}

pub fn run(ctx: &Ctx) -> Result<i32, Fail> {
    if !ctx.args().is_empty() {
        return Err(Fail::usage("\"prototypie status\" takes no arguments. Run \"prototypie status --help\"."));
    }
    let (skill_line, skill) = skill_json();
    let down = |f: Fail| {
        if ctx.json {
            out::print_json(&json!({ "running": false, "server": ctx.server.host, "skill": skill.clone() }));
        }
        f
    };
    let (code, info) = ctx.call("GET", "/api/notes/info", None, http::NORMAL).map_err(down)?;
    if code != 200 || info["service"] != "prototypie-notes" {
        return Err(down(http::no_notes_service(&ctx.server)));
    }
    let queue = match ctx.call("GET", "/api/agent-queue", None, http::NORMAL) {
        Ok((200, v)) => v["items"].as_array().map(Vec::len),
        _ => None,
    };
    if ctx.json {
        out::print_json(&json!({ "running": true, "server": ctx.server.host, "info": info, "queueWaiting": queue, "skill": skill }));
    } else {
        out::print(&status_text(&ctx.server.host, &info, queue, &skill_line));
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn cli_status_times() {
        assert_eq!(super::chrono_ms("1970-01-01T00:00:01.500Z"), Some(1500));
        assert_eq!(super::chrono_ms("2026-11-20T10:00:00.123Z"), Some(1_795_168_800_123));
        assert_eq!(super::chrono_ms("garbage"), None);
    }
}
