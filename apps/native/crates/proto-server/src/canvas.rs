//! Agent canvas skills (reference `src/lib/server/agent-canvas.ts`): describe the canvas, add cards, start
//! explorations, all as one store batch each.

use std::collections::BTreeMap;

use axum::http::StatusCode;
use serde_json::{Map, Value, json};

use crate::context::{ROOT, ROOT_NAME};
use crate::store::{self, Change, SharedStore, StateFile, live_map};
use crate::util::{self, Check, Issue, p};

const NODE_W: f64 = 380.0;
const NODE_H: f64 = 420.0;
const TALL_H: f64 = 560.0;
const GAP: f64 = 160.0;
const PORTAL_W: f64 = 300.0;
const PORTAL_H: f64 = 330.0;
const MARGIN: f64 = 40.0;

const ROOT_REJECTED: &str = "The main canvas holds only projects: pass the spaceId of a project or one of its explorations (GET /api/agent/canvas lists them; kind \"project\").";

#[derive(Clone, Copy, Debug, PartialEq)]
struct Box2 {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// A failed canvas edit: message + status.
#[derive(Debug, PartialEq)]
pub struct CanvasError(pub StatusCode, pub String);

fn err(status: StatusCode, msg: impl Into<String>) -> CanvasError {
    CanvasError(status, msg.into())
}

/// A validated card input.
#[derive(Clone, Debug, Default)]
pub struct CardInput {
    pub title: String,
    pub rationale: Option<String>,
    pub html: String,
    pub viewport: Option<i64>,
    pub viewport_height: Option<i64>,
    pub kind: Option<String>,
    pub parent_id: Option<String>,
    pub component: bool,
}

pub struct CardsBody {
    pub space_id: Option<String>,
    pub near_id: Option<String>,
    pub cards: Vec<CardInput>,
}

pub struct ExplorationBody {
    pub name: String,
    pub parent_space_id: Option<String>,
    pub seed_id: Option<String>,
    pub cards: Vec<CardInput>,
}

fn parse_card(c: &mut Check, v: &Value, at: &[Value]) -> Option<CardInput> {
    if !v.is_object() {
        c.push(at, "invalid_type", "Expected object");
        return None;
    }
    let path = |k: &str| {
        let mut x = at.to_vec();
        x.push(json!(k));
        x
    };
    let n = c.issues.len();
    let title = c.string(&v["title"], &path("title"), true, 1, 200);
    let rationale = c.opt_string(&v["rationale"], &path("rationale"), false, 0, 4000);
    let html = match v["html"].as_str() {
        Some(h) if h.trim().is_empty() => {
            c.push(&path("html"), "too_small", "html is empty");
            None
        }
        _ => c.string(&v["html"], &path("html"), true, 1, usize::MAX),
    };
    let viewport = c.opt_int(&v["viewport"], &path("viewport"), 200, 4000);
    let viewport_height = c.opt_int(&v["viewportHeight"], &path("viewportHeight"), 200, 4000);
    let kind = match &v["kind"] {
        Value::Null => None,
        Value::String(k) if ["page", "variation", "spoke"].contains(&k.as_str()) => Some(k.clone()),
        _ => {
            c.push(&path("kind"), "invalid_enum_value", "Invalid enum value. Expected 'page' | 'variation' | 'spoke'");
            None
        }
    };
    let parent_id = c.opt_id(&v["parentId"], &path("parentId"));
    let component = match &v["component"] {
        Value::Null => false,
        Value::Bool(b) => *b,
        _ => {
            c.push(&path("component"), "invalid_type", "Expected boolean");
            false
        }
    };
    if c.issues.len() > n {
        return None;
    }
    // The reference keeps the trimmed title and html (zod .trim()).
    Some(CardInput { title: title?, rationale, html: html?, viewport, viewport_height, kind, parent_id, component })
}

fn parse_cards(c: &mut Check, v: &Value, min: usize) -> Vec<CardInput> {
    let Some(list) = v.as_array() else {
        if !(v.is_null() && min == 0) {
            c.push(&p(&[&"cards"]), "invalid_type", if v.is_null() { "Required" } else { "Expected array" });
        }
        return Vec::new();
    };
    if list.len() < min {
        c.push(&p(&[&"cards"]), "too_small", format!("Array must contain at least {min} element(s)"));
    }
    if list.len() > 50 {
        c.push(&p(&[&"cards"]), "too_big", "Array must contain at most 50 element(s)");
    }
    list.iter().enumerate().filter_map(|(i, x)| parse_card(c, x, &p(&[&"cards", &i]))).collect()
}

pub fn parse_cards_body(v: &Value) -> Result<CardsBody, Vec<Issue>> {
    let mut c = Check::default();
    if !v.is_object() {
        c.push(&[], "invalid_type", "Expected object, received null");
        return Err(c.issues);
    }
    let space_id = c.opt_id(&v["spaceId"], &p(&[&"spaceId"]));
    let near_id = c.opt_id(&v["nearId"], &p(&[&"nearId"]));
    let cards = parse_cards(&mut c, &v["cards"], 1);
    if c.ok() { Ok(CardsBody { space_id, near_id, cards }) } else { Err(c.issues) }
}

pub fn parse_exploration_body(v: &Value) -> Result<ExplorationBody, Vec<Issue>> {
    let mut c = Check::default();
    if !v.is_object() {
        c.push(&[], "invalid_type", "Expected object, received null");
        return Err(c.issues);
    }
    let name = c.string(&v["name"], &p(&[&"name"]), true, 1, 120);
    let parent_space_id = c.opt_id(&v["parentSpaceId"], &p(&[&"parentSpaceId"]));
    let seed_id = c.opt_id(&v["seedId"], &p(&[&"seedId"]));
    let cards = parse_cards(&mut c, &v["cards"], 0);
    match name {
        Some(name) if c.ok() => Ok(ExplorationBody { name, parent_space_id, seed_id, cards }),
        _ => Err(c.issues),
    }
}

struct Snap {
    spaces: BTreeMap<String, Value>,
    designs: BTreeMap<String, Value>,
    portals: BTreeMap<String, Value>,
}

impl Snap {
    fn of(state: &StateFile) -> Self {
        Self { spaces: live_map(state, "spaces"), designs: live_map(state, "designs"), portals: live_map(state, "portals") }
    }
}

fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

fn pos(d: &Value) -> (f64, f64) {
    let p = &d["position"];
    (num(p, "x").unwrap_or(0.0), num(p, "y").unwrap_or(0.0))
}

/// Box height of a card (phone-width and portrait fixed-size designs render taller).
fn card_h(viewport: Option<f64>, viewport_height: Option<f64>) -> f64 {
    let w = viewport.unwrap_or(1280.0);
    if w < 600.0 || viewport_height.unwrap_or(0.0) > w { TALL_H } else { NODE_H }
}

fn boxes(s: &Snap, space_id: &str) -> Vec<Box2> {
    let mut out: Vec<Box2> = s
        .designs
        .values()
        .filter(|d| d["spaceId"].as_str() == Some(space_id))
        .map(|d| {
            let (x, y) = pos(d);
            Box2 { x, y, w: NODE_W, h: card_h(num(d, "viewport"), num(d, "viewportHeight")) }
        })
        .collect();
    for (id, p) in &s.portals {
        if s.spaces.get(id).and_then(|sp| sp["parentSpaceId"].as_str()) == Some(space_id) {
            out.push(Box2 { x: num(p, "x").unwrap_or(0.0), y: num(p, "y").unwrap_or(0.0), w: PORTAL_W, h: PORTAL_H });
        }
    }
    out
}

fn hits(a: &Box2, list: &[Box2]) -> bool {
    list.iter().any(|b| a.x < b.x + b.w + MARGIN && a.x + a.w + MARGIN > b.x && a.y < b.y + b.h + MARGIN && a.y + a.h + MARGIN > b.y)
}

/// Free spots for `n` items of size w×h in a row: right of `near` when given, else below everything.
fn place_row(taken: &[Box2], n: usize, w: f64, h: f64, near: Option<(f64, f64)>) -> Vec<(f64, f64)> {
    let mut occupied = taken.to_vec();
    let (mut x, y) = match near {
        Some((nx, ny)) => (nx + NODE_W + GAP, ny),
        None if occupied.is_empty() => (0.0, 0.0),
        None => (
            occupied.iter().map(|b| b.x).fold(f64::INFINITY, f64::min),
            occupied.iter().map(|b| b.y + b.h).fold(f64::NEG_INFINITY, f64::max) + GAP,
        ),
    };
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let mut b = Box2 { x, y, w, h };
        while hits(&b, &occupied) {
            b.x += w + GAP;
        }
        occupied.push(b);
        out.push((b.x, b.y));
        x = b.x + w + GAP;
    }
    out
}

