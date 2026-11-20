//! The notes service's data: stored shapes (camelCase JSON), input checks, the derived `tag`, ids and times.
//! Spec: spec-public/05 AN-CLI-104…113, -122, -131, -132, -150, -153.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::util;

/// The four states, in the order the service names them.
pub const STATUSES: [&str; 4] = ["pending", "acknowledged", "resolved", "dismissed"];
/// Collection states.
pub const STATES: [&str; 2] = ["active", "closed"];

pub const MAX_BODY: usize = 1024 * 1024;
pub const MAX_COMMENT: usize = 100_000;
pub const MAX_TEXT: usize = 20_000;
pub const MAX_LOCATION: usize = 2048;
pub const MAX_ATTRIBUTES: usize = 200;
pub const MAX_RECTS: usize = 200;
pub const MAX_MESSAGE: usize = 20_000;
pub const MAX_NOTES_PER_COLLECTION: usize = 5000;
pub const MAX_MESSAGES_PER_NOTE: usize = 500;
pub const MAX_COLLECTIONS: usize = 1000;
pub const MAX_WATCHERS: usize = 32;
/// The file format this build writes and reads.
pub const FORMAT: u64 = 1;

// ---- errors ------------------------------------------------------------------------------------------------

/// A refusal: status, `code`, the human sentence, and the optional `field` / current `note`.
#[derive(Clone, Debug, PartialEq)]
pub struct ApiError {
    pub status: u16,
    pub code: &'static str,
    pub error: String,
    pub field: Option<String>,
    pub note: Option<Value>,
}

impl ApiError {
    pub fn new(status: u16, code: &'static str, error: impl Into<String>) -> Self {
        ApiError { status, code, error: error.into(), field: None, note: None }
    }

    /// 400 `invalid_field` for `field`: "`<field>` must be `<what>`".
    pub fn field(field: &str, what: &str) -> Self {
        let mut e = ApiError::new(400, "invalid_field", format!("{field} must be {what}"));
        e.field = Some(field.to_string());
        e
    }

    /// 400 `invalid_query` for a query parameter.
    pub fn query(param: &str, what: &str) -> Self {
        ApiError::new(400, "invalid_query", format!("{param} must be {what}"))
    }

    pub fn invalid_json() -> Self {
        ApiError::new(400, "invalid_json", "The request body must be a JSON object")
    }

    pub fn missing_fields() -> Self {
        ApiError::new(400, "missing_fields", "comment, element and elementPath are required")
    }

    pub fn missing_thread_fields() -> Self {
        ApiError::new(400, "missing_thread_fields", "role and content are required")
    }

    pub fn note_not_found() -> Self {
        ApiError::new(404, "note_not_found", "Note not found")
    }

    pub fn collection_not_found() -> Self {
        ApiError::new(404, "collection_not_found", "Collection not found")
    }

    pub fn not_found() -> Self {
        ApiError::new(404, "not_found", "Not found")
    }

    pub fn method_not_allowed() -> Self {
        ApiError::new(405, "method_not_allowed", "Method not allowed")
    }

    pub fn limit(what: &str, limit: &str) -> Self {
        ApiError::new(409, "limit_reached", format!("{what} is full ({limit})"))
    }

    pub fn storage(cause: impl std::fmt::Display) -> Self {
        ApiError::new(500, "storage_failed", format!("Couldn't save the notes: {cause}"))
    }

    pub fn shutting_down() -> Self {
        ApiError::new(503, "shutting_down", "Prototypie is quitting")
    }

    pub fn body(&self) -> Value {
        let mut v = json!({ "error": self.error, "code": self.code });
        if let Some(f) = &self.field {
            v["field"] = json!(f);
        }
        if let Some(n) = &self.note {
            v["note"] = n.clone();
        }
        v
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        respond(self.status, &self.body())
    }
}

/// A JSON answer with `Cache-Control: no-store` (every answer of the service, AN-CLI-102).
pub fn respond(status: u16, body: &Value) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut res = (code, axum::Json(body.clone())).into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

// ---- ids and times -----------------------------------------------------------------------------------------

const ID_CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// `prefix` plus 10 characters from `0-9a-z` (AN-CLI-104). Callers re-roll on a collision.
pub fn new_id(prefix: char) -> String {
    let mut rng = rand::rng();
    let mut s = String::with_capacity(11);
    s.push(prefix);
    for _ in 0..10 {
        s.push(ID_CHARS[rng.random_range(0..ID_CHARS.len())] as char);
    }
    s
}

/// A collection file stem the service made itself: `^c[0-9a-z]{10}$` (AN-CLI-505).
pub fn is_collection_id(s: &str) -> bool {
    s.len() == 11 && s.starts_with('c') && s[1..].bytes().all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
}

