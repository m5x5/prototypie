//! The composer's two menus: recent prompts (bottom bar) and "Who works on it" (the send
//! button's agent badge). Popovers anchored to their buttons, closed by a pick, Esc or the
//! button again.

use rune::{Align, El, Side, col, row, text};

use super::{Menu, agents::{AGENTS, agent_icon}};
use crate::app::prefs::AgentChoice;
use crate::app::state::AppState;
use crate::inspector::icons::{self, CHECK, HISTORY};
use crate::inspector::style::{self, Ink};

/// History rows shown (the reference shows the newest 10).
const HISTORY_ROWS: usize = 10;

fn close(s: &mut AppState) {
    s.composer.menu = Menu::None;
}

fn menu_label(t: &str, k: &Ink) -> El {
    text(t.to_string()).font(style::XS_MEDIUM).color(k.muted_fg).px(6.0).py(4.0)
}

fn item(key: String, k: &Ink) -> El {
    style::menu_item(row(), k).key(key).role("menuitem").w_full().h(28.0).px(6.0).gap(6.0).radius(style::R_SM).center_y().hover(k.muted)
}

pub fn overlay(s: &AppState, k: &Ink) -> Option<El> {
    match s.composer.menu {
        Menu::None => None,
        Menu::History => Some(history(s, k)),
        Menu::Agent => Some(agent(s, k)),
    }
}

fn history(s: &AppState, k: &Ink) -> El {
    let rows = s.prefs.prompt_history.iter().take(HISTORY_ROWS).enumerate().map(|(i, p)| {
        let pick = p.clone();
        item(format!("composer-history-{i}"), k)
            .label(p.clone()).help(p.clone())
            .on_press(move |s: &mut AppState| {
                s.composer.menu = Menu::None;
                super::set_text(s, &pick);
            })
            .child(icons::draw(&HISTORY, 16.0, k.muted_fg))
            .child(text(p.lines().next().unwrap_or("").to_string()).font(style::MENU).color(k.fg).truncate().grow(1.0).min_w(0.0))
    });
    style::popover(col().key("composer-history-menu").role("menu").focus_group().w(288.0).padding(4.0), k)
        .modal(true, close)
        .child(menu_label("Recent prompts", k))
        .children(rows)
        .anchored("composer-history", Side::Bottom, Align::End, 4.0)
}

fn agent(s: &AppState, k: &Ink) -> El {
    let current = s.prefs.agent_choice;
    let rows = AGENTS.iter().map(|a| {
        let choice: AgentChoice = a.choice;
        item(format!("composer-agent-{}", a.label), k)
            .role("menuitemradio").label(a.label).help(a.hint)
            .on_press(move |s: &mut AppState| {
                s.composer.menu = Menu::None;
                s.prefs_mut().agent_choice = choice;
            })
            .child(agent_icon(choice, 16.0, k))
            .child(text(a.label).font(style::MENU).color(k.fg).grow(1.0))
            .children((choice == current).then(|| icons::draw(&CHECK, 14.0, k.fg)))
    });
    style::popover(col().key("composer-agent-menu").role("menu").focus_group().w(176.0).padding(4.0), k)
        .modal(true, close)
        .child(menu_label("Who works on it", k))
        .children(rows)
        .anchored("composer-send", Side::Top, Align::End, 6.0)
}
