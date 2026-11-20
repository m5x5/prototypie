//! Pure store writes of the runner, computed from a snapshot: the run record and placeholder cards of a
//! generation (reference `src/store/core.ts` generate/follow/defaultOrigin/withoutBase), and the in-place edit's
//! `editing` / finish / fail writes (reference `src/lib/server/ai-edit.ts`). No I/O, no runtime: unit-tested.

use proto_core::ops::{NODE_H, NODE_W, now_ms};
use proto_core::{Change, Design, DesignKind, DesignStatus, Point, RecordKind, Run, Snapshot, VariationRequest};
use serde_json::{Value, json};

/// Horizontal gap between sibling placeholders (reference `SIBLING_GAP`).
pub const SIBLING_GAP: f64 = 160.0;
/// Versions an edited card keeps (reference `MAX_VERSIONS`).
pub const MAX_VERSIONS: usize = 5;
/// Longest guidelines text passed to the model (reference `MAX_TEXT`).
const MAX_GUIDELINES: usize = proto_ai::context::MAX_GUIDELINES_TEXT;

fn put<T: serde::Serialize>(kind: RecordKind, id: &str, v: &T) -> Change {
    Change::put(kind, id, v).expect("domain types serialise")
}

/// proto-core's request → proto-ai's (same JSON), landing in `space_id`.
pub fn to_ai_request(r: &VariationRequest, space_id: &str) -> Result<proto_ai::VariationRequest, String> {
    let mut v = serde_json::to_value(r).map_err(|e| e.to_string())?;
    v["spaceId"] = json!(space_id);
    serde_json::from_value(v).map_err(|e| format!("Invalid request: {e}"))
}

/// What a run and its cards keep of a request (`withoutBase`): no base html, no screenshot, and component / base
/// html blanked (a retry refills them from the current cards).
pub fn stored_request(r: &VariationRequest) -> VariationRequest {
    let mut out = r.clone();
    out.base_html = None;
    if let Some(Value::Object(from)) = out.extra.get_mut("importFrom") {
        from.remove("screenshot");
    }
    for key in ["components", "bases"] {
        if let Some(Value::Array(list)) = out.extra.get_mut(key) {
            for item in list.iter_mut() {
                if let Value::Object(o) = item {
                    o.insert("html".into(), json!(""));
                }
            }
        }
    }
    out
}

/// The guidelines that apply in `space_id`, as proto-ai wants them (`guidelinesFor`).
pub fn guidelines(snap: &Snapshot, space_id: &str) -> Option<proto_ai::Guidelines> {
    let card = snap.guidelines_for(space_id)?;
    let from_space = if card.space_id == proto_core::ROOT_SPACE_ID {
        "Main canvas".to_string()
    } else {
        snap.space(&card.space_id).map_or_else(|| card.space_id.clone(), |s| s.name.clone())
    };
    let text = proto_ai::context::html_to_text(&card.html);
    Some(proto_ai::Guidelines {
        card_id: card.id.clone(),
        title: card.title.clone(),
        from_space,
        text: proto_ai::context::truncate_chars(&text, MAX_GUIDELINES).to_string(),
    })
}

/// Top-left of the first new card (`defaultOrigin`): a centred row below the parent (sliding right past
/// anything in the way), else right of everything in the space.
pub fn default_origin(snap: &Snapshot, space_id: &str, parent: Option<&Design>, count: usize) -> Point {
    let in_space: Vec<Point> = snap.designs_in(space_id).map(|d| d.position).collect();
    if let Some(parent) = parent {
        let n = count.max(1) as f64;
        let width = n * NODE_W + (n - 1.0) * SIBLING_GAP;
        let y = parent.position.y + NODE_H + 220.0;
        let mut x = parent.position.x + NODE_W / 2.0 - width / 2.0;
        for _ in 0..50 {
            let hits: Vec<&Point> = in_space
                .iter()
                .filter(|p| p.x < x + width + SIBLING_GAP && p.x + NODE_W + SIBLING_GAP > x && p.y < y + NODE_H && p.y + NODE_H > y)
                .collect();
            if hits.is_empty() {
                break;
            }
            x = hits.iter().map(|p| p.x).fold(f64::MIN, f64::max) + NODE_W + SIBLING_GAP;
        }
        return Point { x, y };
    }
    if in_space.is_empty() {
        return Point::default();
    }
    let max_x = in_space.iter().map(|p| p.x).fold(f64::MIN, f64::max);
    let min_y = in_space.iter().map(|p| p.y).fold(f64::MAX, f64::min);
    Point { x: max_x + NODE_W + 160.0, y: min_y }
}

