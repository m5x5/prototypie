//! The side panel's "Neighbours" navigator (user note mwn9n4um-iv8iq2): prev/next in the
//! open set, and the design's graph neighbours — the design it came from, its siblings (the same
//! round: the parent's other variations) and its own variations — as small picture chips.
//! Choosing one switches the view in place (no close/reopen) and selects it on the canvas behind.
//! Keys: ↑ the parent, ↓ the first variation (alongside ←/→).

use std::sync::Arc;

use proto_core::{Design, Snapshot};
use rune::{El, col, row, text};

use super::View;
use crate::app::state::AppState;
use crate::cards::icons::{CHEVRON_LEFT, CHEVRON_RIGHT};
use crate::cards::theme::{XS, XS_MED, XS_NUM};
use crate::cards::{CardTheme, icon_btn, preview};

/// Chips shown per group before "+N".
const PER_GROUP: usize = 6;
/// A chip's picture (pt) and the device px asked for it.
const CHIP_PIC: [f32; 2] = [40.0, 30.0];
pub const CHIP_OUT_W: u32 = 96;
const CHIP_W: f32 = (288.0 - 6.0) / 2.0;

/// The open design's ready graph neighbours.
pub struct Neighbours {
    pub parent: Option<Arc<Design>>,
    pub siblings: Vec<Arc<Design>>,
    pub children: Vec<Arc<Design>>,
}

impl Neighbours {
    pub fn of(snap: &Snapshot, d: &Design) -> Neighbours {
        let parent = d.parent_id.as_deref().and_then(|p| snap.design(p)).filter(|p| p.is_ready());
        let siblings = match d.parent_id.as_deref() {
            Some(p) => snap.children_of(p).into_iter().filter(|x| x.id != d.id && x.is_ready()).collect(),
            None => Vec::new(),
        };
        let children = snap.children_of(&d.id).into_iter().filter(|x| x.is_ready()).collect();
        Neighbours { parent, siblings, children }
    }

    /// Every id shown (for their pictures).
    pub fn ids(&self) -> Vec<String> {
        self.parent.iter().chain(&self.siblings).chain(&self.children).map(|d| d.id.clone()).collect()
    }
}

/// Switches the open view to `id` in place (the set stays, so ←/→ keep working when `id` is in
/// it) and selects it on the canvas behind, entering its space when it lives elsewhere.
pub fn go(s: &mut AppState, id: &str) {
    let Some(d) = s.snap().design(id) else { return };
    if !d.is_ready() {
        return;
    }
    super::note::flush(s);
    let set = match &s.fullscreen.view {
        View::Open { set, .. } => set.clone(),
        View::Closed => return,
    };
    s.fullscreen.view = View::Open { id: id.to_string(), set };
    if d.space_id == s.route.space {
        s.route.selected = vec![id.to_string()];
    } else {
        s.enter_space(&d.space_id, Some(id.to_string()));
    }
}

/// ↑: the parent; ↓: the first variation. True when it moved.
pub fn go_up_down(s: &mut AppState, down: bool) -> bool {
    let Some(id) = super::open_id(s) else { return false };
    let Some(d) = s.snap().design(id) else { return false };
    let n = Neighbours::of(s.snap(), &d);
    let target = if down { n.children.first() } else { n.parent.as_ref() };
    let Some(t) = target.map(|t| t.id.clone()) else { return false };
    go(s, &t);
    true
}

/// The navigator, or None when there's nowhere to go.
pub fn view(s: &AppState, d: &Design, steps: Option<(usize, usize)>, t: &CardTheme) -> Option<El> {
    let n = Neighbours::of(s.snap(), d);
    if steps.is_none() && n.parent.is_none() && n.siblings.is_empty() && n.children.is_empty() {
        return None;
    }
    let mut head = row().w_full().center_y().gap(2.0).child(text("Neighbours").font(XS_MED).color(t.fg)).child(rune::spacer());
    if let Some((at, len)) = steps {
        head = head
            .child(icon_btn("fs:nb:prev".into(), "Previous design (←)", &CHEVRON_LEFT, 24.0, 14.0, t.fg, t, None)
                .on_press(|s: &mut AppState| super::step(s, -1)))
            .child(row().key("fs:nb:pos").min_w(36.0).center().child(text(format!("{} / {len}", at + 1)).font(XS_NUM).color(t.muted_fg)))
            .child(icon_btn("fs:nb:next".into(), "Next design (→)", &CHEVRON_RIGHT, 24.0, 14.0, t.fg, t, None)
                .on_press(|s: &mut AppState| super::step(s, 1)));
    }
    let mut out = col().key(format!("fs:neighbours:{}", d.id)).w_full().gap(6.0).child(head);
    let groups: [(&str, &str, Vec<Arc<Design>>); 3] = [
        ("from", "From (↑)", n.parent.into_iter().collect()),
        ("round", "Same round", n.siblings),
        ("vars", "Variations (↓)", n.children),
    ];
    for (key, label, ds) in groups {
        if ds.is_empty() {
            continue;
        }
        let mut g = col().key(format!("fs:nb:{key}")).w_full().gap(4.0).child(text(label).font(XS).color(t.muted_fg));
        for pair in ds.iter().take(PER_GROUP).collect::<Vec<_>>().chunks(2) {
            g = g.child(row().w_full().gap(6.0).children(pair.iter().map(|x| chip(s, x, t))));
        }
        if ds.len() > PER_GROUP {
            g = g.child(text(format!("+{} more", ds.len() - PER_GROUP)).font(XS).color(t.muted_fg));
        }
        out = out.child(g);
    }
    Some(out)
}

/// A neighbour: its picture and title; pressing it switches the view.
fn chip(s: &AppState, d: &Design, t: &CardTheme) -> El {
    let b = preview::PreviewBox { w: CHIP_PIC[0], h: CHIP_PIC[1], centred: false, transparent: false };
    let pic = crate::cards::picture(s, &d.id);
    let id = d.id.clone();
    row().key(format!("fs:nb:chip:{}", d.id)).role("button").label(d.title.clone()).help(d.title.clone())
        .w(CHIP_W).h(40.0).px(4.0).gap(6.0).center_y().radius(6.0).border(1.0, t.border).hover(t.ghost_hover)
        .focusable(true).focus_ring(3.0, 0.0, t.ring.alpha(0.5))
        .child(rune::el().radius(3.0).clip_rounded(3.0)
            .child(preview::thumb(format!("fs:nb:pic:{}", d.id), b, pic, d.viewport.unwrap_or(1280.0) as f32, t)))
        .child(text(d.title.clone()).font(XS).color(t.fg).single_line().truncate().shrink().min_w(0.0))
        .on_press(move |s: &mut AppState| go(s, &id))
}
