//! Pure rules of Annotate that need no window: paths and keys, element labels and looks, numbering, the
//! connection state, the markdown export and local ids (spec 01 §4, §6, §8, §14, AN-UI-034).

use super::model::{Note, Status, Target};

/// The notes service's default address (spec 00).
pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:3124";

// ---- runtime paths (AN-UI-042) ----

/// Segments that are element keys: not `root`/`app` (the shell's wrappers) and not unnamed `#n` children.
fn key_segments(path: &str) -> impl Iterator<Item = &str> {
    path.split('/').filter(|s| !s.is_empty() && !s.starts_with('#') && *s != "root" && *s != "app")
}

/// The last key segment of a runtime path.
pub fn nearest_key(path: &str) -> Option<String> {
    key_segments(path).last().map(str::to_string)
}

/// All key segments joined with ` > `.
pub fn key_path(path: &str) -> String {
    key_segments(path).collect::<Vec<_>>().join(" > ")
}

/// The file name with line of a `file:line` source (`dock.rs:33`).
pub fn file_of(source: &str) -> &str {
    source.rsplit('/').next().unwrap_or(source)
}

/// The hover chip's text (AN-UI-045): the last two key-path segments, then the source file when known.
pub fn chip_text(key_path: &str, source: Option<&str>) -> String {
    let parts: Vec<&str> = key_path.split(" > ").filter(|s| !s.is_empty()).collect();
    let mut t = match parts.len() {
        0 => "element".to_string(),
        1 => parts[0].to_string(),
        n => format!("{} › {}", parts[n - 2], parts[n - 1]),
    };
    if let Some(src) = source {
        t.push_str("  ·  ");
        t.push_str(file_of(src));
    }
    t
}

/// The second location line of a target (AN-UI-101): its source without `crates/proto-app/src/`, else its key
/// path, else nothing.
pub fn location_line(t: &Target) -> Option<String> {
    if let Some(src) = &t.source {
        return Some(src.strip_prefix("crates/proto-app/src/").unwrap_or(src).to_string());
    }
    (!t.element_path.is_empty()).then(|| t.element_path.clone())
}

// ---- element facts (AN-UI-070/071) ----

/// What the analysis of one drawn element found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Analysis {
    pub path: String,
    pub role: String,
    pub label: Option<String>,
    pub text: Option<String>,
    pub rect: [f32; 4],
    pub styles: String,
    pub nearby: String,
}

/// One element of a frame, the parts the analysis reads (from `rune::ElementInfo`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fact {
    pub path: String,
    pub key: Option<String>,
    pub role: String,
    pub label: Option<String>,
    pub text: Option<String>,
    pub rect: [f32; 4],
    /// RGBA 0–1.
    pub fill: [f32; 4],
    pub radius: f32,
    pub text_color: Option<[f32; 4]>,
}

/// `#rrggbb` (or `#rrggbbaa` below full opacity), channels clamped to 0–1.
pub fn hex(c: [f32; 4]) -> String {
    let ch = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut s = format!("#{:02x}{:02x}{:02x}", ch(c[0]), ch(c[1]), ch(c[2]));
    if c[3] < 1.0 {
        s.push_str(&format!("{:02x}", ch(c[3])));
    }
    s
}

/// A number without needless decimals (`8`, `7.5`).
pub fn num(v: f32) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    if r == r.trunc() { format!("{}", r as i64) } else { format!("{r}") }
}

/// `size: W×H; fill: …; radius: …; color: …`.
pub fn style_text(f: &Fact) -> String {
    let mut s = format!("size: {}×{}", f.rect[2].round() as i64, f.rect[3].round() as i64);
    if f.fill[3] > 0.0 {
        s.push_str(&format!("; fill: {}", hex(f.fill)));
    }
    if f.radius > 0.0 {
        s.push_str(&format!("; radius: {}", num(f.radius)));
    }
    if let Some(c) = f.text_color {
        s.push_str(&format!("; color: {}", hex(c)));
    }
    s
}

