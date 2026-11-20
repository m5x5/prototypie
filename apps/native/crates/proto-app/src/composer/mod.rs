//! The prompt composer (proto-worker-6; spec: docs/spec-inspector.md §3): target tabs joined to
//! one box holding the mention chips, the text and a bottom bar with the 1–6 count, recent
//! prompts, the @/# buttons and a round brand-blue send arrow whose badge picks the agent.
//!
//! Sending builds the request and queues it in `ComposerState::outbox`; proto-ai's runner (wired
//! by the app) takes it from there.

mod agents;
mod count;
pub mod hooks;
mod menus;

use proto_core::{Design, DesignKind, VariationRequest};
use rune::{Cx, El, Key, KeyEvent, TextEdit, Tx, col, el, row, text, text_input};

use crate::app::prefs::AgentChoice;
use crate::app::state::AppState;
use crate::app::style::{self as app_style, Palette};
use crate::inspector::data;
use crate::inspector::icons::{self, ARROW_UP, HISTORY, LAYERS, PEN_LINE, WAND_SPARKLES};
use crate::inspector::style::{self, Ink};

/// Which design the prompt goes to while one is selected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Target {
    /// New variations of the selected design.
    #[default]
    This,
    /// One revision of each of its ready variations.
    Kids,
    /// Change the selected design in place.
    Edit,
}

/// What a send does, from the selection and the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A brief for a new design (nothing ready selected).
    New,
    This,
    Kids,
    Edit,
}

/// The open menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Menu {
    #[default]
    None,
    History,
    Agent,
}

/// A request the composer sent, waiting for the generation runner.
#[derive(Clone, Debug, PartialEq)]
pub enum Outgoing {
    /// POST /api/variations: new designs in `space_id`, branching off `parent_id`. `origin` = top-left of the
    /// first new card (default: below the parent / right of everything); `run_id` = a pre-allocated run id
    /// (`runner::new_run_id()`), so the sender can key lanes or cancel by it. Run by `crate::runner`.
    Generate {
        space_id: String,
        parent_id: Option<String>,
        kind: DesignKind,
        label: Option<String>,
        request: VariationRequest,
        origin: Option<proto_core::Point>,
        run_id: Option<String>,
    },
    /// POST /api/edit: change `design_id` in place.
    Edit { design_id: String, request: VariationRequest },
}

pub struct ComposerState {
    pub edit: TextEdit,
    /// The draft key `edit` belongs to (`draft_key`).
    pub key: String,
    pub target: Target,
    /// Variations per send (1–6).
    pub count: u8,
    pub menu: Menu,
    /// Why the last in-place edit couldn't start.
    pub edit_error: Option<String>,
    /// Sent requests for the runner; drained by it.
    pub outbox: Vec<Outgoing>,
    /// The caret at the last poll: a click that only moves it re-runs mention detection.
    pub last_caret: Option<usize>,
}

impl Default for ComposerState {
    fn default() -> Self {
        ComposerState {
            edit: TextEdit::default(),
            key: NEW.into(),
            target: Target::default(),
            count: 3,
            menu: Menu::None,
            edit_error: None,
            outbox: Vec::new(),
            last_caret: None,
        }
    }
}

/// The draft key with nothing selected.
const NEW: &str = "new";

/// The draft key: the selected design's id, or "new". Mentions are kept per key too.
pub fn draft_key(s: &AppState) -> String {
    data::selected_design(s).map_or_else(|| NEW.to_string(), |d| d.id.clone())
}

pub fn edit(s: &AppState) -> &TextEdit {
    &s.composer.edit
}

/// The field's model, for changes from outside (mentions). It belongs to the current selection's
/// draft; the next poll saves the change to it.
pub fn edit_mut(s: &mut AppState) -> &mut TextEdit {
    follow_selection(s);
    &mut s.composer.edit
}

/// Focus the prompt field.
pub fn focus(tx: &mut Tx) {
    tx.focus("composer", false);
}

