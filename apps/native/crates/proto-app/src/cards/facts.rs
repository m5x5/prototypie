//! What a card shows beyond its own record, derived from a proto-core snapshot (the reference's
//! per-node store selectors). Pure functions over `Snapshot`, so cards build without touching the
//! store and tests use an in-memory one.

use std::collections::HashSet;

use proto_core::{Design, DesignKind, DesignStatus, RecordKind, Snapshot, Space, Typed};


/// Where a promoted copy came from (the "from {space}" badge).
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct Source {
    /// The source space, if it still exists.
    pub space_id: Option<String>,
    pub space_name: String,
    pub design_id: String,
}

/// Facts for one design card.
#[derive(Clone, Debug, Default, PartialEq, Hash)]
pub struct DesignFacts {
    /// Ready designs with `parentId == id` (Compare (N)).
    pub ready_children: usize,
    /// Unresolved Annotate notes.
    pub open_notes: usize,
    /// The parent space's name when designs here can be promoted (parent exists and isn't root).
    pub promote_to: Option<String>,
    /// This design's promoted copy in the parent space.
    pub promoted_copy: Option<String>,
    /// Set when this design is itself a promoted copy.
    pub source: Option<Source>,
    /// A live view places this card (or one of its variations): the Main toggle shows.
    pub in_live_view: bool,
    /// For a spoke whose hub exists: whether the hub is ready (Merge instead of Spokes).
    pub hub_ready: Option<bool>,
    /// The space's name (guidelines tooltip).
    pub space_name: String,
}

pub fn design_facts(snap: &Snapshot, d: &Design, live_lineage: &HashSet<String>) -> DesignFacts {
    let space = snap.space(&d.space_id);
    let parent = space.as_ref().and_then(|s| s.parent_space_id.clone());
    let promote_to = proto_core::ops::can_promote_to(parent.as_deref())
        .then(|| parent.as_deref().and_then(|p| snap.space(p)))
        .flatten()
        .map(|s| s.name.clone());
    let source = snap.promoted_from(&d.id).map(|r| {
        let live = snap.space(&r.space_id);
        Source { space_id: live.as_ref().map(|s| s.id.clone()), space_name: live.map_or_else(|| r.space_name.clone(), |s| s.name.clone()), design_id: r.design_id.clone() }
    });
    DesignFacts {
        ready_children: snap.children_of(&d.id).iter().filter(|c| c.status == DesignStatus::Ready).count(),
        open_notes: snap.notes(&d.id).map_or(0, |n| n.iter().filter(|x| x.resolved != Some(true)).count()),
        promote_to,
        promoted_copy: snap.promoted_copy_of(&d.id).map(|c| c.id.clone()),
        source,
        in_live_view: live_lineage.contains(&d.id),
        hub_ready: (d.kind == DesignKind::Spoke).then(|| snap.hub_of(&d.id)).flatten().map(|h| h.is_ready()),
        space_name: space.map_or_else(|| "this space".to_string(), |s| s.name.clone()),
    }
}

/// Facts for one project/exploration card (docs/spec-cards.md §3).
#[derive(Clone, Debug, Default, PartialEq, Hash)]
pub struct PortalFacts {
    pub project: bool,
    /// Local folder (own `localPath`, else the nearest ancestor's), `/Users/<name>` shown as `~`.
    pub local_path: Option<String>,
    pub designs: usize,
    pub starred: usize,
    pub promoted: usize,
    pub generating: usize,
    pub nested: usize,
    /// The hero design's id, if one is ready.
    pub hero: Option<String>,
    pub hero_starred: bool,
    /// The pool has designs but none is ready yet (shimmer).
    pub designing: bool,
    /// Own designs, not generating, created after `since` (ms).
    pub new_count: usize,
}

