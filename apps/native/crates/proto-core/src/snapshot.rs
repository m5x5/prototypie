//! An immutable view of the whole state at one version. Cheap to clone (`Arc<Snapshot>`), safe to read from
//! any thread without locking. Raw JSON is the canonical form (lossless); typed views are parsed on first use and
//! cached per record.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use serde_json::Value;

use crate::record::{split_path, RecordKind, StateFile, StoredRecord};
use crate::types::{Design, DesignNote, Lane, Point, PromotedRef, PublishedPoll, Run, Space};

/// A typed record. `Invalid` when the stored JSON doesn't fit the type (the raw value is still kept and served).
#[derive(Debug, Clone)]
pub enum Typed {
    Space(Arc<Space>),
    Design(Arc<Design>),
    Run(Arc<Run>),
    Portal(Point),
    PromotedFrom(Arc<PromotedRef>),
    SeededFrom(Arc<str>),
    Notes(Arc<Vec<DesignNote>>),
    Reviewed(bool),
    Lane(Arc<Lane>),
    SpokeFocus(Arc<str>),
    PublishedPoll(Arc<PublishedPoll>),
    PollWinner(Arc<str>),
    Invalid,
}

impl Typed {
    fn parse(kind: RecordKind, v: &Value) -> Typed {
        fn de<T: serde::de::DeserializeOwned>(v: &Value) -> Option<T> {
            T::deserialize(v).ok()
        }
        let s = |v: &Value| v.as_str().map(Arc::<str>::from);
        let t = match kind {
            RecordKind::Spaces => de(v).map(|x| Typed::Space(Arc::new(x))),
            RecordKind::Designs => de(v).map(|x| Typed::Design(Arc::new(x))),
            RecordKind::Runs => de(v).map(|x| Typed::Run(Arc::new(x))),
            RecordKind::Portals => de(v).map(Typed::Portal),
            RecordKind::PromotedFrom => de(v).map(|x| Typed::PromotedFrom(Arc::new(x))),
            RecordKind::SeededFrom => s(v).map(Typed::SeededFrom),
            RecordKind::Notes => de(v).map(|x| Typed::Notes(Arc::new(x))),
            RecordKind::ReviewedIds => v.as_bool().map(Typed::Reviewed),
            RecordKind::Lanes => de(v).map(|x| Typed::Lane(Arc::new(x))),
            RecordKind::SpokeFocus => s(v).map(Typed::SpokeFocus),
            RecordKind::PublishedPolls => de(v).map(|x| Typed::PublishedPoll(Arc::new(x))),
            RecordKind::PollWinners => s(v).map(Typed::PollWinner),
        };
        t.unwrap_or(Typed::Invalid)
    }
}

/// One record at this version.
#[derive(Debug)]
pub struct Entry {
    pub kind: RecordKind,
    /// The version that last wrote this record.
    pub rev: u64,
    /// None for a tombstone.
    pub value: Option<Arc<Value>>,
    typed: OnceLock<Typed>,
}

impl Entry {
    pub(crate) fn new(kind: RecordKind, rev: u64, value: Option<Arc<Value>>) -> Entry {
        Entry { kind, rev, value, typed: OnceLock::new() }
    }

    pub fn is_deleted(&self) -> bool {
        self.value.is_none()
    }

    /// The typed view (parsed once). `Typed::Invalid` for tombstones and values that don't fit.
    pub fn typed(&self) -> &Typed {
        self.typed.get_or_init(|| match &self.value {
            Some(v) => Typed::parse(self.kind, v),
            None => Typed::Invalid,
        })
    }

    pub fn stored(&self) -> StoredRecord {
        StoredRecord { rev: self.rev, value: self.value.clone(), deleted: self.value.is_none() }
    }
}

/// The whole state at one version.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub epoch: Arc<str>,
    pub version: u64,
    /// By path, in path order (so all records of a kind are one contiguous range). Tombstones included.
    pub(crate) records: BTreeMap<Arc<str>, Arc<Entry>>,
}

