//! Agent tasks for the header capacity and task panel (reference `src/lib/server/agent-tasks.ts`): Rex agent
//! capacity, queue items with their notes in the notes service, open notes, and Implement notes in any state.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use proto_ai::{QueueItem, QueueStatus};
use serde_json::{Value, json};

use crate::context;
use crate::notes::{Note, NotesService, ReplyError, Status};
use crate::store::SharedStore;
use crate::util;

const MAX_QUEUE_TASKS: usize = 80;

/// Caches shared by the task routes (capacity 3 s, Implement scan 60 s).
#[derive(Clone, Default)]
pub struct TaskCache {
    capacity: Arc<Mutex<Option<(Instant, Value)>>>,
    implement: Arc<Mutex<Option<(Instant, Vec<Note>)>>>,
}

async fn rex(args: &[&str]) -> Option<String> {
    let bin = std::env::var("REX_BIN_PATH").unwrap_or_else(|_| "rex".into());
    let fut = tokio::process::Command::new(bin).args(args).kill_on_drop(true).output();
    let out = tokio::time::timeout(Duration::from_secs(4), fut).await.ok()?.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `{ working, total, agents: [{name, status}] }` from the proto-* blocks in the Rex session; nulls when Rex is unreachable.
pub async fn capacity(cache: &TaskCache) -> Value {
    if let Some((at, v)) = cache.capacity.lock().unwrap().as_ref()
        && at.elapsed() < Duration::from_secs(3)
    {
        return v.clone();
    }
    // `REX_SESSION` names the session; the app started from the Dock has none, so then every session is looked at.
    let sessions: Vec<String> = match std::env::var("REX_SESSION") {
        Ok(s) if !s.is_empty() => vec![s],
        _ => match rex(&["session", "ls", "--json"]).await.and_then(|o| serde_json::from_str::<Value>(&o).ok()) {
            Some(v) => v["sessions"].as_array().map(|a| a.iter().filter_map(|x| x["label"].as_str().map(str::to_string)).collect()).unwrap_or_default(),
            None => Vec::new(),
        },
    };
    let mut value = json!({ "working": null, "total": null, "agents": [] });
    let mut reached = false;
    let mut blocks: Vec<(String, String)> = Vec::new();
    for session in &sessions {
        let Some(out) = rex(&["block", "ls", "-s", session, "--json"]).await else { continue };
        let Ok(parsed) = serde_json::from_str::<Value>(&out) else { continue };
        reached = true;
        blocks.extend(parsed["blocks"].as_array().map(Vec::as_slice).unwrap_or_default().iter().filter_map(|b| {
            let label = b["label"].as_str()?;
            label.starts_with("proto-").then(|| (b["block_id"].as_str().unwrap_or_default().to_string(), label.to_string()))
        }));
    }
    if reached {
        let agents = futures_util::future::join_all(blocks.into_iter().map(|(id, name)| async move {
            let screen = rex(&["capture", "-b", &id, "--trim"]).await;
            let status = match &screen {
                None => "unknown",
                Some(s) => {
                    let lines: Vec<&str> = s.split('\n').collect();
                    let tail = lines[lines.len().saturating_sub(12)..].join("\n");
                    if tail.contains("esc to interrupt") { "working" } else { "idle" }
                }
            };
            json!({ "name": name, "status": status })
        }))
        .await;
        let working = agents.iter().filter(|a| a["status"] == "working").count();
        value = json!({ "working": working, "total": agents.len(), "agents": agents });
    }
    *cache.capacity.lock().unwrap() = Some((Instant::now(), value.clone()));
    value
}

fn implement_tag(comment: &str) -> Option<&str> {
    let rest = comment.strip_prefix("[prototypie implement ")?;
    let end = rest.find(']')?;
    let id = &rest[..end];
    util::is_id(id, usize::MAX).then_some(id)
}

fn is_app_url(url: &str) -> bool {
    let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"));
    let Some(rest) = rest else { return false };
    let host = rest.split('/').next().unwrap_or("");
    let name = host.split(':').next().unwrap_or("");
    (name == "localhost" || name == "127.0.0.1") && rest.len() > host.len() && host.split(':').nth(1).is_none_or(|p| p.bytes().all(|b| b.is_ascii_digit()) && !p.is_empty())
}

/// Implement notes in any state, scanned from the notes filed under app URLs (≤ once a minute unless `fresh`).
async fn implement_notes(ns: &NotesService, cache: &TaskCache, fresh: bool) -> Vec<Note> {
    let cached = cache.implement.lock().unwrap().clone();
    if !fresh
        && let Some((at, v)) = &cached
        && at.elapsed() < Duration::from_secs(60)
    {
        return v.clone();
    }
    let Some(locations) = ns.locations().await else { return cached.map(|c| c.1).unwrap_or_default() };
    let ours: Vec<String> = locations.into_iter().filter(|l| is_app_url(l)).collect();
    let mut found = Vec::new();
    // At most 12 reads at a time; a location that can't be read is skipped.
    for chunk in ours.chunks(12) {
        let got = futures_util::future::join_all(chunk.iter().map(|l| ns.notes_at(l))).await;
        for notes in got.into_iter().flatten() {
            found.extend(notes.into_iter().filter(|n| implement_tag(&n.comment).is_some()));
        }
    }
    *cache.implement.lock().unwrap() = Some((Instant::now(), found.clone()));
    found
}

/// Implement notes read now, for /api/implement's duplicate check; None when the notes service can't be read.
pub async fn current_implement_notes(ns: &NotesService, cache: &TaskCache) -> Option<Vec<Note>> {
    ns.locations().await?;
    Some(implement_notes(ns, cache, true).await)
}

pub fn forget_implement_notes(cache: &TaskCache) {
    *cache.implement.lock().unwrap() = None;
}

fn thread_of(n: Option<&Note>) -> Vec<Value> {
    n.map(|n| n.thread.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|m| json!({ "id": m.id, "role": m.role, "content": m.content, "at": util::iso_from_ms(m.at_ms) }))
        .collect()
}

fn strip_tag(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix("[prototypie ")
        && let Some(end) = rest.find(']')
    {
        return rest[end + 1..].trim_start();
    }
    s
}

/// First line of a comment without the "[prototypie …]" tag, at most 90 characters.
fn note_title(comment: &str) -> String {
    let line = strip_tag(comment.split('\n').next().unwrap_or("")).trim();
    if line.chars().count() > 90 {
        format!("{}…", line.chars().take(88).collect::<String>())
    } else if line.is_empty() {
        "Notiz".into()
    } else {
        line.to_string()
    }
}

fn agent_msgs(thread: &[Value]) -> (Option<&Value>, Option<&Value>) {
    let first = thread.iter().find(|m| m["role"] == "agent");
    let last = thread.iter().rev().find(|m| m["role"] == "agent");
    (first, last)
}

fn step(key: &str, label: &str, at: Option<Value>, done: bool) -> Value {
    json!({ "key": key, "label": label, "at": at.unwrap_or(Value::Null), "done": done })
}

fn queue_task(item: &QueueItem, done_at: Option<String>, note: Option<&Note>) -> Value {
    let thread = thread_of(note);
    let (first, last) = agent_msgs(&thread);
    let ns = note.and_then(|n| n.status);
    let cancelled = item.status == QueueStatus::Cancelled || ns == Some(Status::Dismissed);
    let resolved = ns == Some(Status::Resolved);
    let delivered = item.status == QueueStatus::Done;
    let status = if cancelled {
        "cancelled"
    } else if resolved {
        "resolved"
    } else if delivered {
        "delivered"
    } else if ns == Some(Status::Acknowledged) || first.is_some() {
        "working"
    } else {
        "waiting"
    };
    let s = |v: Option<&String>| v.map(|x| json!(x));
    let mut steps = vec![
        step("received", "Eingang", Some(json!(item.created_at)), true),
        step("announced", "Notiz", note.and_then(|n| s(n.created_at.as_ref())), note.is_some()),
        step("working", "In Arbeit", first.map(|m| m["at"].clone()), status != "waiting" && status != "cancelled"),
        step("delivered", "Geliefert", if delivered { done_at.clone().map(Value::from) } else { None }, delivered || resolved),
        step(
            "resolved",
            "Abgeschlossen",
            if resolved { last.map(|m| m["at"].clone()).or_else(|| note.and_then(|n| s(n.updated_at.as_ref()))) } else { None },
            resolved,
        ),
    ];
    if cancelled {
        steps.push(step("cancelled", "Abgebrochen", done_at.clone().map(Value::from).or_else(|| note.and_then(|n| s(n.updated_at.as_ref()))), true));
    }
    let dir = Some(&item.direction.title).filter(|t| !t.is_empty() && !is_direction_n(t));
    let subject = item.base_title.clone().or_else(|| {
        item.bases.as_ref().filter(|b| !b.is_empty()).map(|b| b.iter().map(|x| x.title.as_str()).collect::<Vec<_>>().join(" + "))
    });
    let title = dir.cloned().or_else(|| subject.clone()).unwrap_or_else(|| note_title(&item.instruction));
    let request = if item.instruction.is_empty() { item.direction.brief.clone() } else { item.instruction.clone() };
    let mut t = json!({
        "id": note.map(|n| n.id.clone()).unwrap_or_else(|| proto_ai::queue::key(&item.job_id, item.index)),
        "kind": "queue",
        "title": title,
        "request": request,
        "jobId": item.job_id,
        "index": item.index,
        "status": status,
        "createdAt": item.created_at,
        "updatedAt": note.and_then(|n| n.updated_at.clone()).or(done_at).unwrap_or_else(|| item.created_at.clone()),
        "steps": steps,
        "thread": thread,
    });
    let o = t.as_object_mut().expect("object");
    if let Some(sub) = subject {
        o.insert("baseTitle".into(), json!(sub));
    }
    if let Some(agent) = &item.agent {
        o.insert("agent".into(), json!(agent));
        o.insert("claimedAt".into(), json!(item.claimed_at));
    }
    if let Some(ctx) = &item.context {
        let mut target = json!({ "spaceId": ctx["spaceId"], "spaceName": ctx["spaceName"], "spacePath": ctx["spacePath"] });
        if let Some(ph) = ctx.get("placeholderId") {
            target["designId"] = ph.clone();
            o.insert("designId".into(), ph.clone());
        }
        o.insert("target".into(), target);
    }
    if let Some(n) = note {
        o.insert("noteId".into(), json!(n.id));
        if let Some(st) = n.status {
            o.insert("noteStatus".into(), json!(st.as_str()));
        }
    }
    if let Some(l) = last {
        o.insert("summary".into(), l["content"].clone());
    }
    t
}

fn is_direction_n(t: &str) -> bool {
    t.strip_prefix("Direction ").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn note_task(a: &Note) -> Value {
    let thread = thread_of(Some(a));
    let (first, last) = agent_msgs(&thread);
    let comment = a.comment.as_str();
    let design_id = implement_tag(comment);
    let status = match a.status {
        Some(Status::Resolved) => "resolved",
        Some(Status::Dismissed) => "cancelled",
        Some(Status::Acknowledged) => "working",
        _ if first.is_some() => "working",
        _ => "waiting",
    };
    let created = a.created_at.clone().unwrap_or_else(util::iso_now);
    let mut steps = vec![
        step("received", if design_id.is_some() { "Gesendet" } else { "Notiz" }, Some(json!(created)), true),
        step("working", "In Arbeit", first.map(|m| m["at"].clone()), status == "working" || status == "resolved"),
        step(
            "resolved",
            "Abgeschlossen",
            if status == "resolved" { last.map(|m| m["at"].clone()).or_else(|| a.updated_at.clone().map(Value::from)) } else { None },
            status == "resolved",
        ),
    ];
    if status == "cancelled" {
        steps.push(step("cancelled", "Verworfen", a.updated_at.clone().map(Value::from), true));
    }
    let mut t = json!({
        "id": a.id,
        "kind": if design_id.is_some() { "implement" } else { "note" },
        "title": note_title(comment),
        "request": strip_tag(comment),
        "status": status,
        "createdAt": created,
        "updatedAt": a.updated_at.clone().unwrap_or_else(|| created.clone()),
        "steps": steps,
        "noteId": a.id,
        "thread": thread,
    });
    let o = t.as_object_mut().expect("object");
    if let Some(d) = design_id {
        o.insert("designId".into(), json!(d));
    }
    if let Some(l) = last {
        o.insert("summary".into(), l["content"].clone());
    }
    if let Some(s) = a.status {
        o.insert("noteStatus".into(), json!(s.as_str()));
    }
    if let Some(e) = &a.element {
        o.insert("element".into(), json!(e));
    }
    t
}

fn iso_time(t: SystemTime) -> String {
    util::iso(chrono::DateTime::<chrono::Utc>::from(t))
}

/// `{ capacity, tasks, notesService, notesWatchers? }`, newest activity first. `notesService`: whether the open notes could be read.
pub async fn list(store: &SharedStore, ai: &proto_ai::Ai, ns: &NotesService, cache: &TaskCache) -> Value {
    let read_queue = async {
        let mut all = ai.queue().read_all().await;
        all.sort_by(|a, b| b.0.created_at.cmp(&a.0.created_at));
        all.truncate(MAX_QUEUE_TASKS);
        let state = store.read(0, None);
        all.into_iter()
            .map(|(mut item, mtime)| {
                item.base_html = None;
                item.context = context::queue_context(&state, &item);
                let done_at = (item.status != QueueStatus::Pending).then(|| iso_time(mtime));
                (item, done_at)
            })
            .collect::<Vec<_>>()
    };
    let (capacity, queue, pending, implemented, watchers) =
        tokio::join!(capacity(cache), read_queue, ns.open_notes(), implement_notes(ns, cache, false), ns.watchers());
    let notes = futures_util::future::join_all(queue.iter().map(|(item, _)| async move {
        match &item.annotation_id {
            Some(id) => ns.note(id).await,
            None => None,
        }
    }))
    .await;
    let mut tasks: Vec<Value> = queue.iter().zip(&notes).map(|((item, done), note)| queue_task(item, done.clone(), note.as_ref())).collect();
    let mut known: std::collections::HashSet<String> = queue.iter().filter_map(|(i, _)| i.annotation_id.clone()).collect();
    let reachable = pending.is_some();
    for a in pending.unwrap_or_default().iter().chain(implemented.iter()) {
        if known.contains(&a.id) || a.url.as_deref().is_some_and(|u| !is_app_url(u)) {
            continue;
        }
        if a.comment.starts_with("[prototypie job ") {
            continue;
        }
        known.insert(a.id.clone());
        tasks.push(note_task(a));
    }
    tasks.sort_by(|a, b| b["updatedAt"].as_str().unwrap_or("").cmp(a["updatedAt"].as_str().unwrap_or("")));
    let mut out = json!({ "capacity": capacity, "tasks": tasks, "notesService": reachable });
    // Who waited for notes in the last 600 s (the panel's "no agent connected" without Rex); absent if unreadable.
    if let Some(w) = watchers {
        out["notesWatchers"] = w.iter().map(|w| json!({ "name": w.name, "waiting": w.waiting, "lastSeenAt": w.last_seen_at })).collect();
    }
    out
}

/// Add a panel note to a listed task's thread in the notes service (role human).
pub async fn add_note(store: &SharedStore, ai: &proto_ai::Ai, ns: &NotesService, cache: &TaskCache, note_id: &str, text: &str) -> Result<Value, (u16, String)> {
    let listed = list(store, ai, ns, cache).await;
    let known = listed["tasks"].as_array().is_some_and(|t| t.iter().any(|t| t["noteId"].as_str() == Some(note_id)));
    if !known {
        return Err((404, "Unbekannte Aufgabe".into()));
    }
    match ns.reply(note_id, "human", text).await {
        Ok(n) => Ok(json!({ "ok": true, "thread": thread_of(Some(&n)) })),
        Err(ReplyError::Refused(status)) => Err((502, format!("Notizdienst antwortet {status}"))),
        Err(ReplyError::Unreachable) => Err((502, "Notizdienst nicht erreichbar".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_titles_and_urls() {
        assert_eq!(implement_tag("[prototypie implement ab-1] Implement"), Some("ab-1"));
        assert_eq!(implement_tag("[prototypie job x#0] y"), None);
        assert_eq!(note_title("[prototypie job x#0] Hello\nmore"), "Hello");
        assert_eq!(note_title(""), "Notiz");
        assert!(is_app_url("http://localhost:3124/"));
        assert!(is_app_url("http://127.0.0.1/x"));
        assert!(!is_app_url("http://localhost:3124"));
        assert!(!is_app_url("https://example.com/"));
        assert!(is_direction_n("Direction 12"));
        assert!(!is_direction_n("Direction x"));
    }
}