/// Put `text` in the box (the "Reuse prompt" and history picks), caret at the end.
pub fn set_text(s: &mut AppState, text: &str) {
    follow_selection(s);
    let mut e = TextEdit::new(text);
    e.set_caret(text.len());
    e.focused = true;
    s.composer.edit = e;
    save_draft(s);
}

fn save_draft(s: &mut AppState) {
    let key = s.composer.key.clone();
    let text = s.composer.edit.text.clone();
    if s.prefs.draft(&key) != text {
        s.prefs_mut().set_draft(&key, &text);
    }
}

/// Follow the selection: each design (or "new") keeps its own draft and starts on "This design".
/// Focuses the field when a feature asks (Explore lands on its seed: `features.focus_prompt`).
pub fn poll(s: &mut AppState, _clock: f64, tx: &mut Tx) -> bool {
    if s.features.focus_prompt.take().is_some() {
        focus(tx);
    }
    // Rune reports text edits, not caret-only moves: a click elsewhere in the text re-detects here.
    let caret = s.composer.edit.focused.then(|| s.composer.edit.range().1);
    if caret != s.composer.last_caret {
        s.composer.last_caret = caret;
        if caret.is_some() {
            hooks::on_change(s, tx);
        }
    }
    let switched = follow_selection(s);
    if !switched && s.prefs.draft(&s.composer.key) != s.composer.edit.text {
        save_draft(s);
    }
    switched
}

/// Load the selection's draft when the selection changed (each design, or "new", keeps its own,
/// starting on "This design"). True when it switched.
fn follow_selection(s: &mut AppState) -> bool {
    let key = draft_key(s);
    if key == s.composer.key {
        return false;
    }
    let text = s.prefs.draft(&key).to_string();
    let c = &mut s.composer;
    c.key = key;
    c.edit = TextEdit::new(text);
    c.target = Target::This;
    c.menu = Menu::None;
    c.edit_error = None;
    true
}

/// The selection's mode and how many ready variations it has.
pub fn mode(s: &AppState, design: Option<&Design>) -> (Mode, usize) {
    let Some(d) = design.filter(|d| data::is_ready(d)) else { return (Mode::New, 0) };
    let kids = data::ready_children(s, d).len();
    let mode = match s.composer.target {
        Target::Edit => Mode::Edit,
        Target::Kids if kids > 0 => Mode::Kids,
        _ => Mode::This,
    };
    (mode, kids)
}

fn can_send(mode: Mode, text: &str, design: Option<&Design>) -> bool {
    let has_text = !text.trim().is_empty();
    match mode {
        Mode::Edit => has_text && design.is_some_and(|d| d.editing.is_none()),
        Mode::Kids => has_text,
        Mode::This => true,
        Mode::New => has_text,
    }
}

fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

fn send_label(mode: Mode, count: u8, kids: usize) -> String {
    match mode {
        Mode::New => format!("Generate {}", plural(count.into(), "direction")),
        Mode::Kids => format!("Revise {}", plural(kids, "variation")),
        Mode::Edit => "Edit this design in place".into(),
        Mode::This => format!("Propose {}", plural(count.into(), "variation")),
    }
}

fn placeholder(mode: Mode) -> &'static str {
    match mode {
        Mode::New => "Describe what to build, e.g. landing page for a climbing gym…",
        Mode::Kids => "What should change in all of them? e.g. use Lucide icons…",
        Mode::Edit => "What should change? e.g. make the save button pill-shaped…",
        Mode::This => "How should it vary? e.g. bolder hierarchy, try a dark theme…",
    }
}

/// The field's accessible name.
fn field_label(mode: Mode, kids: usize) -> String {
    match mode {
        Mode::New => "Brief".into(),
        Mode::Kids => format!("What should change in its {kids} variations?"),
        Mode::Edit => "What should change in this design?".into(),
        Mode::This => "How should it vary?".into(),
    }
}

