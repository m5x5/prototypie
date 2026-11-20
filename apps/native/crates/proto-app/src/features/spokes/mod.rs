//! Hub & spoke (the reference's `features/spokes`): break any design into components and explore each in its
//! own lane ("Break into spokes"), start a rough full page ("+ → Page", the block of clay), and merge a refined
//! spoke back into its hub. Generation goes through `features::launch` (the runner's outbox).

pub mod view;

use std::collections::{BTreeMap, HashMap};

use proto_core::{Design, DesignKind, LaneType, VariationRequest};
use rune::{Latest, TextEdit, Tx, Waker};
use serde::Deserialize;

use crate::app::state::AppState;
use crate::features::launch::{self, LaneInfo, Launch};
use crate::features::{http, notify};

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct SpokeComponent {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub selector: Option<String>,
}

/// One row of the picker: an analysed component or one the designer added.
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    pub key: String,
    pub name: String,
    pub description: String,
    pub selector: Option<String>,
    pub custom: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Analysis {
    Loading,
    Error(String),
    Ready(Vec<SpokeComponent>),
}

/// The open picker for one hub.
pub struct Picker {
    pub hub_id: String,
    pub analysis: Analysis,
    pub custom: Vec<Pick>,
    pub checked: BTreeMap<String, bool>,
    pub draft: TextEdit,
    pub direction: TextEdit,
    /// Designs per spoke (1–6, default 3).
    pub count: u32,
}

#[derive(Default)]
pub struct SpokesState {
    pub picker: Option<Picker>,
    /// Analyses this session, per hub id (reopening doesn't ask again).
    pub(crate) cache: HashMap<String, Vec<SpokeComponent>>,
    analyse: Latest<(String, Result<Vec<SpokeComponent>, String>)>,
    /// The "+ → Page" dialog's brief while open.
    pub clay: Option<TextEdit>,
    /// Merge notes per spoke id (the inspector's "Optional note").
    pub merge_notes: HashMap<String, TextEdit>,
}

impl SpokesState {
    pub fn setup(&mut self, waker: Waker) {
        self.analyse.set_waker(waker);
    }
}

impl Picker {
    /// Analysed rows (first three pre-checked by `open_picker`), then custom ones.
    pub fn rows(&self) -> Vec<Pick> {
        let mut out: Vec<Pick> = match &self.analysis {
            Analysis::Ready(cs) => cs
                .iter()
                .enumerate()
                .map(|(i, c)| Pick { key: format!("ai-{i}"), name: c.name.clone(), description: c.description.clone(), selector: c.selector.clone(), custom: false })
                .collect(),
            _ => Vec::new(),
        };
        out.extend(self.custom.iter().cloned());
        out
    }

    pub fn picked(&self) -> Vec<Pick> {
        self.rows().into_iter().filter(|p| self.checked.get(&p.key).copied().unwrap_or(false)).collect()
    }
}

fn default_checked(components: &[SpokeComponent]) -> BTreeMap<String, bool> {
    (0..components.len().min(3)).map(|i| (format!("ai-{i}"), true)).collect()
}

/// Card "Spokes" / inspector "Break into spokes".
pub fn open_picker(s: &mut AppState, hub_id: &str) {
    let Some(hub) = s.snap().design(hub_id).filter(|d| d.is_ready()) else { return };
    let cached = s.features.spokes.cache.get(hub_id).cloned();
    let analysis = match &cached {
        Some(cs) => Analysis::Ready(cs.clone()),
        None => Analysis::Loading,
    };
    s.features.spokes.picker = Some(Picker {
        hub_id: hub_id.to_string(),
        checked: cached.as_deref().map(default_checked).unwrap_or_default(),
        analysis,
        custom: Vec::new(),
        draft: TextEdit::new(""),
        direction: TextEdit::new(""),
        count: 3,
    });
    if cached.is_none() {
        request_analysis(s, &hub);
    }
}

fn request_analysis(s: &mut AppState, hub: &Design) {
    let body = serde_json::json!({ "html": hub.html, "title": hub.title }).to_string();
    let (addr, id) = (http::server_addr(), hub.id.clone());
    s.features.spokes.analyse.request((addr, id, body), |(a, id, body)| {
        #[derive(Deserialize)]
        struct Resp {
            components: Vec<SpokeComponent>,
        }
        let r = http::request(&a, "POST", "/api/spokes", Some(&body))
            .and_then(|b| serde_json::from_slice::<Resp>(&b).map(|r| r.components).map_err(|e| format!("bad answer: {e}")));
        (id, r)
    });
}

pub fn retry(s: &mut AppState) {
    let Some(hub) = s.features.spokes.picker.as_ref().and_then(|p| s.snap().design(&p.hub_id)) else { return };
    if let Some(p) = &mut s.features.spokes.picker {
        p.analysis = Analysis::Loading;
    }
    request_analysis(s, &hub);
}

