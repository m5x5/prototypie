//! Wire types shared with the reference app (`src/lib/types.ts`, `src/lib/ai-edit.ts`). JSON shapes are camelCase
//! and optional fields are omitted when absent, so items and events round-trip with the reference unchanged.

use serde::{Deserialize, Serialize};

/// One design direction: what the plan step proposes and what each queue item builds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Direction {
    pub title: String,
    pub brief: String,
}

/// Shape the AI (or a queue delivery) returns for one design.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variation {
    pub title: String,
    pub rationale: String,
    pub html: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport_height: Option<u32>,
}

/// NDJSON events of a generation job (`/api/variations`), one JSON object per line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum VariationEvent {
    Job { id: String },
    Plan { directions: Vec<Direction> },
    Variation { index: usize, variation: Variation },
    Error { index: Option<usize>, message: String },
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentChoice {
    /// Call the LLM directly (server default when a request names no agent).
    #[default]
    Codex,
    /// Queue the work for a Claude Code session (scripts/agent-queue.md).
    Claude,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Page,
    Component,
}

/// An Annotate note pinned on a design: `[element] comment`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    pub comment: String,
}

/// A design passed in full (bases of a "Combine").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseDesign {
    pub id: String,
    pub title: String,
    pub html: String,
}

/// A card #-mentioned as a component to reuse as it is.
pub type ComponentRef = BaseDesign;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportFrom {
    pub element: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PodAccess {
    #[serde(rename = "live")]
    Live,
    #[serde(rename = "401")]
    Locked,
    #[serde(rename = "ref")]
    Ref,
    #[serde(rename = "cache")]
    Cache,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PodField {
    pub p: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v: Option<String>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PodRelation {
    pub p: String,
    pub target: String,
}

/// One @-mentioned pod catalog entry as sent with a generation request (selected fields only).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PodContextEntry {
    pub id: String,
    /// pod | container | shape | resource
    pub kind: String,
    pub label: String,
    pub source: String,
    pub access: PodAccess,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stand: Option<String>,
    #[serde(default)]
    pub fields: Vec<PodField>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relations: Option<Vec<PodRelation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Body of `POST /api/variations`.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariationRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_html: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bases: Option<Vec<BaseDesign>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_title: Option<String>,
    #[serde(default)]
    pub instruction: String,
    /// Clamped to 1..=8 (default 3) when the job starts.
    #[serde(default)]
    pub count: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Vec<Annotation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod_context: Option<Vec<PodContextEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_from: Option<ImportFrom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<ComponentRef>>,
}

impl VariationRequest {
    /// `Math.min(Math.max(Math.round(count || 3), 1), 8)`
    pub fn clamped_count(&self) -> usize {
        let c = if self.count.is_finite() && self.count != 0.0 { self.count } else { 3.0 };
        // JS Math.round rounds .5 up (towards +inf).
        (c + 0.5).floor().clamp(1.0, 8.0) as usize
    }
}

/// Body of `POST /api/edit` (validated by the server).
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditRequest {
    pub design_id: String,
    pub instruction: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Vec<Annotation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod_context: Option<Vec<PodContextEntry>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<ComponentRef>>,
}

/// The card an in-place edit rewrites (read from the store by the server).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditTarget {
    pub id: String,
    pub title: String,
    pub html: String,
    pub space_id: String,
}

/// A space's design guidelines (`guidelinesFor` in the server).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Guidelines {
    pub card_id: String,
    pub title: String,
    /// Name of the space that owns the guidelines.
    pub from_space: String,
    pub text: String,
}

/// The part of the guidelines a queue item carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuidelinesRef {
    pub card_id: String,
    pub title: String,
    pub text: String,
}

impl From<&Guidelines> for GuidelinesRef {
    fn from(g: &Guidelines) -> Self {
        GuidelinesRef { card_id: g.card_id.clone(), title: g.title.clone(), text: g.text.clone() }
    }
}

/// One component found by `spokes`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpokeComponent {
    pub name: String,
    pub description: String,
    pub selector: String,
}

/// A design offered to `decisions`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionCandidate {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub rationale: String,
    pub html: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionOption {
    pub design_id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestedDecision {
    pub question: String,
    pub why: String,
    pub options: Vec<DecisionOption>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn events_match_reference_json() {
        let e = VariationEvent::Error { index: None, message: "x".into() };
        assert_eq!(serde_json::to_value(&e).unwrap(), json!({"type":"error","index":null,"message":"x"}));
        assert_eq!(serde_json::to_value(VariationEvent::Done).unwrap(), json!({"type":"done"}));
        let v = VariationEvent::Variation {
            index: 1,
            variation: Variation { title: "t".into(), rationale: "r".into(), html: "h".into(), viewport: None, viewport_height: Some(960) },
        };
        assert_eq!(
            serde_json::to_value(&v).unwrap(),
            json!({"type":"variation","index":1,"variation":{"title":"t","rationale":"r","html":"h","viewportHeight":960}})
        );
        let back: VariationEvent = serde_json::from_value(json!({"type":"plan","directions":[{"title":"a","brief":"b"}]})).unwrap();
        assert_eq!(back, VariationEvent::Plan { directions: vec![Direction { title: "a".into(), brief: "b".into() }] });
    }

    #[test]
    fn request_parses_reference_body() {
        let r: VariationRequest = serde_json::from_value(json!({
            "instruction": "go", "count": 2, "baseHtml": "<p>", "agent": "claude", "scope": "component",
            "podContext": [{"id":"a","kind":"resource","label":"Tasks","source":"pod","access":"401","fields":[{"p":"ical:due","type":"date"}]}],
            "importFrom": {"element":"Toolbar","source":"/repo"}
        }))
        .unwrap();
        assert_eq!(r.agent, Some(AgentChoice::Claude));
        assert_eq!(r.scope, Some(Scope::Component));
        assert_eq!(r.pod_context.as_ref().unwrap()[0].access, PodAccess::Locked);
        assert_eq!(r.pod_context.as_ref().unwrap()[0].fields[0].kind.as_deref(), Some("date"));
        assert_eq!(r.clamped_count(), 2);
    }

    #[test]
    fn count_clamps_like_js() {
        let mut r = VariationRequest::default();
        assert_eq!(r.clamped_count(), 3);
        r.count = 2.5;
        assert_eq!(r.clamped_count(), 3);
        r.count = 40.0;
        assert_eq!(r.clamped_count(), 8);
        r.count = -2.0;
        assert_eq!(r.clamped_count(), 1);
    }
}
