//! The Layout-Finder's model (the reference's `pod-layouts/model.ts`): groups of resources from the pod catalog
//! (one per shape, plus resources no shape claims) and the relations the catalog documents between them. Only
//! catalog values are used; "←" reverse edges are skipped.

use std::collections::HashMap;

use crate::features::mention::{Catalog, Entry};

pub const COLORS: [&str; 7] = ["#d97757", "#2563eb", "#7c3aed", "#0d9488", "#ca8a04", "#db2777", "#525252"];

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub label: String,
    /// Epoch ms, only for items whose shape has a time field.
    pub start: Option<f64>,
    pub end: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub id: String,
    pub name: String,
    pub type_label: String,
    pub access: String,
    pub color: String,
    pub items: Vec<Item>,
    pub source: String,
}

impl Shape {
    pub fn is_timed(&self) -> bool {
        self.items.iter().any(|x| x.start.is_some())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Relation {
    pub a: String,
    pub b: String,
    pub predicate: String,
    pub access: String,
    /// Set when only implied through a shared target.
    pub via: Option<String>,
    /// Item pairs [a item, b item].
    pub pairs: Vec<(String, String)>,
    pub evidence: Option<String>,
}

impl Relation {
    /// Does it connect item `ai` of shape `a_id` with item `bi` (either stored direction)?
    pub fn links(&self, a_id: &str, ai: &str, bi: &str) -> bool {
        self.pairs.iter().any(|(x, y)| if self.a == a_id { x == ai && y == bi } else { x == bi && y == ai })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutModel {
    pub shapes: Vec<Shape>,
    pub relations: Vec<Relation>,
}

fn rank(access: &str) -> u8 {
    match access {
        "401" => 0,
        "ref" => 1,
        "cache" => 2,
        _ => 3,
    }
}

fn weakest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if rank(a) <= rank(b) { a } else { b }
}

/// "2026-10-21T12:15 (Europe/Berlin)" (wall time, read in the local zone) or an ISO instant → epoch ms.
pub fn parse_time(v: Option<&str>) -> Option<f64> {
    let v = v?.trim();
    if let Some((wall, _zone)) = v.split_once(" (") {
        let iso = if wall.len() == 16 { format!("{wall}:00") } else { wall.to_string() };
        let t = crate::features::time::parse_iso(&format!("{iso}Z"))?;
        // Wall time in the user's zone: subtract its offset at that moment.
        return Some(((t - crate::features::time::utc_offset_s(t)) * 1000) as f64);
    }
    crate::features::time::parse_iso(v).map(|t| (t * 1000) as f64)
}

fn field<'a>(e: &'a Entry, ps: &[&str]) -> Option<&'a str> {
    e.fields.iter().find(|f| ps.contains(&f.p.as_str())).and_then(|f| f.v.as_deref())
}

fn item(e: &Entry) -> Item {
    Item {
        id: e.id.clone(),
        label: e.label.clone(),
        start: parse_time(field(e, &["ical:due", "start", "dtstart", "schema:startDate"])),
        end: parse_time(field(e, &["end", "dtend", "schema:endDate"])),
    }
}

fn group_name(members: &[&Entry], fallback: &str) -> String {
    let containers: Vec<&str> = members.iter().map(|m| m.path.find('/').map_or("", |i| &m.path[..=i])).collect();
    if let Some(first) = containers.first()
        && containers.iter().all(|c| c == first)
    {
        let name = match *first {
            "tasks/" => Some("Aufgaben"),
            "events/" => Some("Termine"),
            "courses/" => Some("Kurse"),
            "profile/" => Some("Profile"),
            _ => None,
        };
        if let Some(n) = name {
            return n.into();
        }
    }
    fallback.into()
}

pub fn build(cat: &Catalog) -> LayoutModel {
    let by_id: HashMap<&str, &Entry> = cat.entries.iter().map(|e| (e.id.as_str(), e)).collect();
    let pod_name = |id: &str| cat.pods.iter().find(|p| p.id == id).map_or(id.to_string(), |p| p.display.clone());
    let mut shapes: Vec<Shape> = Vec::new();
    let mut group_of: HashMap<String, String> = HashMap::new();
    for s in cat.entries.iter().filter(|e| e.kind == "shape") {
        let members: Vec<&Entry> = s.children.iter().filter_map(|id| by_id.get(id.as_str()).copied()).filter(|m| m.kind == "resource").collect();
        for m in &members {
            group_of.insert(m.id.clone(), s.id.clone());
        }
        let items = members
            .iter()
            .map(|m| {
                let mut it = item(m);
                if members.iter().any(|o| o.id != m.id && o.label == m.label) {
                    let pod = cat.pods.iter().find(|p| p.id == m.pod).map_or(m.pod.clone(), |p| p.label.clone());
                    it.label = format!("{} · {pod}", m.label);
                }
                it
            })
            .collect();
        shapes.push(Shape { id: s.id.clone(), name: group_name(&members, &s.label), type_label: s.label.clone(), access: s.access.clone(), color: String::new(), items, source: format!("{} · {}", pod_name(&s.pod), s.path) });
    }
    let unclaimed: Vec<&Entry> = cat.entries.iter().filter(|e| e.kind == "resource" && !group_of.contains_key(&e.id)).collect();
    for r in unclaimed {
        group_of.insert(r.id.clone(), r.id.clone());
        shapes.push(Shape { id: r.id.clone(), name: group_name(&[r], &r.label), type_label: r.kind.clone(), access: r.access.clone(), color: String::new(), items: vec![item(r)], source: format!("{} · {}", pod_name(&r.pod), r.path) });
    }
    shapes.sort_by_key(|s| s.items.is_empty());
    for (i, s) in shapes.iter_mut().enumerate() {
        s.color = COLORS[i % COLORS.len()].into();
    }
    // Direct edges per (group, predicate, group).
    let mut direct: Vec<Relation> = Vec::new();
    for r in cat.entries.iter().filter(|e| e.kind == "resource") {
        for rel in &r.relations {
            if rel.p.starts_with('←') {
                continue;
            }
            let (Some(ga), Some(target)) = (group_of.get(&r.id), by_id.get(rel.target.as_str())) else { continue };
            let Some(gb) = group_of.get(&target.id) else { continue };
            if ga == gb {
                continue;
            }
            match direct.iter_mut().find(|d| &d.a == ga && d.predicate == rel.p && &d.b == gb) {
                Some(d) => {
                    d.pairs.push((r.id.clone(), target.id.clone()));
                    d.access = weakest(&d.access, &r.access).to_string();
                }
                None => direct.push(Relation { a: ga.clone(), b: gb.clone(), predicate: rel.p.clone(), access: r.access.clone(), via: None, pairs: vec![(r.id.clone(), target.id.clone())], evidence: None }),
            }
        }
    }
    let mut relations = direct.clone();
    // Two groups pointing at the same target item are related through it.
    for i in 0..direct.len() {
        for j in i + 1..direct.len() {
            let (x, y) = (&direct[i], &direct[j]);
            if x.b != y.b || x.a == y.a {
                continue;
            }
            let pairs: Vec<(String, String)> = x.pairs.iter().flat_map(|(xa, xt)| y.pairs.iter().filter(move |(_, yt)| yt == xt).map(move |(ya, _)| (xa.clone(), ya.clone()))).collect();
            if pairs.is_empty() {
                continue;
            }
            let via = shapes.iter().find(|s| s.id == x.b).map(|s| if s.items.len() == 1 { s.items[0].label.clone() } else { s.name.clone() });
            relations.push(Relation { a: x.a.clone(), b: y.a.clone(), predicate: format!("{} ↔ {}", x.predicate, y.predicate), access: weakest(&x.access, &y.access).to_string(), via, pairs, evidence: None });
        }
    }
    LayoutModel { shapes, relations }
}

impl LayoutModel {
    pub fn shape(&self, id: &str) -> Option<&Shape> {
        self.shapes.iter().find(|s| s.id == id)
    }

    pub fn relations_of(&self, a: &str, b: &str) -> Vec<&Relation> {
        self.relations.iter().filter(|r| (r.a == a && r.b == b) || (r.a == b && r.b == a)).collect()
    }

    /// Shapes with a documented relation to `a`.
    pub fn partners(&self, a: &str) -> Vec<&Shape> {
        let mut ids: Vec<&str> = self.relations.iter().filter(|r| r.a == a || r.b == a).map(|r| if r.a == a { r.b.as_str() } else { r.a.as_str() }).collect();
        ids.dedup();
        ids.iter().filter_map(|id| self.shape(id)).collect()
    }
}
