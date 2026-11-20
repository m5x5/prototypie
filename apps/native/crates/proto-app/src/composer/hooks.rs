//! The seam to `crate::features::mention` (worker-8; spec-features §7): @ pod data and # component
//! mentions — chips over the text, the @/# buttons, the field's keys and trigger detection, the
//! list anchored to "composer-box", "Flows into generation", and what a send carries.

pub use crate::features::mention::{buttons, chips, clear, context_panel, listbox, on_change, on_key, request_parts};
