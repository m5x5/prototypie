//! @ pod-data and # component mentions in the composer (the reference's `pod-mention`): the trigger state
//! machine over the composer's text field, the pod catalog (`GET /api/pod-catalog`, once per run), the
//! component options (cards of this space and its ancestors), chips, and what a request carries
//! (`podContext`, `components`). Worker-6's composer calls these hooks (composer/hooks.rs).

pub mod view;

use std::collections::{BTreeSet, HashMap};

use proto_core::Snapshot;
use rune::{KeyEvent, Latest, Tx, Waker};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::state::AppState;
use crate::features::http;

pub use view::{buttons, chips, context_panel, listbox};

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Field {
    pub p: String,
    pub v: Option<String>,
    #[serde(rename = "type")]
    pub ty: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Relation {
    pub p: String,
    pub target: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Entry {
    pub id: String,
    pub kind: String,
    pub pod: String,
    pub label: String,
    #[serde(default)]
    pub path: String,
    pub access: String,
    pub stand: Option<String>,
    #[serde(default)]
    pub fields: Vec<Field>,
    #[serde(default)]
    pub relations: Vec<Relation>,
    #[serde(default)]
    pub children: Vec<String>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Pod {
    pub id: String,
    pub label: String,
    pub display: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Catalog {
    pub pods: Vec<Pod>,
    pub entries: Vec<Entry>,
}

pub const KIND_ORDER: [&str; 4] = ["resource", "container", "shape", "pod"];

pub fn kind_label(kind: &str) -> &'static str {
    match kind {
        "resource" => "Resources",
        "container" => "Containers",
        "shape" => "Shapes · types",
        _ => "Pods",
    }
}

impl Catalog {
    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// "<pod display> · <path>" for shapes, else the pod display + "/" + path.
    pub fn source_of(&self, e: &Entry) -> String {
        let root = self.pods.iter().find(|p| p.id == e.pod).map_or(e.pod.clone(), |p| p.display.clone());
        if e.kind == "shape" {
            return format!("{root} · {}", e.path);
        }
        if e.path.is_empty() {
            root
        } else if root.ends_with('/') {
            format!("{root}{}", e.path)
        } else {
            format!("{root}/{}", e.path)
        }
    }

    /// Entries matching the text after "@" (label, path or group label), grouped in display order.
    pub fn matching(&self, query: &str) -> Vec<&Entry> {
        let q = query.trim().to_lowercase();
        let hit = |e: &Entry| q.is_empty() || [e.label.as_str(), e.path.as_str(), kind_label(&e.kind)].iter().any(|x| x.to_lowercase().contains(&q));
        KIND_ORDER.iter().flat_map(|k| self.entries.iter().filter(move |e| e.kind == *k && hit(e))).collect()
    }
}

/// A "#" option: a ready card of this space or an ancestor (not the main canvas, unless that's where you are).
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentOption {
    pub id: String,
    pub title: String,
    pub component: bool,
    /// The parent space's name, for cards not in the current space.
    pub from: Option<String>,
}

pub fn component_options(s: &Snapshot, space: &str, query: &str) -> Vec<ComponentOption> {
    let mut chain: Vec<String> = Vec::new();
    if space == proto_core::ROOT_SPACE_ID {
        chain.push(space.into());
    } else {
        let mut cur = Some(space.to_string());
        while let Some(c) = cur.filter(|c| c != proto_core::ROOT_SPACE_ID && chain.len() < 20) {
            cur = s.space(&c).and_then(|sp| sp.parent_space_id.clone());
            chain.push(c);
        }
    }
    let q = query.trim().to_lowercase();
    let mut v: Vec<(ComponentOption, usize)> = s
        .designs()
        .filter(|d| d.is_ready() && !d.html.is_empty() && (q.is_empty() || d.title.to_lowercase().contains(&q)))
        .filter_map(|d| {
            let depth = chain.iter().position(|c| *c == d.space_id)?;
            let component = d.is_component() || d.request.as_ref().and_then(|r| r.scope.as_deref()) == Some("component") || d.kind == proto_core::DesignKind::Spoke;
            let from = (depth > 0).then(|| s.space(&d.space_id).map_or(d.space_id.clone(), |sp| sp.name.clone()));
            Some((ComponentOption { id: d.id.clone(), title: d.title.clone(), component, from }, depth))
        })
        .collect();
    v.sort_by(|a, b| b.0.component.cmp(&a.0.component).then(a.1.cmp(&b.1)).then(a.0.title.cmp(&b.0.title)));
    v.into_iter().map(|x| x.0).collect()
}

/// A mention chip: pod data or a component card.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Chip {
    Pod(String),
    Component(String),
}

/// The open list: which trigger, where it started in the text (byte offset of the trigger), the query.
#[derive(Clone, Debug, PartialEq)]
pub struct Open {
    pub trigger: char,
    pub start: usize,
    pub query: String,
    pub active: usize,
}

/// Mentions of one composer draft.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mentions {
    pub chips: Vec<Chip>,
    /// Pod fields switched off: "<entryId>:<fieldIndex>".
    pub off: BTreeSet<String>,
}

#[derive(Default)]
pub struct MentionState {
    pub catalog: Option<Catalog>,
    pub catalog_error: Option<String>,
    catalog_req: Latest<Result<Catalog, String>>,
    requested: bool,
    pub open: Option<Open>,
    pub by_draft: HashMap<String, Mentions>,
}

impl MentionState {
    pub fn setup(&mut self, waker: Waker) {
        self.catalog_req.set_waker(waker);
    }
}

fn key(s: &AppState) -> String {
    crate::composer::draft_key(s)
}

pub fn mentions(s: &AppState) -> Option<&Mentions> {
    s.features.mention.by_draft.get(&key(s))
}

fn mentions_mut(s: &mut AppState) -> &mut Mentions {
    let k = key(s);
    s.features.mention.by_draft.entry(k).or_default()
}

pub fn ensure_catalog(s: &mut AppState) {
    let m = &mut s.features.mention;
    if m.catalog.is_some() || m.requested {
        return;
    }
    m.requested = true;
    m.catalog_req.request(http::server_addr(), |a| {
        http::request(&a, "GET", "/api/pod-catalog", None).and_then(|b| serde_json::from_slice::<Catalog>(&b).map_err(|e| e.to_string()))
    });
}

/// Land the catalog; true when it arrived (or failed).
pub fn poll(s: &mut AppState) -> bool {
    let Some(r) = s.features.mention.catalog_req.take() else { return false };
    let m = &mut s.features.mention;
    m.requested = false;
    match r {
        Ok(c) => m.catalog = Some(c),
        Err(e) => m.catalog_error = Some(e),
    }
    true
}

/// The trigger before the caret: `(^|\s)T([^\sT]*)$` → (trigger byte offset, query).
pub fn detect(text: &str, caret: usize, trigger: char) -> Option<(usize, String)> {
    let before = text.get(..caret)?;
    let start = before.rfind(trigger)?;
    let query = &before[start + trigger.len_utf8()..];
    if query.chars().any(|c| c.is_whitespace() || c == trigger) {
        return None;
    }
    let ok_before = before[..start].chars().next_back().is_none_or(char::is_whitespace);
    ok_before.then(|| (start, query.to_string()))
}

/// Options for the open list (ids, in display order).
pub fn options(s: &AppState) -> Vec<String> {
    let Some(o) = &s.features.mention.open else { return Vec::new() };
    match o.trigger {
        '@' => s.features.mention.catalog.as_ref().map(|c| c.matching(&o.query).iter().map(|e| e.id.clone()).collect()).unwrap_or_default(),
        _ => component_options(s.snap(), &s.route.space, &o.query).into_iter().map(|c| c.id).collect(),
    }
}

/// The text or caret changed: open, update or close the list.
pub fn on_change(s: &mut AppState, _tx: &mut Tx) {
    let e = crate::composer::edit(s);
    let (text, caret) = (e.text.clone(), e.focus.min(e.text.len()));
    let found = ['@', '#'].into_iter().find_map(|t| detect(&text, caret, t).map(|(start, q)| (t, start, q)));
    let prev = s.features.mention.open.clone();
    s.features.mention.open = found.map(|(trigger, start, query)| {
        let active = match &prev {
            Some(p) if p.trigger == trigger && p.query == query => p.active,
            _ => 0,
        };
        Open { trigger, start, query, active }
    });
    if s.features.mention.open.as_ref().is_some_and(|o| o.trigger == '@') {
        ensure_catalog(s);
    }
}

/// Pick an option: remove "<trigger>query" from the text, put the caret there, add the chip.
pub fn pick(s: &mut AppState, id: &str, tx: &mut Tx) {
    let Some(o) = s.features.mention.open.take() else { return };
    let e = crate::composer::edit_mut(s);
    let end = (o.start + o.trigger.len_utf8() + o.query.len()).min(e.text.len());
    let mut text = e.text.clone();
    text.replace_range(o.start..end, "");
    e.replace_text(text);
    e.anchor = o.start;
    e.focus = o.start;
    let chip = if o.trigger == '@' { Chip::Pod(id.into()) } else { Chip::Component(id.into()) };
    let m = mentions_mut(s);
    if !m.chips.contains(&chip) {
        m.chips.push(chip);
    }
    crate::composer::focus(tx);
}

pub fn remove(s: &mut AppState, chip: &Chip) {
    mentions_mut(s).chips.retain(|c| c != chip);
}

/// The trigger buttons: insert the trigger at the caret (with a space before it when needed) and open the list.
pub fn open_from_button(s: &mut AppState, trigger: char, tx: &mut Tx) {
    if s.features.mention.open.as_ref().is_some_and(|o| o.trigger == trigger) {
        s.features.mention.open = None;
        return;
    }
    let e = crate::composer::edit_mut(s);
    let pos = e.focus.min(e.text.len());
    let lead = if pos > 0 && !e.text[..pos].ends_with(char::is_whitespace) { " " } else { "" };
    let mut text = e.text.clone();
    text.insert_str(pos, &format!("{lead}{trigger}"));
    e.replace_text(text);
    let start = pos + lead.len();
    e.anchor = start + trigger.len_utf8();
    e.focus = start + trigger.len_utf8();
    s.features.mention.open = Some(Open { trigger, start, query: String::new(), active: 0 });
    if trigger == '@' {
        ensure_catalog(s);
    }
    crate::composer::focus(tx);
}

/// The field's keys first: ↑/↓ move, Enter/Tab pick, Esc closes, Backspace at the very start removes the last
/// chip. True = consumed (the composer must not send).
pub fn on_key(s: &mut AppState, ev: &KeyEvent, tx: &mut Tx) -> bool {
    use rune::Key;
    if s.features.mention.open.is_some() {
        let opts = options(s);
        let o = s.features.mention.open.as_mut().expect("open");
        match ev.key {
            Key::Down | Key::Up if !opts.is_empty() => {
                let n = opts.len();
                o.active = if ev.key == Key::Down { (o.active + 1) % n } else { (o.active + n - 1) % n };
                return true;
            }
            Key::Enter | Key::Tab if !opts.is_empty() => {
                let id = opts[o.active.min(opts.len() - 1)].clone();
                pick(s, &id, tx);
                return true;
            }
            Key::Escape => {
                s.features.mention.open = None;
                return true;
            }
            _ => {}
        }
    }
    if ev.key == Key::Backspace {
        let e = crate::composer::edit(s);
        if e.anchor == 0 && e.focus == 0 && mentions(s).is_some_and(|m| !m.chips.is_empty()) {
            mentions_mut(s).chips.pop();
            return true;
        }
    }
    false
}

/// What a request carries: `podContext` (mentioned entries with their switched-on fields) and `components`
/// (current html of each mentioned card; deleted ones drop out). JSON in the reference's shape.
pub fn request_parts(s: &AppState) -> (Option<Vec<Value>>, Option<Vec<Value>>) {
    let Some(m) = mentions(s) else { return (None, None) };
    let mut pods = Vec::new();
    let mut comps = Vec::new();
    for c in &m.chips {
        match c {
            Chip::Pod(id) => {
                let Some(cat) = &s.features.mention.catalog else { continue };
                let Some(e) = cat.entry(id) else { continue };
                let fields: Vec<Value> = e
                    .fields
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !m.off.contains(&format!("{}:{i}", e.id)))
                    .map(|(_, f)| {
                        let mut v = json!({ "p": f.p });
                        if let Some(x) = &f.v { v["v"] = json!(x); }
                        if let Some(x) = &f.ty { v["type"] = json!(x); }
                        v
                    })
                    .collect();
                let mut v = json!({"id": e.id, "kind": e.kind, "label": e.label, "source": cat.source_of(e), "access": e.access, "fields": fields});
                if let Some(st) = &e.stand { v["stand"] = json!(st); }
                if !e.relations.is_empty() { v["relations"] = json!(e.relations.iter().map(|r| json!({"p": r.p, "target": r.target})).collect::<Vec<_>>()); }
                if let Some(n) = &e.note { v["note"] = json!(n); }
                pods.push(v);
            }
            Chip::Component(id) => {
                if let Some(d) = s.snap().design(id).filter(|d| !d.html.is_empty()) {
                    comps.push(json!({"id": d.id, "title": d.title, "html": d.html}));
                }
            }
        }
    }
    ((!pods.is_empty()).then_some(pods), (!comps.is_empty()).then_some(comps))
}