/// A new run (`generate`): its record goes to the store at once so a restart can tell it was interrupted.
#[allow(clippy::too_many_arguments)]
pub fn new_run(
    snap: &Snapshot,
    run_id: &str,
    job_id: &str,
    space_id: &str,
    parent_id: Option<&str>,
    kind: DesignKind,
    label: Option<&str>,
    request: &VariationRequest,
    origin: Option<Point>,
) -> Run {
    let parent = parent_id.and_then(|id| snap.design(id));
    let label = match label {
        Some(l) => l.to_string(),
        None => proto_ai::context::truncate_chars(if request.instruction.is_empty() { "Variations" } else { &request.instruction }, 60)
            .to_string(),
    };
    Run {
        id: run_id.to_string(),
        server_job_id: Some(job_id.to_string()),
        space_id: space_id.to_string(),
        parent_id: parent_id.map(str::to_string),
        kind,
        instruction: request.instruction.clone(),
        origin: origin.unwrap_or_else(|| default_origin(snap, space_id, parent.as_deref(), request.count.max(1) as usize)),
        label,
        request: stored_request(request),
        design_ids: Vec::new(),
        extra: Default::default(),
    }
}

pub fn put_run(run: &Run) -> Change {
    put(RecordKind::Runs, &run.id, run)
}

/// `plan`: a placeholder per direction (on a replay only the missing ones). Updates `run.design_ids`.
pub fn on_plan(snap: &Snapshot, run: &mut Run, directions: &[proto_ai::Direction]) -> Vec<Change> {
    let parent = run.parent_id.as_deref().and_then(|id| snap.design(id));
    let mut changes = Vec::new();
    for (i, dir) in directions.iter().enumerate() {
        if run.design_ids.get(i).is_some_and(|id| snap.design(id).is_some()) {
            continue;
        }
        let id = proto_core::ops::new_id(10);
        let mut request = serde_json::to_value(&run.request).unwrap_or_else(|_| json!({}));
        request["count"] = json!(1);
        request["direction"] = json!({ "title": dir.title, "brief": dir.brief });
        let mut d = json!({
            "id": id,
            "spaceId": run.space_id,
            "kind": run.kind,
            "title": dir.title,
            "rationale": dir.brief,
            "html": "",
            "status": "generating",
            "position": { "x": run.origin.x + i as f64 * (NODE_W + SIBLING_GAP), "y": run.origin.y },
            "createdAt": now_ms(),
            "instruction": run.instruction,
            "request": request,
        });
        if let Some(p) = &run.parent_id {
            d["parentId"] = json!(p);
        }
        // Component-scope results (spokes, "Import from app…") are components for # mentions.
        if run.request.scope.as_deref() == Some("component") {
            d["component"] = json!(true);
        }
        // A variation keeps its base's frame: phone designs stay phone-sized.
        if let Some(p) = &parent {
            if let Some(w) = p.viewport {
                d["viewport"] = json!(w);
            }
            if let Some(h) = p.viewport_height {
                d["viewportHeight"] = json!(h);
            }
        }
        changes.push(Change::set(RecordKind::Designs.path(&id), d));
        if run.design_ids.len() <= i {
            run.design_ids.resize(i + 1, String::new());
        }
        run.design_ids[i] = id;
    }
    changes.push(put_run(run));
    changes
}

fn update(snap: &Snapshot, id: &str, f: impl FnOnce(&mut Design)) -> Option<Change> {
    let mut d = (*snap.design(id)?).clone();
    f(&mut d);
    Some(put(RecordKind::Designs, id, &d))
}

/// `variation`: fill its card (ready).
pub fn on_variation(snap: &Snapshot, run: &Run, index: usize, v: &proto_ai::Variation) -> Option<Change> {
    let id = run.design_ids.get(index).filter(|id| !id.is_empty())?;
    update(snap, id, |d| {
        d.title = v.title.clone();
        d.rationale = v.rationale.clone();
        d.html = v.html.clone();
        if let Some(w) = v.viewport {
            d.viewport = Some(w as f64);
        }
        if let Some(h) = v.viewport_height {
            d.viewport_height = Some(h as f64);
        }
        d.status = DesignStatus::Ready;
        d.error = None;
    })
}

/// An `error` for one direction: its card shows it.
pub fn on_error(snap: &Snapshot, run: &Run, index: usize, message: &str) -> Option<Change> {
    let id = run.design_ids.get(index).filter(|id| !id.is_empty())?;
    update(snap, id, |d| {
        d.status = DesignStatus::Error;
        d.error = Some(message.to_string());
    })
}

