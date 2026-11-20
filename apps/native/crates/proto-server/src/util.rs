//! Small shared helpers: JSON responses, ids, times, request bodies.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rand::Rng;
use serde::Serialize;
use serde_json::{Value, json};

/// `{ error }` with a status, the reference's error shape.
pub fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (status, axum::Json(json!({ "error": message.into() }))).into_response()
}

/// JSON with `Cache-Control: no-store`.
pub fn no_store<T: Serialize>(body: T) -> Response {
    let mut res = axum::Json(body).into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

pub fn ok<T: Serialize>(body: T) -> Response {
    axum::Json(body).into_response()
}

/// A body parsed leniently: anything that isn't JSON becomes `Value::Null` (the reference's `.catch(() => null)`).
pub fn parse_body(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

const ALPHABET: &[u8] = b"useandom-26T198340PX75pxJACKVERYMINDBUSHWOLF_GQZbfghjklqvwyzrict";

/// nanoid-style id of `len` characters from `[A-Za-z0-9_-]`.
pub fn nanoid(len: usize) -> String {
    let mut rng = rand::rng();
    (0..len).map(|_| ALPHABET[rng.random_range(0..64)] as char).collect()
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// ISO-8601 with milliseconds and `Z`, like JavaScript's `toISOString()`.
pub fn iso_now() -> String {
    iso(chrono::Utc::now())
}

pub fn iso(t: chrono::DateTime<chrono::Utc>) -> String {
    t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

pub fn iso_from_ms(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(iso).unwrap_or_default()
}

/// `^[\w-]{1,max}$` (ASCII word characters).
pub fn is_id(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// One validation issue, zod-like.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Issue {
    pub path: Vec<Value>,
    pub message: String,
    pub code: &'static str,
}

/// Collects validation issues for an object body.
#[derive(Default)]
pub struct Check {
    pub issues: Vec<Issue>,
}

impl Check {
    pub fn push(&mut self, path: &[Value], code: &'static str, message: impl Into<String>) {
        self.issues.push(Issue { path: path.to_vec(), message: message.into(), code });
    }

    pub fn ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Required string, trimmed when `trim`, length in `min..=max` (chars).
    pub fn string(&mut self, v: &Value, path: &[Value], trim: bool, min: usize, max: usize) -> Option<String> {
        let Some(s) = v.as_str() else {
            self.push(path, "invalid_type", if v.is_null() { "Required" } else { "Expected string" });
            return None;
        };
        let s = if trim { s.trim() } else { s };
        let n = s.chars().count();
        if n < min {
            self.push(path, "too_small", format!("String must contain at least {min} character(s)"));
            return None;
        }
        if n > max {
            self.push(path, "too_big", format!("String must contain at most {max} character(s)"));
            return None;
        }
        Some(s.to_string())
    }

    /// Optional string (absent or null = None).
    pub fn opt_string(&mut self, v: &Value, path: &[Value], trim: bool, min: usize, max: usize) -> Option<String> {
        if v.is_null() { None } else { self.string(v, path, trim, min, max) }
    }

    /// Optional integer in `min..=max`.
    pub fn opt_int(&mut self, v: &Value, path: &[Value], min: i64, max: i64) -> Option<i64> {
        if v.is_null() {
            return None;
        }
        match v.as_f64() {
            Some(f) if f.fract() == 0.0 && f >= min as f64 && f <= max as f64 => Some(f as i64),
            Some(f) if f.fract() != 0.0 => {
                self.push(path, "invalid_type", "Expected integer, received float");
                None
            }
            Some(_) => {
                self.push(path, "out_of_range", format!("Number must be between {min} and {max}"));
                None
            }
            None => {
                self.push(path, "invalid_type", "Expected number");
                None
            }
        }
    }

    /// Optional id (`^[\w-]{1,64}$`).
    pub fn opt_id(&mut self, v: &Value, path: &[Value]) -> Option<String> {
        if v.is_null() {
            return None;
        }
        match v.as_str() {
            Some(s) if is_id(s, 64) => Some(s.to_string()),
            _ => {
                self.push(path, "invalid_string", "must be an id (letters, digits, _ or -)");
                None
            }
        }
    }
}

/// `path` helper: `p(&["cards", 0, "title"])`.
pub fn p(parts: &[&dyn ToPath]) -> Vec<Value> {
    parts.iter().map(|x| x.to_path()).collect()
}

pub trait ToPath {
    fn to_path(&self) -> Value;
}
impl ToPath for &str {
    fn to_path(&self) -> Value {
        Value::from(*self)
    }
}
impl ToPath for usize {
    fn to_path(&self) -> Value {
        Value::from(*self)
    }
}

/// Status 400 `{ error: "Invalid body", issues }`.
pub fn invalid_body(issues: Vec<Issue>) -> Response {
    (StatusCode::BAD_REQUEST, axum::Json(json!({ "error": "Invalid body", "issues": issues }))).into_response()
}

/// Status 400 `{ error: issues }` (routes that put the issues straight into `error`).
pub fn issues_error(issues: Vec<Issue>) -> Response {
    (StatusCode::BAD_REQUEST, axum::Json(json!({ "error": issues }))).into_response()
}

/// First issue as "path: message" (the poll routes' `issueMessage`).
pub fn issue_message(issues: &[Issue], fallback: &str) -> String {
    let Some(i) = issues.first() else { return fallback.to_string() };
    if i.path.is_empty() {
        return i.message.clone();
    }
    let path: Vec<String> = i.path.iter().map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).collect();
    format!("{}: {}", path.join("."), i.message)
}
