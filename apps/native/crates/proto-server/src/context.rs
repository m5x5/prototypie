//! Where queue items land on the canvas (reference `withContext` / `placeholderOf` in agent-queue.ts), computed
//! from shared state.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use proto_ai::QueueItem;
use crate::store::{StateFile, live_map};

pub const ROOT: &str = "root";
pub const ROOT_NAME: &str = "Main canvas";

/// The `runs/*` record that drives a server job.
pub struct Run {
    pub space_id: String,
    pub parent_id: Option<String>,
    pub design_ids: Vec<String>,
}

pub fn run_for_job(state: &StateFile, job_id: &str) -> Option<Run> {
    live_map(state, "runs").into_values().find_map(|r| {
        if r.get("serverJobId").and_then(Value::as_str) != Some(job_id) {
            return None;
        }
        Some(Run {
            space_id: r.get("spaceId").and_then(Value::as_str).unwrap_or_default().to_string(),
            parent_id: r.get("parentId").and_then(Value::as_str).map(str::to_string),
            design_ids: r
                .get("designIds")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
        })
    })
}

/// Spaces with names and parents, for paths.
pub struct Spaces(BTreeMap<String, Value>);

impl Spaces {
    pub fn of(state: &StateFile) -> Self {
        Self(live_map(state, "spaces"))
    }

    pub fn parent(&self, id: &str) -> Option<String> {
        self.0.get(id)?.get("parentSpaceId")?.as_str().map(str::to_string)
    }

    pub fn name(&self, id: &str) -> String {
        if id == ROOT {
            return ROOT_NAME.into();
        }
        self.0.get(id).and_then(|s| s.get("name")).and_then(Value::as_str).unwrap_or(id).to_string()
    }

    /// Names from the main canvas down to `id` (always starts with "Main canvas"; at most 20 levels).
    pub fn path(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = Some(id.to_string());
        let mut n = 0;
        while let Some(c) = cur {
            if n >= 20 {
                break;
            }
            out.insert(0, self.name(&c));
            cur = self.parent(&c);
            n += 1;
        }
        if out.first().map(String::as_str) != Some(ROOT_NAME) {
            out.insert(0, ROOT_NAME.into());
        }
        out
    }
}

fn str_of<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// `{ spaceId, spaceName, spacePath, baseDesignId?, placeholderId?, cards, seedDesign? }` for an item.
pub fn queue_context(state: &StateFile, item: &QueueItem) -> Option<Value> {
    let spaces = Spaces::of(state);
    let designs: Vec<Value> = live_map(state, "designs").into_values().collect();
    let cards_in = |space: &str| -> Vec<Value> {
        designs
            .iter()
            .filter(|d| str_of(d, "spaceId") == space)
            .map(|d| json!({ "id": str_of(d, "id"), "title": str_of(d, "title") }))
            .collect()
    };
    let run = run_for_job(state, &item.job_id);
    if run.is_none()
        && let Some(target_id) = &item.target_design_id
        && let Some(t) = designs.iter().find(|d| str_of(d, "id") == target_id)
    {
        let space = str_of(t, "spaceId");
        return Some(json!({
            "spaceId": space,
            "spaceName": spaces.name(space),
            "spacePath": spaces.path(space),
            "baseDesignId": target_id,
            "placeholderId": target_id,
            "cards": cards_in(space),
        }));
    }
    let run = run?;
    let seeded: BTreeMap<String, Value> = live_map(state, "seededFrom");
    let mut ctx = json!({
        "spaceId": run.space_id,
        "spaceName": spaces.name(&run.space_id),
        "spacePath": spaces.path(&run.space_id),
        "cards": cards_in(&run.space_id),
    });
    let o = ctx.as_object_mut()?;
    if let Some(p) = &run.parent_id {
        o.insert("baseDesignId".into(), json!(p));
    }
    if let Some(id) = run.design_ids.get(item.index) {
        o.insert("placeholderId".into(), json!(id));
    }
    let seed = designs.iter().find(|d| str_of(d, "spaceId") == run.space_id && seeded.get(str_of(d, "id")).is_some_and(Value::is_string));
    if let Some(s) = seed {
        let id = str_of(s, "id");
        o.insert("seedDesign".into(), json!({ "id": id, "title": str_of(s, "title"), "originalId": seeded[id] }));
    }
    Some(ctx)
}

/// `guidelinesFor(spaceId)`: the space's guidelines card, else the nearest ancestor's (reference guidelines.ts).
pub fn guidelines_for(store: &proto_core::Store, space_id: Option<&str>) -> Option<proto_ai::Guidelines> {
    let space_id = space_id?;
    let snap = store.snapshot();
    let card = snap.guidelines_for(space_id)?;
    let from_space = if card.space_id == ROOT {
        ROOT_NAME.to_string()
    } else {
        snap.space(&card.space_id).map(|s| s.name.clone()).unwrap_or_else(|| card.space_id.clone())
    };
    let text = proto_ai::context::truncate_chars(&proto_ai::context::html_to_text(&card.html), proto_ai::context::MAX_GUIDELINES_TEXT).to_string();
    Some(proto_ai::Guidelines { card_id: card.id.clone(), title: card.title.clone(), from_space, text })
}
