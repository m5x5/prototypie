//! Domain types, serde-compatible with the reference app's records (`src/lib/types.ts` and the feature slices).
//!
//! Field names are the reference's camelCase JSON names. Every struct keeps fields it doesn't know in `extra`, so
//! a record read from a newer reference state and written back loses nothing.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Numbers written like JavaScript's `JSON.stringify`: integral values without a fraction (`1793478911991`, not
/// `1793478911991.0`), so records written back match the reference byte for byte.
mod js_num {
    use serde::Serializer;

    /// Below 2^53 every integral f64 is an exact i64.
    const MAX_SAFE: f64 = 9_007_199_254_740_992.0;

    pub fn f64<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
        if v.is_finite() && v.fract() == 0.0 && v.abs() < MAX_SAFE {
            s.serialize_i64(*v as i64)
        } else {
            s.serialize_f64(*v)
        }
    }

    pub fn opt_f64<S: Serializer>(v: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(v) => f64(v, s),
            None => s.serialize_none(),
        }
    }
}

/// Unknown JSON fields, kept verbatim for a lossless round trip.
pub type Extra = Map<String, Value>;

/// Id of the main canvas space. It is implicit in the reference (no `spaces/root` record is required).
pub const ROOT_SPACE_ID: &str = "root";

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    #[serde(serialize_with = "js_num::f64")]
    pub x: f64,
    #[serde(serialize_with = "js_num::f64")]
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DesignKind {
    /// A full page ("block of clay").
    Page,
    /// One component pulled out of a page (hub & spoke).
    Spoke,
    Variation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DesignStatus {
    Generating,
    Ready,
    Error,
}

/// A card on a canvas (`designs/<id>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Design {
    pub id: String,
    pub space_id: String,
    /// Design this was derived from (draws an edge when in the same space).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub kind: DesignKind,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub rationale: String,
    /// Self-contained HTML document.
    #[serde(default)]
    pub html: String,
    pub status: DesignStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub position: Point,
    /// Milliseconds since the Unix epoch.
    #[serde(serialize_with = "js_num::f64", default)]
    pub created_at: f64,
    /// Prompt that produced it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starred: Option<bool>,
    /// The designer's free-text note on this card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Virtual layout width (default 1280; phones ~400).
    #[serde(default, serialize_with = "js_num::opt_f64", skip_serializing_if = "Option::is_none")]
    pub viewport: Option<f64>,
    /// Height of a fixed-size screen: the card frame keeps `viewport : viewport_height`.
    #[serde(default, serialize_with = "js_num::opt_f64", skip_serializing_if = "Option::is_none")]
    pub viewport_height: Option<f64>,
    /// A reusable component (listed first for # mentions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<bool>,
    /// The request that produced it (without the base html), so it can be retried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<VariationRequest>,
    #[serde(default, serialize_with = "js_num::opt_f64", skip_serializing_if = "Option::is_none")]
    pub implement_requested_at: Option<f64>,
    /// Set when this card is its space's design guidelines (newest mark per space wins).
    #[serde(default, serialize_with = "js_num::opt_f64", skip_serializing_if = "Option::is_none")]
    pub guidelines_at: Option<f64>,
    /// Set when this card is the main variant of a component placed by live views (newest mark wins).
    #[serde(default, serialize_with = "js_num::opt_f64", skip_serializing_if = "Option::is_none")]
    pub main_at: Option<f64>,
    /// Set while an AI edit runs on this card (shape owned by the edit feature).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editing: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit_error: Option<String>,
    /// Earlier contents, newest last (at most 5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub versions: Option<Vec<DesignVersion>>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Design {
    pub fn is_ready(&self) -> bool {
        self.status == DesignStatus::Ready
    }
    pub fn is_starred(&self) -> bool {
        self.starred == Some(true)
    }
    pub fn is_component(&self) -> bool {
        self.component == Some(true)
    }
}

/// One earlier content of a card (kept by in-place AI edits for undo).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignVersion {
    #[serde(default)]
    pub html: String,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpaceKind {
    /// A top-level project: the main canvas holds only projects, and projects never nest.
    Project,
}

/// A canvas (`spaces/<id>`). Nested spaces are entered through a portal card on their parent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Space {
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// None only for the main canvas (and for stray top-level spaces, e.g. old benchmark data).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_space_id: Option<String>,
    #[serde(serialize_with = "js_num::f64", default)]
    pub created_at: f64,
    /// The local project folder (sub-spaces inherit it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<SpaceKind>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Space {
    pub fn is_project(&self) -> bool {
        self.kind == Some(SpaceKind::Project)
    }
}

/// Request body of POST /api/variations, as stored on designs and runs (`baseHtml` omitted there).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariationRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_html: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_title: Option<String>,
    #[serde(default)]
    pub instruction: String,
    #[serde(default)]
    pub count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    /// "page" or "component".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,
    /// "codex" or "claude".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_id: Option<String>,
    /// bases, annotations, podContext, importFrom, components, … (shapes owned by proto-ai).
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Direction {
    pub title: String,
    pub brief: String,
}

/// A generation in flight (`runs/<id>`), so a restart can reattach to the job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_job_id: Option<String>,
    pub space_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub kind: DesignKind,
    #[serde(default)]
    pub instruction: String,
    #[serde(default)]
    pub origin: Point,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub request: VariationRequest,
    /// Placeholder design ids by direction index.
    #[serde(default)]
    pub design_ids: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LaneType {
    Clay,
    Spoke,
    Round,
    Merge,
}

/// One exploration lane of the spokes feature (`lanes/<jobId>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lane {
    pub job_id: String,
    #[serde(rename = "type")]
    pub lane_type: LaneType,
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hub_id: Option<String>,
    #[serde(default)]
    pub count: u32,
    #[serde(serialize_with = "js_num::f64", default)]
    pub started_at: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Where a promoted copy came from (`promotedFrom/<copyId>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotedRef {
    pub space_id: String,
    pub design_id: String,
    /// Captured so the badge survives deleting the source space.
    #[serde(default)]
    pub space_name: String,
    #[serde(serialize_with = "js_num::f64", default)]
    pub at: f64,
    #[serde(flatten)]
    pub extra: Extra,
}

/// An Annotate note pinned on a design (`notes/<designId>` holds a list of these, oldest first).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesignNote {
    pub id: String,
    pub design_id: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub element: String,
    #[serde(default)]
    pub element_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_text: Option<String>,
    #[serde(serialize_with = "js_num::f64", default)]
    pub timestamp: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A poll published for voting (`publishedPolls/<id>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishedPoll {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(serialize_with = "js_num::f64", default)]
    pub created_at: f64,
    #[serde(default)]
    pub share_url: String,
    #[serde(default)]
    pub decisions: Vec<PollDecision>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PollDecision {
    pub id: String,
    #[serde(default)]
    pub question: String,
    #[serde(default)]
    pub options: Vec<PollOption>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PollOption {
    pub id: String,
    pub design_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub title: String,
    #[serde(flatten)]
    pub extra: Extra,
}