pub fn close_picker(s: &mut AppState) {
    s.features.spokes.picker = None;
}

/// Add the draft as a custom component: "Name: description" (or "Name - description"), else the text is both.
pub fn add_custom(s: &mut AppState) {
    let Some(p) = &mut s.features.spokes.picker else { return };
    let text = p.draft.text.trim().to_string();
    if text.is_empty() {
        return;
    }
    let (name, description) = split_custom(&text);
    let key = format!("custom-{}", p.custom.len() + p.checked.len() + 1);
    p.custom.push(Pick { key: key.clone(), name, description, selector: None, custom: true });
    p.checked.insert(key, true);
    p.draft = TextEdit::new("");
}

pub fn split_custom(text: &str) -> (String, String) {
    for sep in [": ", " – ", " — ", " - ", ":"] {
        if let Some((a, b)) = text.split_once(sep)
            && !a.trim().is_empty()
            && !b.trim().is_empty()
        {
            return (a.trim().chars().take(40).collect(), b.trim().to_string());
        }
    }
    (text.chars().take(40).collect(), text.to_string())
}

fn agent(s: &AppState) -> String {
    match s.prefs.agent_choice {
        crate::app::prefs::AgentChoice::Claude => "claude".into(),
        crate::app::prefs::AgentChoice::Codex => "codex".into(),
    }
}

/// "Explore N spokes": one lane per picked component, rows around the hub.
pub fn launch_picked(s: &mut AppState) {
    let Some(p) = s.features.spokes.picker.take() else { return };
    let picked = p.picked();
    let Some(hub) = s.snap().design(&p.hub_id) else { return };
    if picked.is_empty() {
        s.features.spokes.picker = Some(p);
        return;
    }
    let count = p.count.clamp(1, 6);
    let origins = launch::spoke_row_origins(s.snap(), &hub, picked.len(), count);
    let direction = p.direction.text.trim().to_string();
    let agent = agent(s);
    for (pick, origin) in picked.iter().zip(origins) {
        let focus = match &pick.selector {
            Some(sel) => format!("{}: {} (in the HTML: {sel})", pick.name, pick.description),
            None => format!("{}: {}", pick.name, pick.description),
        };
        let instruction = if direction.is_empty() { format!("Explore {count} meaningfully different takes on the {}.", pick.name) } else { direction.clone() };
        launch::launch(s, Launch {
            space_id: hub.space_id.clone(),
            parent_id: Some(hub.id.clone()),
            kind: DesignKind::Spoke,
            origin: Some(origin),
            label: format!("Spoke · {}", pick.name),
            request: VariationRequest {
                base_html: Some(hub.html.clone()),
                base_title: Some(hub.title.clone()),
                instruction,
                count,
                focus: Some(focus.clone()),
                scope: Some("component".into()),
                agent: Some(agent.clone()),
                space_id: Some(hub.space_id.clone()),
                ..Default::default()
            },
            lane: Some(LaneInfo { lane_type: LaneType::Spoke, name: pick.name.clone(), focus: Some(focus), hub_id: Some(hub.id.clone()) }),
        });
    }
    let n = picked.len();
    notify(s, &format!("Exploring {n} spoke{} in parallel", if n == 1 { "" } else { "s" }), "Results join the review queue as each one lands.");
}

/// "+ → Page": open the block-of-clay dialog (not on the main canvas).
pub fn open_clay(s: &mut AppState, _tx: &mut Tx) {
    let here = s.route.space.clone();
    if !crate::features::nested::can_create_in(s, &here) {
        return;
    }
    let mut e = TextEdit::new(s.prefs.draft("clay:brief"));
    e.focused = true;
    s.features.spokes.clay = Some(e);
}

pub fn close_clay(s: &mut AppState) {
    if let Some(e) = s.features.spokes.clay.take() {
        let text = e.text.clone();
        s.prefs_mut().set_draft("clay:brief", &text);
    }
}

pub fn submit_clay(s: &mut AppState) {
    let Some(e) = s.features.spokes.clay.take() else { return };
    let brief = e.text.trim().to_string();
    if brief.is_empty() {
        s.features.spokes.clay = Some(e);
        return;
    }
    let space = s.route.space.clone();
    let agent = agent(s);
    launch::launch(s, Launch {
        space_id: space.clone(),
        parent_id: None,
        kind: DesignKind::Page,
        origin: None,
        label: "Block of clay".into(),
        request: VariationRequest {
            instruction: format!("{brief}\n\nThis is a rough first \"block of clay\": a complete, plausible full page with every section a real version would need. Favour breadth and structure over polish; the designer will refine it component by component."),
            count: 1,
            scope: Some("page".into()),
            agent: Some(agent),
            space_id: Some(space),
            ..Default::default()
        },
        lane: Some(LaneInfo { lane_type: LaneType::Clay, name: "Block of clay".into(), focus: None, hub_id: None }),
    });
    s.prefs_mut().set_draft("clay:brief", "");
    notify(s, "Shaping a block of clay…", "It appears on the canvas as soon as it's ready.");
}

