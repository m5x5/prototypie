//! Write helpers: each returns the `Change`s for one `Store::apply` (one version, one undo step for the caller),
//! computed from a snapshot. Pure: ids and times come from the caller-visible [`new_id`] / [`now_ms`], placement
//! follows the reference's `src/features/nested/slice.ts` (`findFreeSpot`, `defaultSpot`) with its node sizes.

use std::collections::HashSet;
use std::sync::Arc;

use serde_json::json;

use crate::record::{Change, RecordKind};
use crate::snapshot::Snapshot;
use crate::types::{Design, Point, PromotedRef, Space, SpaceKind, ROOT_SPACE_ID};

/// Approximate size of a design card on the canvas (reference `NODE_W` / `NODE_H`), used for layout.
pub const NODE_W: f64 = 380.0;
pub const NODE_H: f64 = 420.0;
/// Approximate size of a portal (space) card (reference `PORTAL_W` / `PORTAL_H`).
pub const PORTAL_W: f64 = 300.0;
pub const PORTAL_H: f64 = 330.0;
const GAP: f64 = 40.0;

/// Milliseconds since the Unix epoch, as the reference stores times.
pub fn now_ms() -> f64 {
    crate::persist::now_ms() as f64
}

/// A random id like the reference's `nanoid(len)` (alphabet `A-Za-z0-9_-`): 8 for spaces/runs, 10 for designs.
pub fn new_id(len: usize) -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
    let mut out = String::with_capacity(len);
    let mut bits = 0u64;
    let mut left = 0;
    for i in 0..len {
        if left < 6 {
            let mut h = RandomState::new().build_hasher();
            h.write_u128(crate::persist::now_ms());
            h.write_usize(i);
            bits = h.finish();
            left = 64;
        }
        out.push(ALPHABET[(bits & 63) as usize] as char);
        bits >>= 6;
        left -= 6;
    }
    out
}

fn put<T: serde::Serialize>(kind: RecordKind, id: &str, v: &T) -> Change {
    Change::put(kind, id, v).expect("domain types serialise")
}

// ---------------------------------------------------------------------------------------------
// Placement

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Boxes taken in a space: its designs and the portals of its child spaces.
fn occupied(s: &Snapshot, space_id: &str) -> Vec<Rect> {
    let mut out: Vec<Rect> = s.designs_in(space_id).map(|d| Rect { x: d.position.x, y: d.position.y, w: NODE_W, h: NODE_H }).collect();
    for c in s.child_spaces(space_id) {
        if let Some(p) = s.portal(&c.id) {
            out.push(Rect { x: p.x, y: p.y, w: PORTAL_W, h: PORTAL_H });
        }
    }
    out
}

/// Right of everything in the space, aligned with the top-most box (origin for an empty space).
fn default_spot(rects: &[Rect]) -> Point {
    if rects.is_empty() {
        return Point::default();
    }
    let right = rects.iter().map(|r| r.x + r.w).fold(f64::MIN, f64::max);
    let top = rects.iter().map(|r| r.y).fold(f64::MAX, f64::min);
    Point { x: right + 120.0, y: top }
}

enum Mode {
    Right,
    Grid,
}

/// First spot near `start` where a `w × h` box doesn't overlap anything (with a gap).
fn find_free_spot(rects: &[Rect], start: Point, w: f64, h: f64, mode: Mode) -> Point {
    let hits = |p: Point| rects.iter().any(|r| p.x < r.x + r.w + GAP && p.x + w + GAP > r.x && p.y < r.y + r.h + GAP && p.y + h + GAP > r.y);
    match mode {
        Mode::Right => (0..40).map(|i| Point { x: start.x + i as f64 * (w + GAP), y: start.y }).find(|p| !hits(*p)),
        Mode::Grid => (0..20)
            .flat_map(|row| (0..4).map(move |col| (row, col)))
            .map(|(row, col)| Point { x: start.x + col as f64 * (w + GAP), y: start.y + row as f64 * (h + GAP) })
            .find(|p| !hits(*p)),
    }
    .unwrap_or(start)
}

/// Where a new portal goes in `space_id` (right of everything), e.g. for a space created elsewhere.
pub fn free_portal_spot(s: &Snapshot, space_id: &str) -> Point {
    let rects = occupied(s, space_id);
    find_free_spot(&rects, default_spot(&rects), PORTAL_W, PORTAL_H, Mode::Right)
}