/// `left_at`: when this device last left the space (ms), else `None` (then "new" = the last 24 h of `now`).
pub fn portal_facts(snap: &Snapshot, sp: &Space, promoted_ids: &HashSet<String>, left_at: Option<f64>, now: f64) -> PortalFacts {
    let project = sp.is_project();
    let subtree = snap.subtree_designs(&sp.id);
    let own: Vec<_> = subtree.iter().filter(|d| d.space_id == sp.id).cloned().collect();
    let pool = if project { &subtree } else { &own };

    // Hero refs, newest promotion first: a project's promoted copies living in it; an exploration's
    // designs promoted out of it.
    let mut refs: Vec<(f64, String)> = snap
        .live(RecordKind::PromotedFrom)
        .filter_map(|(copy_id, e)| match e.typed() {
            Typed::PromotedFrom(r) => Some((copy_id, r.clone())),
            _ => None,
        })
        .filter_map(|(copy_id, r)| {
            if project {
                own.iter().any(|d| d.id == copy_id).then(|| (r.at, copy_id.to_string()))
            } else {
                (r.space_id == sp.id).then(|| (r.at, r.design_id.clone()))
            }
        })
        .collect();
    refs.sort_by(|a, b| b.0.total_cmp(&a.0));
    let ready = |id: &str| pool.iter().find(|d| d.id == id && d.is_ready()).cloned();
    let hero = refs.iter().find_map(|(_, id)| ready(id)).or_else(|| {
        pool.iter().filter(|d| d.is_ready()).max_by(|a, b| a.created_at.total_cmp(&b.created_at)).cloned()
    });

    const DAY: f64 = 24.0 * 60.0 * 60.0 * 1000.0;
    PortalFacts {
        project,
        local_path: snap.local_path_of(&sp.id).map(|p| tilde(&p)),
        designs: subtree.len(),
        starred: subtree.iter().filter(|d| d.is_starred()).count(),
        promoted: subtree.iter().filter(|d| promoted_ids.contains(&d.id)).count(),
        generating: subtree.iter().filter(|d| d.status == DesignStatus::Generating).count(),
        nested: snap.descendant_space_ids(&sp.id).len(),
        hero_starred: hero.as_ref().is_some_and(|h| h.is_starred()),
        designing: hero.is_none() && !pool.is_empty(),
        hero: hero.map(|h| h.id.clone()),
        new_count: own
            .iter()
            .filter(|d| d.status != DesignStatus::Generating && left_at.map_or(now - d.created_at < DAY, |t| d.created_at > t))
            .count(),
    }
}

/// Child spaces of `space` with their portal positions (world). A portal without a stored position
/// (a space created elsewhere) sits right of everything (core's free-spot rule), one slot per such
/// portal, so they never overlap the cards or each other. Pure: stable from frame to frame.
pub fn portals(snap: &Snapshot, space: &str) -> Vec<(std::sync::Arc<Space>, [f32; 2])> {
    let spot = proto_core::ops::free_portal_spot(snap, space);
    let mut missing = 0usize;
    snap.child_spaces(space)
        .into_iter()
        .map(|sp| {
            let pos = match snap.portal(&sp.id) {
                Some(p) => [p.x as f32, p.y as f32],
                None => {
                    let x = spot.x as f32 + missing as f32 * (super::theme::PORTAL_W + 80.0);
                    missing += 1;
                    [x, spot.y as f32]
                }
            };
            (sp, pos)
        })
        .collect()
}

/// `/Users/<name>/…` or `/home/<name>/…` → `~/…` (the reference's `tildePath`).
pub fn tilde(path: &str) -> String {
    for root in ["/Users/", "/home/"] {
        if let Some(rest) = path.strip_prefix(root) {
            return match rest.find('/') {
                Some(i) => format!("~{}", &rest[i..]),
                None => "~".to_string(),
            };
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::tilde;

    #[test]
    fn tilde_shortens_home() {
        assert_eq!(tilde("/Users/example/Software/x"), "~/Software/x");
        assert_eq!(tilde("/home/me"), "~");
        assert_eq!(tilde("/opt/x"), "/opt/x");
    }
}
