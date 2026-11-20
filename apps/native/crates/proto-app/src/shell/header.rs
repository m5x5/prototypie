//! The header band (48pt, shared with the macOS traffic lights). No bar: the left group (logo ›
//! breadcrumb) and the right group (actions) each sit on a soft backdrop in the canvas colour
//! that fades out downwards and towards the centre, so the middle shows the canvas.

use rune::{Cx, El, TextStyle, el, row, text};

use super::icons;
use crate::app::insets::HEADER_H;
use crate::app::state::AppState;
use crate::app::style::{self, Palette};

/// How far a backdrop reaches past its group towards the centre, fading out over that width.
const FADE_X: f32 = 64.0;
/// The backdrop is solid for the top 40% and fades out over the rest (the reference's
/// `from-40% to-transparent`).
const FADE_Y: f32 = HEADER_H * 0.6;

const LABEL: TextStyle = TextStyle::new(14.0, 600.0);
const CRUMB: TextStyle = TextStyle::new(14.0, 500.0);

/// A group's backdrop: a solid fill clipped with soft edges on the inner side and the bottom.
/// It drags the window (the titlebar band), and controls on top keep their presses; not in
/// immersive mode, where the window fills the display and has no titlebar.
fn backdrop(key: &'static str, p: &Palette, fade_left: f32, fade_right: f32, drag: bool) -> El {
    let e = el().key(key)
        .pin(Some(-fade_left), Some(0.0), Some(-fade_right), Some(0.0))
        .clip_fade(fade_left, 0.0, fade_right, FADE_Y);
    (if drag { e.window_drag() } else { e })
        .child(el().w_full().h_full().fill(p.canvas_bg))
}

fn ghost_icon(key: &'static str, label: &'static str, p: &Palette, icon: El) -> El {
    row().key(key).role("button").label(label).tab_index(1).size(32.0, 32.0).radius(16.0).center()
        .hover(p.hover).press(p.press)
        .child(icon)
}

fn logo(p: &Palette) -> El {
    let mark = el().key("logo-mark").size(30.0, 30.0).radius(15.0).center()
        .linear_gradient(145f32.to_radians(), if p.dark {
            [(0.0, rune::color::hex("#2a2a2a").unwrap()), (1.0, rune::color::hex("#151515").unwrap())]
        } else {
            [(0.0, rune::Color::WHITE), (1.0, rune::color::hex("#ededed").unwrap())]
        })
        .border(1.0, p.border)
        .child(icons::layers(15.0, p.fg));
    row().key("logo").role("button").label("Main canvas").tab_index(1).h(40.0).gap(9.0).pr(6.0).center_y()
        .hover_group()
        .on_press(|s: &mut AppState| s.go_to_main_canvas())
        .child(mark.group_opacity(1.0, 0.8))
        .child(text("Prototypie").font(LABEL).color(p.fg).group_opacity(1.0, 0.8))
}

/// The space path after the logo: ancestors navigate, the active space is bold. (No "Back up"
/// button: the logo goes to the main canvas, the crumbs to their spaces.)
fn breadcrumb(s: &AppState, p: &Palette) -> Option<El> {
    s.route.path.last()?;
    let sep = || el().w(14.0).h(14.0).center().child(icons::chevron_right(14.0, p.muted_fg.alpha(0.6)));
    // Crumbs, the "…" collapse and the inline rename live in `crumbs` (proto-worker-7).
    Some(row().key("breadcrumb").gap(4.0).center_y().child(sep()).children(super::crumbs::row_children(s, p, &sep)))
}

/// The ⋯ button's icon: a spinner while a bundle imports (the reference's import busy state).
fn more_icon(s: &AppState, p: &Palette) -> El {
    if s.features.imports.busy {
        rune::icon(&crate::cards::icons::LOADER_CIRCLE, 16.0).color(p.fg).looping(rune::LoopSpec::rotation(1.0))
    } else {
        icons::ellipsis(16.0, p.fg)
    }
}

/// The left group: logo › breadcrumb, after the traffic lights.
pub fn left(s: &AppState, cx: &mut Cx<AppState>, p: &Palette) -> El {
    let immersive = super::immersive::active(s);
    // Immersive: no traffic lights, and the group stays left of the camera housing.
    let leading = if immersive { 0.0 } else { cx.chrome_insets().leading };
    let mut content = row().key("header-left-content").h(HEADER_H).center_y().gap(4.0)
        .pl(if leading > 0.0 { leading + 8.0 } else { 16.0 })
        .child(logo(p))
        .children(breadcrumb(s, p));
    if let Some(n) = super::immersive::notch(s) {
        content = content.max_w(n.left_w - 8.0).clip_soft(16.0);
    }
    el().key("header-left").pin(Some(0.0), Some(0.0), None, None).h(HEADER_H)
        .child(backdrop("header-left-fade", p, 0.0, FADE_X, !immersive))
        .child(content)
}

/// The "+" create button (brand blue): round, or the "+ New ⌄" pill while its menu is open
/// (the reference widens it on hover, focus and open; Rune's hover can't re-run the view, so the
/// pill shows while open).
fn new_button(open: bool) -> El {
    let white = rune::Color::WHITE;
    let b = row().key("new").role("button").label("New").tab_index(1).h(32.0).radius(16.0).center_y()
        .fill(style::brand()).box_shadow(style::brand_highlight())
        .hover(white.alpha(0.08)).press(rune::Color::BLACK.alpha(0.12))
        .help("New page, exploration or project")
        .on_press(|s: &mut AppState| s.shell.new_menu_open = !s.shell.new_menu_open)
        .child(el().size(32.0, 32.0).center().child(icons::plus(16.0, white)));
    if open {
        b.pr(10.0).ml(0.0).gap(0.0)
            .child(text("New").font(TextStyle::new(14.0, 500.0)).color(white))
            .child(el().size(14.0, 14.0).ml(2.0).opacity(0.6).center().child(icons::chevron_down(14.0, white)))
    } else {
        b.w(32.0)
    }
}

/// The right group, in the reference's order: agent capacity · + · feature actions (Review) ·
/// fullscreen · ⋯.
pub fn right(s: &AppState, cx: &mut Cx<AppState>, p: &Palette) -> El {
    let actions = row().key("header-actions").gap(8.0).center_y().children(crate::features::header_actions(s, cx));
    let content = row().key("header-right-content").h(HEADER_H).center_y().gap(8.0).pr(16.0)
        .children(crate::features::agent_capacity(s, cx, p))
        .child(new_button(s.shell.new_menu_open))
        .child(actions)
        // Collapses the inspector sidebar, which frees its width for the canvas.
        .child(style::glass_pill(ghost_icon("sidebar-toggle", if s.canvas.inspector_hidden { "Show sidebar" } else { "Hide sidebar" }, p,
                rune::icon(&crate::cards::icons::PANEL_RIGHT, 16.0).color(p.fg)).size(28.0, 28.0).radius(14.0), p)
            .help(if s.canvas.inspector_hidden { "Show the sidebar" } else { "Hide the sidebar" })
            .on_press(|s: &mut AppState| s.canvas.inspector_hidden = !s.canvas.inspector_hidden))
        // The reference's More trigger is 28×28 (its menu opens 4pt below, at y 42).
        .child(style::glass_pill(ghost_icon("more", "More actions", p, more_icon(s, p)).size(28.0, 28.0).radius(14.0), p)
            .on_press(|s: &mut AppState| s.shell.more_open = !s.shell.more_open));
    el().key("header-right").pin(None, Some(0.0), Some(0.0), None).h(HEADER_H)
        .child(content)
}
