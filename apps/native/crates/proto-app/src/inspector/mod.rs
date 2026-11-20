//! The floating glass inspector (proto-worker-6; spec: docs/spec-inspector.md): the selected
//! design's details, the prompt composer, the space's explorations or projects, agent runs.
//! The shell's slot places the panel (340pt, below the header, 16pt from the right and bottom).

pub mod data;
mod details;
pub mod icons;
pub mod more_menu;
mod nested;
mod pods;
mod starred;
pub mod scenes;
pub mod style;

use rune::{Cx, El, Tx, col, el, text};

use crate::app::state::AppState;
use crate::app::style::Palette;

#[derive(Default)]
pub struct InspectorState {
    pub details: details::DetailsState,
    /// The ⋯ menu was open at the last poll (to notice it opening).
    pub more_was_open: bool,
    /// Projects' pod records (pod links).
    pub pods: pods::PodsState,
}

/// The main canvas' note in place of the composer.
const ROOT_ONLY_PROJECTS: &str = "The main canvas holds only projects. Open a project first, or create one with + → Project.";

/// Timers (the "copied" tick). True when something visible changed.
pub fn poll(s: &mut AppState, clock: f64, _tx: &mut Tx) -> bool {
    s.inspector.details.poll(clock) | more_menu::poll(s) | pods::poll(s) | starred::poll(s)
}

/// The ⋯ menu's popup, shown while `s.shell.more_open` (anchored under the header's "more"
/// button, top right). Close it with `s.shell.more_open = false`.
pub fn more_menu(s: &AppState, _cx: &mut Cx<AppState>, p: &Palette) -> Option<El> {
    Some(more_menu::view(s, &style::ink(p)))
}

/// The panel: `GLASS_PANEL` with a scrolling body.
pub fn panel(s: &AppState, cx: &mut Cx<AppState>, p: &Palette) -> El {
    let k = style::ink(p);
    let design = data::selected_design(s);
    // `p-4` inside the panel's 1px border (CSS lays the border out; Rune's takes no space).
    let mut body = col().key("inspector-body").w_full().padding(style::PAD + 1.0).gap(style::PAD)
        .children(design.as_deref().map(|d| details::view(s, cx, d, &k)));
    body = if s.route.is_root() {
        body.child(el().key("insp-root-note").w_full().radius(style::R_LG).border(1.0, k.border).border_dash(4.0, 3.0)
            .px(12.0).py(10.0)
            .child(text(ROOT_ONLY_PROJECTS).font(style::XS).color(k.muted_fg).wrap().w_full()))
    } else {
        body.child(crate::composer::view(s, cx, design.as_deref(), p, &k))
    };
    // Feature sections (worker-8: lanes, hub/spokes, agent progress, notes), each with its own separator.
    body = body.children(crate::features::inspector_sections(s, cx, p, design.as_deref()));
    body = body.children(nested::view(s, cx, design.as_deref(), &k));
    let scroll = col().key("inspector-scroll").w_full().h_full().min_h(0.0).clip_rounded(style::R_2XL)
        .scroll(rune::ScrollSpec { stick_bottom: false })
        .child(body);
    // Popovers sit after the scroll body, outside its clip.
    let mut overlays = details::overlays(s, design.as_deref(), &k);
    if !s.route.is_root() {
        overlays.extend(crate::composer::overlays(s, cx, p, &k));
    }
    style::glass_panel(col().key("inspector").w_full().h_full().radius(style::R_2XL), p)
        .child(scroll)
        .children(overlays)
}
