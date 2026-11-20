//! Starting generations from the features (the reference's `launchLane`): one `Outgoing::Generate` into the
//! runner's queue (`s.composer.outbox`), its lane record written in the same handler, and the outcome handled
//! when the runner reports the run finished (spoke focus, the lane's design ids, notices). Plus the layout
//! helpers that place lanes (`spokeRowOrigins`, `belowOrigins`).

use std::collections::HashMap;

use proto_core::{Design, DesignKind, DesignStatus, Lane, LaneType, Point, Snapshot, VariationRequest, ops};
use serde_json::json;

use crate::app::state::AppState;
use crate::composer::Outgoing;
use crate::features::{nav, notify};
use crate::runner::Outcome;

pub const NODE_W: f64 = 380.0;
pub const NODE_H: f64 = 420.0;
const GAP: f64 = 40.0;
const ROW_GAP: f64 = 120.0;

/// What a lane is, for its record and the finish handling.
#[derive(Clone, Debug, PartialEq)]
pub struct LaneInfo {
    pub lane_type: LaneType,
    pub name: String,
    pub focus: Option<String>,
    pub hub_id: Option<String>,
}

/// One generation to start.
pub struct Launch {
    pub space_id: String,
    pub parent_id: Option<String>,
    pub kind: DesignKind,
    pub origin: Option<Point>,
    pub label: String,
    pub request: VariationRequest,
    /// Register a lane (`lanes/<runId>`) for it; None for plain generations (multi-select revise).
    pub lane: Option<LaneInfo>,
}

/// Runs this module started, by run id.
#[derive(Default)]
pub struct LaunchState {
    pub runs: HashMap<String, Option<LaneInfo>>,
    /// Every run started this session (the reference lists only this tab's jobs in the lanes panel).
    pub session: std::collections::HashSet<String>,
}

/// Queue the generation and write its lane. Returns the run id.
pub fn launch(s: &mut AppState, l: Launch) -> String {
    let run_id = crate::runner::new_run_id();
    if let Some(info) = &l.lane {
        let lane = Lane {
            job_id: run_id.clone(),
            lane_type: info.lane_type,
            name: info.name.clone(),
            focus: info.focus.clone(),
            hub_id: info.hub_id.clone(),
            count: l.request.count,
            started_at: ops::now_ms(),
            extra: Default::default(),
        };
        let changes = ops::put_lane(s.snap(), &lane);
        nav::apply(s, changes, "lane");
    }
    s.features.launch.runs.insert(run_id.clone(), l.lane.clone());
    s.features.launch.session.insert(run_id.clone());
    s.composer.outbox.push(Outgoing::Generate {
        space_id: l.space_id,
        parent_id: l.parent_id,
        kind: l.kind,
        label: Some(l.label),
        request: l.request,
        origin: l.origin,
        run_id: Some(run_id.clone()),
    });
    run_id
}

/// Handle finished runs of ours: spoke focus on the results, the lane's design ids, notices. True when
/// something visible changed.
pub fn poll(s: &mut AppState) -> bool {
    let mine = &s.features.launch.runs;
    let finished = s.runner.take_finished_where(|f| mine.contains_key(&f.run_id));
    let mut changed = false;
    for f in finished {
        let Some(lane) = s.features.launch.runs.remove(&f.run_id) else { continue };
        changed = true;
        let name = lane.as_ref().map_or("Variations".to_string(), |l| l.name.clone());
        let ids: Vec<&str> = f.design_ids.iter().map(String::as_str).collect();
        let mut changes = Vec::new();
        if let Some(info) = &lane {
            if let Some(focus) = &info.focus
                && !ids.is_empty()
            {
                changes.extend(ops::set_spoke_focus(&ids, focus));
            }
            if let Some(mut rec) = s.snap().lane(&f.run_id).map(|l| Lane::clone(&l)) {
                rec.extra.insert("designIds".into(), json!(f.design_ids));
                changes.extend(ops::put_lane(s.snap(), &rec));
            }
        }
        nav::apply(s, changes, "lane finished");
        match f.outcome {
            Outcome::Cancelled => notify(s, &format!("{name} cancelled"), ""),
            Outcome::Failed(msg) => notify(s, &format!("{name} failed"), &msg),
            Outcome::Done => {
                let all_failed = !ids.is_empty() && ids.iter().all(|id| s.snap().design(id).is_some_and(|d| d.status == DesignStatus::Error));
                if all_failed {
                    notify(s, &format!("{name}: every direction failed"), "");
                }
            }
        }
    }
    changed
}

