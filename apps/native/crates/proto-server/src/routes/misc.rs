//! Projects, pod catalog, bundles, agent tasks, spokes and decisions (reference routes of the same names).

use std::collections::HashMap;
use std::path::PathBuf;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{Value, json};

use crate::AppState;
use crate::notes::NewNote;
use crate::projects;
use crate::tasks;
use crate::util;

// ---- projects ----------------------------------------------------------------------------------------------

/// GET /api/projects[?match=<name>]
pub async fn projects_list(Query(q): Query<HashMap<String, String>>) -> Response {
    let ttl = projects::projects_ttl();
    if let Some(name) = q.get("match").cloned() {
        let path = tokio::task::spawn_blocking(move || projects::match_local_path(&ttl, &name)).await.ok().flatten();
        let body = match path {
            Some(p) => json!({ "localPath": p }),
            None => json!({}),
        };
        return util::no_store(body);
    }
    let file = ttl.to_string_lossy().into_owned();
    let pod = projects::pod_projects().await;
    let list = tokio::task::spawn_blocking(move || projects::list_all(&ttl, &pod)).await.ok().flatten();
    util::no_store(json!({ "available": list.is_some(), "file": file, "projects": list.unwrap_or_default() }))
}

async fn project(id: String) -> Option<projects::Project> {
    let ttl = projects::projects_ttl();
    let pod = projects::pod_projects().await;
    tokio::task::spawn_blocking(move || projects::get(&ttl, &id, &pod)).await.ok().flatten()
}

/// GET /api/projects/:id
pub async fn project_get(Path(id): Path<String>) -> Response {
    match project(id).await {
        Some(p) => util::no_store(p),
        None => util::error(StatusCode::NOT_FOUND, "Unknown project"),
    }
}

/// POST /api/projects/:id { spaceId } — ask an agent to enrich a new project space.
pub async fn project_enrich(State(app): State<AppState>, Path(id): Path<String>, body: Bytes) -> Response {
    let body = util::parse_body(&body);
    let Some(space_id) = body["spaceId"].as_str().filter(|s| util::is_id(s, usize::MAX)).map(str::to_string) else {
        return util::error(StatusCode::BAD_REQUEST, "spaceId required");
    };
    let Some(p) = project(id).await else { return util::error(StatusCode::NOT_FOUND, "Unknown project") };
    let repo = p.local_path.clone().or(p.repo.clone()).unwrap_or_else(|| "kein Repo".into());
    let comment = format!(
        "[prototypie project-info {space_id}] Projekt {} ({repo}): Projekt-Info recherchieren und Karten anlegen. \
         Space {space_id} hat schon eine Karte „Projekt-Info“ (Name, Repo, Keywords{}). \
         Mit einem project-info-Skill, falls vorhanden; Karten mit `prototypie cards create --space {space_id}` (Skill prototypie). Keine Geheimnisse übernehmen.",
        p.name,
        if p.readme.is_some() { ", README-Absatz" } else { "" }
    );
    let element = format!("space {space_id}: {}", p.name);
    let path = p.local_path.clone().or(p.repo.clone()).unwrap_or_else(|| "private-projects.ttl".into());
    match app.notes.create(NewNote { comment: &comment, element: &element, element_path: &path }).await {
        Some(id) => util::ok(json!({ "ok": true, "annotationId": id })),
        None => util::error(StatusCode::SERVICE_UNAVAILABLE, "No coding agent reachable (the notes service did not take the note; agents read notes with `prototypie notes`)"),
    }
}

// ---- pod catalog -------------------------------------------------------------------------------------------

