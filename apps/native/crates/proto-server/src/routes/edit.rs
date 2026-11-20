//! In-place AI edit: `POST /api/edit`, `DELETE /api/edit/:designId` (reference `src/lib/server/ai-edit.ts`).
//! proto-ai runs the job; this module guards the card, marks it `editing`, and on the job's result writes the new
//! content (old content kept in `versions`, at most 5) or the error.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use proto_ai::{AgentChoice, EditRequest, EditTarget, Variation, VariationEvent};
use serde_json::{Map, Value, json};

use crate::AppState;
use crate::context;
use crate::store::{self, Change, SharedStore};
use crate::util::{self, Check, p};

const MAX_VERSIONS: usize = 5;

fn current(store: &SharedStore, design_id: &str) -> Option<Map<String, Value>> {
    match store::live_value(&store.read(0, None), &format!("designs/{design_id}")) {
        Some(Value::Object(o)) => Some(o),
        _ => None,
    }
}

fn write(store: &SharedStore, d: Map<String, Value>, reason: String) {
    let id = d.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
    let _ = store.apply(vec![Change::set(format!("designs/{id}"), Value::Object(d))], store::meta("server:ai-edit", reason));
}

fn parse(v: &Value) -> Result<EditRequest, Vec<util::Issue>> {
    let mut c = Check::default();
    if !v.is_object() {
        c.push(&[], "invalid_type", "Expected object, received null");
        return Err(c.issues);
    }
    let design_id = match v["designId"].as_str() {
        Some(s) if util::is_id(s, 64) => Some(s.to_string()),
        _ => {
            c.push(&p(&[&"designId"]), "invalid_string", "Invalid");
            None
        }
    };
    let instruction = c.string(&v["instruction"], &p(&[&"instruction"]), true, 1, 4000);
    let focus = c.opt_string(&v["focus"], &p(&[&"focus"]), false, 0, 2000);
    if v["annotations"].as_array().is_some_and(|a| a.len() > 50) {
        c.push(&p(&[&"annotations"]), "too_big", "Array must contain at most 50 element(s)");
    }
    if v["components"].as_array().is_some_and(|a| a.len() > 20) {
        c.push(&p(&[&"components"]), "too_big", "Array must contain at most 20 element(s)");
    }
    if !v["agent"].is_null() && !matches!(v["agent"].as_str(), Some("codex" | "claude")) {
        c.push(&p(&[&"agent"]), "invalid_enum_value", "Invalid enum value. Expected 'codex' | 'claude'");
    }
    // The typed fields (annotations, podContext, components) go through serde.
    let mut req: EditRequest = match serde_json::from_value(v.clone()) {
        Ok(r) => r,
        Err(e) => {
            c.push(&[], "invalid_type", e.to_string());
            return Err(c.issues);
        }
    };
    match (design_id, instruction) {
        (Some(id), Some(ins)) if c.ok() => {
            req.design_id = id;
            req.instruction = ins;
            req.focus = focus;
            Ok(req)
        }
        _ => Err(c.issues),
    }
}