/// Unix ms of a service time text, if it is one.
pub fn ms_of(iso: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(iso).ok().map(|t| t.timestamp_millis())
}

/// The next change time for a note: now, but always later than `prev` (strictly rising `updatedAt`, AN-CLI-105).
pub fn next_time(prev: Option<&str>) -> (String, i64) {
    let now = util::now_ms();
    let ms = match prev.and_then(ms_of) {
        Some(p) if p >= now => p + 1,
        _ => now,
    };
    (util::iso_from_ms(ms), ms)
}

// ---- stored shapes -----------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CollectionMeta {
    pub id: String,
    pub location: String,
    pub state: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoredMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
    pub created_at_ms: i64,
    /// Wake-up number of a person's reply (same counter as the notes' `seq`), for `watch?replies=1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoredNote {
    pub id: String,
    pub location: String,
    pub seq: u64,
    pub kind: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
    pub comment: String,
    pub element: String,
    pub element_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_timestamp: Option<serde_json::Number>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default = "empty_object")]
    pub target: Value,
    #[serde(default)]
    pub thread: Vec<StoredMessage>,
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

/// One file per collection (`notes/<id>.json`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollectionFile {
    pub format: u64,
    pub collection: CollectionMeta,
    #[serde(default)]
    pub notes: Vec<StoredNote>,
}

// ---- JSON out ----------------------------------------------------------------------------------------------

/// The `tag` derived from a `[prototypie job|implement|project-info …]` prefix (AN-CLI-112).
pub fn tag_of(comment: &str) -> Option<Value> {
    let rest = comment.strip_prefix("[prototypie ")?;
    let inner = &rest[..rest.find(']')?];
    let (kind, arg) = inner.split_once(' ')?;
    if arg.is_empty() || arg.contains(' ') {
        return None;
    }
    match kind {
        "job" => {
            let (job, index) = arg.rsplit_once('#')?;
            if job.is_empty() || index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let index: u64 = index.parse().ok()?;
            Some(json!({ "kind": "job", "jobId": job, "index": index }))
        }
        "implement" => Some(json!({ "kind": "implement", "designId": arg })),
        "project-info" => Some(json!({ "kind": "project-info", "spaceId": arg })),
        _ => None,
    }
}

pub fn message_json(m: &StoredMessage) -> Value {
    json!({ "id": m.id, "role": m.role, "content": m.content, "createdAt": m.created_at, "createdAtMs": m.created_at_ms })
}

/// A note as the service answers it (AN-CLI-111): stored fields plus `collectionId` and `tag`.
pub fn note_json(collection_id: &str, n: &StoredNote) -> Value {
    let mut o = Map::new();
    o.insert("id".into(), json!(n.id));
    o.insert("collectionId".into(), json!(collection_id));
    o.insert("location".into(), json!(n.location));
    o.insert("seq".into(), json!(n.seq));
    o.insert("kind".into(), json!(n.kind));
    o.insert("status".into(), json!(n.status));
    if let Some(v) = &n.closed_by {
        o.insert("closedBy".into(), json!(v));
    }
    if let Some(v) = &n.closed_at {
        o.insert("closedAt".into(), json!(v));
    }
    if let Some(t) = tag_of(&n.comment) {
        o.insert("tag".into(), t);
    }
    o.insert("comment".into(), json!(n.comment));
    o.insert("element".into(), json!(n.element));
    o.insert("elementPath".into(), json!(n.element_path));
    if let Some(v) = &n.design_id {
        o.insert("designId".into(), json!(v));
    }
    if let Some(v) = &n.client_ref {
        o.insert("clientRef".into(), json!(v));
    }
    if let Some(v) = &n.client_timestamp {
        o.insert("clientTimestamp".into(), Value::Number(v.clone()));
    }
    o.insert("createdAt".into(), json!(n.created_at));
    if let Some(v) = &n.updated_at {
        o.insert("updatedAt".into(), json!(v));
    }
    o.insert("target".into(), n.target.clone());
    o.insert("thread".into(), Value::Array(n.thread.iter().map(message_json).collect()));
    Value::Object(o)
}

pub fn collection_json(meta: &CollectionMeta, notes: &[StoredNote]) -> Value {
    let mut o = Map::new();
    o.insert("id".into(), json!(meta.id));
    o.insert("location".into(), json!(meta.location));
    o.insert("state".into(), json!(meta.state));
    o.insert("createdAt".into(), json!(meta.created_at));
    if let Some(u) = &meta.updated_at {
        o.insert("updatedAt".into(), json!(u));
    }
    o.insert("noteCount".into(), json!(notes.len()));
    o.insert("pendingCount".into(), json!(notes.iter().filter(|n| n.status == "pending").count()));
    Value::Object(o)
}