fn agent_name(a: AgentChoice) -> String {
    match a {
        AgentChoice::Claude => "claude",
        AgentChoice::Codex => "codex",
    }
    .into()
}

/// Send the prompt (Enter or the arrow). Not on the main canvas, which takes no designs.
pub fn send(s: &mut AppState, _tx: &mut Tx) {
    if s.route.is_root() {
        return;
    }
    let design = data::selected_design(s);
    let (mode, _) = mode(s, design.as_deref());
    let text = s.composer.edit.text.trim().to_string();
    if !can_send(mode, &text, design.as_deref()) {
        return;
    }
    let (pod, components) = hooks::request_parts(s);
    let base = |count: u32| {
        let mut r = VariationRequest { instruction: text.clone(), count, agent: Some(agent_name(s.prefs.agent_choice)), ..VariationRequest::default() };
        if let Some(p) = &pod {
            r.extra.insert("podContext".into(), serde_json::to_value(p).unwrap_or_default());
        }
        if let Some(c) = &components {
            r.extra.insert("components".into(), serde_json::to_value(c).unwrap_or_default());
        }
        r
    };
    let out: Vec<Outgoing> = match (mode, design.as_deref()) {
        (Mode::Edit, Some(d)) => vec![Outgoing::Edit { design_id: d.id.clone(), request: base(1) }],
        (Mode::Kids, Some(d)) => {
            let head: String = text.chars().take(40).collect();
            data::ready_children(s, d).into_iter().map(|c| Outgoing::Generate {
                space_id: c.space_id.clone(),
                parent_id: Some(c.id.clone()),
                kind: DesignKind::Variation,
                label: Some(format!("{head}: {}", c.title)),
                request: VariationRequest { base_html: Some(c.html.clone()), base_title: Some(c.title.clone()), ..base(1) },
                origin: None,
                run_id: None,
            }).collect()
        }
        (Mode::This, Some(d)) => vec![Outgoing::Generate {
            space_id: s.route.space.clone(),
            parent_id: Some(d.id.clone()),
            kind: DesignKind::Variation,
            label: None,
            request: VariationRequest { base_html: Some(d.html.clone()), base_title: Some(d.title.clone()), ..base(s.composer.count.into()) },
            origin: None,
            run_id: None,
        }],
        _ => vec![Outgoing::Generate {
            space_id: s.route.space.clone(),
            parent_id: None,
            kind: DesignKind::Page,
            label: None,
            request: base(s.composer.count.into()),
            origin: None,
            run_id: None,
        }],
    };
    s.composer.outbox.extend(out);
    s.composer.edit_error = None;
    s.composer.edit.clear();
    s.prefs_mut().remember_prompt(&text);
    save_draft(s);
    hooks::clear(s);
}

/// Keys of the prompt field: mentions first, then Esc closes a menu. Enter sends via `on_submit`.
fn on_key(s: &mut AppState, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if hooks::on_key(s, ev, tx) {
        return true;
    }
    if ev.key == Key::Escape && s.composer.menu != Menu::None {
        s.composer.menu = Menu::None;
        return true;
    }
    false
}

fn toggle_menu(s: &mut AppState, m: Menu) {
    s.composer.menu = if s.composer.menu == m { Menu::None } else { m };
}