/// The element at `path` in `facts` (the first with that path), analysed; None when it isn't in the frame.
pub fn analyse(facts: &[Fact], path: &str) -> Option<Analysis> {
    let f = facts.iter().find(|f| f.path == path)?;
    let parent = path.rsplit_once('/').map_or("", |(p, _)| p);
    let prefix = format!("{parent}/");
    let nearby: Vec<&str> = facts.iter()
        .filter(|o| o.path != path && o.path.starts_with(&prefix))
        .filter_map(|o| match &o.text {
            Some(t) => Some(t.as_str()),
            None => o.label.as_deref(),
        })
        .filter(|t| !t.trim().is_empty())
        .take(4)
        .collect();
    Some(Analysis {
        path: path.to_string(),
        role: f.role.clone(),
        label: f.label.clone(),
        text: f.text.clone(),
        rect: f.rect,
        styles: style_text(f),
        nearby: nearby.join(" · "),
    })
}

/// The element's description: `role "name"`, else ``role `key` ``, else `role`.
pub fn element_label(analysis: Option<&Analysis>, key: Option<&str>) -> String {
    let (role, name) = match analysis {
        Some(a) => {
            let name = a.label.as_deref().or(a.text.as_deref()).unwrap_or("");
            (a.role.as_str(), name.chars().take(60).collect::<String>())
        }
        None => ("element", String::new()),
    };
    if !name.is_empty() {
        format!("{role} \"{name}\"")
    } else if let Some(k) = key {
        format!("{role} `{k}`")
    } else {
        role.to_string()
    }
}

/// A target from the hover (AN-UI-071). `analysis` counts only when it is for exactly this path.
pub fn target_from_hover(path: &str, analysis: Option<&Analysis>, key_rect: Option<[f32; 4]>, source: Option<(String, String)>) -> Target {
    let analysis = analysis.filter(|a| a.path == path);
    let key = nearest_key(path);
    let accessibility = match analysis {
        Some(a) => {
            let mut s = format!("role={}", a.role);
            if let Some(l) = &a.label {
                s.push_str(&format!(", label=\"{l}\""));
            }
            s
        }
        None => String::new(),
    };
    let (source, module) = match source {
        Some((s, m)) => (Some(s), Some(m)),
        None => (None, None),
    };
    Target {
        element: element_label(analysis, key.as_deref()),
        element_path: key_path(path),
        full_path: path.to_string(),
        rect: analysis.map(|a| a.rect).or(key_rect).unwrap_or([0.0; 4]),
        nearby_text: analysis.map(|a| a.nearby.clone()).unwrap_or_default(),
        styles: analysis.map(|a| a.styles.clone()).unwrap_or_default(),
        accessibility,
        source,
        module,
        key,
        ..Target::default()
    }
}

// ---- notes of a space (AN-UI-065/066) ----

/// Open notes of `space`, numbered from 1 in list order.
pub fn numbered<'a>(notes: &'a [Note], space: &str) -> Vec<(u32, &'a Note)> {
    notes.iter().filter(|n| n.space == space && n.status.is_open()).enumerate().map(|(i, n)| (i as u32 + 1, n)).collect()
}

/// A note's number among the open notes of its space (0 when closed).
pub fn number_of(notes: &[Note], id: &str, space: &str) -> u32 {
    numbered(notes, space).into_iter().find(|(_, n)| n.id == id).map_or(0, |(i, _)| i)
}

/// Status colour (AN-UI-063), as hex.
pub fn status_hex(s: Status) -> &'static str {
    match s {
        Status::Queued | Status::Dismissed => "#737373",
        Status::Pending => "#3b82f6",
        Status::Acknowledged => "#f59e0b",
        Status::Resolved => "#10b981",
    }
}

// ---- markdown (AN-UI-151) ----

/// The open notes of `space` as markdown, in number order.
pub fn markdown(notes: &[Note], space: &str) -> String {
    let mut out = String::from("## Prototypie Rune notes\n");
    for (i, n) in numbered(notes, space) {
        out.push('\n');
        let title = n.targets.first().map_or("note", |t| t.element.as_str());
        out.push_str(&format!("### {i}. {title}\n"));
        for t in &n.targets {
            if !t.element_path.is_empty() {
                out.push_str(&format!("**Location:** `{}`\n", t.element_path));
            }
            if let Some(src) = &t.source {
                match &t.module {
                    Some(m) => out.push_str(&format!("**Source:** {src} ({m})\n")),
                    None => out.push_str(&format!("**Source:** {src}\n")),
                }
            }
        }
        if let Some(d) = n.design_id() {
            out.push_str(&format!("**Design:** {d}\n"));
        }
        out.push_str(&format!("**Feedback:** {}\n", n.comment.trim()));
    }
    out
}

