//! `POST /api/implement` (reference `src/app/api/implement/route.ts`, `implement-dedupe.ts`, `implement-links.ts`):
//! hand a design to the coding agent, at most one open request per design and its promoted source/copies.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::AppState;
use crate::notes::{NewNote, Note, Status};
use crate::store::{self, live_map};
use crate::tasks;
use crate::util;

/// Notes this server posted in the last 2 minutes, in case a scan doesn't list them yet.
static RECENT: Mutex<Vec<(Vec<String>, String, Instant)>> = Mutex::new(Vec::new());
/// One decision at a time: four quick clicks must not post four notes.
static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const RECENT_FOR: Duration = Duration::from_secs(120);

/// `design_id` plus every design linked to it through promotion, both directions, transitively.
pub fn linked_design_ids(links: &BTreeMap<String, String>, design_id: &str) -> Vec<String> {
    let mut out = vec![design_id.to_string()];
    let mut seen: BTreeSet<String> = out.iter().cloned().collect();
    let mut stack = out.clone();
    while let Some(cur) = stack.pop() {
        if let Some(src) = links.get(&cur)
            && seen.insert(src.clone())
        {
            out.push(src.clone());
            stack.push(src.clone());
        }
        for (copy, src) in links {
            if src == &cur && seen.insert(copy.clone()) {
                out.push(copy.clone());
                stack.push(copy.clone());
            }
        }
    }
    out
}

fn tag_id(comment: &str) -> Option<&str> {
    let rest = comment.strip_prefix("[prototypie implement ")?;
    Some(&rest[..rest.find(']')?])
}

/// Newest open (pending/acknowledged) and newest resolved Implement note for any of `ids`.
fn existing<'a>(notes: &'a [Note], ids: &[String]) -> (Option<&'a Note>, Option<&'a Note>) {
    let mut mine: Vec<&Note> = notes.iter().filter(|n| tag_id(&n.comment).is_some_and(|id| ids.iter().any(|x| x == id))).collect();
    mine.sort_by(|a, b| b.created_at.as_deref().unwrap_or("").cmp(a.created_at.as_deref().unwrap_or("")));
    let open = mine.iter().copied().find(|n| matches!(n.status.unwrap_or(Status::Pending), Status::Pending | Status::Acknowledged));
    let resolved = mine.iter().copied().find(|n| n.status == Some(Status::Resolved));
    (open, resolved)
}

