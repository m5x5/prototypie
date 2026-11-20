//! Design and project cards on the canvas (proto-worker-5). Spec: docs/spec-cards.md §2, §3, §7.
//!
//! Interface with the canvas engine (worker-4):
//! - Cards build node *content* (`design::node`, `portal::node`); `canvas::node_frame` places it at
//!   world coordinates and owns drag, select and double-press. The preview and portal body carry no
//!   handlers; card controls keep their own `on_press` (topmost wins, so they never start a drag).
//! - Double-press on a node calls `on_node_double_press` (fullscreen for a design, enter for a portal).
//! - `anchors` gives the edge handles; the canvas draws handle dots and edges.
//! - `CardsState::on_leave` runs when the active space changes (`AppState::enter`).
//!
//! Card buttons don't reach into other modules: a press becomes a `CardAction`. Cards handle their
//! own (fullscreen, record writes, navigation) and queue the rest for their owners (`CardsState::take_actions`).

mod actions;
mod controls;
mod design;
mod facts;
mod model;
pub mod fixture;
mod ops;
pub mod icons;
mod portal;
pub mod pictures;
pub mod thumbs;
pub mod preview;
pub mod theme;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use rune::{Cx, El, Tx, el, memo};

use crate::app::state::{AppState, ThemePref};
use crate::app::style;
use crate::canvas::{NodeKind, node_frame};
// Shared with other modules' card controls and layout (inspector, features, canvas).
#[allow(unused_imports)]
pub use controls::{Badge, BadgeLook, Variant, btn_sm, button_sm, icon_btn};
pub use design::morph_id;
#[allow(unused_imports)]
pub use theme::{CardTheme, NODE_H, NODE_W, PORTAL_H, PORTAL_W, TITLE_H};

/// What a card control asks for. Ids are design ids unless named otherwise.
#[derive(Clone, Debug, PartialEq)]
pub enum CardAction {
    OpenFullscreen(String),
    ToggleStar(String),
    Delete(String),
    Retry(String),
    /// Focus the composer on the design (worker-6).
    Vary(String),
    /// Compare dialog (worker-8).
    Compare(String),
    Explore(String),
    Promote(String),
    ShowPromoted(String),
    /// Implement request / task panel (worker-8).
    Implement(String),
    /// Composer in edit mode (worker-6).
    AiEdit(String),
    UndoEdit(String),
    CancelEdit(String),
    DismissEditError(String),
    ToggleMain(String),
    SetGuidelines(String, bool),
    JumpToSource { space_id: String, design_id: String },
    /// Spokes picker (worker-8).
    Spokes(String),
    Merge(String),
    TogglePoll(String),
    /// "Erledigt" → chevron → "Erneut senden …": ask, cancel, confirm (sends with force).
    AskResend(String),
    CancelResend,
    Resend(String),
    /// Ids are space ids.
    EnterSpace(String),
    DeleteSpace(String),
}

#[derive(Default)]
pub struct CardsState {
    /// Actions waiting for their owner (`take_actions`).
    pending: Vec<CardAction>,
    /// When this device last left each space (ms since epoch), for "N new" on its card.
    /// Per device, not synced (the reference keeps it in localStorage). TODO: persist in prefs.
    left_at: HashMap<String, f64>,
    /// Card pictures from proto-web.
    pub thumbs: thumbs::Thumbs,
    /// The design whose "send again?" confirmation is showing.
    pub resend_confirm: Option<String>,
    /// The active space's cards with their facts, built once per snapshot version/space by
    /// whichever comes first, `poll` or the view (interior mutability so the view can fill it).
    model: RefCell<Option<Rc<model::Model>>>,
}

/// From `AppState::setup` (shell): remember the waker for proto-web.
pub fn setup(s: &mut AppState, wake: rune::Waker) {
    s.cards.thumbs.setup(wake.clone());
    crate::fullscreen::setup(s, wake);
}

