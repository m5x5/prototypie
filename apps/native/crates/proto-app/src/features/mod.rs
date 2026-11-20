//! Spokes, reviews, polls, multi-select, imports, agent tasks, pod, live views, and the creation side of
//! nested spaces (proto-worker-8 owns this module). Spec: docs/spec-features.md.

pub mod agent_tasks;
pub mod annotate;
pub mod ai_edit;
pub mod compare;
pub mod http;
pub mod icons;
pub mod imports;
pub mod lanes;
pub mod mix;
pub mod multi;
pub mod launch;
pub mod mention;
pub mod nav;
pub mod nested;
pub mod notes;
pub mod pod_layouts;
mod notice;
pub mod polls;
pub mod review;
pub mod scenes;
pub mod spokes;
pub mod time;

use rune::{Cx, El, Tx, Waker};

use crate::app::state::AppState;
use crate::app::style::Palette;
pub use mix::{DropTarget, ExtraEdge, ExtraNode};
pub use notice::{NoticeAction, notify, notify_with, undo_for};

#[derive(Default)]
pub struct FeaturesState {
    /// Agent capacity pill, task panel and the shared `/api/agent-tasks` poll.
    pub tasks: agent_tasks::TasksState,
    /// A design the composer should focus once (set by Explore); the composer takes it.
    pub focus_prompt: Option<String>,
    /// The bottom notice (the reference's toasts).
    pub notice: notice::NoticeState,
    /// Review queue, dock entry and review mode.
    pub review: review::ReviewState,
    /// Generations this module started (finish handling).
    pub launch: launch::LaunchState,
    /// Spokes picker, block-of-clay dialog, merge notes.
    pub spokes: spokes::SpokesState,
    /// Drag-to-mix draft cards (per device).
    pub mix: mix::MixState,
    /// The multi-select bar.
    pub multi: multi::MultiState,
    /// Poll tray, Decisions popover and panel.
    pub polls: polls::PollsState,
    /// Bundles and "Import from app".
    pub imports: imports::ImportsState,
    /// Implement requests in flight.
    pub implement: agent_tasks::implement::ImplementState,
    /// @ pod / # component mentions in the composer.
    pub mention: mention::MentionState,
    /// The fullscreen "Änderung anfragen" box.
    pub ai_edit: ai_edit::AiEditState,
    /// "+ → Layout finden…".
    pub layouts: pod_layouts::LayoutFinder,
    /// The inspector's lanes panel.
    pub lanes: lanes::LanesState,
    /// Side-by-side compare dialog.
    pub compare: compare::CompareState,
    /// "+ → Project" dialog.
    pub project: nested::project::ProjectDialog,
    /// The Annotate toolbar and annotate mode.
    pub annotate: annotate::AnnotateState,
}

/// One entry of the header's "+" New menu (the shell renders the menu; `run` acts or opens a dialog).
pub struct CreateItem {
    pub key: &'static str,
    /// The tile's lucide icon (the reference's 2-column tile menu shows one per entry).
    pub icon: &'static rune::Icon,
    pub label: &'static str,
    pub hint: &'static str,
    pub enabled: bool,
    pub run: fn(&mut AppState, &mut Tx),
}

/// The New menu's entries, in the reference's order. The main canvas holds only projects, so there only
/// Project is enabled.
pub fn create_items(s: &AppState) -> Vec<CreateItem> {
    let in_space = !s.route.is_root();
    vec![
        CreateItem { key: "create-page", icon: &crate::cards::icons::WAND_SPARKLES, label: "Page", hint: "Rough full-page prototype from a prompt", enabled: in_space,
            run: spokes::open_clay },
        CreateItem { key: "create-exploration", icon: &crate::inspector::icons::FOLDER_INPUT, label: "Exploration", hint: "New empty space inside this canvas", enabled: in_space,
            run: |s, _| nested::new_exploration(s) },
        CreateItem { key: "create-project", icon: &crate::cards::icons::LAYOUT_GRID, label: "Project", hint: "One of your projects, on the main canvas", enabled: true,
            run: |s, _| nested::project::open(s) },
        CreateItem { key: "create-layout", icon: &crate::cards::icons::LAYERS, label: "Layout finden…", hint: "Two pod shapes, one relation, four layouts", enabled: in_space,
            run: pod_layouts::open },
        CreateItem { key: "create-import", icon: &crate::inspector::icons::LAYOUT_TEMPLATE, label: "Import from app…", hint: "Recreate an existing element from the app's source", enabled: in_space,
            run: imports::open_from_app },
    ]
}

