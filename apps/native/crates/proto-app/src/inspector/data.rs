//! The one place the inspector and composer read proto-core data from: the app's cached snapshot
//! (`AppState::snap`, refreshed by the shell's poll).

use std::sync::Arc;

use proto_core::{Design, DesignStatus, Snapshot};

use crate::app::state::AppState;

pub fn snap(s: &AppState) -> &Snapshot {
    s.snap()
}

/// The inspected design: the single selected design in the active space.
pub fn selected_design(s: &AppState) -> Option<Arc<Design>> {
    match s.route.selected.as_slice() {
        [id] => snap(s).design(id),
        _ => None,
    }
}

pub fn is_ready(d: &Design) -> bool {
    d.status == DesignStatus::Ready
}

/// Ready variations made from `design` (the composer's "Its variations" target).
pub fn ready_children(s: &AppState, design: &Design) -> Vec<Arc<Design>> {
    snap(s).children_of(&design.id).into_iter().filter(|d| is_ready(d)).collect()
}

/// `~` for the home folder (`/Users/<name>` or `/home/<name>`), as the reference's `tildePath`.
pub fn tilde_path(p: &str) -> String {
    for root in ["/Users/", "/home/"] {
        if let Some(rest) = p.strip_prefix(root) {
            return match rest.find('/') {
                Some(i) => format!("~{}", &rest[i..]),
                None => "~".into(),
            };
        }
    }
    p.to_string()
}

/// A folder path's last segment.
pub fn folder_name(p: &str) -> &str {
    p.split('/').filter(|s| !s.is_empty()).next_back().unwrap_or(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilde_paths() {
        assert_eq!(tilde_path("/Users/example/Software/x"), "~/Software/x");
        assert_eq!(tilde_path("/home/me"), "~");
        assert_eq!(tilde_path("/opt/x"), "/opt/x");
        assert_eq!(folder_name("/Users/me/code/prototypie/"), "prototypie");
    }
}