fn build_cards(s: &Snap, space_id: &str, taken: &[Box2], cards: &[CardInput], near: Option<(f64, f64)>) -> Result<Vec<Value>, CanvasError> {
    for (i, c) in cards.iter().enumerate() {
        if let Some(pid) = &c.parent_id
            && !s.designs.contains_key(pid)
        {
            return Err(err(StatusCode::NOT_FOUND, format!("cards[{i}].parentId: unknown design")));
        }
    }
    let h = cards.iter().map(|c| card_h(c.viewport.map(|v| v as f64), c.viewport_height.map(|v| v as f64))).fold(f64::NEG_INFINITY, f64::max);
    let spots = place_row(taken, cards.len(), NODE_W, h, near);
    let now = util::now_ms();
    Ok(cards
        .iter()
        .zip(spots)
        .enumerate()
        .map(|(i, (c, (x, y)))| {
            let mut d = Map::new();
            d.insert("id".into(), json!(util::nanoid(10)));
            d.insert("spaceId".into(), json!(space_id));
            if let Some(pid) = &c.parent_id {
                d.insert("parentId".into(), json!(pid));
            }
            d.insert("kind".into(), json!(c.kind.as_deref().unwrap_or("variation")));
            d.insert("title".into(), json!(c.title));
            d.insert("rationale".into(), json!(c.rationale.as_deref().unwrap_or("")));
            d.insert("html".into(), json!(c.html));
            d.insert("status".into(), json!("ready"));
            d.insert("position".into(), json!({ "x": x, "y": y }));
            d.insert("createdAt".into(), json!(now + i as i64));
            if let Some(v) = c.viewport {
                d.insert("viewport".into(), json!(v));
            }
            if let Some(v) = c.viewport_height {
                d.insert("viewportHeight".into(), json!(v));
            }
            if c.component {
                d.insert("component".into(), json!(true));
            }
            Value::Object(d)
        })
        .collect())
}

