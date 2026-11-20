//! Live views: a card whose html places other cards by reference instead of copying them
//! (port of the reference's `src/features/live-view/compose.ts`).
//!
//! ```html
//! <prototypie-slot data-design="<design id>" class="…" style="…" [data-fit="fill"] [data-height="N"]></prototypie-slot>
//! ```
//!
//! At render time each slot shows the current html of that design's main variant (the newest ready card marked
//! `mainAt` among the design and its variations, else the design itself), in an iframe inside a div that keeps
//! the slot's attributes. [`compose_live`] is pure: every render path (snapshots, fullscreen, export) uses it.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock};

use regex::Regex;

use crate::snapshot::Snapshot;
use crate::types::Design;

static HAS_SLOT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<prototypie-slot\b").unwrap());
static SLOT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<prototypie-slot\b([^>]*)>(?:\s*</prototypie-slot>)?").unwrap());
static ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)\bdata-design\s*=\s*["']?([A-Za-z0-9_-]+)"#).unwrap());
static HEIGHT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)\bdata-height\s*=\s*["']?([0-9]+)"#).unwrap());
static CLASS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)\bclass\s*=\s*"([^"]*)""#).unwrap());

/// How deep slots inside slotted components are expanded.
const MAX_DEPTH: usize = 3;

/// Injected once into a composed document: iframe styles plus the script that grows each iframe to its content.
pub const SIZER: &str = r#"<style data-prototypie-live>.pt-slot>iframe{display:block;width:100%;border:0;background:transparent}.pt-slot[data-fit=fill]>iframe{height:100%}</style>
<script data-prototypie-live>(()=>{const fit=f=>{try{const d=f.contentDocument;if(!d||!d.body)return;const h=Math.max(d.documentElement.scrollHeight,d.body.scrollHeight,...[...d.body.children].map(c=>c.getBoundingClientRect().top+c.scrollHeight));if(h>0)f.style.height=Math.ceil(h)+'px'}catch(e){}};
const hook=f=>{const s=f.parentElement;if(!s||s.dataset.fit==='fill'||s.dataset.height)return;const go=()=>{fit(f);try{new ResizeObserver(()=>fit(f)).observe(f.contentDocument.body)}catch(e){}};f.addEventListener('load',go);if(f.contentDocument&&f.contentDocument.readyState==='complete')go()};
document.querySelectorAll('.pt-slot>iframe').forEach(hook)})()</script>"#;

/// Whether `html` contains a slot (case-insensitive).
pub fn has_slots(html: &str) -> bool {
    HAS_SLOT_RE.is_match(html)
}

/// Ids referenced by the slots in `html`, in document order.
pub fn slot_ids(html: &str) -> Vec<String> {
    SLOT_RE
        .captures_iter(html)
        .filter_map(|c| ID_RE.captures(c.get(1).map_or("", |m| m.as_str())).map(|m| m[1].to_string()))
        .collect()
}

/// Designs by id plus a `parentId` → children index, built once per call over a snapshot.
pub(crate) struct Lineage {
    designs: HashMap<String, Arc<Design>>,
    children: HashMap<String, Vec<String>>,
}

impl Lineage {
    pub(crate) fn new(snap: &Snapshot) -> Lineage {
        let mut designs = HashMap::new();
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        for d in snap.designs() {
            if let Some(p) = &d.parent_id {
                children.entry(p.clone()).or_default().push(d.id.clone());
            }
            designs.insert(d.id.clone(), d);
        }
        Lineage { designs, children }
    }

    /// See [`Snapshot::main_variant`].
    pub(crate) fn main_variant(&self, id: &str) -> Option<Arc<Design>> {
        let root = self.designs.get(id)?;
        let mut best: Option<&Arc<Design>> = None;
        let mut seen = HashSet::new();
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            if !seen.insert(cur) {
                continue;
            }
            let d = self.designs.get(cur);
            // A live view derived from the component is not one of its variants: skip it and its branch.
            if cur != id && d.is_some_and(|d| has_slots(&d.html)) {
                continue;
            }
            if let Some(d) = d
                && let Some(at) = d.main_at.filter(|at| *at != 0.0 && d.is_ready())
                && best.is_none_or(|b| at > b.main_at.unwrap_or(0.0))
            {
                best = Some(d);
            }
            stack.extend(self.children.get(cur).into_iter().flatten().map(String::as_str));
        }
        Some(best.unwrap_or(root).clone())
    }

    /// The html a slot for `id` shows (its main variant's, when ready), None otherwise.
    fn slot_html(&self, id: &str) -> Option<(Arc<Design>, &str)> {
        let d = self.main_variant(id)?;
        if !d.is_ready() {
            return None;
        }
        // Borrow the html from the index (same Arc), so callers can keep a &str.
        let html = self.designs.get(&d.id).map(|x| x.html.as_str())?;
        Some((d, html))
    }
}

/// Ids of every design that is a slot target somewhere, or a variation of one (so it can be made main). The walk
/// stops at a descendant that has slots itself and isn't a target. Memoise per snapshot version.
pub fn slot_lineage(snap: &Snapshot) -> HashSet<String> {
    let lin = Lineage::new(snap);
    let mut targets = HashSet::new();
    for d in lin.designs.values() {
        if has_slots(&d.html) {
            targets.extend(slot_ids(&d.html));
        }
    }
    let mut out = HashSet::new();
    let mut stack: Vec<String> = targets.iter().cloned().collect();
    while let Some(cur) = stack.pop() {
        if out.contains(&cur) || (!targets.contains(&cur) && lin.designs.get(&cur).is_some_and(|d| has_slots(&d.html))) {
            continue;
        }
        stack.extend(lin.children.get(&cur).into_iter().flatten().cloned());
        out.insert(cur);
    }
    out
}