/// Once at launch (from `AppState::setup`): start pollers, keep the waker.
pub fn setup(s: &mut AppState, wake: Waker) {
    s.features.project.setup(wake.clone());
    s.features.spokes.setup(wake.clone());
    s.features.polls.setup(wake.clone());
    s.features.imports.setup(wake.clone());
    s.features.implement.setup(wake.clone());
    s.features.mention.setup(wake.clone());
    s.features.compare.live.setup(wake.clone());
    s.features.review.live.setup(wake.clone());
    annotate::setup(s, wake.clone());
    s.features.tasks.setup(wake);
}

/// Every frame (from `AppState::poll`); returns whether the view must be rebuilt.
pub fn poll(s: &mut AppState, clock: f64, tx: &mut Tx) -> bool {
    http::allow_writes(s.store().data_dir().is_some());
    let (mut changed, note_sent) = s.features.tasks.poll(clock, tx);
    if note_sent {
        notify(s, "Notiz angehängt", "");
    }
    changed |= s.features.notice.poll(clock, tx);
    changed |= nested::project::poll(s);
    changed |= take_card_actions(s, tx);
    changed |= review::sync_round(s);
    changed |= launch::poll(s);
    changed |= spokes::poll(s);
    changed |= polls::poll(s);
    changed |= imports::poll(s);
    changed |= mention::poll(s);
    changed |= compare::poll(s, clock, tx);
    changed |= review::poll_live(s, clock, tx);
    changed |= annotate::poll(s, clock, tx);
    changed |= agent_tasks::implement::poll(s, tx);
    changed |= request_pictures(s);
    changed
}

/// Pictures for designs shown outside their cards: the review stage and its siblings, compare columns.
/// Widths are bucketed (device px, 2× backing) so the requests stay stable.
fn request_pictures(s: &mut AppState) -> bool {
    let mut want: Vec<(String, u32)> = Vec::new();
    if s.features.review.open {
        let queue = review::queue_ids(s.snap());
        if let Some(cur) = s.features.review.effective(s.snap(), &queue) {
            want.extend(review::siblings_of(s.snap(), &cur).iter().map(|d| (d.id.clone(), 512)));
            want.push((cur, 1792));
        }
    }
    if let Some(base) = s.features.compare.base.clone() {
        want.push((base.clone(), 1280));
        want.extend(compare::results(s, &base).iter().map(|d| (d.id.clone(), 1280)));
    }
    let snap = s.store().snapshot();
    let mut changed = false;
    for (id, w) in want {
        changed |= s.cards.thumbs.request_extra(&snap, &id, w);
    }
    changed
}

/// Card presses the card queues for features (worker-5's `CardAction`s): Compare now; Implement, Spokes,
/// Merge and poll toggles as those features land.
fn take_card_actions(s: &mut AppState, tx: &mut Tx) -> bool {
    use crate::cards::CardAction;
    let mine = s.cards.take_actions_where(|a| matches!(a, CardAction::Compare(_) | CardAction::Spokes(_) | CardAction::Merge(_) | CardAction::TogglePoll(_) | CardAction::Implement(_) | CardAction::UndoEdit(_) | CardAction::CancelEdit(_) | CardAction::DismissEditError(_)));
    let any = !mine.is_empty();
    for a in mine {
        match a {
            CardAction::Compare(id) => compare::open(s, &id),
            CardAction::Spokes(id) => spokes::open_picker(s, &id),
            CardAction::Merge(id) => spokes::merge_into_hub(s, &id),
            CardAction::TogglePoll(id) => polls::toggle_candidate(s, &id),
            CardAction::Implement(id) => agent_tasks::implement::press(s, &id, tx),
            CardAction::UndoEdit(id) => ai_edit::undo(s, &id),
            CardAction::CancelEdit(id) => ai_edit::cancel(s, &id),
            CardAction::DismissEditError(id) => ai_edit::dismiss_error(s, &id),
            _ => {}
        }
    }
    any
}