fn id_of(d: &Value) -> String {
    d["id"].as_str().unwrap_or_default().to_string()
}

fn write(store: &SharedStore, changes: Vec<Change>, reason: &str) -> Result<(), CanvasError> {
    store.apply(changes, store::meta("agent", reason)).map(|_| ()).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// POST /api/agent/cards → `{ spaceId, ids }`.
pub fn create_cards(store: &SharedStore, input: CardsBody) -> Result<Value, CanvasError> {
    let s = Snap::of(&store.read(0, None));
    let near = match &input.near_id {
        Some(id) => Some(s.designs.get(id).ok_or_else(|| err(StatusCode::NOT_FOUND, format!("Unknown design {id}")))?),
        None => None,
    };
    let space_id = input.space_id.clone().or_else(|| near.and_then(|n| n["spaceId"].as_str().map(str::to_string))).unwrap_or(ROOT.into());
    if space_id == ROOT {
        return Err(err(StatusCode::BAD_REQUEST, ROOT_REJECTED));
    }
    if !s.spaces.contains_key(&space_id) {
        return Err(err(StatusCode::NOT_FOUND, format!("Unknown space {space_id}")));
    }
    let near_pos = near.filter(|n| n["spaceId"].as_str() == Some(&space_id)).map(pos);
    let created = build_cards(&s, &space_id, &boxes(&s, &space_id), &input.cards, near_pos)?;
    let ids: Vec<String> = created.iter().map(id_of).collect();
    write(store, created.into_iter().map(|d| Change::set(format!("designs/{}", id_of(&d)), d)).collect(), "agent-cards")?;
    Ok(json!({ "spaceId": space_id, "ids": ids }))
}

/// POST /api/agent/explorations → `{ spaceId, portal, seedId?, cardIds }`.
pub fn create_exploration(store: &SharedStore, input: ExplorationBody) -> Result<Value, CanvasError> {
    let s = Snap::of(&store.read(0, None));
    let seed = match &input.seed_id {
        Some(id) => Some(s.designs.get(id).ok_or_else(|| err(StatusCode::NOT_FOUND, format!("Unknown design {id}")))?),
        None => None,
    };
    let parent = input.parent_space_id.clone().or_else(|| seed.and_then(|d| d["spaceId"].as_str().map(str::to_string))).unwrap_or(ROOT.into());
    if parent == ROOT {
        return Err(err(StatusCode::BAD_REQUEST, ROOT_REJECTED));
    }
    if !s.spaces.contains_key(&parent) {
        return Err(err(StatusCode::NOT_FOUND, format!("Unknown space {parent}")));
    }
    let space_id = util::nanoid(8);
    let now = util::now_ms();
    let seed_near = seed.filter(|d| d["spaceId"].as_str() == Some(&parent)).map(pos);
    let (px, py) = place_row(&boxes(&s, &parent), 1, PORTAL_W, PORTAL_H, seed_near)[0];
    let portal = json!({ "x": px, "y": py });
    let mut changes = vec![
        Change::set(format!("spaces/{space_id}"), json!({ "id": space_id, "name": input.name, "parentSpaceId": parent, "createdAt": now })),
        Change::set(format!("portals/{space_id}"), portal.clone()),
    ];
    let mut seed_copy = None;
    let mut inside = Vec::new();
    if let Some(seed) = seed {
        let copy_id = util::nanoid(10);
        let mut d = seed.as_object().cloned().unwrap_or_default();
        d.insert("id".into(), json!(copy_id));
        d.insert("spaceId".into(), json!(space_id));
        d.remove("parentId");
        d.insert("starred".into(), json!(false));
        d.insert("position".into(), json!({ "x": 0, "y": 0 }));
        d.insert("createdAt".into(), json!(now));
        changes.push(Change::set(format!("designs/{copy_id}"), Value::Object(d)));
        changes.push(Change::set(format!("seededFrom/{copy_id}"), json!(id_of(seed))));
        inside.push(Box2 { x: 0.0, y: 0.0, w: NODE_W, h: card_h(num(seed, "viewport"), num(seed, "viewportHeight")) });
        seed_copy = Some(copy_id);
    }
    let cards =
        if input.cards.is_empty() { Vec::new() } else { build_cards(&s, &space_id, &inside, &input.cards, seed.map(|_| (0.0, 0.0)))? };
    let card_ids: Vec<String> = cards.iter().map(id_of).collect();
    changes.extend(cards.into_iter().map(|d| Change::set(format!("designs/{}", id_of(&d)), d)));
    write(store, changes, "agent-exploration")?;
    let mut out = json!({ "spaceId": space_id, "portal": portal, "cardIds": card_ids });
    if let Some(id) = seed_copy {
        out["seedId"] = json!(id);
    }
    Ok(out)
}

/// GET /api/agent/canvas → spaces with their cards, the main canvas first.
pub fn describe(store: &SharedStore) -> Value {
    let s = Snap::of(&store.read(0, None));
    let mut spaces = vec![json!({ "id": ROOT, "name": ROOT_NAME, "createdAt": 0 })];
    spaces.extend(s.spaces.values().cloned());
    let list: Vec<Value> = spaces
        .iter()
        .map(|sp| {
            let id = sp["id"].as_str().unwrap_or_default();
            let mut o = json!({ "id": id, "name": sp["name"] });
            if let Some(par) = sp.get("parentSpaceId").filter(|v| !v.is_null()) {
                o["parentSpaceId"] = par.clone();
            }
            if let Some(k) = sp.get("kind").filter(|v| v.as_str().is_some_and(|s| !s.is_empty())) {
                o["kind"] = k.clone();
            }
            if let Some(l) = sp.get("localPath").filter(|v| v.as_str().is_some_and(|s| !s.is_empty())) {
                o["localPath"] = l.clone();
            }
            o["cards"] = s
                .designs
                .values()
                .filter(|d| d["spaceId"].as_str() == Some(id))
                .map(|d| json!({ "id": d["id"], "title": d["title"], "status": d["status"], "kind": d["kind"], "position": d["position"] }))
                .collect();
            o
        })
        .collect();
    json!({ "spaces": list })
}

#[cfg(test)]
#[path = "canvas_tests.rs"]
mod tests;
