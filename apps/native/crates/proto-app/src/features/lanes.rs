//! "Exploration lanes" in the inspector (the reference's `spokes/lanes.tsx`): every generation in flight (the
//! runner's active runs) and the lanes started this session (`lanes/<runId>` records), with progress, cancel and
//! reveal (the reference lists only the current tab's jobs).
//! Finished lanes keep their design ids in the lane record's `designIds` (written when the run ends).

use proto_core::{DesignStatus, LaneType, Snapshot};
use rune::color::hex;
use rune::{Color, El, TextStyle, col, el, row, text};

use crate::app::state::AppState;
use crate::app::style::Palette;
use crate::features::icons;

const LABEL: TextStyle = TextStyle::new(12.0, 600.0);
const ROW: TextStyle = TextStyle::new(12.0, 500.0);
const SMALL: TextStyle = TextStyle::new(11.0, 400.0);
/// Rows considered (running first, then newest).
const MAX_ROWS: usize = 30;
/// Finished lanes shown before "Show N archived".
const RECENT: usize = 3;

#[derive(Default)]
pub struct LanesState {
    pub show_archived: bool,
}

/// One row: a run in flight or a finished lane.
#[derive(Clone, Debug, PartialEq)]
pub struct LaneRow {
    pub id: String,
    pub name: String,
    pub focus: Option<String>,
    pub lane_type: Option<LaneType>,
    pub running: bool,
    pub design_ids: Vec<String>,
    pub requested: u32,
    pub started_at: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Progress {
    pub total: usize,
    pub ready: usize,
    pub failed: usize,
}

pub fn progress(s: &Snapshot, row: &LaneRow) -> Progress {
    let ds: Vec<_> = row.design_ids.iter().filter_map(|id| s.design(id)).collect();
    Progress {
        total: if row.design_ids.is_empty() { row.requested as usize } else { row.design_ids.len() },
        ready: ds.iter().filter(|d| d.status == DesignStatus::Ready).count(),
        failed: ds.iter().filter(|d| d.status == DesignStatus::Error).count(),
    }
}

/// Running rows first, then newest; at most 30.
pub fn rows(s: &AppState) -> Vec<LaneRow> {
    let snap = s.snap();
    let mut out: Vec<LaneRow> = s
        .runner
        .active_runs()
        .map(|a| {
            let lane = snap.lane(&a.run.id);
            LaneRow {
                id: a.run.id.clone(),
                name: lane.as_ref().map_or_else(|| if a.run.label.is_empty() { "Variations".into() } else { a.run.label.clone() }, |l| l.name.clone()),
                focus: lane.as_ref().and_then(|l| l.focus.clone()),
                lane_type: lane.as_ref().map(|l| l.lane_type),
                running: true,
                design_ids: a.run.design_ids.clone(),
                requested: a.run.request.count,
                started_at: lane.as_ref().map_or(f64::MAX, |l| l.started_at),
            }
        })
        .collect();
    let running: Vec<String> = out.iter().map(|r| r.id.clone()).collect();
    for l in snap.lanes() {
        if running.contains(&l.job_id) || !s.features.launch.session.contains(&l.job_id) {
            continue;
        }
        let ids = l.extra.get("designIds").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
        out.push(LaneRow {
            id: l.job_id.clone(),
            name: l.name.clone(),
            focus: l.focus.clone(),
            lane_type: Some(l.lane_type),
            running: false,
            design_ids: ids,
            requested: l.count,
            started_at: l.started_at,
        });
    }
    out.sort_by(|a, b| b.running.cmp(&a.running).then(b.started_at.total_cmp(&a.started_at)));
    out.truncate(MAX_ROWS);
    out
}

fn green() -> Color {
    hex("#10b981").expect("hex")
}

/// The panel; None when there's nothing to list.
pub fn panel(s: &AppState, p: &Palette) -> Option<El> {
    let all = rows(s);
    if all.is_empty() {
        return None;
    }
    let running = all.iter().filter(|r| r.running).count();
    let (recent, archived): (Vec<_>, Vec<_>) = all.iter().enumerate().partition(|(i, r)| *i < RECENT || r.running);
    let show_all = s.features.lanes.show_archived;
    let mut c = col().key("lanes").w_full().gap(8.0)
        .child(row().w_full().justify_between().center_y()
            .child(text("Exploration lanes").font(LABEL).color(p.fg))
            .child(text(if running > 0 { format!("{running} running") } else { "all settled".into() }).font(SMALL).color(p.muted_fg)));
    let shown: Vec<&LaneRow> = if show_all { all.iter().collect() } else { recent.iter().map(|(_, r)| *r).collect() };
    for r in shown {
        c = c.child(lane_row(s, r, p));
    }
    if !archived.is_empty() {
        let label = if show_all { "Hide archived".to_string() } else { format!("Show {} archived", archived.len()) };
        c = c.child(row().key("lanes-archived").role("button").label(label.clone()).h(22.0).center_y()
            .on_press(|s: &mut AppState| s.features.lanes.show_archived = !s.features.lanes.show_archived)
            .child(text(label).font(SMALL).color(p.muted_fg)));
    }
    Some(c)
}

fn lane_row(s: &AppState, r: &LaneRow, p: &Palette) -> El {
    let pr = progress(s.snap(), r);
    let (status, color) = if r.running {
        (if pr.total == 0 { "planning".to_string() } else { format!("{}/{}", pr.ready, pr.total) }, p.muted_fg)
    } else if r.design_ids.is_empty() {
        ("settled".to_string(), p.muted_fg)
    } else if pr.ready == 0 && pr.failed > 0 {
        ("failed".to_string(), hex("#ef4444").expect("hex"))
    } else {
        (format!("{}/{}", pr.ready, pr.total), if pr.failed > 0 { hex("#d97706").expect("hex") } else { green() })
    };
    let reveal_ids = r.design_ids.clone();
    let mut head = row().w_full().gap(8.0).center_y()
        .child(icons::lane(14.0, p.muted_fg, r.lane_type))
        .child(row().key(format!("lane-name-{}", r.id)).role("button").label(r.name.clone()).grow(1.0).min_w(0.0)
            .on_press(move |s: &mut AppState| reveal(s, &reveal_ids))
            .child(text(r.name.clone()).font(ROW).color(p.fg).truncate()))
        .child(text(status).font(SMALL).color(color));
    if r.running {
        let id = r.id.clone();
        head = head.child(row().key(format!("lane-cancel-{}", r.id)).role("button").label("Cancel lane").size(20.0, 20.0).radius(4.0).center()
            .hover(p.hover).press(p.press)
            .on_press(move |s: &mut AppState| crate::runner::cancel_run(s, &id))
            .child(el().size(8.0, 8.0).radius(1.5).fill(p.fg)));
    } else if let Some(first) = r.design_ids.iter().find(|id| s.snap().design(id).is_some_and(|d| d.is_ready())).cloned() {
        head = head.child(row().key(format!("lane-review-{}", r.id)).role("button").label("Review this lane").size(20.0, 20.0).radius(4.0).center()
            .hover(p.hover).press(p.press)
            .on_press(move |s: &mut AppState| crate::features::review::open(s, Some(first.clone())))
            .child(icons::inbox(13.0, p.muted_fg)));
    }
    let frac = if pr.total == 0 { 0.0 } else { (pr.ready + pr.failed) as f32 / pr.total as f32 };
    let (bar_w, bar_c) = if r.running { (frac.max(0.06), hex("#3b82f6").expect("hex")) } else { (1.0, green()) };
    col().key(format!("lane-{}", r.id)).w_full().gap(6.0).px(10.0).py(8.0).radius(8.0).border(1.0, p.border)
        .child(head)
        .child(el().w_full().h(4.0).radius(2.0).fill(p.hover).clip_soft(0.0)
            .child(el().w_frac(bar_w).h_full().radius(2.0).fill(bar_c)))
}

/// Go to the lane's designs: their space, the first one selected.
fn reveal(s: &mut AppState, ids: &[String]) {
    let Some(d) = ids.iter().find_map(|id| s.snap().design(id)) else { return };
    crate::features::nav::enter_space(s, &d.space_id, Some(&d.id));
}
