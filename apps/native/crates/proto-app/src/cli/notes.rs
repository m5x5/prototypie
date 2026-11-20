//! `prototypie notes …` (spec-public/05 AN-CLI-210…218).

use serde_json::{Value, json};

use super::args::{self, show_duration};
use super::http::{self, Gone, enc, query};
use super::out::{self, Fail, plural};
use super::{Ctx, checked_text, read_input, refused, text_option};

pub fn run(ctx: &Ctx, command: &str) -> Result<i32, Fail> {
    match command {
        "list" => list(ctx),
        "watch" => watch(ctx),
        "show" => show(ctx),
        "ack" => ack(ctx),
        "reply" => reply(ctx),
        "resolve" => resolve(ctx),
        "dismiss" => dismiss(ctx),
        "collections" => collections(ctx),
        _ => unreachable!("checked by the dispatcher"),
    }
}

fn one_id(ctx: &Ctx) -> Result<String, Fail> {
    match ctx.args() {
        [id] => Ok(id.clone()),
        [] => Err(ctx.needs("a note ID")),
        _ => Err(Fail::usage(format!("\"prototypie {}\" takes one note ID. Run \"prototypie {} --help\".", ctx.full, ctx.full))),
    }
}

fn no_note(id: &str) -> Fail {
    Fail::not_found(format!("No note with id {}.", out::safe_line(id)))
}

fn status_word(v: Option<&str>) -> Result<String, Fail> {
    match v {
        None => Ok("pending".into()),
        Some(s @ ("pending" | "acknowledged" | "resolved" | "dismissed" | "active" | "all")) => Ok(s.into()),
        Some(_) => Err(Fail::usage("--status must be pending, acknowledged, resolved, dismissed, active or all.")),
    }
}

fn tag_word(ctx: &Ctx) -> Result<Option<String>, Fail> {
    match ctx.line.value("tag") {
        None => Ok(None),
        Some(t @ ("job" | "implement" | "project-info" | "none")) => Ok(Some(t.into())),
        Some(_) => Err(Fail::usage("--tag must be job, implement, project-info or none.")),
    }
}

/// The filters `list` and `watch` share, as query pairs.
fn filters(ctx: &Ctx) -> Result<Vec<(&'static str, Option<String>)>, Fail> {
    let v = |k: &str| ctx.line.value(k).map(str::to_string);
    Ok(vec![("location", v("location")), ("locationPrefix", v("prefix")), ("collection", v("collection")), ("tag", tag_word(ctx)?)])
}

/// `2 pending notes` / `1 open note` / `3 notes`; empty: `No pending notes.`
fn heading(n: usize, status: &str) -> String {
    let word = match status {
        "all" => String::new(),
        "active" => "open ".into(),
        s => format!("{s} "),
    };
    if n == 0 { format!("No {word}notes.") } else { format!("{n} {word}{}", if n == 1 { "note" } else { "notes" }) }
}

// ---- list --------------------------------------------------------------------------------------------------

fn list(ctx: &Ctx) -> Result<i32, Fail> {
    if !ctx.args().is_empty() {
        return Err(Fail::usage(format!("\"prototypie notes list\" takes no arguments. Run \"prototypie notes list --help\".")));
    }
    let status = if ctx.line.has("all") { "all".to_string() } else { status_word(ctx.line.value("status"))? };
    let limit = args::number_opt(&ctx.line, "limit", 1, 5000)?.unwrap_or(100);
    let mut q = vec![("status", Some(status.clone()))];
    q.extend(filters(ctx)?);
    q.push(("limit", Some(limit.to_string())));
    let (code, v) = ctx.notes("GET", &format!("/api/notes{}", query(&q)), None)?;
    if code != 200 {
        return Err(refused(code, &v));
    }
    ctx.show(&v, || list_text(&v, &status));
    Ok(0)
}

pub fn list_text(v: &Value, status: &str) -> String {
    let notes = v["notes"].as_array().cloned().unwrap_or_default();
    let mut s = format!("{}\n", heading(notes.len(), status));
    if !notes.is_empty() {
        s.push('\n');
        s.push_str(&out::note_blocks(&notes, &[]));
    }
    if v["more"] == true {
        s.push_str("\n(more notes: raise --limit)\n");
    }
    s
}

// ---- watch -------------------------------------------------------------------------------------------------

