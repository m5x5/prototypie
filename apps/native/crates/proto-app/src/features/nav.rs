//! Writing to the store and moving to what was written, through the shell's `AppState::apply` (refreshes the
//! snapshot at once) and `AppState::enter_space`.

use proto_core::Change;

use crate::app::state::AppState;

/// Apply one batch from the features; false (logged) when the store rejects it.
pub fn apply(s: &mut AppState, changes: Vec<Change>, reason: &str) -> bool {
    if changes.is_empty() {
        return true;
    }
    match s.apply(changes, reason) {
        Ok(_) => true,
        Err(e) => {
            eprintln!("proto-app features: {reason} rejected: {e}");
            false
        }
    }
}

/// Go to `space`, selecting `design` when given.
pub fn enter_space(s: &mut AppState, space: &str, design: Option<&str>) {
    s.enter_space(space, design.map(str::to_string));
}
