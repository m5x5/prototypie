//! The breadcrumb's crumbs (reference `src/features/nested/toolbar.tsx` SpacePath + EditableName; proto-worker-7
//! in worker-4's shell): ancestors navigate, the middle of a deep path collapses into a "…" menu (root … parent
//! › current), and the current space's name renames in place: click it or press F2, Enter or blur saves, Esc
//! cancels.

use rune::{Align, El, Key, KeyEvent, Side, TextEdit, TextStyle, Tx, col, el, row, text, text_input};

use super::icons;
use crate::app::state::{AppState, breadcrumb_path};
use crate::app::style::{self, Palette};

/// Ancestors from this many on collapse their middle into the "…" menu (reference `MAX_CRUMBS`).
pub const MAX_CRUMBS: usize = 4;

const CRUMB: TextStyle = TextStyle::new(14.0, 500.0);
const LABEL: TextStyle = TextStyle::new(14.0, 600.0);
const ITEM: TextStyle = TextStyle::new(13.5, 500.0);
/// The rename field: about one 8pt character each, 6 to 28 characters wide (reference `size`, `max-w-56`).
const CHAR_W: f32 = 8.0;
const FIELD_MIN: f32 = 6.0 * CHAR_W;
const FIELD_MAX: f32 = 224.0;

const FIELD_KEY: &str = "crumb-rename";
const MORE_KEY: &str = "crumb-more";

/// A rename in progress: which space, and whether its field has had focus yet (losing it then saves).
#[derive(Clone, Debug, PartialEq)]
pub struct Renaming {
    pub space_id: String,
    seen_focus: bool,
}

#[derive(Default)]
pub struct CrumbsState {
    /// Some while the current space's name is being edited.
    pub renaming: Option<Renaming>,
    /// The rename field's text (meaningful while `renaming`).
    pub edit: TextEdit,
    /// The "…" menu of collapsed ancestors is open.
    pub menu_open: bool,
}

/// Which ancestors show and which collapse: `(visible, hidden)` indices into `ancestors` (path minus the
/// current space). Deep paths keep the first and the last ancestor and hide the middle.
pub fn split(ancestors: usize) -> (Vec<usize>, Vec<usize>) {
    if ancestors >= MAX_CRUMBS {
        (vec![0, ancestors - 1], (1..ancestors - 1).collect())
    } else {
        ((0..ancestors).collect(), Vec::new())
    }
}

/// Start renaming the current space (not on the main canvas), name selected, field focused.
pub fn start(s: &mut AppState, tx: &mut Tx) {
    if begin(s) {
        tx.focus(FIELD_KEY, false);
    }
}

/// `start` without focusing (scenes): false on the main canvas.
pub fn begin(s: &mut AppState) -> bool {
    let Some((id, name)) = s.route.path.last().cloned() else { return false };
    let mut edit = TextEdit::new(name.as_str());
    edit.set_selection(0, name.len());
    edit.focused = true;
    s.shell.crumbs = CrumbsState { renaming: Some(Renaming { space_id: id, seen_focus: false }), edit, menu_open: false };
    true
}

/// Save the draft (trimmed; empty or unchanged does nothing) and leave the field.
pub fn commit(s: &mut AppState) {
    let Some(r) = s.shell.crumbs.renaming.take() else { return };
    let name = s.shell.crumbs.edit.text.trim().to_string();
    let current = s.snap().space(&r.space_id).map(|sp| sp.name.clone());
    if name.is_empty() || current.as_deref() == Some(name.as_str()) {
        return;
    }
    let Some(change) = proto_core::ops::rename_space(s.snap(), &r.space_id, &name) else { return };
    if s.apply(vec![change], "rename space").is_ok() && s.route.space == r.space_id {
        s.route.path = breadcrumb_path(s.snap(), &r.space_id);
    }
}

pub fn cancel(s: &mut AppState) {
    s.shell.crumbs.renaming = None;
}

/// F2 renames the current space (keys nobody focused took; not on the main canvas).
pub fn on_key(s: &mut AppState, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if ev.key != Key::Function(2) || s.route.is_root() || s.shell.crumbs.renaming.is_some() {
        return false;
    }
    start(s, tx);
    true
}

/// Blur saves (reference `onBlur={commit}`); leaving the space drops the rename and the menu.
pub fn poll(s: &mut AppState) -> bool {
    let c = &mut s.shell.crumbs;
    let Some(r) = &mut c.renaming else {
        if c.menu_open && s.route.path.len() <= MAX_CRUMBS {
            c.menu_open = false;
            return true;
        }
        return false;
    };
    if r.space_id != s.route.space {
        c.renaming = None;
        return true;
    }
    if c.edit.focused {
        r.seen_focus = true;
        return false;
    }
    if r.seen_focus {
        commit(s);
        return true;
    }
    false
}

