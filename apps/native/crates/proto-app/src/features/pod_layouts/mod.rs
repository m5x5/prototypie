//! "+ → Layout finden…" (the reference's `pod-layouts`): pick two shapes from the pod catalog and one documented
//! relation; four layouts are previewed from the catalog's real entries; "Übernehmen" makes a card (in a new
//! exploration, or here) with the provenance in its rationale. German UI, as the reference.

pub mod model;
pub mod scene;

use proto_core::{Change, RecordKind, ops};
use rune::{Color, El, TextEdit, TextStyle, Tx, col, el, row, text, text_input};
use serde_json::json;

use crate::app::state::AppState;
use crate::app::style::{self, Palette};
use crate::features::{nav, notify};
use model::{LayoutModel, Relation, Shape};
use scene::{Layout, access_label, unavailable};

#[derive(Default)]
pub struct LayoutFinder {
    pub open: bool,
    pub a: Option<String>,
    pub b: Option<String>,
    pub rel: usize,
    pub layout: Option<Layout>,
    pub as_space: bool,
    pub name: Option<TextEdit>,
}

/// The model from the mention catalog (fetched once per run, shared with @-mentions).
pub fn model(s: &AppState) -> Option<LayoutModel> {
    s.features.mention.catalog.as_ref().map(model::build)
}

pub fn open(s: &mut AppState, _tx: &mut Tx) {
    let here = s.route.space.clone();
    if !crate::features::nested::can_create_in(s, &here) {
        return;
    }
    crate::features::mention::ensure_catalog(s);
    s.features.layouts = LayoutFinder { open: true, as_space: true, ..Default::default() };
}

pub fn close(s: &mut AppState) {
    s.features.layouts.open = false;
}

/// The current choice, with defaults: the first documented relation's shapes, the first drawable layout.
pub struct Choice<'m> {
    pub a: &'m Shape,
    pub b: &'m Shape,
    pub rels: Vec<&'m Relation>,
    pub rel: Option<&'m Relation>,
    pub layout: Layout,
}

pub fn choice<'m>(f: &LayoutFinder, m: &'m LayoutModel) -> Option<Choice<'m>> {
    let first = m.relations.first();
    let a_id = f.a.clone().or_else(|| first.map(|r| r.a.clone())).or_else(|| m.shapes.first().map(|s| s.id.clone()))?;
    let b_id = f.b.clone().or_else(|| first.map(|r| r.b.clone())).or_else(|| m.shapes.get(1).map(|s| s.id.clone()))?;
    let (a, b) = (m.shape(&a_id)?, m.shape(&b_id)?);
    let rels = m.relations_of(&a.id, &b.id);
    let rel = rels.get(f.rel.min(rels.len().saturating_sub(1))).copied();
    let wanted = f.layout.unwrap_or(Layout::Lanes);
    let layout = if unavailable(wanted, a, b, rel).is_some() { Layout::ALL.into_iter().find(|l| unavailable(*l, a, b, rel).is_none()).unwrap_or(wanted) } else { wanted };
    Some(Choice { a, b, rels, rel, layout })
}

fn default_name(c: &Choice) -> String {
    let ok = c.rel.is_some() && unavailable(c.layout, c.a, c.b, c.rel).is_none();
    format!("{} × {}{}", c.a.name, c.b.name, if ok { format!(" · {}", c.layout.name()) } else { String::new() })
}

fn rationale(a: &Shape, b: &Shape, r: &Relation, layout: &str) -> String {
    let n = |s: &Shape| if s.items.len() == 1 { "Eintrag" } else { "Einträge" };
    [
        format!("Layout „{layout}“ aus dem Pod-Katalog (GET /api/pod-catalog)."),
        format!("{}: {} ({}, {} {}).", a.name, a.source, access_label(&a.access), a.items.len(), n(a)),
        format!("{}: {} ({}, {} {}).", b.name, b.source, access_label(&b.access), b.items.len(), n(b)),
        format!("Beziehung {}{} ({}){}.", r.predicate, r.via.as_ref().map(|v| format!(" über {v}")).unwrap_or_default(), access_label(&r.access), r.evidence.as_ref().map(|e| format!(", Beleg: {e}")).unwrap_or_default()),
        "Nur belegte Einträge und Kanten; nichts ergänzt.".into(),
    ]
    .join(" ")
}