/// Forget the draft's mentions after sending.
pub fn clear(s: &mut AppState) {
    let k = key(s);
    s.features.mention.by_draft.remove(&k);
    s.features.mention.open = None;
}

pub fn toggle_field(s: &mut AppState, field_key: &str) {
    let m = mentions_mut(s);
    if !m.off.remove(field_key) {
        m.off.insert(field_key.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_open_only_at_word_starts() {
        assert_eq!(detect("hi @tas", 7, '@'), Some((3, "tas".into())));
        assert_eq!(detect("@", 1, '@'), Some((0, String::new())));
        assert_eq!(detect("mail@x", 6, '@'), None);
        assert_eq!(detect("@a b", 4, '@'), None);
        assert_eq!(detect("use #hero", 9, '#'), Some((4, "hero".into())));
    }

    #[test]
    fn components_come_first_then_nearer_spaces() {
        let mut s = AppState::new();
        s.use_store(crate::features::scenes::fixture());
        let o = component_options(s.snap(), "zzfx", "");
        assert_eq!(o.len(), 4);
        assert!(o.iter().all(|x| x.from.is_none()));
        assert_eq!(component_options(s.snap(), "zzfx", "conf")[0].title, "Confetti summary");
    }

    #[test]
    fn requests_carry_mentioned_components() {
        let mut s = AppState::new();
        s.use_store(crate::features::scenes::fixture());
        s.enter_space("zzfx", None);
        mentions_mut(&mut s).chips.push(Chip::Component("zzv2".into()));
        let (pods, comps) = request_parts(&s);
        assert!(pods.is_none());
        assert_eq!(comps.unwrap()[0]["title"], "Confetti summary");
        clear(&mut s);
        assert_eq!(request_parts(&s), (None, None));
    }
}
