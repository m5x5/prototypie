//! Breadcrumb collapse and inline rename, on an in-memory store with a 5-deep path.

use proto_core::{Change, Store};
use rune::{Env, Key, KeyEvent, Runtime};
use serde_json::json;

use super::*;

/// main canvas › Proj › Two › Three › Four › Five (current); `depth` spaces deep.
fn state(depth: usize) -> AppState {
    let mut s = AppState::new();
    s.use_store(Store::in_memory());
    let names = ["Proj", "Two", "Three", "Four", "Five"];
    let changes: Vec<Change> = (0..depth)
        .map(|i| {
            let parent = if i == 0 { "root".to_string() } else { format!("s{}", i - 1) };
            let mut sp = json!({"id": format!("s{i}"), "name": names[i], "parentSpaceId": parent, "createdAt": 1});
            if i == 0 {
                sp["kind"] = json!("project");
            }
            Change::set(format!("spaces/s{i}"), sp)
        })
        .collect();
    s.apply(changes, "test").unwrap();
    s.enter_space(&format!("s{}", depth - 1), None);
    s
}

fn render(s: AppState) -> Runtime<AppState> {
    let mut rt = Runtime::new(s, crate::shell::view, (1280.0, 800.0), Env::new(true, 2.0));
    rt.poll = Some(|s, clock, tx| s.poll(clock, tx));
    rt.frame(0.0);
    rt
}

fn click(rt: &mut Runtime<AppState>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
}

fn press(rt: &mut Runtime<AppState>, key: Key) {
    rt.key(KeyEvent { key, mods: Default::default() });
    rt.frame(1.0 / 60.0);
}

fn current_name(s: &AppState) -> String {
    s.route.path.last().unwrap().1.clone()
}

#[test]
fn split_keeps_first_and_last_ancestor_from_four() {
    assert_eq!(split(0), (vec![], vec![]));
    assert_eq!(split(3), (vec![0, 1, 2], vec![]));
    assert_eq!(split(4), (vec![0, 3], vec![1, 2]));
    assert_eq!(split(6), (vec![0, 5], vec![1, 2, 3, 4]));
}

#[test]
fn deep_paths_collapse_into_a_menu_that_navigates() {
    let mut rt = render(state(5));
    for k in ["crumb-0", "crumb-more", "crumb-3", "crumb-current"] {
        assert!(rt.find(k).is_some(), "{k} shown");
    }
    assert!(rt.find("crumb-1").is_none() && rt.find("crumb-2").is_none(), "middle collapsed");
    assert!(rt.find("crumb-menu").is_none());
    click(&mut rt, "crumb-more");
    assert!(rt.find("crumb-menu-1").is_some() && rt.find("crumb-menu-2").is_some());
    click(&mut rt, "crumb-menu-1");
    assert_eq!(rt.state.route.space, "s1");
    assert_eq!(rt.state.route.path.len(), 2, "enter_space rebuilt the path");
    assert!(!rt.state.shell.crumbs.menu_open);

    let rt = render(state(4));
    assert!(rt.find("crumb-more").is_none(), "three ancestors all show");
    assert!(rt.find("crumb-2").is_some());
}

#[test]
fn click_rename_type_enter_saves() {
    let mut rt = render(state(3));
    click(&mut rt, "crumb-current");
    assert!(rt.find("crumb-rename").is_some(), "the field replaces the name");
    // The name is selected, so a paste replaces it.
    rt.paste("  Renamed space ");
    rt.frame(1.0 / 60.0);
    press(&mut rt, Key::Enter);
    assert!(rt.state.shell.crumbs.renaming.is_none());
    assert_eq!(rt.state.snap().space("s2").unwrap().name, "Renamed space");
    assert_eq!(current_name(&rt.state), "Renamed space", "breadcrumb follows at once");
    assert!(rt.find("crumb-rename").is_none());
}

#[test]
fn escape_cancels_and_f2_starts() {
    let mut rt = render(state(3));
    rt.update(|s, tx| {
        assert!(on_key(s, &KeyEvent { key: Key::Function(2), mods: Default::default() }, tx));
    });
    rt.frame(1.0 / 60.0);
    assert!(rt.state.shell.crumbs.renaming.is_some());
    rt.paste("Nope");
    rt.frame(1.0 / 60.0);
    press(&mut rt, Key::Escape);
    assert!(rt.state.shell.crumbs.renaming.is_none());
    assert_eq!(rt.state.snap().space("s2").unwrap().name, "Three");

    // Not on the main canvas, and not for other keys.
    let mut s = AppState::new();
    let mut tx = rune::Tx::default();
    assert!(!on_key(&mut s, &KeyEvent { key: Key::Function(2), mods: Default::default() }, &mut tx));
    let mut s = state(2);
    assert!(!on_key(&mut s, &KeyEvent { key: Key::Function(3), mods: Default::default() }, &mut tx));
}

#[test]
fn blur_saves_empty_or_unchanged_does_nothing_and_leaving_cancels() {
    let mut tx = rune::Tx::default();
    let mut s = state(2);
    start(&mut s, &mut tx);
    s.shell.crumbs.edit = TextEdit::new("Blurred");
    s.shell.crumbs.edit.focused = true;
    assert!(!poll(&mut s), "focused: keep editing");
    s.shell.crumbs.edit.focused = false;
    assert!(poll(&mut s), "focus lost: save");
    assert_eq!(s.snap().space("s1").unwrap().name, "Blurred");

    let v = s.store().cursor();
    for draft in ["   ", "Blurred"] {
        start(&mut s, &mut tx);
        s.shell.crumbs.edit = TextEdit::new(draft);
        commit(&mut s);
        assert!(s.shell.crumbs.renaming.is_none());
    }
    assert_eq!(s.store().cursor(), v, "empty or unchanged names write nothing");

    start(&mut s, &mut tx);
    s.enter_space("s0", None);
    assert!(poll(&mut s));
    assert!(s.shell.crumbs.renaming.is_none(), "leaving the space drops the rename");
}