/// Where a new card goes in `space_id`: right of `near` if given, else right of everything.
pub fn free_card_spot(s: &Snapshot, space_id: &str, near: Option<Point>) -> Point {
    let rects = occupied(s, space_id);
    let start = near.map(|p| Point { x: p.x + NODE_W + 80.0, y: p.y }).unwrap_or_else(|| default_spot(&rects));
    find_free_spot(&rects, start, NODE_W, NODE_H, Mode::Right)
}

// ---------------------------------------------------------------------------------------------
// Moving

/// A card dragged to `pos`. None if the design is gone.
pub fn move_design(s: &Snapshot, design_id: &str, pos: Point) -> Option<Change> {
    let mut v = (**s.value(&RecordKind::Designs.path(design_id))?).clone();
    v["position"] = json!(pos);
    Some(Change::set(RecordKind::Designs.path(design_id), v))
}

/// A portal (space card) dragged to `pos` on its parent canvas.
pub fn move_portal(space_id: &str, pos: Point) -> Change {
    put(RecordKind::Portals, space_id, &pos)
}

// ---------------------------------------------------------------------------------------------
// Spaces

/// Result of creating a space.
#[derive(Debug, Clone, PartialEq)]
pub struct Created {
    pub space_id: String,
    /// The seed copy inside the new space, when created from a design ("Explore").
    pub seed_id: Option<String>,
    pub changes: Vec<Change>,
}

/// A nested space under `parent_space_id` with its portal (reference `createExploration`). With `seed_design_id`,
/// a copy of that design becomes its first card (at the origin) and the portal sits right of the original.
/// Name: `name`, else the seed's title, else "Exploration N".
pub fn create_exploration(s: &Snapshot, parent_space_id: &str, name: Option<&str>, seed_design_id: Option<&str>) -> Created {
    let now = now_ms();
    let seed = seed_design_id.and_then(|id| s.design(id));
    let siblings = s.child_spaces(parent_space_id).len();
    let space_name = name
        .map(str::to_string)
        .or_else(|| seed.as_ref().map(|d| d.title.clone()))
        .unwrap_or_else(|| format!("Exploration {}", siblings + 1));
    let space_id = new_id(8);
    let space = Space {
        id: space_id.clone(),
        name: space_name,
        parent_space_id: Some(parent_space_id.to_string()),
        created_at: now,
        local_path: None,
        kind: None,
        extra: Default::default(),
    };
    let rects = occupied(s, parent_space_id);
    let start = seed.as_ref().map(|d| Point { x: d.position.x + NODE_W + 80.0, y: d.position.y }).unwrap_or_else(|| default_spot(&rects));
    let pos = find_free_spot(&rects, start, PORTAL_W, PORTAL_H, Mode::Right);
    let mut changes = vec![put(RecordKind::Spaces, &space_id, &space), move_portal(&space_id, pos)];
    let mut seed_id = None;
    if let Some(src) = seed {
        let id = new_id(10);
        let copy = Design {
            id: id.clone(),
            space_id: space_id.clone(),
            parent_id: None,
            starred: Some(false),
            position: Point::default(),
            created_at: now,
            ..Design::clone(&src)
        };
        changes.push(put(RecordKind::Designs, &id, &copy));
        changes.push(Change::set(RecordKind::SeededFrom.path(&id), json!(src.id)));
        seed_id = Some(id);
    }
    Created { space_id, seed_id, changes }
}

/// A project on the main canvas. One per local folder: if a project with the same `local_path` (case-insensitive)
/// exists, it is returned with no changes (`created: false` ⇔ `changes` empty).
pub fn create_project(s: &Snapshot, name: &str, local_path: Option<&str>) -> (Created, bool) {
    if let Some(key) = local_path.map(str::to_lowercase)
        && let Some(p) = s.spaces().find(|sp| sp.is_project() && sp.local_path.as_deref().map(str::to_lowercase).as_deref() == Some(key.as_str()))
    {
        return (Created { space_id: p.id.clone(), seed_id: None, changes: vec![] }, false);
    }
    let mut c = create_exploration(s, ROOT_SPACE_ID, Some(name), None);
    let mut space: Space = serde_json::from_value(c.changes[0].value.clone().expect("space value")).expect("space");
    space.kind = Some(SpaceKind::Project);
    space.local_path = local_path.map(str::to_string);
    c.changes[0] = put(RecordKind::Spaces, &c.space_id, &space);
    (c, true)
}

/// Rename a space.
pub fn rename_space(s: &Snapshot, space_id: &str, name: &str) -> Option<Change> {
    let mut v = (**s.value(&RecordKind::Spaces.path(space_id))?).clone();
    v["name"] = json!(name);
    Some(Change::set(RecordKind::Spaces.path(space_id), v))
}

