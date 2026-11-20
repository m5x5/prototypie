//! Projects, explorations and promote (the creation side of the reference's `features/nested`).
//! Navigation and the space path belong to the shell; `model` plans the record changes, the actions here
//! apply them and move there. Card buttons and dialogs call these.

pub mod project;
pub mod project_view;

use proto_core::{ROOT_SPACE_ID, ops};

use crate::app::state::AppState;
use crate::features::nav;

/// Why creating on the main canvas is refused (the reference's `ROOT_ONLY_PROJECTS`).
pub const ROOT_ONLY_PROJECTS: &str = "The main canvas holds only projects. Open a project first, or create one with + → Project.";

/// For create actions: false (with a notice) on the main canvas.
pub fn can_create_in(s: &mut AppState, space: &str) -> bool {
    if space != ROOT_SPACE_ID {
        return true;
    }
    crate::features::notify(s, "Open a project first", ROOT_ONLY_PROJECTS);
    false
}

/// "+ → Exploration": a new empty space inside the current canvas; enters it.
pub fn new_exploration(s: &mut AppState) {
    let here = s.route.space.clone();
    if !can_create_in(s, &here) {
        return;
    }
    let created = ops::create_exploration(s.snap(), &here, None, None);
    if nav::apply(s, created.changes, "new exploration") {
        nav::enter_space(s, &created.space_id, None);
    }
}

/// Card "Explore": a nested space seeded with a copy of the design; lands on the seed (the composer can
/// focus it: `explored` holds the seed id for one frame).
pub fn explore(s: &mut AppState, design_id: &str) {
    let Some(d) = s.snap().design(design_id).filter(|d| d.is_ready()) else { return };
    let created = ops::create_exploration(s.snap(), &d.space_id, None, Some(design_id));
    if !nav::apply(s, created.changes, "explore") {
        return;
    }
    nav::enter_space(s, &created.space_id, created.seed_id.as_deref());
    s.features.focus_prompt = created.seed_id;
}

/// Card "Promote" (and multi-select promote): copies up to the parent space; a notice offers to show them.
pub fn promote(s: &mut AppState, design_ids: &[String]) -> Vec<String> {
    let snap = s.store().snapshot();
    let refs: Vec<&str> = design_ids.iter().map(String::as_str).collect();
    let parent = design_ids
        .iter()
        .find_map(|id| snap.design(id))
        .and_then(|d| snap.space(&d.space_id))
        .and_then(|sp| sp.parent_space_id.clone());
    let (changes, copies) = ops::promote(&snap, &refs);
    if copies.is_empty() || !nav::apply(s, changes, "promote") {
        return Vec::new();
    }
    let parent_name = parent.as_deref().and_then(|p| snap.space(p)).map_or("parent".to_string(), |sp| sp.name.clone());
    let title = match copies.len() {
        1 => format!("Promoted to {parent_name}"),
        n => format!("Promoted {n} designs to {parent_name}"),
    };
    let show = parent.zip(copies.first().cloned());
    crate::features::notify_with(s, &title, "", show.map(|(space, design)| crate::features::NoticeAction::Show { space, design }));
    copies
}

/// Card "Promoted": jump to the copy in the parent space.
pub fn show_promoted_copy(s: &mut AppState, design_id: &str) {
    if let Some(copy) = s.snap().promoted_copy_of(design_id) {
        nav::enter_space(s, &copy.space_id, Some(&copy.id));
    }
}

/// "+ → Project" for a typed name or absolute folder path (the dialog's first row): creates the project or
/// opens the existing one with that folder.
pub fn create_project_from_query(s: &mut AppState, query: &str) {
    let (name, local_path) = from_query(query);
    let (created, is_new) = ops::create_project(s.snap(), &name, local_path.as_deref());
    if !nav::apply(s, created.changes, "create project") {
        return;
    }
    if !is_new {
        let name = s.snap().space(&created.space_id).map_or(name, |sp| sp.name.clone());
        crate::features::notify(s, &format!("„{name}“ is already on the canvas"), "");
    }
    nav::enter_space(s, &created.space_id, None);
}

/// A typed absolute path stands for that folder (its last segment names the project); anything else is a name.
pub fn from_query(query: &str) -> (String, Option<String>) {
    let q = query.trim().trim_end_matches('/');
    if q.starts_with('/') && q.len() > 1 {
        let name = q.rsplit('/').next().unwrap_or(q).to_string();
        return (name, Some(q.to_string()));
    }
    (if q.is_empty() { "New project".to_string() } else { q.to_string() }, None)
}

/// Mark (or unmark) a card as its space's design guidelines; any other mark in that space is cleared.
pub fn set_guidelines(s: &mut AppState, design_id: &str, on: bool) {
    let changes = ops::set_guidelines(s.snap(), design_id, on);
    nav::apply(s, changes, "guidelines");
}

#[cfg(test)]
mod query_tests {
    #[test]
    fn query_paths_become_folders() {
        assert_eq!(super::from_query(" /Users/x/App/ "), ("App".into(), Some("/Users/x/App".into())));
        assert_eq!(super::from_query("Shop"), ("Shop".into(), None));
        assert_eq!(super::from_query("  "), ("New project".into(), None));
    }
}