/// POST /api/edit → `{ jobId }`.
pub async fn start(State(app): State<AppState>, body: Bytes) -> Response {
    let req = match parse(&util::parse_body(&body)) {
        Ok(r) => r,
        Err(issues) => return util::issues_error(issues),
    };
    let Some(d) = current(&app.store, &req.design_id) else {
        return util::error(StatusCode::NOT_FOUND, "No such design");
    };
    let html = d.get("html").and_then(Value::as_str).unwrap_or("").to_string();
    if d.get("status").and_then(Value::as_str) != Some("ready") || html.is_empty() {
        return util::error(StatusCode::CONFLICT, "Only a finished design can be edited");
    }
    if d.get("editing").is_some_and(|e| !e.is_null()) {
        return util::error(StatusCode::CONFLICT, "This design is already being edited");
    }
    let space_id = d.get("spaceId").and_then(Value::as_str).unwrap_or("").to_string();
    let agent = req.agent.unwrap_or(AgentChoice::Codex);
    let instruction = req.instruction.clone();
    let target = EditTarget { id: req.design_id.clone(), title: d.get("title").and_then(Value::as_str).unwrap_or("").into(), html, space_id: space_id.clone() };
    let guidelines = context::guidelines_for(&app.store, Some(&space_id));
    let job_id = app.ai.start_edit(target, req.clone(), guidelines);

    let mut marked = d;
    marked.insert(
        "editing".into(),
        json!({ "jobId": job_id, "instruction": instruction, "agent": if agent == AgentChoice::Claude { "claude" } else { "codex" }, "startedAt": util::now_ms() }),
    );
    marked.remove("editError");
    write(&app.store, marked, format!("ai edit start {job_id}"));

    // Follow the job and write its outcome into the card.
    if let Some(events) = app.ai.jobs().subscribe(&job_id, 0) {
        let (store, design_id, job) = (app.store.clone(), req.design_id.clone(), job_id.clone());
        tokio::spawn(async move {
            let mut events = std::pin::pin!(events);
            while let Some(e) = events.next().await {
                match e {
                    VariationEvent::Variation { variation, .. } => finish(&store, &design_id, &job, &variation, &instruction),
                    VariationEvent::Error { message, .. } => {
                        let msg = (message != "Cancelled").then_some(message);
                        fail(&store, &design_id, &job, msg);
                    }
                    _ => {}
                }
            }
        });
    }
    util::ok(json!({ "jobId": job_id }))
}

fn editing_job(d: &Map<String, Value>) -> Option<&str> {
    d.get("editing")?.get("jobId")?.as_str()
}

/// Replace the card's content with the result; the old content becomes the newest version.
fn finish(store: &SharedStore, design_id: &str, job_id: &str, v: &Variation, instruction: &str) {
    let Some(mut d) = current(store, design_id) else { return };
    if editing_job(&d) != Some(job_id) {
        return; // deleted, cancelled or superseded meanwhile
    }
    let mut prev = Map::new();
    for k in ["html", "title", "rationale"] {
        prev.insert(k.into(), d.get(k).cloned().unwrap_or(json!("")));
    }
    for k in ["viewport", "viewportHeight"] {
        if let Some(x) = d.get(k).filter(|x| !x.is_null() && x.as_f64() != Some(0.0)) {
            prev.insert(k.into(), x.clone());
        }
    }
    prev.insert("replacedAt".into(), json!(util::now_ms()));
    prev.insert("instruction".into(), json!(instruction));
    let mut versions = d.get("versions").and_then(Value::as_array).cloned().unwrap_or_default();
    versions.push(Value::Object(prev));
    let skip = versions.len().saturating_sub(MAX_VERSIONS);
    d.insert("html".into(), json!(v.html));
    if !v.rationale.is_empty() {
        d.insert("rationale".into(), json!(v.rationale));
    }
    if let Some(w) = v.viewport {
        d.insert("viewport".into(), json!(w));
    }
    if let Some(h) = v.viewport_height {
        d.insert("viewportHeight".into(), json!(h));
    }
    d.insert("versions".into(), Value::Array(versions.split_off(skip)));
    d.remove("editing");
    d.remove("editError");
    write(store, d, format!("ai edit done {job_id}"));
}

/// Clear the busy state; record the error unless the edit was cancelled.
fn fail(store: &SharedStore, design_id: &str, job_id: &str, message: Option<String>) {
    let Some(mut d) = current(store, design_id) else { return };
    if editing_job(&d) != Some(job_id) {
        return;
    }
    d.remove("editing");
    match message {
        Some(m) => d.insert("editError".into(), json!(m)),
        None => d.remove("editError"),
    };
    write(store, d, format!("ai edit failed {job_id}"));
}

/// DELETE /api/edit/:designId → 204 cancelled (also clears a stale flag) · 404 not being edited.
pub async fn cancel(State(app): State<AppState>, Path(design_id): Path<String>) -> Response {
    let Some(mut d) = current(&app.store, &design_id) else { return StatusCode::NOT_FOUND.into_response() };
    let Some(job) = editing_job(&d).map(str::to_string) else { return StatusCode::NOT_FOUND.into_response() };
    app.ai.jobs().cancel(&job);
    d.remove("editing");
    write(&app.store, d, format!("ai edit cancelled {job}"));
    StatusCode::NO_CONTENT.into_response()
}