/// Delete a space with all nested spaces, their portals and designs, and the promotedFrom / seededFrom entries of
/// the removed designs (reference `deleteSpace`). Never the main canvas. Promoted copies outside stay. Cancelling
/// running jobs in those spaces is the caller's (proto-ai's) part.
pub fn delete_space(s: &Snapshot, space_id: &str) -> Vec<Change> {
    let Some(target) = s.space(space_id) else { return vec![] };
    if target.parent_space_id.is_none() || space_id == ROOT_SPACE_ID {
        return vec![];
    }
    let mut doomed: HashSet<String> = s.descendant_space_ids(space_id).into_iter().collect();
    doomed.insert(space_id.to_string());
    let mut changes = Vec::new();
    for id in &doomed {
        changes.push(Change::delete(RecordKind::Spaces.path(id)));
        if s.portal(id).is_some() {
            changes.push(Change::delete(RecordKind::Portals.path(id)));
        }
    }
    for d in s.designs().filter(|d| doomed.contains(&d.space_id)) {
        changes.push(Change::delete(RecordKind::Designs.path(&d.id)));
        for kind in [RecordKind::PromotedFrom, RecordKind::SeededFrom] {
            if s.entry(kind, &d.id).is_some() {
                changes.push(Change::delete(kind.path(&d.id)));
            }
        }
    }
    changes
}

// ---------------------------------------------------------------------------------------------
// Promotion

/// Designs can be promoted into any space except the main canvas, which holds only projects.
pub fn can_promote_to(parent_space_id: Option<&str>) -> bool {
    parent_space_id.is_some_and(|p| p != ROOT_SPACE_ID)
}

/// Copy ready designs from their (nested) space up into its parent (reference `promoteDesigns`). Skips designs
/// that aren't ready, already promoted, or whose parent is the main canvas. Copies go below the space's portal in
/// a grid, get an edge to the nearest relative already in the parent, and a `promotedFrom` entry.
/// Returns the changes and the new copies' ids.
pub fn promote(s: &Snapshot, design_ids: &[&str]) -> (Vec<Change>, Vec<String>) {
    let now = now_ms();
    let mut changes = Vec::new();
    let mut created = Vec::new();
    // Boxes placed by this batch, per parent space, so copies don't land on each other.
    let mut placed: Vec<(String, Rect)> = Vec::new();
    for &id in design_ids {
        let Some(src) = s.design(id) else { continue };
        if !src.is_ready() || s.promoted_copy_of(id).is_some() {
            continue;
        }
        let Some(space) = s.space(&src.space_id) else { continue };
        let Some(parent) = space.parent_space_id.clone().filter(|p| can_promote_to(Some(p)) && s.space(p).is_some()) else { continue };
        let portal = s.portal(&space.id).unwrap_or_else(|| default_spot(&occupied(s, &parent)));
        let start = Point { x: portal.x, y: portal.y + PORTAL_H + 80.0 };
        let mut rects = occupied(s, &parent);
        rects.extend(placed.iter().filter(|(p, _)| *p == parent).map(|(_, r)| *r));
        let pos = find_free_spot(&rects, start, NODE_W, NODE_H, Mode::Grid);
        placed.push((parent.clone(), Rect { x: pos.x, y: pos.y, w: NODE_W, h: NODE_H }));

        let copy_id = new_id(10);
        let copy = Design {
            id: copy_id.clone(),
            space_id: parent.clone(),
            parent_id: lineage_parent(s, &src, &parent),
            starred: Some(false),
            position: pos,
            created_at: now,
            ..Design::clone(&src)
        };
        let r = PromotedRef { space_id: space.id.clone(), design_id: id.to_string(), space_name: space.name.clone(), at: now, extra: Default::default() };
        changes.push(put(RecordKind::Designs, &copy_id, &copy));
        changes.push(put(RecordKind::PromotedFrom, &copy_id, &r));
        created.push(copy_id);
    }
    (changes, created)
}

