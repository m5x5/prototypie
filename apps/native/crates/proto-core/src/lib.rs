//! proto-core: Prototypie's domain types and record store. See `docs/api-core.md`.
//!
//! - [`types`]: `Design`, `Space`, `Run`, `Lane`, … serde-compatible with the reference app's records.
//! - [`Store`]: one versioned state per data dir; lock-free [`Snapshot`] reads, batched writes (`apply`), change
//!   events (`subscribe`), atomic saves and the audit log on a writer thread.
//! - Queries ([`Snapshot::child_spaces`], [`Snapshot::project_of`], [`Snapshot::hub_of`], …).
//! - [`import_reference`]: one-time copy of the reference app's `data/state.json`.
//! - [`live_view`]: slot expansion for live views (`compose_live`); [`preview`]: html + content key for snapshots.
//! - [`ops`]: write helpers (move, create exploration/project, promote, delete space, flags) returning `Change`s.

pub mod live_view;
mod lock;
pub mod ops;
mod persist;
pub mod preview;
mod query;
mod record;
mod snapshot;
mod store;
pub mod types;

pub use persist::{
    default_data_dir, import_reference, import_reference_assets, public_dir, reference_state_path, AssetImport, ImportOutcome,
    LOG_FILE, REFERENCE_ASSET_DIRS, STATE_FILE,
};
pub use live_view::has_slots;
pub use lock::{lock_holder, StoreError, LOCK_FILE};
pub use record::{
    is_valid_path, split_path, ApplyError, Change, ChangeEvent, ChangeMeta, Cursor, RecordKind, StateFile, StoredRecord,
    SHARED_KEYS,
};
pub use preview::{PreviewInput, PreviewInputs};
pub use snapshot::{Entry, Snapshot, Typed};
pub use store::Store;
pub use types::*;

/// Re-exported so dependents name the same channel types.
pub use tokio::sync::broadcast;