/// Canvas hooks (worker-4): draft cards as extra nodes in the active space, their edges plus combined-base
/// edges, a draft moved, a card handle released.
pub fn extra_nodes(s: &AppState, p: &Palette) -> Vec<ExtraNode> {
    mix::extra_nodes(s, p)
}

pub fn extra_edges(s: &AppState) -> Vec<ExtraEdge> {
    mix::extra_edges(s)
}

pub fn on_extra_node_move(s: &mut AppState, id: &str, pos: [f32; 2]) {
    mix::on_extra_node_move(s, id, pos);
}

pub fn on_handle_drop(s: &mut AppState, from_design: &str, drop: DropTarget, tx: &mut Tx) {
    mix::on_handle_drop(s, from_design, drop, tx);
}

/// The ⋯ menu's "Decisions" item (worker-6 renders it): its status and what it opens.
pub fn decisions_status(s: &AppState) -> String {
    polls::status(s)
}

pub fn open_decisions(s: &mut AppState) {
    s.features.polls.dock_open = true;
    polls::load_sharing(s);
}

/// The ⋯ menu's bundle section (worker-6): refresh on open, the list (None = loading), import one.
pub fn on_more_open(s: &mut AppState) {
    imports::on_more_open(s);
}

pub fn bundles(s: &AppState) -> Option<&[imports::BundleInfo]> {
    s.features.imports.bundles.as_deref()
}

pub fn import_bundle(s: &mut AppState, file: &str) {
    imports::import_bundle(s, file);
}

/// Fullscreen side panel pieces (worker-5 places them): the AI edit box and the notes list.
pub fn fullscreen_sections(s: &AppState, p: &Palette, d: &proto_core::Design) -> Vec<El> {
    let mut v = vec![ai_edit::request_box(s, p, d)];
    v.extend(notes::panel(s, p, &d.id));
    v
}

/// The "N fields from the pod" strip for cards made with @-mentions (reads only the design record).
pub fn pod_provenance(_s: &AppState, d: &proto_core::Design, p: &Palette) -> Option<El> {
    mention::view::pod_provenance(d, p)
}

/// The card's notes badge count (open Annotate notes).
pub fn open_notes_count(s: &AppState, design_id: &str) -> usize {
    notes::open_count(s, design_id)
}

/// The card's Implement button phase (worker-5 draws the button).
pub fn implement_phase(s: &AppState, d: &proto_core::Design) -> agent_tasks::implement::ImplementPhase {
    agent_tasks::implement::phase(s, d)
}

/// Extra buttons for the header's right group, before New (e.g. Review, Spokes).
pub fn header_actions(s: &AppState, cx: &mut Cx<AppState>) -> Vec<El> {
    let p = header_palette(s, cx);
    vec![review::dock::button(s, &p)]
}

fn header_palette(s: &AppState, cx: &mut Cx<AppState>) -> Palette {
    use crate::app::state::ThemePref;
    let dark = match s.theme {
        ThemePref::System => cx.system_dark(),
        ThemePref::Dark => true,
        ThemePref::Light => false,
    };
    crate::app::style::palette(dark)
}

