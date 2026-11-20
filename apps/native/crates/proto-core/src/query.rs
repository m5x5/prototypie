//! Read-only queries over a [`Snapshot`], ported from the reference app's helpers (named in each doc comment).
//! All are plain scans over the snapshot (a few thousand records); cache results per snapshot version if a
//! frame needs them repeatedly.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::snapshot::Snapshot;
use crate::types::{Design, DesignKind, Space, ROOT_SPACE_ID};

/// Upper bound when walking parent chains, so a cycle in bad data can't hang a query.
const MAX_DEPTH: usize = 64;

impl Snapshot {
    /// Spaces directly inside `space_id`, oldest first (the order portals are laid out in).
    pub fn child_spaces(&self, space_id: &str) -> Vec<Arc<Space>> {
        let mut out: Vec<_> = self.spaces().filter(|s| s.parent_space_id.as_deref() == Some(space_id)).collect();
        out.sort_by(|a, b| a.created_at.total_cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
        out
    }

    /// The projects on the main canvas, oldest first.
    pub fn projects(&self) -> Vec<Arc<Space>> {
        self.child_spaces(ROOT_SPACE_ID).into_iter().filter(|s| s.is_project()).collect()
    }

    /// Designs whose `space_id` is `space_id` (not those of nested spaces).
    pub fn designs_in<'a>(&'a self, space_id: &'a str) -> impl Iterator<Item = Arc<Design>> + 'a {
        self.designs().filter(move |d| d.space_id == space_id)
    }

    /// All spaces nested under `space_id`, at any depth (`descendantSpaceIds`).
    pub fn descendant_space_ids(&self, space_id: &str) -> Vec<String> {
        let mut by_parent: HashMap<&str, Vec<&str>> = HashMap::new();
        let spaces: Vec<_> = self.spaces().collect();
        for s in &spaces {
            if let Some(p) = s.parent_space_id.as_deref() {
                by_parent.entry(p).or_default().push(&s.id);
            }
        }
        let mut out = Vec::new();
        let mut seen = HashSet::from([space_id]);
        let mut stack = vec![space_id];
        while let Some(cur) = stack.pop() {
            for &child in by_parent.get(cur).map(Vec::as_slice).unwrap_or_default() {
                if seen.insert(child) {
                    out.push(child.to_string());
                    stack.push(child);
                }
            }
        }
        out
    }

    /// Designs in `space_id` and all of its nested spaces (`selectSubtreeDesigns`).
    pub fn subtree_designs(&self, space_id: &str) -> Vec<Arc<Design>> {
        let mut ids: HashSet<String> = self.descendant_space_ids(space_id).into_iter().collect();
        ids.insert(space_id.to_string());
        self.designs().filter(|d| ids.contains(&d.space_id)).collect()
    }

    /// Spaces from the main canvas down to `space_id`, root first (`spacePath`; the breadcrumb).
    pub fn space_path(&self, space_id: &str) -> Vec<Arc<Space>> {
        let mut path = self.ancestors_and_self(space_id);
        path.reverse();
        path
    }

    /// `space_id`'s space, then its parent, … up to the top.
    fn ancestors_and_self(&self, space_id: &str) -> Vec<Arc<Space>> {
        let mut out: Vec<Arc<Space>> = Vec::new();
        let mut cur = self.space(space_id);
        while let Some(s) = cur {
            if out.len() >= MAX_DEPTH || out.iter().any(|x| x.id == s.id) {
                break;
            }
            cur = s.parent_space_id.as_deref().and_then(|p| self.space(p));
            out.push(s);
        }
        out
    }

    /// The project a space belongs to: itself or its nearest ancestor with `kind: "project"`.
    pub fn project_of(&self, space_id: &str) -> Option<Arc<Space>> {
        self.ancestors_and_self(space_id).into_iter().find(|s| s.is_project())
    }

    /// The local folder a space belongs to: its own `localPath`, else the nearest ancestor's (`linkedPath`).
    /// The reference also falls back to matching the space's name against the project list; that lookup lives
    /// outside the core (it needs private-projects.ttl and the file system).
    pub fn local_path_of(&self, space_id: &str) -> Option<String> {
        self.ancestors_and_self(space_id).into_iter().find_map(|s| s.local_path.clone())
    }

    /// The hub page a design belongs to (`hubOf`): walking up `parentId`, the first page, or for a spoke the first
    /// non-spoke ancestor.
    pub fn hub_of(&self, design_id: &str) -> Option<Arc<Design>> {
        let start = self.design(design_id)?;
        let from_spoke = start.kind == DesignKind::Spoke;
        let mut cur = start.parent_id.as_deref().and_then(|p| self.design(p));
        for _ in 0..50 {
            let d = cur?;
            if d.kind == DesignKind::Page || (from_spoke && d.kind != DesignKind::Spoke) {
                return Some(d);
            }
            cur = d.parent_id.as_deref().and_then(|p| self.design(p));
        }
        None
    }

    /// Designs derived from `design_id` (its `parentId` children), oldest first.
    pub fn children_of(&self, design_id: &str) -> Vec<Arc<Design>> {
        let mut out: Vec<_> = self.designs().filter(|d| d.parent_id.as_deref() == Some(design_id)).collect();
        out.sort_by(|a, b| a.created_at.total_cmp(&b.created_at));
        out
    }

    /// The main variant live views show for a component (`mainVariant`): among the design and its variations (its
    /// `parentId` subtree, skipping derived live views and their branches), the ready one with the newest
    /// `mainAt`; else the design itself.
    pub fn main_variant(&self, design_id: &str) -> Option<Arc<Design>> {
        crate::live_view::Lineage::new(self).main_variant(design_id)
    }

    /// The live copy of `design_id` in its parent space, if it was promoted (`findPromotedCopy`).
    pub fn promoted_copy_of(&self, design_id: &str) -> Option<Arc<Design>> {
        let src = self.design(design_id)?;
        let parent = self.space(&src.space_id)?.parent_space_id.clone()?;
        self.live(crate::RecordKind::PromotedFrom)
            .filter(|(_, e)| matches!(e.typed(), crate::Typed::PromotedFrom(r) if r.design_id == design_id))
            .find_map(|(copy_id, _)| self.design(copy_id).filter(|c| c.space_id == parent))
    }

    /// Ids of designs that have been promoted (the originals, not the copies).
    pub fn promoted_design_ids(&self) -> HashSet<String> {
        self.promoted_refs().map(|r| r.design_id.clone()).collect()
    }

    /// Ids of designs marked reviewed.
    pub fn reviewed_ids(&self) -> HashSet<String> {
        self.live(crate::RecordKind::ReviewedIds)
            .filter(|(_, e)| matches!(e.typed(), crate::Typed::Reviewed(true)))
            .map(|(id, _)| id.to_string())
            .collect()
    }

    /// The guidelines card that applies in `space_id` (`guidelinesFor`): the space's own card marked with
    /// `guidelinesAt` (ready, with html; newest mark wins), else the nearest ancestor's.
    pub fn guidelines_for(&self, space_id: &str) -> Option<Arc<Design>> {
        let mut by_space: HashMap<String, Arc<Design>> = HashMap::new();
        for d in self.designs() {
            let Some(at) = d.guidelines_at else { continue };
            if !d.is_ready() || d.html.is_empty() {
                continue;
            }
            match by_space.get(&d.space_id) {
                Some(cur) if cur.guidelines_at.unwrap_or(0.0) >= at => {}
                _ => {
                    by_space.insert(d.space_id.clone(), d);
                }
            }
        }
        self.ancestors_and_self(space_id).into_iter().find_map(|s| by_space.get(&s.id).cloned())
    }
}