/// "Übernehmen": a card with the chosen layout, in a new exploration (and enter it) or right of this space's cards.
pub fn apply(s: &mut AppState) {
    let Some(m) = model(s) else { return };
    let f = &s.features.layouts;
    let Some(c) = choice(f, &m) else { return };
    let Some(r) = c.rel.filter(|_| unavailable(c.layout, c.a, c.b, c.rel).is_none()) else { return };
    let name = f.name.as_ref().map(|e| e.text.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| default_name(&c));
    let as_space = f.as_space;
    let here = s.route.space.clone();
    let id = ops::new_id(10);
    let mut changes = Vec::new();
    let (space, pos) = if as_space {
        let created = ops::create_exploration(s.snap(), &here, Some(&name), None);
        changes.extend(created.changes);
        (created.space_id, proto_core::Point { x: 0.0, y: 0.0 })
    } else {
        let last = s.snap().designs_in(&here).max_by(|a, b| a.position.x.total_cmp(&b.position.x));
        (here.clone(), last.map_or(proto_core::Point { x: 0.0, y: 0.0 }, |d| proto_core::Point { x: d.position.x + crate::features::launch::NODE_W + 80.0, y: d.position.y }))
    };
    let card = json!({
        "id": id, "spaceId": space, "kind": "page", "title": name,
        "rationale": rationale(c.a, c.b, r, c.layout.name()),
        "html": scene::card_html(&name, c.layout, c.a, c.b, r), "status": "ready",
        "instruction": format!("Layout-Finder: {} × {} · {} · {}", c.a.name, c.b.name, r.predicate, c.layout.name()),
        "position": {"x": pos.x, "y": pos.y}, "createdAt": ops::now_ms(),
    });
    changes.push(Change::set(RecordKind::Designs.path(&id), card));
    if !nav::apply(s, changes, "layout finder") {
        notify(s, "Konnte nicht angelegt werden", "");
        return;
    }
    s.features.layouts.open = false;
    if as_space {
        nav::enter_space(s, &space, Some(&id));
        notify(s, &format!("Space „{name}“ angelegt"), "");
    } else {
        s.route.selected = vec![id];
        notify(s, &format!("Karte „{name}“ angelegt"), "");
    }
}

const TITLE: TextStyle = TextStyle::new(16.0, 600.0);
const LABEL: TextStyle = TextStyle::new(12.5, 600.0);
const SMALL: TextStyle = TextStyle::new(11.5, 400.0);
const BODY: TextStyle = TextStyle::new(13.0, 400.0);

fn tag(access: &str, p: &Palette) -> El {
    let c = rune::color::hex(match access {
        "live" => "#10b981",
        "cache" => "#f59e0b",
        "ref" => "#0ea5e9",
        _ => "#ef4444",
    })
    .unwrap_or(p.muted_fg);
    row().h(16.0).px(6.0).radius(4.0).center().fill(c.alpha(0.14)).child(text(access_label(access)).font(SMALL).color(c))
}