/// The target tabs over the box (not for a new design).
fn tabs(mode: Mode, kids: usize, k: &Ink) -> El {
    let tab = |key: &'static str, active: bool, disabled: bool, help: &str, target: Target| {
        // `flex-1` (basis 0): equal shares, but never narrower than the label's longest word.
        let t = style::focusable(row(), k).key(key).role("tab").w(0.0).grow(1.0).h(32.0).px(1.0).gap(6.0).center()
            .corners([style::R_LG, style::R_LG, 0.0, 0.0]).help(help.to_string())
            .on_press(move |s: &mut AppState| {
                s.composer.target = target;
                s.composer.edit_error = None;
            });
        let t = if active { t.fill(k.field).border_sides([1.0, 1.0, 0.0, 1.0], k.input) } else { t };
        if disabled { t.disabled(true).opacity(0.5) } else { t }
    };
    let ink = |active: bool| if active { k.fg } else { k.muted_fg };
    // Labels wrap and centre, as the reference's button text does when its tab is narrow.
    let label = |t: &str, c: rune::Color| text(t.to_string()).font(style::TAB).color(c).wrap().align(rune::Align::Center);
    let this = mode == Mode::This;
    let edit = mode == Mode::Edit;
    let kid = mode == Mode::Kids;
    row().key("composer-tabs").role("group").label("Target").focus_group().w_full()
        .child(tab("composer-tab-this", this, false, "New variations of this design", Target::This)
            .child(icons::draw(&PEN_LINE, 14.0, ink(this)))
            .child(label("This design", ink(this))))
        .child(tab("composer-tab-kids", kid, kids == 0,
            if kids > 0 { "Revise each variation, one revision each" } else { "No variations of this design yet" }, Target::Kids)
            .child(icons::draw(&LAYERS, 14.0, ink(kid)))
            .child(label("Its variations", ink(kid)))
            .child(text(kids.to_string()).font(style::PILL_NUM).color(ink(kid)).px(6.0).radius(999.0).fill(k.muted)))
        .child(tab("composer-tab-edit", edit, false, "Change this design in place with AI (undo on the card)", Target::Edit)
            .child(icons::draw(&WAND_SPARKLES, 14.0, ink(edit)))
            .child(label("Edit", ink(edit))))
}

/// A 26pt ghost icon button of the bottom bar.
pub fn bar_button(key: &'static str, label: &str, open: bool, k: &Ink) -> El {
    let b = style::focusable(row(), k).key(key).role("button").label(label.to_string()).help(label.to_string())
        .size(26.0, 26.0).radius(style::R_MD).center().hover(k.muted).press(k.press);
    if open { b.fill(k.muted) } else { b }
}

/// The round send arrow with the agent badge at its bottom-right.
fn send_button(s: &AppState, mode: Mode, kids: usize, enabled: bool, k: &Ink) -> El {
    let label = send_label(mode, s.composer.count, kids);
    let arrow = style::focusable(row(), k).key("composer-send").role("button").label(format!("{label} (Enter)")).help(format!("{label} · ↵"))
        .size(32.0, 32.0).radius(16.0).center()
        .fill(app_style::brand()).box_shadow(app_style::brand_highlight())
        .hover(rune::Color::WHITE.alpha(0.08)).press(rune::Color::BLACK.alpha(0.12))
        .on_press_tx(send)
        .child(icons::draw(&ARROW_UP, 16.0, rune::Color::WHITE));
    let arrow = if enabled { arrow } else { arrow.disabled(true).opacity(0.5) };
    let agent = s.prefs.agent_choice;
    let a = agents::agent(agent);
    let badge = style::focusable(row(), k).key("composer-agent").role("button").label(format!("Agent: {}. Change agent", a.label))
        .help(format!("{} works on it", a.label))
        .absolute(20.0, 20.0).size(18.0, 18.0).radius(9.0).center()
        // The reference rings it in `#151515` in both themes.
        .fill(k.muted).border(2.0, rune::color::hex("#151515").expect("hex")).hover(k.ring)
        .on_press(|s: &mut AppState| toggle_menu(s, Menu::Agent))
        .child(agents::agent_icon(agent, 10.0, k));
    el().key("composer-send-wrap").size(32.0, 32.0).child(arrow).child(badge)
}