/* ---------------------------------------------------------------- layout */

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

pub fn row_width(count: u32) -> f64 {
    let n = count.max(1) as f64;
    n * NODE_W + (n - 1.0) * GAP
}

fn occupied(s: &Snapshot, space: &str) -> Vec<Rect> {
    s.designs_in(space).map(|d| Rect { x: d.position.x, y: d.position.y, w: NODE_W, h: NODE_H }).collect()
}

fn hits(r: Rect, list: &[Rect]) -> bool {
    let pad = 30.0;
    list.iter().any(|o| r.x < o.x + o.w + pad && r.x + r.w + pad > o.x && r.y < o.y + o.h + pad && r.y + r.h + pad > o.y)
}

fn first_free(candidate: impl Fn(usize) -> Point, w: f64, taken: &mut Vec<Rect>) -> Point {
    for k in 0..400 {
        let p = candidate(k);
        let r = Rect { x: p.x, y: p.y, w, h: NODE_H };
        if !hits(r, taken) {
            taken.push(r);
            return p;
        }
    }
    let p = candidate(0);
    taken.push(Rect { x: p.x, y: p.y, w, h: NODE_H });
    p
}

/// Origins for `rows` spoke rows around `hub`: alternating right/left columns of rows, never overlapping
/// each other or what's on the canvas.
pub fn spoke_row_origins(s: &Snapshot, hub: &Design, rows: usize, count: u32) -> Vec<Point> {
    let mut taken = occupied(s, &hub.space_id);
    let w = row_width(count);
    (0..rows)
        .map(|_| {
            first_free(
                |k| {
                    let right = k % 2 == 0;
                    let row = (k / 2) as f64;
                    Point { x: if right { hub.position.x + NODE_W + 220.0 } else { hub.position.x - 220.0 - w }, y: hub.position.y + row * (NODE_H + ROW_GAP) }
                },
                w,
                &mut taken,
            )
        })
        .collect()
}

/// Origins for follow-up rounds: below each parent, shifted sideways when that spot is taken.
pub fn below_origins(s: &Snapshot, parents: &[&Design], count: u32) -> Vec<Point> {
    let w = row_width(count);
    let mut by_space: HashMap<String, Vec<Rect>> = HashMap::new();
    parents
        .iter()
        .map(|p| {
            let taken = by_space.entry(p.space_id.clone()).or_insert_with(|| occupied(s, &p.space_id));
            first_free(
                |k| {
                    let row = (k / 5) as f64;
                    let shift = [0.0, 1.0, -1.0, 2.0, -2.0][k % 5] * (w + 80.0);
                    Point { x: p.position.x + NODE_W / 2.0 - w / 2.0 + shift, y: p.position.y + NODE_H + ROW_GAP + row * (NODE_H + ROW_GAP) }
                },
                w,
                taken,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto_core::{Change, ChangeMeta, Store};

    fn store() -> Store {
        let st = Store::in_memory();
        let d = |id: &str, x: f64, y: f64| Change::set(format!("designs/{id}"), json!({"id": id, "spaceId": "sp", "kind": "page", "title": id,
            "rationale": "", "html": "x", "status": "ready", "position": {"x": x, "y": y}, "createdAt": 1}));
        st.apply(vec![d("hub", 0.0, 0.0), d("other", 0.0, 540.0)], ChangeMeta::client("t")).unwrap();
        st
    }

    #[test]
    fn spoke_rows_alternate_sides_without_overlap() {
        let st = store();
        let s = st.snapshot();
        let hub = s.design("hub").unwrap();
        let o = spoke_row_origins(&s, &hub, 3, 2);
        assert_eq!((o[0].x, o[0].y), (NODE_W + 220.0, 0.0));
        assert_eq!(o[1].x, -220.0 - row_width(2));
        assert!(o[2].y > 0.0);
    }

    #[test]
    fn rounds_go_below_and_dodge_existing_cards() {
        let st = store();
        let s = st.snapshot();
        let hub = s.design("hub").unwrap();
        let o = below_origins(&s, &[&hub], 1);
        // Straight below is taken by "other" (y 540): shifted one row-width + 80 to the right.
        assert_eq!(o[0].y, NODE_H + ROW_GAP);
        assert_eq!(o[0].x, NODE_W / 2.0 - row_width(1) / 2.0 + row_width(1) + 80.0);
    }
}