pub fn view(s: &AppState, p: &Palette) -> Option<El> {
    let f = &s.features.layouts;
    if !f.open {
        return None;
    }
    let mut body = col().w_full().gap(14.0);
    match model(s) {
        None => body = body.child(text(s.features.mention.catalog_error.clone().unwrap_or_else(|| "Lädt den Pod-Katalog …".into())).font(BODY).color(p.muted_fg)),
        Some(m) => {
            if let Some(c) = choice(f, &m) {
                body = body.child(shape_chips(&m, &c, true, p)).child(shape_chips(&m, &c, false, p));
                if c.rels.len() > 1 {
                    let mut rr = row().gap(6.0).flex_wrap().child(text("Beziehung").font(SMALL).color(p.muted_fg));
                    for (i, r) in c.rels.iter().enumerate() {
                        let on = c.rel.is_some_and(|x| std::ptr::eq(x, *r));
                        rr = rr.child(row().key(format!("lf-rel-{i}")).role("radio").label(r.predicate.clone()).checked(on).h(26.0).px(8.0).radius(6.0).center()
                            .border(1.0, if on { p.fg } else { p.border })
                            .on_press(move |s: &mut AppState| s.features.layouts.rel = i)
                            .child(text(r.predicate.clone()).font(SMALL).color(p.fg)));
                    }
                    body = body.child(rr);
                }
                if c.rel.is_none() {
                    let partners: Vec<String> = m.partners(&c.a.id).iter().map(|s| s.name.clone()).collect();
                    body = body.child(text(if partners.is_empty() { "Keine belegte Beziehung zwischen diesen beiden.".to_string() } else { format!("Keine belegte Beziehung. Mit {} gibt es eine: {}", c.a.name, partners.join(", ")) }).font(SMALL).color(p.muted_fg).wrap());
                }
                let mut tiles = row().w_full().gap(10.0);
                for l in Layout::ALL {
                    let why = unavailable(l, c.a, c.b, c.rel);
                    let on = l == c.layout && why.is_none();
                    let prims = scene::scene(l, c.a, c.b, c.rel, 200.0, 130.0, 1.0);
                    let mut tile = col().key(format!("lf-{l:?}")).role("radio").label(l.name()).checked(on).grow(1.0).min_w(0.0).gap(6.0).padding(6.0).radius(10.0)
                        .border(if on { 2.0 } else { 1.0 }, if on { p.fg } else { p.border }).opacity(if why.is_some() { 0.5 } else { 1.0 })
                        .child(el().w_full().h(130.0).radius(6.0).fill(p.hover).clip_soft(0.0)
                            .child(scene::rune(&prims, 200.0, 130.0, p.fg, p.muted_fg, p.border, p.canvas_bg)))
                        .child(text(l.name()).font(LABEL).color(p.fg).truncate())
                        .children(why.map(|w| text(w).font(SMALL).color(p.muted_fg)));
                    if why.is_none() {
                        tile = tile.on_press(move |s: &mut AppState| s.features.layouts.layout = Some(l));
                    }
                    tiles = tiles.child(tile);
                }
                body = body.child(tiles);
                let default = default_name(&c);
                let edit = f.name.clone().unwrap_or_else(|| TextEdit::new(default.clone()));
                let ok = c.rel.is_some() && unavailable(c.layout, c.a, c.b, c.rel).is_none();
                let seg = |key: &'static str, label: &'static str, on: bool, v: bool| row().key(key).role("radio").label(label).checked(on).h(28.0).px(10.0).radius(6.0).center()
                    .fill(if on { p.canvas_bg } else { Color::TRANSPARENT }).on_press(move |s: &mut AppState| s.features.layouts.as_space = v)
                    .child(text(label).font(SMALL).color(if on { p.fg } else { p.muted_fg }));
                let name_box = row().key("lf-name-box").grow(1.0).min_w(0.0).h(34.0).px(10.0).radius(8.0).border(1.0, p.border).center_y()
                    .child(text_input(&edit, BODY).color(p.fg).key("lf-name").label("Name").single_line().w_full()
                        .on_edit(move |s: &mut AppState| s.features.layouts.name.get_or_insert_with(|| TextEdit::new(default.clone()))));
                let go = row().key("lf-apply").role("button").label("Übernehmen").disabled(!ok).h(34.0).px(14.0).radius(8.0).center().fill(p.fg).opacity(if ok { 1.0 } else { 0.5 })
                    .child(text("Übernehmen").font(LABEL).color(p.canvas_bg));
                body = body.child(row().w_full().gap(10.0).center_y().child(name_box)
                    .child(row().gap(2.0).padding(2.0).radius(8.0).fill(p.hover).child(seg("lf-space", "Neuer Space", f.as_space, true)).child(seg("lf-here", "Karte hier", !f.as_space, false)))
                    .child(if ok { go.press(Color::BLACK.alpha(0.15)).on_press(apply) } else { go }));
            }
        }
    }
    let card = col().key("layout-finder").role("dialog").label("Layout finden").w(920.0).gap(14.0).padding(20.0).radius(16.0)
        .child(row().w_full().justify_between().center_y().child(text("Layout finden").font(TITLE).color(p.fg))
            .child(row().key("lf-close").role("button").label("Schließen").size(28.0, 28.0).radius(6.0).center().hover(p.hover).on_press(close).child(crate::features::icons::x(14.0, p.fg))))
        .child(body);
    Some(el().key("lf-modal").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center().fill(Color::BLACK.alpha(0.45))
        .modal(true, close).child(style::glass(card, p, 0.96)))
}