/// The run ended: anything still generating never arrived (`failed` or "No result"); the run record goes.
pub fn finish_run(snap: &Snapshot, run: &Run, failed: Option<&str>) -> Vec<Change> {
    let mut changes: Vec<Change> = run
        .design_ids
        .iter()
        .filter(|id| snap.design(id).is_some_and(|d| d.status == DesignStatus::Generating))
        .filter_map(|id| {
            update(snap, id, |d| {
                d.status = DesignStatus::Error;
                d.error = Some(failed.unwrap_or("No result").to_string());
            })
        })
        .collect();
    changes.push(Change::delete(RecordKind::Runs.path(&run.id)));
    changes
}

/// Runs and edits left over from an earlier process (their jobs died with it; `is_alive` says which jobs this
/// process still has): generating cards → error "Interrupted", run records deleted, stale `editing` cleared.
pub fn reap(snap: &Snapshot, is_alive: impl Fn(&str) -> bool) -> Vec<Change> {
    let mut changes = Vec::new();
    for run in snap.runs() {
        if run.server_job_id.as_deref().is_some_and(&is_alive) {
            continue;
        }
        let msg = if run.server_job_id.is_some() { "Interrupted" } else { "Interrupted before it started" };
        changes.extend(finish_run(snap, &run, Some(msg)));
    }
    for d in snap.designs() {
        let job = d.editing.as_ref().and_then(|e| e.get("jobId")).and_then(Value::as_str);
        if d.editing.is_some() && !job.is_some_and(&is_alive) {
            changes.extend(update(snap, &d.id, |d| {
                d.editing = None;
                d.edit_error = Some("Interrupted".into());
            }));
        }
    }
    changes
}

/// Why `design_id` can't be edited now (`startEdit`'s checks), or the design.
pub fn edit_target(snap: &Snapshot, design_id: &str) -> Result<std::sync::Arc<Design>, String> {
    let d = snap.design(design_id).ok_or("No such design")?;
    if d.status != DesignStatus::Ready || d.html.is_empty() {
        return Err("Only a finished design can be edited".into());
    }
    if d.editing.is_some() {
        return Err("This design is already being edited".into());
    }
    Ok(d)
}

/// Mark the card as being edited by `job_id` (clears the last edit error).
pub fn edit_start(snap: &Snapshot, design_id: &str, job_id: &str, instruction: &str, agent: &str) -> Option<Change> {
    update(snap, design_id, |d| {
        d.editing = Some(json!({ "jobId": job_id, "instruction": instruction, "agent": agent, "startedAt": now_ms() }));
        d.edit_error = None;
    })
}

fn edited_by(d: &Design, job_id: &str) -> bool {
    d.editing.as_ref().and_then(|e| e.get("jobId")).and_then(Value::as_str) == Some(job_id)
}

/// Replace the card's content with the edit result; the old content becomes the newest version. None when the
/// card is gone or no longer edited by this job (cancelled or superseded).
pub fn edit_finish(snap: &Snapshot, design_id: &str, job_id: &str, v: &proto_ai::Variation, instruction: &str) -> Option<Change> {
    let d = snap.design(design_id)?;
    if !edited_by(&d, job_id) {
        return None;
    }
    let mut prev = json!({ "html": d.html, "title": d.title, "rationale": d.rationale, "replacedAt": now_ms(), "instruction": instruction });
    if let Some(w) = d.viewport {
        prev["viewport"] = json!(w);
    }
    if let Some(h) = d.viewport_height {
        prev["viewportHeight"] = json!(h);
    }
    let prev = serde_json::from_value(prev).ok()?;
    update(snap, design_id, |d| {
        d.html = v.html.clone();
        if !v.rationale.is_empty() {
            d.rationale = v.rationale.clone();
        }
        if let Some(w) = v.viewport {
            d.viewport = Some(w as f64);
        }
        if let Some(h) = v.viewport_height {
            d.viewport_height = Some(h as f64);
        }
        let mut versions = d.versions.take().unwrap_or_default();
        versions.push(prev);
        let skip = versions.len().saturating_sub(MAX_VERSIONS);
        d.versions = Some(versions.into_iter().skip(skip).collect());
        d.editing = None;
        d.edit_error = None;
    })
}

/// The edit failed (`message`) or was cancelled (None): clear the busy flag.
pub fn edit_fail(snap: &Snapshot, design_id: &str, job_id: &str, message: Option<&str>) -> Option<Change> {
    let d = snap.design(design_id)?;
    if !edited_by(&d, job_id) {
        return None;
    }
    update(snap, design_id, |d| {
        d.editing = None;
        d.edit_error = message.map(str::to_string);
    })
}