pub async fn implement(State(app): State<AppState>, body: Bytes) -> Response {
    let body = util::parse_body(&body);
    let Some(design_id) = body["designId"].as_str().filter(|s| util::is_id(s, usize::MAX)).map(str::to_string) else {
        return util::error(StatusCode::BAD_REQUEST, "designId required");
    };
    let force = body["force"] == json!(true);
    let state = app.store.read(0, None);
    let links: BTreeMap<String, String> = live_map(&state, "promotedFrom")
        .into_iter()
        .filter_map(|(copy, v)| Some((copy, v.get("designId")?.as_str()?.to_string())))
        .collect();

    let (title, rationale, html) = match (body["html"].as_str().filter(|h| !h.is_empty()), body["title"].as_str()) {
        (Some(h), Some(t)) => (t.to_string(), body["rationale"].as_str().unwrap_or("").to_string(), h.to_string()),
        _ => {
            let Some(d) = store::live_value(&state, &format!("designs/{design_id}")) else {
                return util::error(StatusCode::NOT_FOUND, "Unknown design");
            };
            let html = d["html"].as_str().unwrap_or("");
            if d["status"] != "ready" || html.is_empty() {
                return util::error(StatusCode::CONFLICT, "Design isn't ready");
            }
            (d["title"].as_str().unwrap_or("").into(), d["rationale"].as_str().unwrap_or("").into(), html.into())
        }
    };
    let ids = linked_design_ids(&links, &design_id);

    // Reliable local guard: every sent design carries implementRequestedAt in the shared state.
    if !force
        && let Some(already) = ids.iter().find(|id| {
            store::live_value(&state, &format!("designs/{id}")).is_some_and(|d| d.get("implementRequestedAt").is_some_and(|v| !v.is_null() && v != &json!(0)))
        })
    {
        return (
            StatusCode::CONFLICT,
            axum::Json(json!({ "error": "This design (or a linked copy) was already sent to the agent. Send again with force: true.", "done": true, "designId": already })),
        )
            .into_response();
    }

    let dir = app.data_dir.join("implement");
    let file = dir.join(format!("{design_id}.html"));
    let file_s = file.to_string_lossy().into_owned();
    let _guard = LOCK.lock().await;
    let notes = tasks::current_implement_notes(&app.notes, &app.tasks).await.unwrap_or_default();
    let (open, resolved) = existing(&notes, &ids);
    if let Some(n) = open {
        return util::ok(json!({ "ok": true, "duplicate": true, "annotationId": n.id }));
    }
    {
        let mut recent = RECENT.lock().unwrap();
        recent.retain(|r| r.2.elapsed() < RECENT_FOR);
        let listed: BTreeSet<&str> = notes.iter().map(|n| n.id.as_str()).collect();
        if let Some(r) = recent.iter().find(|r| !listed.contains(r.1.as_str()) && r.0.iter().any(|id| ids.contains(id))) {
            return util::ok(json!({ "ok": true, "duplicate": true, "annotationId": r.1 }));
        }
    }
    if let Some(n) = resolved
        && !force
    {
        return (
            StatusCode::CONFLICT,
            axum::Json(json!({ "error": "This design was already implemented. Send again with force: true.", "done": true, "annotationId": n.id })),
        )
            .into_response();
    }
    // Written only when a note is posted, so a duplicate click doesn't overwrite what the agent is reading.
    let saved = async {
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(&file, &html).await
    }
    .await;
    if let Err(e) = saved {
        return util::error(StatusCode::INTERNAL_SERVER_ERROR, format!("Couldn't save the design: {e}"));
    }
    let comment = format!(
        "[prototypie implement {design_id}] Implement the design \"{title}\" in the product (this codebase). {}The design's HTML is in {file_s}.",
        if rationale.is_empty() { String::new() } else { format!("Why: {rationale} ") }
    );
    let element = format!("design {design_id}: {title}");
    let Some(annotation_id) = app.notes.create(NewNote { comment: &comment, element: &element, element_path: &file_s }).await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(json!({ "error": format!("No coding agent reachable (the notes service did not take the note; agents read notes with `prototypie notes`). The design was saved to {file_s}."), "file": file_s })),
        )
            .into_response();
    };
    RECENT.lock().unwrap().push((ids, annotation_id.clone(), Instant::now()));
    tasks::forget_implement_notes(&app.tasks);
    util::ok(json!({ "ok": true, "annotationId": annotation_id, "file": file_s }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_both_ways_and_transitively() {
        let links: BTreeMap<String, String> = [("b", "a"), ("c", "b"), ("x", "y")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let mut ids = linked_design_ids(&links, "b");
        ids.sort();
        assert_eq!(ids, ["a", "b", "c"]);
        assert_eq!(linked_design_ids(&links, "z"), ["z"]);
    }

    #[test]
    fn open_wins_over_resolved_and_dismissed_is_ignored() {
        let n = |id: &str, d: &str, st: Status, at: &str| Note {
            id: id.into(),
            comment: format!("[prototypie implement {d}] x"),
            status: Some(st),
            created_at: Some(at.into()),
            ..Default::default()
        };
        let notes = vec![n("1", "a", Status::Resolved, "2026-01-02"), n("2", "b", Status::Pending, "2026-01-01"), n("3", "a", Status::Dismissed, "2026-01-03")];
        let ids = vec!["a".to_string(), "b".to_string()];
        let (open, resolved) = existing(&notes, &ids);
        assert_eq!(open.map(|n| n.id.as_str()), Some("2"));
        assert_eq!(resolved.map(|n| n.id.as_str()), Some("1"));
        assert_eq!(existing(&notes, &["q".into()]).0.map(|n| n.id.clone()), None);
    }
}
