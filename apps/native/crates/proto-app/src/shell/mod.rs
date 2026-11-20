//! The window: canvas underneath, header groups, the inspector slot and overlays on top.

pub mod banner;
pub mod crumbs;
mod header;
pub mod icons;
pub mod immersive;
mod new_menu;

use rune::{Cx, El, el};

use crate::app::insets::{EDGE, INSPECTOR_TOP, INSPECTOR_W};
use crate::app::state::{AppState, ThemePref};
use crate::app::style;

thread_local! {
    static WINDOW: std::cell::RefCell<Option<rune::WindowHandle>> = const { std::cell::RefCell::new(None) };
}

/// Called by `App::on_window` each time the native window is created.
pub fn set_window(h: rune::WindowHandle) {
    WINDOW.with(|w| *w.borrow_mut() = Some(h));
}

/// The native window (None headless and before it's created), e.g. to attach a live webview.
/// UI thread only.
pub fn window() -> Option<rune::WindowHandle> {
    WINDOW.with(|w| w.borrow().clone())
}

/// Header and menu state (proto-worker-4).
#[derive(Default)]
pub struct ShellState {
    /// The New menu is open.
    pub new_menu_open: bool,
    /// The ⋯ menu is open (its content is `inspector::more_menu`, worker-6).
    pub more_open: bool,
    /// Breadcrumb: inline rename of the current space and the "…" menu (`crumbs`, proto-worker-7).
    pub crumbs: crumbs::CrumbsState,
    /// The window covers the whole display, notch band included (`immersive`).
    pub immersive: immersive::Immersive,
}

/// The root view. Layers, bottom to top: canvas (dots + world), header groups, inspector slot,
/// feature overlays, fullscreen.
pub fn view(s: &AppState, cx: &mut Cx<AppState>) -> El {
    crate::perf::mark("first-view");
    let dark = match s.theme {
        ThemePref::System => cx.system_dark(),
        ThemePref::Dark => true,
        ThemePref::Light => false,
    };
    cx.caption_color(if dark { rune::Color::WHITE } else { rune::Color::BLACK });
    let p = style::palette(dark);
    // Keyboard focus as the reference has it: every pressable is a Tab stop (header first, via
    // tab_index 1 on its controls), with its ring: `focus-visible:ring-[3px] ring-ring/50`.
    cx.web_focus();
    cx.focus_ring(3.0, 0.0, rune::color::hex("#737373").unwrap().alpha(0.5));
    let canvas = crate::canvas::view(s, cx, &p);
    let left = header::left(s, cx, &p);
    let right = header::right(s, cx, &p);
    // The inspector floats over the canvas, just below the header, 16pt from the right and bottom.
    let inspector = el().key("inspector-slot")
        .pin(None, Some(INSPECTOR_TOP), Some(EDGE), Some(EDGE))
        .w(INSPECTOR_W)
        .child(crate::inspector::panel(s, cx, &p));
    // Everything sits in one keyed "app" subtree, so the Annotate layer (worker-8), its
    // sibling on top, picks elements inside it with `element_at_in("app", …)`.
    let app = el().key("app").w_full().h_full()
        .child(canvas)
        .child(left)
        .child(right)
        .children((!s.canvas.inspector_hidden).then_some(inspector))
        .children(banner::view(s, &p))
        .children(s.shell.new_menu_open.then(|| {
            // Anchored under "+": the feature actions after it shift it left by their width.
            let actions_w = cx.layout_size("header-actions").map_or(0.0, |s| s[0] + 8.0);
            new_menu::view(s, &p, actions_w)
        }))
        .children(crumbs::menu(s, &p))
        .children(s.shell.more_open.then(|| crate::inspector::more_menu(s, cx, &p)).flatten())
        .children(crate::features::overlays(s, cx, &p))
        .children(crate::fullscreen::overlay(s, cx, &p));
    crate::canvas::gestures(el().key("root").w_full().h_full().fill(p.canvas_bg)).child(app)
        .children(crate::features::annotate::layer(s, cx, &p))
}