/// From `AppState::poll` (shell): load and render card pictures. True when one changed.
/// Never per camera frame: the model rebuilds only when the snapshot or space changes, and
/// pictures report a change only when one arrives.
pub fn poll(s: &mut AppState, clock: f64, tx: &mut Tx) -> bool {
    let snap = s.store().snapshot();
    let mut changed = false;
    let stale = !s.cards.model.borrow().as_ref().is_some_and(|m| m.fresh(snap.version, &s.route.space, clock));
    if stale {
        let left = &s.cards.left_at;
        let m = model::Model::build(&snap, &s.route.space, |id| left.get(id).copied(), clock);
        *s.cards.model.borrow_mut() = Some(Rc::new(m));
        changed = true;
    }
    let size = if s.canvas.size[0] > 0.0 { s.canvas.size } else { [1280.0, 800.0] };
    let model = s.cards.model.borrow().clone().expect("built above");
    // WebKit starts only after the first frame and once proto-server has settled (launch perf).
    let can_start = s.first_frame_shown && s.server_settled();
    changed |= s.cards.thumbs.poll(&snap, &model, &s.canvas.camera, size, can_start, clock, tx);
    changed
}

/// A card picture for the views.
pub fn picture<'a>(s: &'a AppState, design_id: &str) -> Option<preview::Picture<'a>> {
    s.cards.thumbs.get(design_id).map(|p| preview::Picture { pixels: &p.pixels, rect: p.rect, cropped: p.crop.is_some() })
}

impl CardsState {
    /// The active space `space` is being left (for the per-device "N new" counter).
    pub fn on_leave(&mut self, space: &str) {
        self.left_at.insert(space.to_string(), now_ms());
    }

    pub fn left_at(&self, space: &str) -> Option<f64> {
        self.left_at.get(space).copied()
    }

    pub fn queue(&mut self, a: CardAction) {
        self.pending.push(a);
    }

    /// Every queued action, oldest first. Prefer `take_actions_where`: the queue is shared by
    /// the composer (Vary, AiEdit …) and features (Compare, Implement, Spokes, Merge, TogglePoll …).
    pub fn take_actions(&mut self) -> Vec<CardAction> {
        std::mem::take(&mut self.pending)
    }

    /// The queued actions `mine` accepts, oldest first; the rest stay queued for their owners.
    pub fn take_actions_where(&mut self, mine: impl Fn(&CardAction) -> bool) -> Vec<CardAction> {
        let (taken, rest) = std::mem::take(&mut self.pending).into_iter().partition(|a| mine(a));
        self.pending = rest;
        taken
    }

    pub fn pending(&self) -> &[CardAction] {
        &self.pending
    }
}

pub fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

/// `dispatch` from a handler with its `Tx`: opening fullscreen flies on the morph's timing.
pub fn dispatch_tx(s: &mut AppState, a: CardAction, tx: &mut Tx) {
    if matches!(a, CardAction::OpenFullscreen(_)) {
        crate::fullscreen::morph_motion(tx);
    }
    dispatch(s, a);
}

/// Runs a card action: cards' own now, everything else queued for its owner.
pub fn dispatch(s: &mut AppState, a: CardAction) {
    match a {
        CardAction::OpenFullscreen(id) => {
            let set = s.route.selected.clone();
            crate::fullscreen::open(s, &id, &set);
        }
        other => {
            if let Some(rest) = ops::apply(s, other) {
                s.cards.queue(rest);
            }
        }
    }
}

/// The card theme for the current appearance.
pub fn theme(s: &AppState, cx: &mut Cx<AppState>) -> CardTheme {
    let dark = match s.theme {
        ThemePref::System => cx.system_dark(),
        ThemePref::Dark => true,
        ThemePref::Light => false,
    };
    CardTheme::new(&style::palette(dark))
}

/// The nodes of the active space at world coordinates: project/exploration cards for its child
/// spaces, design cards for its designs.
pub fn world_nodes(s: &AppState, cx: &mut Cx<AppState>) -> Vec<El> {
    let snap = s.snap();
    // Normally `poll` built it; before the first poll or right after a switch, build and keep it.
    let cached = s.cards.model.borrow().clone().filter(|m| m.version == snap.version && m.space == s.route.space);
    let m = match cached {
        Some(m) => m,
        None => {
            let m = Rc::new(model::Model::build(snap, &s.route.space, |id| s.cards.left_at(id), 0.0));
            *s.cards.model.borrow_mut() = Some(m.clone());
            m
        }
    };
    let t = theme(s, cx);
    // The whole layer is memoised on everything the cards read, so a view re-run that changed
    // nothing here (a header hover, a composer keystroke) skips all of them; inside, each card has
    // its own memo for the runs that changed a few.
    let fs_open = crate::fullscreen::open_id(s);
    let layer_version = (
        m.version,
        &m.space,
        &s.route.selected,
        fs_open,
        &s.cards.resend_confirm,
        s.cards.thumbs.total_generation(),
        // Per-card state features owns: Implement phase and open notes.
        m.designs.iter().map(|e| (crate::features::implement_phase(s, &e.design) as u8, crate::features::open_notes_count(s, &e.design.id))).collect::<Vec<_>>(),
        // Which of this space's designs are poll candidates (features owns the list).
        m.designs.iter().filter(|e| s.features.polls.in_poll(&e.design.id)).map(|e| e.design.id.as_str()).collect::<Vec<_>>(),
        t.dark,
        // Node frames bind the drag's live offset while their node moves (canvas, worker-4).
        crate::canvas::frame_key(s),
    );
    let layer = memo("cards-layer", layer_version, || {
        // A real size, not 0×0: under a changing live_scale Rune scales box instances inside a
        // zero-size parent twice (text once), so cards visibly grew while their text shrank.
        el().key("cards-layer").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .children(card_nodes(s, &m, &t))
    });
    vec![layer]
}