/// The crumbs after the logo's separator: (first ancestor, "…", last ancestor, or all ancestors), each
/// followed by `sep()`, then the current space (its name, or the rename field).
pub fn row_children(s: &AppState, p: &Palette, sep: &dyn Fn() -> El) -> Vec<El> {
    let Some((last, ancestors)) = s.route.path.split_last() else { return Vec::new() };
    let (visible, hidden) = split(ancestors.len());
    let mut out = Vec::new();
    for (n, &i) in visible.iter().enumerate() {
        if n == 1 && !hidden.is_empty() {
            out.push(
                row().key(MORE_KEY).role("button").label("Show hidden spaces").tab_index(1).h(28.0).px(3.0).radius(6.0).center_y()
                    .hover(p.hover).press(p.press)
                    .on_press(|s: &mut AppState| s.shell.crumbs.menu_open = !s.shell.crumbs.menu_open)
                    .child(icons::ellipsis(16.0, p.muted_fg)),
            );
            out.push(sep());
        }
        let (id, name) = &ancestors[i];
        let path = s.route.path[..=i].to_vec();
        let id = id.clone();
        out.push(
            row().key(format!("crumb-{i}")).role("button").label(name.clone()).tab_index(1).h(28.0).px(6.0).radius(6.0).center_y()
                .hover(p.hover).press(p.press)
                .on_press(move |s: &mut AppState| s.enter(id.clone(), path.clone()))
                .child(text(name.clone()).font(CRUMB).color(p.muted_fg).truncate()),
        );
        out.push(sep());
    }
    out.push(current(s, &last.1, p));
    out
}

fn current(s: &AppState, name: &str, p: &Palette) -> El {
    if s.shell.crumbs.renaming.is_some() {
        let chars = s.shell.crumbs.edit.text.chars().count() as f32;
        let w = (chars * CHAR_W + 14.0).clamp(FIELD_MIN, FIELD_MAX);
        let field = text_input(&s.shell.crumbs.edit, CRUMB).key(FIELD_KEY).label("Space name").w(w).single_line()
            .color(p.fg)
            .caret(p.fg, style::brand().alpha(0.35))
            .on_edit(|s: &mut AppState| &mut s.shell.crumbs.edit)
            .on_submit(|s: &mut AppState, _tx: &mut Tx| commit(s))
            .on_input_key(|s: &mut AppState, ev: &KeyEvent, _tx: &mut Tx| {
                if ev.key == Key::Escape {
                    cancel(s);
                    return true;
                }
                false
            });
        return row().key("crumb-current").h(24.0).px(6.0).radius(6.0).center_y()
            .border(1.0, p.border)
            .child(field);
    }
    row().key("crumb-current").role("button").label(format!("Rename {name}")).help("Rename (F2)").tab_index(1).h(28.0).px(6.0).gap(4.0)
        .radius(6.0).center_y()
        .hover(p.hover).press(p.press).hover_group()
        .on_press_tx(|s: &mut AppState, tx: &mut Tx| start(s, tx))
        .child(text(name.to_string()).font(LABEL).color(p.fg).truncate())
        .child(icons::pencil(12.0, p.muted_fg).group_opacity(0.0, 1.0))
}

/// The open "…" menu: the collapsed ancestors, each entering its space. Attach at the root.
pub fn menu(s: &AppState, p: &Palette) -> Option<El> {
    if !s.shell.crumbs.menu_open {
        return None;
    }
    let ancestors = s.route.path.split_last()?.1;
    let (_, hidden) = split(ancestors.len());
    if hidden.is_empty() {
        return None;
    }
    let items = hidden.iter().fold(col().key("crumb-menu-items").gap(2.0).focus_group(), |c, &i| {
        let (id, name) = ancestors[i].clone();
        c.child(
            row().key(format!("crumb-menu-{i}")).role("menuitem").label(name.clone()).focusable(true).w_full().px(10.0).py(7.0)
                .radius(8.0).hover(p.hover).press(p.press)
                .on_press(move |s: &mut AppState| {
                    s.shell.crumbs.menu_open = false;
                    s.enter_space(&id, None);
                })
                .child(text(name).font(ITEM).color(p.fg).truncate()),
        )
    });
    let list = style::glass(col().key("crumb-menu").role("menu").label("Hidden spaces").min_w(160.0).padding(4.0).radius(12.0), p, 0.85)
        .modal(true, |s: &mut AppState| s.shell.crumbs.menu_open = false)
        .child(items);
    Some(el().key("crumb-menu-anchor").anchored(MORE_KEY, Side::Bottom, Align::Start, 6.0).child(list))
}

#[cfg(test)]
#[path = "crumbs_tests.rs"]
mod tests;