// ---- connection state (AN-UI-034) ----

/// What the last exchange with the service said.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reach {
    #[default]
    Unknown,
    Yes,
    No,
}

/// The connection dot's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conn {
    Unknown,
    /// Unreachable with `n` notes waiting.
    Offline(usize),
    Unreachable,
    /// Reachable, but notes wait or the last success is old.
    Syncing(usize),
    Connected,
}

/// The tick interval: 4 s while active (mode on or list open), else 30 s.
pub fn interval(active: bool) -> f64 {
    if active { 4.0 } else { 30.0 }
}

/// Queued notes without a service id, all spaces.
pub fn waiting(notes: &[Note]) -> usize {
    notes.iter().filter(|n| n.server_id.is_none() && n.status == Status::Queued).count()
}

pub fn conn(reach: Reach, waiting: usize, clock: f64, last_ok: f64, active: bool) -> Conn {
    match reach {
        Reach::Unknown => Conn::Unknown,
        Reach::No if waiting > 0 => Conn::Offline(waiting),
        Reach::No => Conn::Unreachable,
        Reach::Yes if waiting > 0 || clock - last_ok > 2.0 * interval(active) + 5.0 => Conn::Syncing(waiting),
        Reach::Yes => Conn::Connected,
    }
}

/// `http://` and trailing slashes off the effective endpoint.
pub fn host(endpoint: &str) -> String {
    let e = endpoint.strip_prefix("http://").unwrap_or(endpoint);
    e.trim_end_matches('/').to_string()
}

fn notes_word(k: usize) -> String {
    if k == 1 { "1 note".into() } else { format!("{k} notes") }
}

pub fn conn_hex(c: Conn) -> &'static str {
    match c {
        Conn::Unknown => "#a3a3a3",
        Conn::Offline(_) | Conn::Syncing(_) => "#f59e0b",
        Conn::Unreachable => "#ef4444",
        Conn::Connected => "#10b981",
    }
}

pub fn conn_text(c: Conn, endpoint: &str) -> String {
    let h = host(endpoint);
    match c {
        Conn::Unknown => format!("Connecting to the notes service ({h})"),
        Conn::Offline(k) => format!("Offline: {} queued ({h} unreachable)", notes_word(k)),
        Conn::Unreachable => format!("Notes service unreachable ({h})"),
        Conn::Syncing(0) => format!("Reconnecting to the notes service ({h})"),
        Conn::Syncing(k) => format!("Connected to the notes service ({h}): sending {}", notes_word(k)),
        Conn::Connected => format!("Connected to the notes service ({h})"),
    }
}

/// The list header's right text (AN-UI-121): by the last exchange only.
pub fn list_status(reach: Reach) -> &'static str {
    match reach {
        Reach::No => "Offline, notes wait here",
        Reach::Yes => "Notes service connected",
        Reach::Unknown => "",
    }
}

// ---- local ids (AN-SY-051) ----

/// `<ms in base 36>-<up to 6 random [0-9a-z]>`.
pub fn local_id(now_ms: f64) -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let base36 = |mut v: u64| {
        let mut s = Vec::new();
        loop {
            s.push(DIGITS[(v % 36) as usize]);
            v /= 36;
            if v == 0 {
                break;
            }
        }
        s.reverse();
        String::from_utf8(s).expect("ascii")
    };
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(SEQ.fetch_add(1, Ordering::Relaxed));
    h.write_u64(now_ms.to_bits());
    let mut r = h.finish();
    let mut tail = String::with_capacity(6);
    for _ in 0..6 {
        tail.push(DIGITS[(r % 36) as usize] as char);
        r /= 36;
    }
    format!("{}-{tail}", base36(now_ms.max(0.0) as u64))
}