/// GET /api/pod-catalog[?refresh=1]
pub async fn pod_catalog(State(app): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    let refresh = q.get("refresh").is_some_and(|v| v == "1");
    match app.pods.get(&app.data_dir, refresh).await {
        Ok(v) => util::ok(v),
        Err(e) => util::error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

// ---- bundles -----------------------------------------------------------------------------------------------

fn imports_dir(app: &AppState) -> PathBuf {
    super::assets::public_dir(app).join("imports")
}

/// GET /api/bundles → `[{ file, name, count, updatedAt }]`, newest first.
pub async fn bundles(State(app): State<AppState>) -> Response {
    let dir = imports_dir(&app);
    let mut list: Vec<(f64, Value)> = Vec::new();
    if let Ok(mut rd) = tokio::fs::read_dir(&dir).await {
        while let Ok(Some(e)) = rd.next_entry().await {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".json") {
                continue;
            }
            let (Ok(meta), Ok(txt)) = (e.metadata().await, tokio::fs::read_to_string(e.path()).await) else { continue };
            let Ok(data) = serde_json::from_str::<Value>(&txt) else { continue };
            let updated = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0.0, |d| d.as_secs_f64() * 1000.0);
            let shown = match &data["name"] {
                Value::Null => name.clone(),
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let count = data["designs"].as_array().map_or(0, Vec::len);
            list.push((updated, json!({ "file": format!("/imports/{name}"), "name": shown, "count": count, "updatedAt": updated })));
        }
    }
    list.sort_by(|a, b| b.0.total_cmp(&a.0));
    util::ok(list.into_iter().map(|x| x.1).collect::<Vec<_>>())
}

// ---- agent tasks -------------------------------------------------------------------------------------------

/// GET /api/agent-tasks
pub async fn agent_tasks(State(app): State<AppState>) -> Response {
    util::no_store(tasks::list(&app.store, &app.ai, &app.notes, &app.tasks).await)
}

/// POST /api/agent-tasks/:id/notes { text }
pub async fn agent_task_note(State(app): State<AppState>, Path(id): Path<String>, body: Bytes) -> Response {
    let body = util::parse_body(&body);
    let text = body["text"].as_str().map(str::trim).filter(|t| !t.is_empty() && t.chars().count() <= 2000);
    let Some(text) = text else {
        return util::error(StatusCode::BAD_REQUEST, "Bitte einen Text eingeben (höchstens 2000 Zeichen).");
    };
    match tasks::add_note(&app.store, &app.ai, &app.notes, &app.tasks, &id, text).await {
        Ok(v) => util::ok(v),
        Err((status, msg)) => util::error(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY), msg),
    }
}

// ---- LLM routes (proto-ai) ---------------------------------------------------------------------------------

/// POST /api/spokes { html, title? } → `{ components }`.
pub async fn spokes(State(app): State<AppState>, body: Bytes) -> Response {
    let Ok(body) = serde_json::from_slice::<Value>(&body) else {
        return util::error(StatusCode::BAD_REQUEST, "Invalid JSON body.");
    };
    let html = body["html"].as_str().unwrap_or("");
    if html.trim().is_empty() {
        return util::error(StatusCode::BAD_REQUEST, "No page HTML to analyse.");
    }
    match app.ai.spokes(html, body["title"].as_str(), proto_ai::CancellationToken::new()).await {
        Ok(components) => util::ok(json!({ "components": components })),
        Err(e) => util::error(StatusCode::BAD_GATEWAY, e.to_string()),
    }
}

/// POST /api/decisions { designs, context? } → `{ decisions }`.
pub async fn decisions(State(app): State<AppState>, body: Bytes) -> Response {
    let Ok(body) = serde_json::from_slice::<Value>(&body) else {
        return util::error(StatusCode::BAD_REQUEST, "Body must be JSON.");
    };
    let (candidates, context) = match parse_decisions(&body) {
        Ok(x) => x,
        Err(msg) => return util::error(StatusCode::BAD_REQUEST, msg),
    };
    match app.ai.decisions(&candidates, context.as_deref(), proto_ai::CancellationToken::new()).await {
        Ok(decisions) => util::ok(json!({ "decisions": decisions })),
        Err(proto_ai::AiError::Empty(m)) => util::error(StatusCode::UNPROCESSABLE_ENTITY, m),
        Err(proto_ai::AiError::Cancelled) => util::error(StatusCode::from_u16(499).unwrap(), "Request cancelled."),
        Err(e @ proto_ai::AiError::Llm(_)) => util::error(StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
        Err(e) => util::error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

fn parse_decisions(v: &Value) -> Result<(Vec<proto_ai::DecisionCandidate>, Option<String>), String> {
    let Some(list) = v["designs"].as_array() else { return Err("designs: Required".into()) };
    if list.len() < 2 {
        return Err("designs: Add at least two designs to the poll first.".into());
    }
    if list.len() > 16 {
        return Err("designs: That's a lot of candidates - keep it to 16 or fewer.".into());
    }
    let mut out = Vec::new();
    for (i, d) in list.iter().enumerate() {
        let s = |k: &str| d[k].as_str().map(str::to_string);
        let id = s("id").filter(|x| !x.is_empty() && x.chars().count() <= 64).ok_or(format!("designs.{i}.id: Invalid"))?;
        let title = s("title").filter(|x| x.chars().count() <= 300).ok_or(format!("designs.{i}.title: Invalid"))?;
        let rationale = match &d["rationale"] {
            Value::Null => String::new(),
            Value::String(r) if r.chars().count() <= 4000 => r.clone(),
            _ => return Err(format!("designs.{i}.rationale: Invalid")),
        };
        let html = s("html").filter(|x| x.len() <= 2_000_000).ok_or(format!("designs.{i}.html: Invalid"))?;
        out.push(proto_ai::DecisionCandidate { id, title, rationale, html });
    }
    let context = match &v["context"] {
        Value::Null => None,
        Value::String(c) if c.chars().count() <= 2000 => Some(c.clone()),
        _ => return Err("context: Invalid".into()),
    };
    Ok((out, context))
}