fn shape_chips(m: &LayoutModel, c: &Choice, side_a: bool, p: &Palette) -> El {
    let (cur, other) = if side_a { (&c.a.id, &c.b.id) } else { (&c.b.id, &c.a.id) };
    let mut r = row().w_full().gap(6.0).flex_wrap().center_y().child(text(if side_a { "A" } else { "B" }).font(LABEL).color(p.muted_fg).w(16.0));
    for s in &m.shapes {
        let off = &s.id == other || s.items.is_empty();
        let on = &s.id == cur;
        let id = s.id.clone();
        let mut chip = row().key(format!("lf-{}-{}", if side_a { "a" } else { "b" }, s.id)).role("radio").label(s.name.clone()).checked(on).h(30.0).px(10.0).gap(6.0).radius(8.0).center_y()
            .border(if on { 2.0 } else { 1.0 }, if on { p.fg } else { p.border }).opacity(if off { 0.5 } else { 1.0 })
            .child(el().size(8.0, 8.0).radius(4.0).fill(rune::color::hex(&s.color).unwrap_or(p.muted_fg)))
            .child(text(s.name.clone()).font(LABEL).color(p.fg))
            .child(tag(&s.access, p));
        if !off {
            chip = chip.on_press(move |s: &mut AppState| {
                let f = &mut s.features.layouts;
                if side_a { f.a = Some(id.clone()) } else { f.b = Some(id.clone()) }
                f.rel = 0;
                f.name = None;
            });
        }
        r = r.child(chip);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::mention::Catalog;

    fn catalog() -> Catalog {
        serde_json::from_value(json!({"pods": [{"id": "home", "label": "Home", "display": "my-pod"}], "entries": [
            {"id": "sh-task", "kind": "shape", "pod": "home", "label": "ical:Vtodo", "path": "tasks", "access": "ref", "children": ["t1", "t2"]},
            {"id": "t1", "kind": "resource", "pod": "home", "label": "Essay", "path": "tasks/t1.ttl", "access": "401",
             "fields": [{"p": "ical:due", "v": "2026-11-20T10:00 (Europe/Berlin)"}], "relations": [{"p": "schema:about", "target": "c1"}]},
            {"id": "t2", "kind": "resource", "pod": "home", "label": "Quiz", "path": "tasks/t2.ttl", "access": "live",
             "fields": [{"p": "ical:due", "v": "2026-11-21T09:00 (Europe/Berlin)"}], "relations": [{"p": "schema:about", "target": "c1"}]},
            {"id": "c1", "kind": "resource", "pod": "home", "label": "Biology", "path": "courses/c1.ttl", "access": "live"}
        ]})).unwrap()
    }

    #[test]
    fn model_groups_and_relates() {
        let m = model::build(&catalog());
        let tasks = m.shape("sh-task").unwrap();
        assert_eq!((tasks.name.as_str(), tasks.items.len()), ("Aufgaben", 2));
        assert!(tasks.is_timed());
        let course = m.shape("c1").unwrap();
        assert_eq!(course.name, "Kurse");
        let r = &m.relations_of("sh-task", "c1")[0];
        assert_eq!((r.predicate.as_str(), r.access.as_str(), r.pairs.len()), ("schema:about", "401", 2));
    }

    #[test]
    fn layouts_place_only_related_items() {
        let m = model::build(&catalog());
        let f = LayoutFinder { open: true, ..Default::default() };
        let c = choice(&f, &m).unwrap();
        assert_eq!(c.layout, Layout::Lanes);
        let lanes = scene::scene(Layout::Lanes, c.a, c.b, c.rel, 1120.0, 520.0, 2.6);
        assert_eq!(lanes.iter().filter(|p| matches!(p, scene::Prim::Item { .. })).count(), 2);
        let tl = scene::scene(Layout::Timeline, c.a, c.b, c.rel, 1120.0, 520.0, 2.6);
        assert!(tl.iter().any(|p| matches!(p, scene::Prim::Axis { .. })));
        let html = scene::card_html("Aufgaben × Kurse", Layout::Lanes, c.a, c.b, c.rel.unwrap());
        assert!(html.contains("class=\"it\"") && html.contains("Beziehung"));
    }
}
