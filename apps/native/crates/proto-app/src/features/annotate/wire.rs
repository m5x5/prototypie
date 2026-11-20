//! What a note looks like at the notes service (spec 03 §C, spec 05 AN-CLI-111/160) and back (spec 03 §D):
//! the body sent, reading the service's notes, rebuilding unknown ones and merging into the local list.

use serde_json::{Map, Value, json};

use super::model::{Message, Note, Status, Target};

/// `prototypie-rune://<space>` (the collection of a space).
pub fn collection_location(space: &str) -> String {
    format!("prototypie-rune://{space}")
}

/// `prototypie-rune://<space>/<pick>`: pick = the design id, else the first target's key, else empty (AN-SY-102).
pub fn note_location(n: &Note) -> String {
    let pick = n.design_id().map(str::to_string).or_else(|| n.targets.first().and_then(|t| t.key.clone())).unwrap_or_default();
    format!("prototypie-rune://{}/{pick}", n.space)
}

/// The comment agents read (AN-SY-090).
pub fn agent_comment(n: &Note) -> String {
    let mut s = n.comment.trim().to_string();
    for t in &n.targets {
        match (&t.source, &t.key) {
            (Some(src), key) => {
                s.push_str("\n— Rune element ");
                if let Some(k) = key {
                    s.push_str(&format!("`{k}` "));
                }
                s.push_str(&format!("at {src}"));
                if let Some(m) = &t.module {
                    s.push_str(&format!(" ({m})"));
                }
            }
            (None, Some(k)) => s.push_str(&format!("\n— Rune element `{k}` (source not found)")),
            (None, None) => {}
        }
    }
    if let Some(d) = n.design_id() {
        s.push_str(&format!("\n— In design {d} (HTML, Prototypie Rune)"));
    }
    s
}

fn joined<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    parts.filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" | ")
}

fn rect_json(r: [f32; 4]) -> Value {
    json!({"x": r[0], "y": r[1], "width": r[2], "height": r[3]})
}

/// The body that creates `n` in its space's collection (AN-SY-091/092, AN-CLI-122).
pub fn note_body(n: &Note) -> Value {
    let first = n.targets.first();
    let r = first.map_or([0.0; 4], |t| t.rect);
    let x = if n.window[0] > 0.0 { (r[0] + r[2] / 2.0) / n.window[0] * 100.0 } else { 0.0 };
    let y = r[1] + r[3] / 2.0;
    let multi = n.targets.len() > 1;
    let mut target = Map::new();
    target.insert("fullPath".into(), json!(joined(n.targets.iter().map(|t| t.full_path.as_str()))));
    target.insert("position".into(), json!({"x": x, "y": y}));
    target.insert("rect".into(), rect_json(r));
    target.insert("fixed".into(), json!(true));
    target.insert("multiSelect".into(), json!(multi));
    if multi {
        target.insert("rects".into(), Value::Array(n.targets.iter().map(|t| rect_json(t.rect)).collect()));
    }
    let mut opt = |name: &str, v: String| {
        if !v.is_empty() {
            target.insert(name.into(), json!(v));
        }
    };
    opt("nearbyText", joined(n.targets.iter().map(|t| t.nearby_text.as_str())));
    opt("nearbyElements", joined(n.targets.iter().map(|t| t.nearby_elements.as_str())));
    opt("cssClasses", joined(n.targets.iter().map(|t| t.css_classes.as_str())));
    opt("computedStyles", joined(n.targets.iter().map(|t| t.styles.as_str())));
    opt("accessibility", joined(n.targets.iter().map(|t| t.accessibility.as_str())));
    let chain: Vec<String> = n.targets.iter().filter(|t| t.key.is_some()).map(|t| format!("Rune: {}", t.element_path)).collect();
    opt("componentChain", chain.join(" | "));
    opt("sourceLocations", joined(n.targets.iter().filter_map(|t| t.source.as_deref())));
    if let Some(sel) = first.and_then(|t| t.selected_text.clone()) {
        opt("selectedText", sel);
    }
    if let Some(t) = first.filter(|t| !t.attributes.is_empty()) {
        let attrs: Map<String, Value> = t.attributes.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
        target.insert("attributes".into(), Value::Object(attrs));
    }
    let element = n.targets.iter().map(|t| t.element.as_str()).collect::<Vec<_>>().join(", ");
    let mut body = json!({
        "kind": "feedback",
        "status": "pending",
        "comment": agent_comment(n),
        "element": element,
        "elementPath": joined(n.targets.iter().map(|t| t.element_path.as_str())),
        "location": note_location(n),
        "clientRef": n.id,
        "clientTimestamp": if n.timestamp.is_finite() { json!(n.timestamp.round() as i64) } else { json!(0) },
        "target": Value::Object(target),
    });
    if let Some(d) = n.design_id() {
        body["designId"] = json!(d);
    }
    body
}

