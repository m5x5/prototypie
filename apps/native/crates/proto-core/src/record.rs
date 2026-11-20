//! Record paths, the on-disk/on-wire state format and the change types.
//!
//! A record is addressed by `"<kind>/<id>"`, e.g. `designs/abc123`. Only the shared kinds of the reference app
//! (`src/store/key-policy.ts`) are valid. Each write bumps a global `version`; each record remembers the version
//! (`rev`) that last touched it. Deletes stay as tombstones (`{rev, deleted: true}`).

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The record kinds that are shared (persisted and synced), in the reference's spelling.
pub const SHARED_KEYS: [&str; 12] = [
    "spaces",
    "designs",
    "runs",
    "portals",
    "promotedFrom",
    "seededFrom",
    "notes",
    "reviewedIds",
    "lanes",
    "spokeFocus",
    "publishedPolls",
    "pollWinners",
];

/// One of [`SHARED_KEYS`], typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RecordKind {
    /// `Space`
    Spaces,
    /// `Design`
    Designs,
    /// `Run`
    Runs,
    /// `Point`: portal position on the parent canvas, keyed by the nested space's id.
    Portals,
    /// `PromotedRef`, keyed by the promoted copy's id.
    PromotedFrom,
    /// String: the original design id, keyed by the seed copy's id.
    SeededFrom,
    /// `Vec<DesignNote>`, keyed by design id.
    Notes,
    /// `true`, keyed by design id.
    ReviewedIds,
    /// `Lane`, keyed by job id.
    Lanes,
    /// String: the component a spoke design explores, keyed by design id.
    SpokeFocus,
    /// `PublishedPoll`
    PublishedPolls,
    /// String: winning option id, keyed by `<pollId>…` as the reference writes it.
    PollWinners,
}

impl RecordKind {
    pub const ALL: [RecordKind; 12] = [
        RecordKind::Spaces,
        RecordKind::Designs,
        RecordKind::Runs,
        RecordKind::Portals,
        RecordKind::PromotedFrom,
        RecordKind::SeededFrom,
        RecordKind::Notes,
        RecordKind::ReviewedIds,
        RecordKind::Lanes,
        RecordKind::SpokeFocus,
        RecordKind::PublishedPolls,
        RecordKind::PollWinners,
    ];

    pub fn as_str(self) -> &'static str {
        SHARED_KEYS[self as usize]
    }

    pub fn parse(key: &str) -> Option<RecordKind> {
        SHARED_KEYS.iter().position(|k| *k == key).map(|i| RecordKind::ALL[i])
    }

    /// `"<kind>/<id>"`.
    pub fn path(self, id: &str) -> String {
        format!("{}/{}", self.as_str(), id)
    }
}

/// Splits a valid record path into its kind and id.
pub fn split_path(path: &str) -> Option<(RecordKind, &str)> {
    if path.len() >= 512 {
        return None;
    }
    let (key, id) = path.split_once('/')?;
    if id.is_empty() {
        return None;
    }
    Some((RecordKind::parse(key)?, id))
}

/// Same rule as the reference's `isValidPath`: `<shared kind>/<non-empty id>`, shorter than 512 chars.
pub fn is_valid_path(path: &str) -> bool {
    split_path(path).is_some()
}

/// One record as stored and served: `{rev, value}` or `{rev, deleted: true}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredRecord {
    pub rev: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Arc<Value>>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
}

/// `data/state.json` and the body of `GET /api/state`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateFile {
    /// Changes when the state is recreated: a client holding a cursor of another epoch must resync fully.
    pub epoch: String,
    pub version: u64,
    pub records: BTreeMap<String, StoredRecord>,
}

/// Where a client is: the state's epoch and the version it has seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub epoch: String,
    pub version: u64,
}

/// One write in a batch: set `path` to `value`, or delete it.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub path: String,
    /// The new value; ignored when `deleted`.
    pub value: Option<Value>,
    pub deleted: bool,
}

impl Change {
    pub fn set(path: impl Into<String>, value: Value) -> Change {
        Change { path: path.into(), value: Some(value), deleted: false }
    }

    pub fn delete(path: impl Into<String>) -> Change {
        Change { path: path.into(), value: None, deleted: true }
    }

    /// Set a typed record, e.g. `Change::put(RecordKind::Designs, &d.id, &d)`.
    pub fn put<T: Serialize>(kind: RecordKind, id: &str, value: &T) -> serde_json::Result<Change> {
        Ok(Change::set(kind.path(id), serde_json::to_value(value)?))
    }
}

/// Who sent a batch, for the audit log.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChangeMeta {
    pub client_id: Option<String>,
    /// Free-form tag for why the batch was sent, e.g. "agent-cards".
    pub reason: Option<String>,
    pub user_agent: Option<String>,
}

impl ChangeMeta {
    pub fn client(client_id: impl Into<String>) -> ChangeMeta {
        ChangeMeta { client_id: Some(client_id.into()), ..ChangeMeta::default() }
    }

    pub fn reason(mut self, reason: impl Into<String>) -> ChangeMeta {
        self.reason = Some(reason.into());
        self
    }
}

/// Sent to subscribers after every accepted batch.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeEvent {
    pub epoch: String,
    pub version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// The paths this batch touched (not part of the reference's SSE payload).
    #[serde(skip)]
    pub paths: Arc<[String]>,
}

/// Why a batch was rejected. Nothing of a rejected batch is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// Not `<shared kind>/<id>`; same message as the reference's 400.
    InvalidPath(String),
    /// A set without a value.
    MissingValue(String),
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApplyError::InvalidPath(p) => write!(f, "Invalid record path (not a shared store key): {p}"),
            ApplyError::MissingValue(p) => write!(f, "Missing value for {p}"),
        }
    }
}

impl std::error::Error for ApplyError {}