struct Watch {
    after: u64,
    timeout: u64,
    batch: u64,
    ack: bool,
    replies: bool,
}

/// The full next command: `prototypie notes watch --after <c>` plus the filters and settings given.
fn next_command(ctx: &Ctx, w: &Watch, cursor: u64) -> String {
    let mut parts = vec![format!("prototypie notes watch --after {cursor}")];
    let quote = |s: &str| {
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-:#=@%+,".contains(&b)) {
            s.to_string()
        } else {
            format!("'{}'", s.replace('\'', "'\\''"))
        }
    };
    for (opt, name) in [("location", "location"), ("prefix", "prefix"), ("collection", "collection"), ("tag", "tag")] {
        if let Some(v) = ctx.line.value(opt) {
            parts.push(format!("--{name} {}", quote(v)));
        }
    }
    if w.ack {
        parts.push("--ack".into());
    }
    if w.replies {
        parts.push("--replies".into());
    }
    if w.timeout != 100 {
        parts.push(format!("--timeout {}", show_duration(w.timeout)));
    }
    if w.batch != 10 {
        parts.push(format!("--batch {}", show_duration(w.batch)));
    }
    if let Some(n) = &ctx.as_option {
        parts.push(format!("--as {}", quote(n)));
    }
    out::safe_line(&parts.join(" "))
}

fn watch(ctx: &Ctx) -> Result<i32, Fail> {
    if !ctx.args().is_empty() {
        return Err(Fail::usage("\"prototypie notes watch\" takes no arguments. Run \"prototypie notes watch --help\"."));
    }
    if ctx.line.has("status") {
        return Err(Fail::usage("Unknown option \"--status\" for \"prototypie notes watch\". Run \"prototypie notes watch --help\"."));
    }
    let mut w = Watch {
        after: match ctx.line.value("after") {
            None => 0,
            Some(a) => a.parse().ok().filter(|_| a.bytes().all(|b| b.is_ascii_digit())).ok_or_else(|| Fail::usage("--after must be a cursor (a whole number of 0 or more)."))?,
        },
        timeout: args::duration_opt(&ctx.line, "timeout", 1, 300, 100)?,
        batch: args::duration_opt(&ctx.line, "batch", 0, 60, 10)?,
        ack: ctx.line.has("ack"),
        replies: ctx.line.has("replies"),
    };
    let follow = ctx.line.has("follow");
    let base = filters(ctx)?;
    loop {
        let mut q = vec![("after", Some(w.after.to_string())), ("timeout", Some(w.timeout.to_string())), ("batchWindow", Some(w.batch.to_string()))];
        q.extend(base.iter().cloned());
        q.push(("by", Some(ctx.name.clone())));
        if w.replies {
            q.push(("replies", Some("1".into())));
        }
        let limit = std::time::Duration::from_secs(w.timeout + w.batch + 15);
        let quit = || Fail::new(3, "quit", "Prototypie quit while waiting for notes.");
        let (code, mut v) = match ctx.send("GET", &format!("/api/notes/watch{}", query(&q)), None, limit) {
            Ok(r) => (r.status, r.json()),
            Err(Gone::Broken) => return Err(quit()),
            Err(g) => return Err(http::unreachable(&ctx.server, &g)),
        };
        if code == 503 {
            return Err(quit());
        }
        super::check_notes_answer(&ctx.server, code, &v)?;
        if code != 200 {
            return Err(refused(code, &v));
        }
        let cursor = v["cursor"].as_u64().unwrap_or(w.after);
        if w.ack {
            ack_batch(ctx, &mut v)?;
        }
        let next = next_command(ctx, &w, cursor);
        v["next"] = json!(next);
        if ctx.json {
            if follow { out::println(&v.to_string()) } else { out::print_json(&v) }
        } else {
            out::print(&watch_text(&v, w.timeout, cursor, &next));
        }
        if !follow {
            return Ok(0);
        }
        w.after = cursor;
    }
}

