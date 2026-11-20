//! Note data, built directly by the `--snap` scenes (spec 04 AN-IF-108…112). Data only; the rules that work on
//! it live next door (`logic`, `wire`).

/// Target of a note: a Rune element of the app, or an HTML element of a shown design (a "web target", which
/// carries a design id).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Target {
    /// Role and label, e.g. `button "Review"`.
    pub element: String,
    /// Readable path (keys for Rune, short selector for HTML).
    pub element_path: String,
    /// `[x, y, w, h]` in window points.
    pub rect: [f32; 4],
    /// Rune: creating `file:line`, if found.
    pub source: Option<String>,
    /// Rune: key the marker is attached to.
    pub key: Option<String>,
    /// Rune: the module of `source` (`features::review::dock`).
    pub module: Option<String>,
    /// Full runtime path (Rune) or the selector from `html` down (web).
    pub full_path: String,
    /// Text around the element (`a · b · c`).
    pub nearby_text: String,
    /// Look of the element as text (`size: 80×36; fill: #1447e6`), or the page's computed styles.
    pub styles: String,
    /// `role=button, label="Review"` or the page's accessibility facts.
    pub accessibility: String,
    /// Web: the design whose page holds the element. `Some` marks a web target.
    pub design_id: Option<String>,
    /// Web: CSS classes.
    pub css_classes: String,
    /// Web: neighbouring elements.
    pub nearby_elements: String,
    /// Web: highlighted text when picked.
    pub selected_text: Option<String>,
    /// Web: telling attributes, sorted by name.
    pub attributes: Vec<(String, String)>,
}

impl Target {
    /// A web target (an element in a design's page).
    pub fn is_web(&self) -> bool {
        self.design_id.is_some()
    }
}

/// Note state. `Queued` = only here so far (not yet on the notes service); the other four are the service's words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Queued,
    Pending,
    Acknowledged,
    Resolved,
    Dismissed,
}

impl Status {
    /// The status word as shown in the UI (and, except `queued`, as the service spells it).
    pub fn word(self) -> &'static str {
        match self {
            Status::Queued => "queued",
            Status::Pending => "pending",
            Status::Acknowledged => "acknowledged",
            Status::Resolved => "resolved",
            Status::Dismissed => "dismissed",
        }
    }

    /// Open notes count, get a number and a pin.
    pub fn is_open(self) -> bool {
        matches!(self, Status::Queued | Status::Pending | Status::Acknowledged)
    }

    /// Reading the service: the three known words map to themselves, anything else (missing too) is pending.
    pub fn from_service(word: Option<&str>) -> Status {
        match word {
            Some("acknowledged") => Status::Acknowledged,
            Some("resolved") => Status::Resolved,
            Some("dismissed") => Status::Dismissed,
            _ => Status::Pending,
        }
    }
}

/// Thread entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    /// `human` | `agent`.
    pub role: String,
    pub content: String,
}

/// User note on one or more targets.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// Local id; stays the same while offline.
    pub id: String,
    /// Id assigned by the notes service, after upload.
    pub server_id: Option<String>,
    /// Space it was created in.
    pub space: String,
    /// What the user wrote.
    pub comment: String,
    pub targets: Vec<Target>,
    /// Unix ms at creation.
    pub timestamp: f64,
    pub status: Status,
    pub thread: Vec<Message>,
    /// `[w, h]` of the window at creation.
    pub window: [f32; 2],
}

impl Note {
    /// The design of the first target that has one (spec 03 AN-SY-101).
    pub fn design_id(&self) -> Option<&str> {
        self.targets.iter().find_map(|t| t.design_id.as_deref())
    }
}