macro_rules! typed_getters {
    ($($(#[$m:meta])* $one:ident, $all:ident, $kind:ident, $variant:ident, $t:ty;)*) => {$(
        $(#[$m])*
        pub fn $one(&self, id: &str) -> Option<$t> {
            match self.entry(RecordKind::$kind, id)?.typed() {
                Typed::$variant(x) => Some(x.clone()),
                _ => None,
            }
        }
        /// All live, well-formed records of this kind, in id order.
        pub fn $all(&self) -> impl Iterator<Item = $t> + '_ {
            self.live(RecordKind::$kind).filter_map(|(_, e)| match e.typed() {
                Typed::$variant(x) => Some(x.clone()),
                _ => None,
            })
        }
    )*};
}

impl Snapshot {
    pub(crate) fn empty(epoch: Arc<str>) -> Snapshot {
        Snapshot { epoch, version: 0, records: BTreeMap::new() }
    }

    pub(crate) fn from_file(file: StateFile) -> (Snapshot, usize) {
        let mut records = BTreeMap::new();
        let mut stray = 0;
        for (path, r) in file.records {
            let Some((kind, _)) = split_path(&path) else {
                // Same as the reference: records of keys that aren't shared are dropped on load.
                stray += 1;
                continue;
            };
            let value = if r.deleted { None } else { r.value };
            records.insert(Arc::from(path), Arc::new(Entry::new(kind, r.rev, value)));
        }
        (Snapshot { epoch: Arc::from(file.epoch), version: file.version, records }, stray)
    }

    /// Records changed after `since` (everything, tombstones included, for `since == 0` or another epoch),
    /// shaped like the reference's `GET /api/state`.
    pub fn read(&self, since: u64, epoch: Option<&str>) -> StateFile {
        let full = since == 0 || epoch.is_some_and(|e| e != &*self.epoch);
        let records = self
            .records
            .iter()
            .filter(|(_, e)| full || e.rev > since)
            .map(|(p, e)| (p.to_string(), e.stored()))
            .collect();
        StateFile { epoch: self.epoch.to_string(), version: self.version, records }
    }

    /// Any record by path, tombstones included.
    pub fn get(&self, path: &str) -> Option<&Arc<Entry>> {
        self.records.get(path)
    }

    /// A live record's raw value.
    pub fn value(&self, path: &str) -> Option<&Arc<Value>> {
        self.records.get(path)?.value.as_ref()
    }

    pub fn entry(&self, kind: RecordKind, id: &str) -> Option<&Arc<Entry>> {
        self.records.get(kind.path(id).as_str()).filter(|e| !e.is_deleted())
    }

    /// Live records of one kind as `(id, entry)`, in id order.
    pub fn live(&self, kind: RecordKind) -> impl Iterator<Item = (&str, &Arc<Entry>)> + '_ {
        let prefix = format!("{}/", kind.as_str());
        let start: Arc<str> = Arc::from(prefix.as_str());
        self.records
            .range(start..)
            .take_while(move |(p, _)| p.starts_with(&prefix))
            .filter(|(_, e)| !e.is_deleted())
            .map(|(p, e)| (&p[p.find('/').map_or(0, |i| i + 1)..], e))
    }

    /// Number of records, tombstones included.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    typed_getters! {
        space, spaces, Spaces, Space, Arc<Space>;
        design, designs, Designs, Design, Arc<Design>;
        run, runs, Runs, Run, Arc<Run>;
        /// Portal position of a nested space on its parent canvas.
        portal, portals, Portals, Portal, Point;
        /// Provenance of a promoted copy, by the copy's id.
        promoted_from, promoted_refs, PromotedFrom, PromotedFrom, Arc<PromotedRef>;
        /// The original a seed copy was made from, by the seed's id.
        seeded_from, seeds, SeededFrom, SeededFrom, Arc<str>;
        /// Annotate notes on a design, oldest first.
        notes, all_notes, Notes, Notes, Arc<Vec<DesignNote>>;
        lane, lanes, Lanes, Lane, Arc<Lane>;
        spoke_focus, spoke_focuses, SpokeFocus, SpokeFocus, Arc<str>;
        published_poll, published_polls, PublishedPolls, PublishedPoll, Arc<PublishedPoll>;
        poll_winner, poll_winners, PollWinners, PollWinner, Arc<str>;
    }

    pub fn is_reviewed(&self, design_id: &str) -> bool {
        matches!(self.entry(RecordKind::ReviewedIds, design_id).map(|e| e.typed()), Some(Typed::Reviewed(true)))
    }
}