pub fn view(s: &AppState, _cx: &mut Cx<AppState>, design: Option<&Design>, p: &Palette, k: &Ink) -> El {
    let (mode, kids) = mode(s, design);
    let enabled = can_send(mode, &s.composer.edit.text, design);
    let field = text_input(&s.composer.edit, style::PROMPT).key("composer").w_full()
        .label(field_label(mode, kids))
        .color(k.fg)
        .placeholder(placeholder(mode), k.muted_fg)
        .caret(k.fg, style::blue_500().alpha(0.35))
        .on_edit(|s: &mut AppState| &mut s.composer.edit)
        .on_submit(send)
        .on_input_key(on_key)
        .on_input_change(|s: &mut AppState, tx: &mut Tx| {
            save_draft(s);
            hooks::on_change(s, tx);
        });
    // `p-1.5` inside the box's 1px border (Rune's border takes no space).
    let mut bar = row().key("composer-bar").w_full().px(7.0).pt(6.0).pb(7.0).gap(6.0).center_y();
    bar = match mode {
        Mode::Kids | Mode::Edit => bar.child(text(if mode == Mode::Kids { "One revision each" } else { "Replaces this card" })
            .font(style::XS).color(k.muted_fg).px(4.0)),
        _ => bar.child(count::view(s.composer.count, k)),
    };
    if !s.prefs.prompt_history.is_empty() {
        bar = bar.child(bar_button("composer-history", "Recent prompts", s.composer.menu == Menu::History, k)
            .on_press(|s: &mut AppState| toggle_menu(s, Menu::History))
            .child(icons::draw(&HISTORY, 14.0, k.muted_fg)));
    }
    bar = bar.children(hooks::buttons(s, p)).child(el().grow(1.0)).child(send_button(s, mode, kids, enabled, k));
    let tabs_on = mode != Mode::New;
    // The box squares the corner under the active tab and tucks 1pt under the tabs.
    let corners = match (tabs_on, mode) {
        (false, _) | (true, Mode::Kids) => [style::R_XL; 4],
        (true, Mode::Edit) => [style::R_XL, 0.0, style::R_XL, style::R_XL],
        _ => [0.0, style::R_XL, style::R_XL, style::R_XL],
    };
    let bx = col().key("composer-box").w_full().corners(corners).border(1.0, k.input).fill(k.field)
        .mt(if tabs_on { -1.0 } else { 0.0 })
        .children(hooks::chips(s, p))
        // `rows=3` at leading-snug (3 × 18.5) + `pt-2 pb-1` + the top border: 67pt, like the reference textarea.
        .child(col().key("composer-field").w_full().min_h(67.0).px(11.0).pt(9.0).pb(4.0).child(field))
        .child(el().w_full().h(1.0).fill(k.border))
        .child(bar);
    let error = (mode == Mode::Edit).then(|| s.composer.edit_error.clone()).flatten()
        .map(|e| text(e).font(style::XS).color(k.destructive).wrap().w_full());
    col().key("composer-wrap").w_full().gap(style::PAD)
        .child(col().key("composer-group").role("group").label("Prompt").w_full()
            .children(tabs_on.then(|| tabs(mode, kids, k)))
            .child(bx))
        .children(error)
        .children(hooks::context_panel(s, p))
}

/// Popups that must draw outside the panel's scroll clip: the open menu and the mention list.
pub fn overlays(s: &AppState, cx: &mut Cx<AppState>, p: &Palette, k: &Ink) -> Vec<El> {
    menus::overlay(s, k).into_iter().chain(hooks::listbox(s, cx, p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_the_mode() {
        assert_eq!(send_label(Mode::New, 1, 0), "Generate 1 direction");
        assert_eq!(send_label(Mode::This, 3, 0), "Propose 3 variations");
        assert_eq!(send_label(Mode::Kids, 3, 1), "Revise 1 variation");
        assert_eq!(send_label(Mode::Edit, 3, 2), "Edit this design in place");
    }

    #[test]
    fn sending_needs_text_except_when_varying() {
        assert!(!can_send(Mode::New, "  ", None));
        assert!(can_send(Mode::New, "a page", None));
        assert!(can_send(Mode::This, "", None));
        assert!(!can_send(Mode::Kids, "", None));
    }
}
