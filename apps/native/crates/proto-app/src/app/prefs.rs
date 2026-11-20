//! Per-device UI state that survives restarts (the reference app's local-only store): the active
//! space (its `?space=` URL), composer drafts, prompt history and the agent choice. Not in
//! proto-core's shared `state.json`; saved by the shell off the UI thread.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The Annotate feature's key in [`Prefs::features`] (toolbar position, notes service address).
pub const ANNOTATE_KEY: &str = "annotate";

/// How many prompts the history keeps (newest first).
pub const PROMPT_HISTORY_MAX: usize = 30;

/// Which agent the composer sends to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentChoice {
    #[default]
    Claude,
    Codex,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    /// The space shown on the canvas (`root` = main canvas), restored at launch.
    pub active_space: Option<String>,
    /// Composer drafts per key: a design id, or `"new"` with nothing selected.
    pub drafts: BTreeMap<String, String>,
    /// Sent prompts, newest first, deduplicated, at most `PROMPT_HISTORY_MAX`.
    pub prompt_history: Vec<String>,
    pub agent_choice: AgentChoice,
    /// Per-feature settings each feature owns under its own key (e.g. `"annotate"`: toolbar
    /// position, endpoint). Saved like the rest after `AppState::prefs_mut`.
    pub features: BTreeMap<String, serde_json::Value>,
}

impl Prefs {
    /// Remember a sent prompt (the reference's `rememberPrompt`).
    pub fn remember_prompt(&mut self, prompt: &str) {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return;
        }
        self.prompt_history.retain(|p| p != prompt);
        self.prompt_history.insert(0, prompt.to_string());
        self.prompt_history.truncate(PROMPT_HISTORY_MAX);
    }

    /// Set or clear (empty text) the draft for `key`.
    pub fn set_draft(&mut self, key: &str, text: &str) {
        if text.is_empty() {
            self.drafts.remove(key);
        } else {
            self.drafts.insert(key.to_string(), text.to_string());
        }
    }

    pub fn draft(&self, key: &str) -> &str {
        self.drafts.get(key).map_or("", String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_newest_first_deduplicated_and_bounded() {
        let mut p = Prefs::default();
        for i in 0..40 {
            p.remember_prompt(&format!("p{i}"));
        }
        p.remember_prompt("p35");
        assert_eq!(p.prompt_history.len(), PROMPT_HISTORY_MAX);
        assert_eq!(p.prompt_history[0], "p35");
        assert_eq!(p.prompt_history.iter().filter(|x| *x == "p35").count(), 1);
    }

    #[test]
    fn round_trips_as_camel_case_json() {
        let mut p = Prefs { agent_choice: AgentChoice::Codex, ..Prefs::default() };
        p.set_draft("new", "a landing page");
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"agentChoice\":\"codex\"") && json.contains("\"promptHistory\""));
        assert_eq!(serde_json::from_str::<Prefs>(&json).unwrap(), p);
    }
}