/// Keys nobody focused took, after fullscreen: review mode, then the dialogs' Esc. True when handled.
pub fn on_key(s: &mut AppState, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
    if annotate::on_key(s, ev) {
        return true;
    }
    if review::on_key(s, ev, tx) {
        return true;
    }
    // Alt/Option+D deletes the selected designs (Option+D types "∂" on a Mac keyboard).
    if ev.mods.alt && !ev.mods.cmd && !ev.mods.ctrl && matches!(&ev.key, rune::Key::Text(t) if t == "d" || t == "D" || t == "∂") {
        return multi::delete_selected(s);
    }
    if ev.key == rune::Key::Escape {
        if s.features.compare.base.is_some() {
            compare::close(s);
            return true;
        }
        if s.features.polls.panel_open {
            s.features.polls.panel_open = false;
            return true;
        }
        if s.features.polls.dock_open {
            s.features.polls.dock_open = false;
            return true;
        }
        if s.features.review.dock_open {
            s.features.review.dock_open = false;
            return true;
        }
    }
    false
}

/// Sections for the inspector (worker-6 places them after the design details): exploration lanes for now;
/// the hub/spoke box, "Beim Agenten" and notes follow.
pub fn inspector_sections(s: &AppState, _cx: &mut Cx<AppState>, p: &Palette, design: Option<&proto_core::Design>) -> Vec<El> {
    // The reference's order and rules: notes, the hub/spoke card and lanes (no separator; bordered cards), then
    // "Beim Agenten" with its own separator.
    let ink = crate::inspector::style::ink(p);
    let mut sections: Vec<El> = Vec::new();
    sections.extend(design.and_then(|d| notes::panel(s, p, &d.id)));
    sections.extend(design.and_then(|d| spokes::view::inspector_box(s, p, d)));
    sections.extend(lanes::panel(s, p));
    if let Some(prog) = design.and_then(|d| agent_tasks::view::progress_section(s, p, &d.id)) {
        sections.push(crate::inspector::style::separator(&ink));
        sections.push(prog);
    }
    sections
}

/// The agent capacity widget, first in the header's right group (`Agenten: n von m`).
pub fn agent_capacity(s: &AppState, cx: &mut Cx<AppState>, p: &Palette) -> Option<El> {
    Some(agent_tasks::view::pill(s, cx, p))
}

/// Floating layers over the canvas: the task panel, the notice.
pub fn overlays(s: &AppState, cx: &mut Cx<AppState>, p: &Palette) -> Vec<El> {
    let mut out = Vec::new();
    if s.features.review.open {
        let t = crate::cards::theme(s, cx);
        let window = cx.layout_size("root").unwrap_or([1280.0, 800.0]);
        out.extend(review::view::view(s, p, &t, window));
    }
    out.extend(review::dock::panel(s, p));
    if s.route.selected.len() >= 2 {
        let window = cx.layout_size("root").unwrap_or([1280.0, 800.0]);
        out.extend(multi::view(s, p, window));
    }
    if s.features.compare.base.is_some() {
        let t = crate::cards::theme(s, cx);
        let window = cx.layout_size("root").unwrap_or([1280.0, 800.0]);
        out.extend(compare::view(s, p, &t, window));
    }
    out.extend(agent_tasks::view::panel(s, cx, p));
    out.extend(polls::view::dock(s, p));
    if s.features.polls.panel_open {
        let t = crate::cards::theme(s, cx);
        let window = cx.layout_size("root").unwrap_or([1280.0, 800.0]);
        out.extend(polls::view::panel(s, p, &t, window));
    }
    out.extend(nested::project_view::view(s, p));
    out.extend(imports::from_app_view(s, p));
    out.extend(pod_layouts::view(s, p));
    out.extend(spokes::view::picker(s, p));
    out.extend(spokes::view::clay(s, p));
    out.extend(notice::view(s, p));
    out
}

/// The agent capacity answered and says no agent is connected.
pub fn no_agent_connected(s: &AppState) -> bool {
    matches!(s.features.tasks.poller.data.as_ref().and_then(agent_tasks::data::AgentTasksData::known), Some((_, 0)))
}