/// The closest relative of `src` already on the parent canvas, so the copy gets an edge (reference
/// `lineageParent`): its own seed original, else walking up its ancestry inside the space, the first ancestor's
/// promoted copy or seed original.
fn lineage_parent(s: &Snapshot, src: &Arc<Design>, parent_space_id: &str) -> Option<String> {
    let in_parent = |id: &str| s.design(id).is_some_and(|d| d.space_id == parent_space_id);
    if let Some(orig) = s.seeded_from(&src.id).filter(|o| in_parent(o)) {
        return Some(orig.to_string());
    }
    let mut seen = HashSet::new();
    let mut cur = src.parent_id.as_deref().and_then(|p| s.design(p));
    while let Some(d) = cur {
        if d.space_id != src.space_id || !seen.insert(d.id.clone()) {
            break;
        }
        if let Some(copy) = s.promoted_copy_of(&d.id) {
            return Some(copy.id.clone());
        }
        if let Some(orig) = s.seeded_from(&d.id).filter(|o| in_parent(o)) {
            return Some(orig.to_string());
        }
        cur = d.parent_id.as_deref().and_then(|p| s.design(p));
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Flags

fn patch_design(s: &Snapshot, id: &str, f: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>)) -> Option<Change> {
    let mut v = (**s.value(&RecordKind::Designs.path(id))?).clone();
    f(v.as_object_mut()?);
    Some(Change::set(RecordKind::Designs.path(id), v))
}

/// Star or unstar a card.
pub fn set_starred(s: &Snapshot, design_id: &str, on: bool) -> Option<Change> {
    patch_design(s, design_id, |o| {
        o.insert("starred".into(), json!(on));
    })
}

/// Make a card its space's design guidelines (unmarking any other in that space), or unmark it.
pub fn set_guidelines(s: &Snapshot, design_id: &str, on: bool) -> Vec<Change> {
    let Some(d) = s.design(design_id) else { return vec![] };
    let now = now_ms();
    let mut out: Vec<Change> = s
        .designs_in(&d.space_id)
        .filter(|o| o.id != d.id && o.guidelines_at.is_some())
        .filter_map(|o| {
            patch_design(s, &o.id, |m| {
                m.remove("guidelinesAt");
            })
        })
        .collect();
    out.extend(patch_design(s, design_id, |m| {
        if on {
            m.insert("guidelinesAt".into(), json!(now as u64));
        } else {
            m.remove("guidelinesAt");
        }
    }));
    out
}

/// Mark a card as the main variant live views show (the newest mark wins), or unmark it.
pub fn set_main(s: &Snapshot, design_id: &str, on: bool) -> Option<Change> {
    let now = now_ms();
    patch_design(s, design_id, |m| {
        if on {
            m.insert("mainAt".into(), json!(now as u64));
        } else {
            m.remove("mainAt");
        }
    })
}

/// Mark designs reviewed (or not): `reviewedIds/<id>` set to true, or deleted.
pub fn set_reviewed(ids: &[&str], on: bool) -> Vec<Change> {
    ids.iter()
        .map(|id| if on { Change::set(RecordKind::ReviewedIds.path(id), json!(true)) } else { Change::delete(RecordKind::ReviewedIds.path(id)) })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Spokes and polls

/// How many lanes are kept (reference: "Keep the most recent 60 lanes so persisted state stays small").
pub const MAX_LANES: usize = 60;

/// Record the component focus of spoke designs (`spokeFocus/<id>`).
pub fn set_spoke_focus(ids: &[&str], focus: &str) -> Vec<Change> {
    ids.iter().map(|id| Change::set(RecordKind::SpokeFocus.path(id), json!(focus))).collect()
}

/// Add or replace a lane, and delete the lanes that fall out of the newest [`MAX_LANES`] (by `startedAt`).
pub fn put_lane(s: &Snapshot, lane: &crate::types::Lane) -> Vec<Change> {
    let mut all: Vec<(String, f64)> = s.lanes().filter(|l| l.job_id != lane.job_id).map(|l| (l.job_id.clone(), l.started_at)).collect();
    all.push((lane.job_id.clone(), lane.started_at));
    all.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut out = Vec::new();
    // The new lane is written even if it is older than the 60 kept (then it's dropped again right away, as in the reference).
    let kept: HashSet<&str> = all.iter().take(MAX_LANES).map(|(id, _)| id.as_str()).collect();
    if kept.contains(lane.job_id.as_str()) {
        out.push(put(RecordKind::Lanes, &lane.job_id, lane));
    }
    out.extend(all.iter().skip(MAX_LANES).filter(|(id, _)| *id != lane.job_id).map(|(id, _)| Change::delete(RecordKind::Lanes.path(id))));
    out
}

/// Set (or clear, with `None`) the winning option of one decision of a published poll (`pollWinners/<pollId>/<decisionId>`).
pub fn set_poll_winner(poll_id: &str, decision_id: &str, option_id: Option<&str>) -> Change {
    let path = RecordKind::PollWinners.path(&format!("{poll_id}/{decision_id}"));
    match option_id {
        Some(o) => Change::set(path, json!(o)),
        None => Change::delete(path),
    }
}