/// The node frames of the model's cards, each card memoised.
fn card_nodes(s: &AppState, m: &model::Model, t: &CardTheme) -> Vec<El> {
    let selected: HashSet<&str> = s.route.selected.iter().map(String::as_str).collect();
    let mut nodes = Vec::with_capacity(m.portals.len() + m.designs.len());
    // Each card's content is memoised: unchanged cards aren't rebuilt when the view re-runs.
    for p in &m.portals {
        let sel = selected.contains(p.space.id.as_str());
        let pic_gen = p.facts.hero.as_ref().map_or(0, |h| s.cards.thumbs.generation(h));
        let content = memo(format!("p:{}:node", p.space.id), (p.version, sel, pic_gen, t.dark), || {
            let pic = p.facts.hero.as_ref().and_then(|h| picture(s, h));
            portal::node(&p.space, &p.facts, sel, pic, p.hero_vw, t)
        });
        nodes.push(node_frame(NodeKind::Portal, &p.space.id, p.pos, content));
    }
    let fs_open = crate::fullscreen::open_id(s);
    for e in &m.designs {
        let d = &e.design;
        let ctx = design::Ctx {
            selected: selected.contains(d.id.as_str()),
            in_poll: s.features.polls.in_poll(&d.id),
            implement: crate::features::implement_phase(s, d),
            open_notes: crate::features::open_notes_count(s, &d.id),
            in_fullscreen: fs_open == Some(d.id.as_str()),
            confirm_resend: s.cards.resend_confirm.as_deref() == Some(d.id.as_str()),
            no_agent: crate::features::no_agent_connected(s),
        };
        let pos = [d.position.x as f32, d.position.y as f32];
        let version = (
            e.version,
            ctx.selected,
            ctx.in_poll,
            ctx.implement as u8,
            ctx.open_notes,
            ctx.in_fullscreen,
            ctx.confirm_resend,
            ctx.no_agent,
            s.cards.thumbs.generation(&d.id),
            t.dark,
        );
        let content = memo(format!("d:{}:node", d.id), version, || {
            // Features' pod provenance strip reads only the design record (covered by e.version).
            let pod = crate::features::pod_provenance(s, d, &style::palette(t.dark));
            design::node(d, &e.facts, ctx, picture(s, &d.id), pod, t)
        });
        nodes.push(node_frame(NodeKind::Design, &d.id, pos, content));
    }
    nodes
}

/// A double-press on a node's frame (not on a card control).
pub fn on_node_double_press(s: &mut AppState, kind: NodeKind, id: &str, tx: &mut Tx) {
    match kind {
        // Only ready designs open (the reference's button is disabled otherwise).
        NodeKind::Design if s.snap().design(id).is_some_and(|d| d.is_ready()) => {
            dispatch_tx(s, CardAction::OpenFullscreen(id.to_string()), tx)
        }
        NodeKind::Design => {}
        NodeKind::Portal => dispatch_tx(s, CardAction::EnterSpace(id.to_string()), tx),
    }
}

/// Edge handle anchors `(top_centre, bottom_centre)` in node-local points for a node of `size`.
pub fn anchors(kind: NodeKind, size: [f32; 2]) -> ([f32; 2], [f32; 2]) {
    let top = if kind == NodeKind::Design { TITLE_H } else { 0.0 };
    ([size[0] / 2.0, top], [size[0] / 2.0, size[1]])
}