/// Take every delivered pending note on (`ifStatus: pending`); notes someone else took drop out silently.
fn ack_batch(ctx: &Ctx, v: &mut Value) -> Result<(), Fail> {
    let notes = v["notes"].as_array().cloned().unwrap_or_default();
    let mut kept = Vec::new();
    for n in notes {
        let id = n["id"].as_str().unwrap_or("");
        if n["status"] != "pending" {
            kept.push(n);
            continue;
        }
        let body = json!({ "status": "acknowledged", "by": ctx.name, "ifStatus": "pending" });
        let (code, r) = ctx.notes("POST", &format!("/api/notes/{}/status", enc(id)), Some(&body))?;
        if code == 200 {
            kept.push(r["note"].clone());
        }
    }
    v["count"] = json!(kept.len());
    v["notes"] = Value::Array(kept);
    Ok(())
}

pub fn watch_text(v: &Value, timeout: u64, cursor: u64, next: &str) -> String {
    let notes = v["notes"].as_array().cloned().unwrap_or_default();
    let replies: Vec<String> = v["replies"].as_array().map(|r| r.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if notes.is_empty() && v["timedOut"] == false {
        // Everything new was taken by another agent (`--ack`).
        return format!("No new notes for you (cursor {cursor}).\nNext: {next}\n");
    }
    if notes.is_empty() {
        return format!("No new notes in {} (cursor {cursor}).\nNext: {next}\n", show_duration(timeout));
    }
    format!("{} (cursor {cursor})\n\n{}\nNext: {next}\n", plural(notes.len(), "new note", "new notes"), out::note_blocks(&notes, &replies))
}

// ---- show, ack, reply, resolve, dismiss --------------------------------------------------------------------

fn get_note(ctx: &Ctx, id: &str) -> Result<Value, Fail> {
    let (code, v) = ctx.notes("GET", &format!("/api/notes/{}", enc(id)), None)?;
    match code {
        200 => Ok(v),
        404 => Err(no_note(id)),
        _ => Err(refused(code, &v)),
    }
}

fn show(ctx: &Ctx) -> Result<i32, Fail> {
    let id = one_id(ctx)?;
    let v = get_note(ctx, &id)?;
    ctx.show(&v, || out::note_detail(&v["note"]));
    Ok(0)
}

/// A status change; `Err` carries the failure for this id.
fn set_status(ctx: &Ctx, id: &str, body: &Value) -> Result<(u16, Value), Fail> {
    let (code, v) = ctx.notes("POST", &format!("/api/notes/{}/status", enc(id)), Some(body))?;
    match code {
        200 | 409 => Ok((code, v)),
        404 => Err(no_note(id)),
        _ => Err(refused(code, &v)),
    }
}

fn ack(ctx: &Ctx) -> Result<i32, Fail> {
    let ids = ctx.args().to_vec();
    if ids.is_empty() {
        return Err(ctx.needs("at least one note ID"));
    }
    let message = text_option(ctx, "message")?;
    let force = ctx.line.has("force");
    let mut done = Vec::new();
    let mut errors = Vec::new();
    let mut worst = 0;
    for id in &ids {
        let mut body = json!({ "status": "acknowledged", "by": ctx.name });
        if !force {
            body["ifStatus"] = json!("pending");
        }
        if let Some(m) = &message {
            body["message"] = json!(m);
        }
        let result = match set_status(ctx, id, &body) {
            Ok((200, v)) => Ok(v),
            Ok((_, v)) => {
                let now = v["note"]["status"].as_str().unwrap_or("taken");
                let safe = out::safe_line(id);
                Err(if now == "acknowledged" {
                    Fail::conflict(format!("Note {safe} is already acknowledged (another agent may have taken it; --force takes it anyway)."))
                } else {
                    Fail::conflict(format!("Note {safe} is already {now}."))
                })
            }
            Err(f) if f.exit == 3 => return Err(f),
            Err(f) => Err(f),
        };
        match result {
            Ok(v) => {
                if !ctx.json {
                    out::println(&format!("Acknowledged {}", out::safe_line(id)));
                }
                done.push(v["note"].clone());
            }
            Err(f) => {
                worst = worst.max(f.exit);
                if !ctx.json {
                    f.report(false);
                }
                errors.push(json!({ "id": id, "error": f.msg, "code": f.code }));
            }
        }
    }
    if ctx.json {
        out::print_json(&json!({ "notes": done, "errors": errors }));
    }
    Ok(worst)
}

fn reply(ctx: &Ctx) -> Result<i32, Fail> {
    let args = ctx.args();
    let Some(id) = args.first().cloned() else { return Err(ctx.needs("a note ID and the text")) };
    let role = match ctx.line.value("role") {
        None | Some("agent") => "agent",
        Some("human") => "human",
        Some(_) => return Err(Fail::usage("--role must be agent or human.")),
    };
    let text = match (args.len() > 1, ctx.line.value("file")) {
        (true, Some(_)) => return Err(Fail::usage("Pass the text as arguments or with --file, not both.")),
        (true, None) => args[1..].join(" "),
        (false, Some(f)) => read_input(f, super::TEXT_MAX, super::TEXT_TOO_LONG)?,
        (false, None) => return Err(ctx.needs("the text (or --file FILE)")),
    };
    let content = if role == "agent" {
        checked_text(ctx, &text)?
    } else {
        if text.trim().is_empty() {
            return Err(Fail::usage("The text is empty."));
        }
        if text.chars().count() > 20_000 {
            return Err(Fail::usage(super::TEXT_TOO_LONG));
        }
        text
    };
    let (code, v) = ctx.notes("POST", &format!("/api/notes/{}/thread", enc(&id)), Some(&json!({ "role": role, "content": content })))?;
    match code {
        200 | 201 => {}
        404 => return Err(no_note(&id)),
        _ => return Err(refused(code, &v)),
    }
    let n = v["note"]["thread"].as_array().map_or(0, Vec::len);
    ctx.show(&v, || format!("Replied to {} ({} in thread)\n", out::safe_line(&id), plural(n, "message", "messages")));
    Ok(0)
}

fn resolve(ctx: &Ctx) -> Result<i32, Fail> {
    let id = one_id(ctx)?;
    let summary = text_option(ctx, "summary")?;
    let mut body = json!({ "status": "resolved", "by": ctx.name, "ifStatus": ["pending", "acknowledged", "dismissed"] });
    if let Some(s) = &summary {
        body["message"] = json!(s);
    }
    let (code, v) = set_status(ctx, &id, &body)?;
    if code == 200 {
        ctx.show(&v, || format!("Resolved {}\n", out::safe_line(&id)));
        return Ok(0);
    }
    // 409: it was resolved already. A summary still goes into the thread.
    let v = match &summary {
        Some(s) => set_status(ctx, &id, &json!({ "status": "resolved", "by": ctx.name, "message": s }))?.1,
        None => json!({ "note": v["note"] }),
    };
    ctx.show(&v, || format!("Note {} was already resolved.\n", out::safe_line(&id)));
    Ok(0)
}

fn dismiss(ctx: &Ctx) -> Result<i32, Fail> {
    let id = one_id(ctx)?;
    let Some(reason) = text_option(ctx, "reason")? else {
        return Err(Fail::usage("dismiss needs --reason TEXT (the reason goes into the note's thread)."));
    };
    let (code, v) = set_status(ctx, &id, &json!({ "status": "dismissed", "by": ctx.name, "message": reason }))?;
    if code != 200 {
        return Err(refused(code, &v));
    }
    ctx.show(&v, || format!("Dismissed {} (reason added to the thread)\n", out::safe_line(&id)));
    Ok(0)
}

// ---- collections -------------------------------------------------------------------------------------------

fn collections(ctx: &Ctx) -> Result<i32, Fail> {
    let state = match ctx.line.value("state") {
        None => None,
        Some(s @ ("active" | "closed")) => Some(s.to_string()),
        Some(_) => return Err(Fail::usage("--state must be active or closed.")),
    };
    let q = query(&[("location", ctx.line.value("location").map(str::to_string)), ("state", state)]);
    let (code, v) = ctx.notes("GET", &format!("/api/notes/collections{q}"), None)?;
    if code != 200 {
        return Err(refused(code, &v));
    }
    ctx.show(&v, || collections_text(&v));
    Ok(0)
}

pub fn collections_text(v: &Value) -> String {
    let list = v["collections"].as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        return "No collections.\n".into();
    }
    list.iter()
        .map(|c| {
            format!(
                "{}  {}  {} pending / {} notes  {}\n",
                out::safe_line(c["id"].as_str().unwrap_or("")),
                out::safe_line(c["state"].as_str().unwrap_or("")),
                c["pendingCount"].as_u64().unwrap_or(0),
                c["noteCount"].as_u64().unwrap_or(0),
                out::safe_line(c["location"].as_str().unwrap_or(""))
            )
        })
        .collect()
}