// ---- reading (AN-SY-120…123) ----

fn text_of<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str)
}

fn num_of(v: Option<&Value>) -> f32 {
    v.and_then(Value::as_f64).filter(|f| f.is_finite()).unwrap_or(0.0) as f32
}

/// A service note's thread: entries with a text role and text content, in order.
pub fn read_thread(v: &Value) -> Vec<Message> {
    v.get("thread").and_then(Value::as_array).map(|a| {
        a.iter().filter_map(|m| Some(Message { role: text_of(m, "role")?.to_string(), content: text_of(m, "content")?.to_string() })).collect()
    }).unwrap_or_default()
}

/// The first part before ` | `.
fn first_part(s: &str) -> &str {
    s.split(" | ").next().unwrap_or("")
}

/// The local note for a service note of `space` that has no local twin; None when its location belongs to
/// another space or it has no id.
pub fn rebuild(v: &Value, space: &str) -> Option<Note> {
    let id = text_of(v, "id")?.to_string();
    let prefix = format!("prototypie-rune://{space}/");
    let pick = text_of(v, "location")?.strip_prefix(&prefix)?.to_string();
    let design = text_of(v, "designId").map(str::to_string);
    let comment = text_of(v, "comment").unwrap_or("");
    let comment = comment.split("\n— ").next().unwrap_or("").to_string();
    let target = v.get("target").filter(|t| t.is_object());
    let tt = |k: &str| target.and_then(|t| text_of(t, k)).unwrap_or("");
    let rect = target.and_then(|t| t.get("rect")).map_or([0.0; 4], |r| {
        [num_of(r.get("x")), num_of(r.get("y")), num_of(r.get("width")), num_of(r.get("height"))]
    });
    let source = first_part(tt("sourceLocations"));
    let t = Target {
        element: text_of(v, "element").unwrap_or("").to_string(),
        element_path: first_part(text_of(v, "elementPath").unwrap_or("")).to_string(),
        full_path: first_part(tt("fullPath")).to_string(),
        source: (!source.is_empty()).then(|| source.to_string()),
        rect,
        key: (design.is_none() && !pick.is_empty()).then(|| pick.clone()),
        design_id: design,
        ..Target::default()
    };
    Some(Note {
        id: id.clone(),
        server_id: Some(id),
        space: space.to_string(),
        comment,
        targets: vec![t],
        timestamp: v.get("clientTimestamp").and_then(Value::as_f64).filter(|f| f.is_finite()).unwrap_or(0.0),
        status: Status::from_service(text_of(v, "status")),
        thread: read_thread(v),
        window: [0.0, 0.0],
    })
}

/// Merge the service's notes of `space` into `notes` (AN-SY-111…115). True when anything changed; then the
/// whole list is sorted by timestamp (stable).
pub fn merge(notes: &mut Vec<Note>, space: &str, service: &[Value]) -> bool {
    let mut changed = false;
    for v in service {
        let Some(id) = text_of(v, "id") else { continue };
        if let Some(local) = notes.iter_mut().find(|n| n.server_id.as_deref() == Some(id)) {
            let status = Status::from_service(text_of(v, "status"));
            let keep_local = !local.status.is_open() && status.is_open();
            if status != local.status && !keep_local {
                local.status = status;
                changed = true;
            }
            let thread = read_thread(v);
            if thread != local.thread && thread.len() >= local.thread.len() {
                local.thread = thread;
                changed = true;
            }
        } else if let Some(n) = rebuild(v, space) {
            notes.push(n);
            changed = true;
        }
    }
    if changed {
        notes.sort_by(|a, b| a.timestamp.total_cmp(&b.timestamp));
    }
    changed
}
