//! The shared-state seam: proto-core's Store (docs/api-core.md), plus small helpers the routes use.

use std::collections::BTreeMap;

use serde_json::Value;

pub use proto_core::{Change, ChangeMeta, StateFile, Store};

pub type SharedStore = Store;

/// Server-side writes: `clientId` + `reason`.
pub fn meta(client_id: &str, reason: impl Into<String>) -> ChangeMeta {
    ChangeMeta::client(client_id).reason(reason)
}

/// Live values under `<key>/`, keyed by id (cloned out of the snapshot).
pub fn live_map(state: &StateFile, key: &str) -> BTreeMap<String, Value> {
    let prefix = format!("{key}/");
    state
        .records
        .iter()
        .filter(|(_, r)| !r.deleted)
        .filter_map(|(p, r)| Some((p.strip_prefix(&prefix)?.to_string(), Value::clone(r.value.as_ref()?))))
        .collect()
}

/// The live value at `path`.
pub fn live_value(state: &StateFile, path: &str) -> Option<Value> {
    let r = state.records.get(path)?;
    if r.deleted { None } else { r.value.as_deref().cloned() }
}

/// Whether `path` is a tombstone.
pub fn is_deleted(state: &StateFile, path: &str) -> bool {
    state.records.get(path).is_some_and(|r| r.deleted)
}