/// The component a spoke explores (its own focus, else up through earlier rounds of the same spoke).
pub fn focus_of(s: &AppState, id: &str) -> Option<String> {
    crate::features::review::focus_of(s.snap(), id)
}

/// Merge a refined spoke back into its hub: one new version of the hub, below it.
pub fn merge_into_hub(s: &mut AppState, spoke_id: &str) {
    let snap = s.store().snapshot();
    let (Some(spoke), Some(hub)) = (snap.design(spoke_id), snap.hub_of(spoke_id)) else { return };
    if !spoke.is_ready() || !hub.is_ready() {
        return;
    }
    let note = s.features.spokes.merge_notes.remove(spoke_id).map(|e| e.text.trim().to_string()).filter(|n| !n.is_empty());
    let focus = focus_of(s, spoke_id).unwrap_or_else(|| spoke.title.clone());
    let origin = launch::below_origins(&snap, &[hub.as_ref()], 1).into_iter().next();
    let mut instruction = format!(
        "Integrate the refined component below into this design, replacing its existing version of it ({focus}). Keep its design, copy and interactions, but adapt spacing, widths and tokens so it sits naturally in the design. Leave the rest of the design as it is."
    );
    if let Some(n) = &note {
        instruction.push_str(&format!("\n\nDesigner's note on this merge (takes priority, e.g. merge only part of it): {n}"));
    }
    instruction.push_str(&format!("\n\n<refined_component title=\"{}\">\n{}\n</refined_component>", spoke.title.replace('"', "'"), spoke.html));
    let label = format!("Merge · {}", spoke.title);
    let agent = agent(s);
    launch::launch(s, Launch {
        space_id: hub.space_id.clone(),
        parent_id: Some(hub.id.clone()),
        kind: hub.kind,
        origin,
        label: label.clone(),
        request: VariationRequest {
            base_html: Some(hub.html.clone()),
            base_title: Some(hub.title.clone()),
            instruction,
            count: 1,
            focus: Some(focus.clone()),
            scope: Some("page".into()),
            agent: Some(agent),
            space_id: Some(hub.space_id.clone()),
            ..Default::default()
        },
        lane: Some(LaneInfo { lane_type: LaneType::Merge, name: label, focus: Some(focus), hub_id: Some(hub.id.clone()) }),
    });
    notify(s, &format!("Merging \"{}\" into {}", spoke.title, hub.title), "A new version appears below the hub.");
}

/// Land the analysis; true when the picker changed.
pub fn poll(s: &mut AppState) -> bool {
    let Some((hub, result)) = s.features.spokes.analyse.take() else { return false };
    if let Ok(cs) = &result {
        s.features.spokes.cache.insert(hub.clone(), cs.clone());
    }
    let Some(p) = s.features.spokes.picker.as_mut().filter(|p| p.hub_id == hub) else { return false };
    match result {
        Ok(cs) => {
            for (k, v) in default_checked(&cs) {
                p.checked.entry(k).or_insert(v);
            }
            p.analysis = Analysis::Ready(cs);
        }
        Err(e) => p.analysis = Analysis::Error(e),
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composer::Outgoing;

    #[test]
    fn custom_rows_split_name_and_description() {
        assert_eq!(split_custom("Pricing card: the middle tier"), ("Pricing card".into(), "the middle tier".into()));
        assert_eq!(split_custom("Footer"), ("Footer".into(), "Footer".into()));
    }

    #[test]
    fn picked_spokes_launch_one_component_lane_each() {
        let mut s = AppState::new();
        s.use_store(crate::features::scenes::fixture());
        s.features.spokes.cache.insert("zzbase".into(), vec![
            SpokeComponent { name: "Hero".into(), description: "top".into(), selector: Some("header".into()) },
            SpokeComponent { name: "Steps".into(), description: "progress".into(), selector: None },
        ]);
        open_picker(&mut s, "zzbase");
        assert_eq!(s.features.spokes.picker.as_ref().unwrap().picked().len(), 2);
        launch_picked(&mut s);
        let gens: Vec<_> = s.composer.outbox.iter().filter_map(|o| match o {
            Outgoing::Generate { kind, request, run_id, .. } => Some((*kind, request.focus.clone(), request.scope.clone(), run_id.clone())),
            _ => None,
        }).collect();
        assert_eq!(gens.len(), 2);
        assert!(gens.iter().all(|g| g.0 == DesignKind::Spoke && g.2.as_deref() == Some("component")));
        assert_eq!(gens[0].1.as_deref(), Some("Hero: top (in the HTML: header)"));
        let lane = s.snap().lane(gens[1].3.as_deref().unwrap()).unwrap();
        assert_eq!((lane.lane_type, lane.hub_id.as_deref()), (LaneType::Spoke, Some("zzbase")));
    }
}
