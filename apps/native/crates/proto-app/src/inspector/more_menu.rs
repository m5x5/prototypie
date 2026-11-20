//! The header's ⋯ menu (spec-inspector §7): the features' items (Decisions with its poll status,
//! worker-8), then "Import bundle" with the bundles found on disk (worker-8's imports). Both
//! come from worker-8's `crate::features` entries.

use rune::{Align, El, Side, col, row, text};

use super::icons::{self, FOLDER_INPUT, LOADER_CIRCLE, VOTE};
use super::style::{self, Ink};
use crate::app::state::AppState;

/// An entry a feature adds to the menu.
pub struct MoreItem {
    pub key: &'static str,
    pub icon: &'static rune::Icon,
    pub label: &'static str,
    /// Muted text at the right (Decisions: "2 in poll · 1 shared" or "Share a poll").
    pub status: Option<String>,
    pub run: fn(&mut AppState),
}

/// A bundle that "Import bundle" can import.
pub struct Bundle {
    pub file: String,
    pub name: String,
    pub count: usize,
}

pub enum Bundles {
    Loading,
    Ready(Vec<Bundle>),
}

/// The features' menu items: Decisions with its poll status (worker-8's popover).
fn feature_items(s: &AppState) -> Vec<MoreItem> {
    vec![MoreItem {
        key: "more-decisions",
        icon: &VOTE,
        label: "Decisions",
        status: Some(crate::features::decisions_status(s)),
        run: crate::features::open_decisions,
    }]
}

/// The menu just opened: worker-8 fetches the bundles once per open.
fn on_open(s: &mut AppState) {
    crate::features::on_more_open(s);
}

/// From `inspector::poll`: call `on_open` when the menu goes from closed to open. True when
/// something visible changed (never: the feature redraws when its fetch lands).
pub fn poll(s: &mut AppState) -> bool {
    let open = s.shell.more_open;
    if open && !s.inspector.more_was_open {
        on_open(s);
    }
    s.inspector.more_was_open = open;
    false
}

/// The bundles (worker-8's imports, `GET /api/bundles`).
fn bundles(s: &AppState) -> Bundles {
    match crate::features::bundles(s) {
        None => Bundles::Loading,
        Some(list) => Bundles::Ready(list.iter().map(|b| Bundle { file: b.file.clone(), name: b.name.clone(), count: b.count }).collect()),
    }
}

/// Import one bundle (worker-8): a new project or exploration, entered, with a notice.
fn import_bundle(s: &mut AppState, file: &str) {
    crate::features::import_bundle(s, file);
}

fn item(key: String, k: &Ink, enabled: bool) -> El {
    let r = style::menu_item(row(), k).key(key).role("menuitem").w_full().h(28.0).px(6.0).gap(6.0).radius(style::R_SM).center_y();
    if enabled { r.hover(k.muted) } else { r.opacity(0.5).disabled(true) }
}

pub fn view(s: &AppState, k: &Ink) -> El {
    let features = feature_items(s).into_iter().map(|it| {
        let run = it.run;
        item(it.key.to_string(), k, true).label(it.label)
            .on_press(move |s: &mut AppState| {
                s.shell.more_open = false;
                run(s);
            })
            .child(icons::draw(it.icon, 16.0, k.muted_fg))
            .child(text(it.label).font(style::MENU).color(k.fg))
            .child(rune::el().grow(1.0))
            .children(it.status.map(|st| text(st).font(style::XS).color(k.muted_fg).truncate().min_w(0.0)))
    });
    let mut menu = col().key("more-menu").role("menu").focus_group().w(256.0).padding(4.0)
        .children(features)
        .child(rune::el().w_full().h(1.0).my(4.0).fill(k.border))
        .child(text("Import bundle").font(style::XS_MEDIUM).color(k.muted_fg).px(6.0).py(4.0));
    menu = match bundles(s) {
        Bundles::Loading => menu.child(item("more-bundles-loading".into(), k, false)
            .child(icons::draw(&LOADER_CIRCLE, 16.0, k.muted_fg))
            .child(text("Loading…").font(style::MENU).color(k.fg))),
        Bundles::Ready(list) if list.is_empty() => menu.child(item("more-bundles-none".into(), k, false)
            .child(text("No bundles yet").font(style::MENU).color(k.fg))),
        Bundles::Ready(list) => menu.children(list.into_iter().map(|b| {
            let file = b.file.clone();
            item(format!("more-bundle-{}", b.file), k, true).label(b.name.clone())
                .on_press(move |s: &mut AppState| {
                    s.shell.more_open = false;
                    import_bundle(s, &file);
                })
                .child(icons::draw(&FOLDER_INPUT, 16.0, k.muted_fg))
                .child(text(b.name).font(style::MENU).color(k.fg).truncate().grow(1.0).min_w(0.0))
                .child(text(b.count.to_string()).font(style::XS_TNUM).color(k.muted_fg))
        })),
    };
    style::popover(menu, k).anchored("more", Side::Bottom, Align::End, 4.0)
        .modal(true, |s: &mut AppState| s.shell.more_open = false)
}