// ---- input checks ------------------------------------------------------------------------------------------

/// Absent and `null` are the same (AN-CLI-111).
fn given(v: &Value) -> Option<&Value> {
    (!v.is_null()).then_some(v)
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

/// A location: 1–2048 characters, no control characters.
pub fn check_location(v: &Value) -> Result<String, ApiError> {
    match v.as_str() {
        Some(s) if (1..=MAX_LOCATION).contains(&chars(s)) && !s.chars().any(char::is_control) => Ok(s.to_string()),
        _ => Err(ApiError::field("location", "1 to 2048 characters without control characters")),
    }
}

pub fn is_status(s: &str) -> bool {
    STATUSES.contains(&s)
}

fn check_text(v: &Value, field: &str, max: usize) -> Result<String, ApiError> {
    match v.as_str() {
        Some(s) if chars(s) <= max => Ok(s.to_string()),
        Some(_) => Err(ApiError::field(field, &format!("at most {max} characters"))),
        None => Err(ApiError::field(field, "a text")),
    }
}

/// A name for `by` (status changes) and watchers: 1–100 characters.
pub fn check_name(v: &Value, field: &str) -> Result<Option<String>, ApiError> {
    match given(v) {
        None => Ok(None),
        Some(Value::String(s)) if (1..=100).contains(&chars(s)) => Ok(Some(s.clone())),
        Some(_) => Err(ApiError::field(field, "1 to 100 characters")),
    }
}

/// The validated input of a new note.
#[derive(Clone, Debug)]
pub struct NewNoteInput {
    pub comment: String,
    pub element: String,
    pub element_path: String,
    pub location: Option<String>,
    pub kind: String,
    pub design_id: Option<String>,
    pub client_ref: Option<String>,
    pub client_timestamp: Option<serde_json::Number>,
    pub target: Value,
}

/// Rules 1–3 and 6 of AN-CLI-122 plus the size limits of AN-CLI-153.
pub fn parse_new_note(body: &Value) -> Result<NewNoteInput, ApiError> {
    let text = |k: &str| body[k].as_str().map(str::to_string);
    let (Some(comment), Some(element), Some(element_path)) = (text("comment"), text("element"), text("elementPath")) else {
        return Err(ApiError::missing_fields());
    };
    if chars(&comment) > MAX_COMMENT {
        return Err(ApiError::field("comment", &format!("at most {MAX_COMMENT} characters")));
    }
    if chars(&element) > MAX_TEXT {
        return Err(ApiError::field("element", &format!("at most {MAX_TEXT} characters")));
    }
    if chars(&element_path) > MAX_TEXT {
        return Err(ApiError::field("elementPath", &format!("at most {MAX_TEXT} characters")));
    }
    if let Some(s) = given(&body["status"])
        && s.as_str() != Some("pending")
    {
        return Err(ApiError::field("status", "pending for a new note"));
    }
    let location = given(&body["location"]).map(check_location).transpose()?;
    let kind = match given(&body["kind"]) {
        None => "feedback".to_string(),
        Some(Value::String(k)) if (1..=40).contains(&k.len()) && k.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') => k.clone(),
        Some(_) => return Err(ApiError::field("kind", "1 to 40 characters from a-z and -")),
    };
    let design_id = match given(&body["designId"]) {
        None => None,
        Some(Value::String(d)) if chars(d) <= 200 => Some(d.clone()),
        Some(_) => return Err(ApiError::field("designId", "a text of at most 200 characters")),
    };
    let client_ref = match given(&body["clientRef"]) {
        None => None,
        Some(Value::String(r)) if (1..=100).contains(&chars(r)) => Some(r.clone()),
        Some(_) => return Err(ApiError::field("clientRef", "1 to 100 characters")),
    };
    let client_timestamp = match given(&body["clientTimestamp"]) {
        None => None,
        Some(Value::Number(n)) => Some(n.clone()),
        Some(_) => return Err(ApiError::field("clientTimestamp", "a number (Unix ms)")),
    };
    let target = match given(&body["target"]) {
        None => empty_object(),
        Some(t) => check_target(t)?,
    };
    Ok(NewNoteInput { comment, element, element_path, location, kind, design_id, client_ref, client_timestamp, target })
}

const TARGET_TEXTS: [&str; 9] = [
    "fullPath",
    "sourceLocations",
    "componentChain",
    "nearbyText",
    "nearbyElements",
    "cssClasses",
    "computedStyles",
    "accessibility",
    "selectedText",
];

fn check_numbers(v: &Value, field: &str, keys: &[&str]) -> Result<Value, ApiError> {
    let what = format!("an object with the numbers {}", keys.join(", "));
    let Some(o) = v.as_object() else { return Err(ApiError::field(field, &what)) };
    let mut out = Map::new();
    for k in keys {
        match o.get(*k) {
            Some(Value::Number(n)) => {
                out.insert((*k).into(), Value::Number(n.clone()));
            }
            _ => return Err(ApiError::field(field, &what)),
        }
    }
    Ok(Value::Object(out))
}

/// `target` with only its known fields, each of the right type (AN-CLI-111, rule 6 of AN-CLI-122).
pub fn check_target(v: &Value) -> Result<Value, ApiError> {
    let Some(o) = v.as_object() else { return Err(ApiError::field("target", "an object")) };
    let mut out = Map::new();
    for (k, val) in o {
        if val.is_null() {
            continue;
        }
        let field = format!("target.{k}");
        let checked = match k.as_str() {
            k if TARGET_TEXTS.contains(&k) => Value::String(check_text(val, &field, MAX_TEXT)?),
            "position" => check_numbers(val, &field, &["x", "y"])?,
            "rect" => check_numbers(val, &field, &["x", "y", "width", "height"])?,
            "rects" => {
                let Some(list) = val.as_array() else { return Err(ApiError::field(&field, "a list of rectangles")) };
                if list.len() > MAX_RECTS {
                    return Err(ApiError::field(&field, &format!("at most {MAX_RECTS} rectangles")));
                }
                Value::Array(list.iter().map(|r| check_numbers(r, &field, &["x", "y", "width", "height"])).collect::<Result<_, _>>()?)
            }
            "fixed" | "multiSelect" => match val {
                Value::Bool(b) => Value::Bool(*b),
                _ => return Err(ApiError::field(&field, "true or false")),
            },
            "attributes" => {
                let Some(attrs) = val.as_object() else { return Err(ApiError::field(&field, "an object of texts")) };
                if attrs.len() > MAX_ATTRIBUTES {
                    return Err(ApiError::field(&field, &format!("at most {MAX_ATTRIBUTES} entries")));
                }
                let mut m = Map::new();
                for (ak, av) in attrs {
                    m.insert(ak.clone(), Value::String(check_text(av, &field, MAX_TEXT)?));
                }
                Value::Object(m)
            }
            _ => continue,
        };
        out.insert(k.clone(), checked);
    }
    Ok(Value::Object(out))
}

/// `POST /api/notes/{id}/status` input (AN-CLI-131).
#[derive(Clone, Debug)]
pub struct StatusInput {
    pub status: String,
    pub by: String,
    pub message: Option<String>,
    pub if_status: Option<Vec<String>>,
}

pub fn parse_status(body: &Value) -> Result<StatusInput, ApiError> {
    let statuses = "one of pending, acknowledged, resolved, dismissed";
    let status = match body["status"].as_str() {
        Some(s) if is_status(s) => s.to_string(),
        _ => return Err(ApiError::field("status", statuses)),
    };
    let by = check_name(&body["by"], "by")?.unwrap_or_else(|| "agent".into());
    let message = match given(&body["message"]) {
        None => None,
        Some(m) => Some(check_text(m, "message", MAX_MESSAGE)?).filter(|m| !m.trim().is_empty()),
    };
    let if_status = match given(&body["ifStatus"]) {
        None => None,
        Some(Value::String(s)) if is_status(s) => Some(vec![s.clone()]),
        Some(Value::Array(list)) if !list.is_empty() && list.iter().all(|x| x.as_str().is_some_and(is_status)) => {
            Some(list.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        }
        Some(_) => return Err(ApiError::field("ifStatus", &format!("{statuses}, or a list of them"))),
    };
    Ok(StatusInput { status, by, message, if_status })
}

/// `POST /api/notes/{id}/thread` input (AN-CLI-132): (role, content as sent).
pub fn parse_thread(body: &Value) -> Result<(String, String), ApiError> {
    let (Some(role), Some(content)) = (body["role"].as_str(), body["content"].as_str()) else {
        return Err(ApiError::missing_thread_fields());
    };
    if content.trim().is_empty() {
        return Err(ApiError::missing_thread_fields());
    }
    if role != "human" && role != "agent" {
        return Err(ApiError::field("role", "human or agent"));
    }
    if chars(content) > MAX_MESSAGE {
        return Err(ApiError::field("content", &format!("at most {MAX_MESSAGE} characters")));
    }
    Ok((role.to_string(), content.to_string()))
}
