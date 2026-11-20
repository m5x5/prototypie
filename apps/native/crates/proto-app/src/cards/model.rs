//! The active space's cards, computed once per (snapshot version, space) in `cards::poll`, not in the
//! view: the facts scan the whole snapshot (thousands of records), and the view re-runs on every
//! commit. Camera motion never touches it.

use std::collections::HashSet;
use std::sync::Arc;

use std::hash::{DefaultHasher, Hash, Hasher};

use proto_core::{Design, RecordKind, Snapshot, Space};

use super::facts::{self, DesignFacts, PortalFacts};

/// "N new" depends on the clock; refresh at most this often while nothing else changes.
const REFRESH_S: f64 = 30.0;

fn hash_of(v: &impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

pub struct DesignEntry {
    pub design: Arc<Design>,
    pub facts: DesignFacts,
    /// Changes when the record or its facts change: the card's memo version.
    pub version: u64,
}

pub struct PortalEntry {
    /// Changes when the space record, its position or its facts change: the card's memo version.
    pub version: u64,
    pub space: Arc<Space>,
    pub pos: [f32; 2],
    pub facts: PortalFacts,
    /// The hero design's layout width (for the picture's scale rule).
    pub hero_vw: f32,
    /// The hero design's fixed screen height, if any.
    pub hero_vh: Option<f64>,
}

pub struct Model {
    pub version: u64,
    pub space: String,
    built_at: f64,
    pub designs: Vec<DesignEntry>,
    pub portals: Vec<PortalEntry>,
}

impl Model {
    pub fn build(snap: &Snapshot, space: &str, left_at: impl Fn(&str) -> Option<f64>, clock: f64) -> Model {
        let now = super::now_ms();
        let promoted = snap.promoted_design_ids();
        let lineage: HashSet<String> = proto_core::live_view::slot_lineage(snap);
        let portals = facts::portals(snap, space)
            .into_iter()
            .map(|(sp, pos)| {
                let f = facts::portal_facts(snap, &sp, &promoted, left_at(&sp.id), now);
                let hero = f.hero.as_ref().and_then(|h| snap.design(h));
                let rev = snap.entry(RecordKind::Spaces, &sp.id).map_or(0, |e| e.rev);
                PortalEntry {
                    version: hash_of(&(rev, pos.map(f32::to_bits), &f)),
                    hero_vw: hero.as_ref().map_or(1280.0, |h| h.viewport.unwrap_or(1280.0) as f32),
                    hero_vh: hero.as_ref().and_then(|h| h.viewport_height),
                    space: sp,
                    pos,
                    facts: f,
                }
            })
            .collect();
        let designs = snap
            .designs_in(space)
            .map(|d| {
                let facts = facts::design_facts(snap, &d, &lineage);
                let rev = snap.entry(RecordKind::Designs, &d.id).map_or(0, |e| e.rev);
                DesignEntry { version: hash_of(&(rev, &facts)), facts, design: d }
            })
            .collect();
        Model { version: snap.version, space: space.to_string(), built_at: clock, designs, portals }
    }

    /// Whether the model still describes `space` at `version` (and "N new" is fresh enough).
    pub fn fresh(&self, version: u64, space: &str, clock: f64) -> bool {
        self.version == version && self.space == space && clock - self.built_at < REFRESH_S
    }
}