/// `&`, `"`, `<`, `>` escaped (in that order) for an attribute value. `<`/`>` too: an unescaped srcdoc would contain
/// the component's own `</body>`, which the outer document is searched for.
fn attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Replaces every slot in `html` with its main variant's current html (recursively, up to 3 levels), and injects
/// [`SIZER`] before the last `</body>` (or appends it). `wrap` can adjust each component document (e.g. freeze
/// animations in thumbnails). html without slots is returned unchanged.
pub fn compose_live(html: &str, snap: &Snapshot, wrap: Option<&dyn Fn(&str) -> String>) -> String {
    if !has_slots(html) {
        return html.to_string();
    }
    compose(html, &Lineage::new(snap), wrap, 0)
}

fn compose(html: &str, lin: &Lineage, wrap: Option<&dyn Fn(&str) -> String>, depth: usize) -> String {
    if !has_slots(html) {
        return html.to_string();
    }
    let out = SLOT_RE.replace_all(html, |c: &regex::Captures| {
        let attrs = c.get(1).map_or("", |m| m.as_str());
        let target = ID_RE.captures(attrs).and_then(|m| lin.slot_html(&m[1]));
        let fixed = HEIGHT_RE.captures(attrs).map(|m| m[1].to_string());
        let frame = match target {
            Some((d, inner)) if !inner.is_empty() => {
                let mut inner = if depth < MAX_DEPTH { compose(inner, lin, wrap, depth + 1) } else { inner.to_string() };
                if let Some(w) = wrap.filter(|_| !inner.is_empty()) {
                    inner = w(&inner);
                }
                if inner.is_empty() {
                    String::new()
                } else {
                    let style = fixed.map(|h| format!(r#" style="height:{h}px""#)).unwrap_or_default();
                    format!(r#"<iframe title="{}" srcdoc="{}"{style}></iframe>"#, attr(&d.title), attr(&inner))
                }
            }
            _ => String::new(),
        };
        let (cls, rest) = match CLASS_RE.captures(attrs) {
            Some(m) => (format!(" {}", &m[1]), attrs.replacen(&m[0], "", 1)),
            None => (String::new(), attrs.to_string()),
        };
        format!(r#"<div class="pt-slot{cls}"{rest}>{frame}</div>"#)
    });
    // ASCII lowercasing keeps byte offsets, so the index applies to `out`.
    match out.to_ascii_lowercase().rfind("</body>") {
        Some(i) => format!("{}{SIZER}{}", &out[..i], &out[i..]),
        None => format!("{out}{SIZER}"),
    }
}

/// Fingerprint of everything a composed live view depends on: the html, then per slot (recursively, up to 3
/// levels) its main variant's html or "none". Key snapshot caches on this, not only the card's rev, or a live
/// view goes stale when a component changes. html without slots hashes just itself.
pub fn fingerprint(html: &str, snap: &Snapshot) -> u64 {
    let mut h = DefaultHasher::new();
    html.hash(&mut h);
    if has_slots(html) {
        hash_parts(html, &Lineage::new(snap), 0, &mut h);
    }
    h.finish()
}

fn hash_parts(html: &str, lin: &Lineage, depth: usize, h: &mut DefaultHasher) {
    for id in slot_ids(html) {
        match lin.slot_html(&id) {
            Some((_, inner)) => {
                1u8.hash(h);
                inner.hash(h);
                if !inner.is_empty() && depth < MAX_DEPTH && has_slots(inner) {
                    hash_parts(inner, lin, depth + 1, h);
                }
            }
            None => 0u8.hash(h),
        }
    }
}

/// [`compose_live`] with memoisation per card: reuses the previous output while the card's [`fingerprint`] is
/// unchanged (the store changes on every drag; composing re-escapes every component). Bounded; the oldest entry is
/// evicted first. Not thread-safe by itself: keep one per thread or behind a mutex.
pub struct ComposeCache {
    cap: usize,
    entries: HashMap<String, (u64, Arc<str>)>,
    order: VecDeque<String>,
}

impl ComposeCache {
    /// `cap` entries at most (the reference uses 200).
    pub fn new(cap: usize) -> ComposeCache {
        ComposeCache { cap: cap.max(1), entries: HashMap::new(), order: VecDeque::new() }
    }

    pub fn compose(&mut self, card_id: &str, html: &str, snap: &Snapshot, wrap: Option<&dyn Fn(&str) -> String>) -> Arc<str> {
        if !has_slots(html) {
            return Arc::from(html);
        }
        let fp = fingerprint(html, snap);
        if let Some((prev, out)) = self.entries.get(card_id)
            && *prev == fp
        {
            return out.clone();
        }
        let out: Arc<str> = Arc::from(compose_live(html, snap, wrap));
        if self.entries.insert(card_id.to_string(), (fp, out.clone())).is_none() {
            self.order.push_back(card_id.to_string());
            while self.order.len() > self.cap {
                if let Some(old) = self.order.pop_front() {
                    self.entries.remove(&old);
                }
            }
        }
        out
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Starting html for a new live view: the components stacked full width, in the given order.
pub fn default_live_html(ids: &[&str], viewport: u32) -> String {
    let slots: Vec<String> = ids.iter().map(|id| format!(r#"<prototypie-slot data-design="{id}"></prototypie-slot>"#)).collect();
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width={viewport}\"><style>html,body{{margin:0;background:#fff}}main{{display:flex;flex-direction:column}}</style></head><body><main>\n{}\n</main></body></html>",
        slots.join("\n")
    )
}
