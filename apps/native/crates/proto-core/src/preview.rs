//! What a card's picture is rendered from: the html to load and a content key for snapshot caches.
//!
//! `proto_web::Snapshots::request(id, rev, html, spec)` re-renders when `rev` changes (compared by equality). The
//! record's own rev is the wrong key: it changes on every drag, star or rename. [`PreviewInputs::get`] returns a
//! key that changes only when what the picture shows changes:
//! - a plain card: a hash of its html, recomputed only when the record's rev moves;
//! - a live view (`<prototypie-slot>`): [`live_view::fingerprint`] (its html plus every slotted component's main
//!   variant), recomposed only when that changes, and the composed html with each component wrapped by `wrap`.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::live_view::{self, has_slots};
use crate::record::RecordKind;
use crate::snapshot::{Snapshot, Typed};

/// Adjusts each slotted component's document before it goes into the srcdoc (thumbnails: freeze animations).
pub type Wrap = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// The html to render and its content key (use it as proto-web's `rev`).
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewInput {
    pub html: Arc<str>,
    pub key: u64,
}

struct Cached {
    /// Record rev the entry was computed at.
    rev: u64,
    /// Snapshot version a live view was checked at (live views depend on other records).
    version: u64,
    live: bool,
    input: PreviewInput,
}

/// Per-card memo of [`PreviewInput`]s. Bounded (oldest first out). One per render path (thumbnails, fullscreen),
/// since `wrap` is part of what is composed. Not thread-safe by itself.
pub struct PreviewInputs {
    wrap: Option<Wrap>,
    cap: usize,
    entries: HashMap<String, Cached>,
    order: VecDeque<String>,
}

impl PreviewInputs {
    /// `wrap`: e.g. `Some(Arc::new(|h| proto_web::spec::for_preview(h, false)))` for thumbnails (the reference's
    /// `composeLiveCached(…, h => forPreview(h, false))`), `None` for the interactive fullscreen view.
    pub fn new(wrap: Option<Wrap>, cap: usize) -> PreviewInputs {
        PreviewInputs { wrap, cap: cap.max(1), entries: HashMap::new(), order: VecDeque::new() }
    }

    /// The input for `design_id` in `snap`; None if the design doesn't exist. Cheap when nothing changed.
    pub fn get(&mut self, snap: &Snapshot, design_id: &str) -> Option<PreviewInput> {
        let entry = snap.entry(RecordKind::Designs, design_id)?;
        let Typed::Design(d) = entry.typed() else { return None };
        if let Some(c) = self.entries.get(design_id) {
            let fresh = if c.live { c.version == snap.version } else { c.rev == entry.rev };
            if fresh {
                return Some(c.input.clone());
            }
        }
        let live = has_slots(&d.html);
        let input = if live {
            let key = live_view::fingerprint(&d.html, snap);
            match self.entries.get(design_id) {
                // A drag elsewhere moved the version but no component changed: keep the composed html.
                Some(c) if c.live && c.input.key == key => c.input.clone(),
                _ => {
                    let wrap = self.wrap.clone();
                    let w = wrap.as_ref().map(|w| w.as_ref() as &dyn Fn(&str) -> String);
                    PreviewInput { html: Arc::from(live_view::compose_live(&d.html, snap, w)), key }
                }
            }
        } else {
            match self.entries.get(design_id) {
                // Same html at a new rev (moved, starred, renamed): same key, no copy.
                Some(c) if !c.live && *c.input.html == *d.html => c.input.clone(),
                _ => {
                    let mut h = DefaultHasher::new();
                    d.html.hash(&mut h);
                    PreviewInput { html: Arc::from(d.html.as_str()), key: h.finish() }
                }
            }
        };
        let cached = Cached { rev: entry.rev, version: snap.version, live, input: input.clone() };
        if self.entries.insert(design_id.to_string(), cached).is_none() {
            self.order.push_back(design_id.to_string());
            while self.order.len() > self.cap {
                if let Some(old) = self.order.pop_front() {
                    self.entries.remove(&old);
                }
            }
        }
        Some(input)
    }

    /// Forget a card (deleted design).
    pub fn invalidate(&mut self, design_id: &str) {
        if self.entries.remove(design_id).is_some() {
            self.order.retain(|id| id != design_id);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
