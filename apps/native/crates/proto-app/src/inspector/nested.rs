//! The space part of the inspector (the reference's `features/nested/inspector.tsx`): inside an
//! exploration its starred designs and promote actions, then the spaces nested directly in the active one
//! (projects on the main canvas, with folder and pod link; explorations elsewhere, grouped by
//! project folder when they span several).

use std::collections::HashSet;
use std::sync::Arc;

use proto_core::{Design, RecordKind, Space, SpaceKind};
use rune::{Cx, El, Tx, col, el, row, text};

use super::data::{self, folder_name, tilde_path};
use super::icons::{self, ARROW_RIGHT, ARROW_UP_FROM_LINE, DATABASE, FOLDER_GIT_2, INFO, LAYERS, STAR};
use super::style::{self, Ink};
use crate::app::state::AppState;

/// A project's record on the Solid pod: where its document lives and its subject.
#[derive(Clone, Debug, PartialEq)]
pub struct PodRecord {
    pub doc: String,
    pub subject: String,
}

/// The pod record for a project folder (proto-server's `/api/projects`).
fn pod_for(s: &AppState, path: &str) -> Option<PodRecord> {
    super::pods::pod_for(s, path)
}

pub fn view(s: &AppState, cx: &mut Cx<AppState>, design: Option<&Design>, k: &Ink) -> Vec<El> {
    let mut out = exploration(s, cx, design, k).unwrap_or_default();
    out.extend(child_spaces(s, k).unwrap_or_default());
    out
}

fn exploration(s: &AppState, cx: &mut Cx<AppState>, design: Option<&Design>, k: &Ink) -> Option<Vec<El>> {
    let snap = data::snap(s);
    let space = snap.space(&s.route.space)?;
    let parent_id = space.parent_space_id.clone()?;
    if !proto_core::ops::can_promote_to(Some(&parent_id)) {
        return None;
    }
    let parent = snap.space(&parent_id)?;
    let here: Vec<Arc<Design>> = snap.designs_in(&space.id).collect();
    // Originals here whose promoted copy lives in the parent space.
    let promoted: HashSet<String> = snap.live(RecordKind::PromotedFrom)
        .filter(|(copy_id, _)| snap.design(copy_id).is_some_and(|c| c.space_id == parent.id))
        .filter_map(|(copy_id, _)| snap.promoted_from(copy_id).map(|r| r.design_id.clone()))
        .collect();
    let starred: Vec<&Arc<Design>> = here.iter().filter(|d| d.starred == Some(true)).collect();
    let to_promote: Vec<String> = starred.iter()
        .filter(|d| data::is_ready(d) && !promoted.contains(&d.id))
        .map(|d| d.id.clone())
        .collect();
    let selected = design.filter(|d| d.space_id == space.id).map(|d| promote_selected(s, d, &parent.name, k));
    if starred.is_empty() {
        // Nothing starred: no list, only the selected design's promote button.
        return selected.map(|b| vec![style::separator(k), col().key("insp-exploration").w_full().child(b)]);
    }
    let head = row().key("insp-expl-head").w_full().gap(6.0).center_y()
        .child(icons::draw(&STAR, 14.0, k.fg))
        .child(text("Starred").font(style::LABEL).color(k.fg))
        .child(text(starred.len().to_string()).font(style::XS_TNUM).color(k.muted_fg).grow(1.0))
        .child(style::focusable(el(), k).key("insp-expl-info").role("button").label("About explorations")
            .help(format!("Make a mess here. Promote the strands worth keeping up to {}.", parent.name))
            .child(icons::draw(&INFO, 14.0, k.muted_fg)));
    let list: Vec<Arc<Design>> = starred.iter().map(|d| Arc::clone(d)).collect();
    let n = to_promote.len();
    let bulk = button("insp-promote-starred", Variant::Secondary, n == 0, k)
        .on_press(move |s: &mut AppState| {
            crate::features::nested::promote(s, &to_promote);
        })
        .child(icons::draw(&ARROW_UP_FROM_LINE, 14.0, k.fg))
        .child(text("Promote starred").font(style::LABEL).color(k.fg).grow(1.0))
        .children((n > 0).then(|| text(n.to_string()).font(style::XS_TNUM).color(k.fg.alpha(0.7))));
    Some(vec![
        style::separator(k),
        col().key("insp-exploration").w_full().gap(10.0)
            .child(head).child(super::starred::view(s, cx, &list, k)).children(selected).child(bulk),
    ])
}

#[derive(Clone, Copy, PartialEq)]
enum Variant {
    Default,
    Secondary,
    Outline,
}

/// A full-width shadcn button (h 32, `px-2.5` + its 1px border, gap 6, radius 10), content centred
/// unless a child grows; disabled at 50%.
fn button(key: &'static str, v: Variant, disabled: bool, k: &Ink) -> El {
    let b = style::focusable(row(), k).key(key).role("button").w_full().h(32.0).px(11.0).gap(6.0).center_y().justify_center()
        .radius(style::R_LG);
    let b = match v {
        Variant::Default => b.fill(k.primary).hover(k.primary.alpha(0.8)),
        Variant::Secondary => b.fill(k.muted).hover(k.muted.alpha(0.8)),
        Variant::Outline => b.border(1.0, k.input).fill(k.input.alpha(0.3)).hover(k.input.alpha(0.5)),
    };
    if disabled { b.opacity(0.5).disabled(true) } else { b }
}

fn promote_selected(s: &AppState, d: &Design, parent_name: &str, k: &Ink) -> El {
    if data::snap(s).promoted_copy_of(&d.id).is_some() {
        let id = d.id.clone();
        return button("insp-promoted", Variant::Outline, false, k)
            .on_press(move |s: &mut AppState| crate::features::nested::show_promoted_copy(s, &id))
            .child(icons::draw(&ARROW_UP_FROM_LINE, 14.0, k.fg))
            .child(text(format!("Promoted · show in {parent_name}")).font(style::LABEL).color(k.fg).truncate().min_w(0.0).shrink());
    }
    let id = d.id.clone();
    button("insp-promote", Variant::Default, !data::is_ready(d), k)
        .on_press(move |s: &mut AppState| {
            crate::features::nested::promote(s, std::slice::from_ref(&id));
        })
        .child(icons::draw(&ARROW_UP_FROM_LINE, 14.0, k.primary_fg))
        .child(text(format!("Promote “{}”", d.title)).font(style::LABEL).color(k.primary_fg).truncate().min_w(0.0).shrink())
}

/// A child space with what its row shows.
struct Child {
    space: Arc<Space>,
    count: usize,
    path: Option<String>,
}

fn child_spaces(s: &AppState, k: &Ink) -> Option<Vec<El>> {
    let snap = data::snap(s);
    let children: Vec<Child> = snap.child_spaces(&s.route.space).into_iter()
        .map(|sp| Child { count: snap.subtree_designs(&sp.id).len(), path: snap.local_path_of(&sp.id), space: sp })
        .collect();
    if children.is_empty() {
        return None;
    }
    let at_root = s.route.is_root();
    // One group per project folder, in order of first appearance; then the spaces without one.
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    let mut rest = Vec::new();
    for (i, c) in children.iter().enumerate() {
        match &c.path {
            Some(p) => match groups.iter_mut().find(|(g, _)| g.eq_ignore_ascii_case(p)) {
                Some((_, rows)) => rows.push(i),
                None => groups.push((p.clone(), vec![i])),
            },
            None => rest.push(i),
        }
    }
    let grouped = !at_root && groups.len() > 1;
    let mut list = col().key("insp-children").w_full().gap(6.0)
        .child(text(if at_root { "Projects" } else { "Explorations here" }).font(style::LABEL).color(k.fg));
    if grouped {
        for (gi, (path, rows)) in groups.iter().enumerate() {
            let name = folder_name(path).to_string();
            let head = row().key(format!("insp-group-{gi}")).w_full().gap(6.0).px(8.0).pt(4.0).center_y()
                .child(icons::draw(&FOLDER_GIT_2, 12.0, k.muted_fg))
                .child(text(name.clone()).font(style::GROUP).color(k.muted_fg))
                .child(text(tilde_path(path)).font(style::MONO).color(k.muted_fg).truncate().grow(1.0).min_w(0.0).help(path.clone()))
                .children(pod_for(s, path).map(|pod| pod_link(&format!("insp-group-pod-{gi}"), &pod, &name, k)));
            list = list.child(col().w_full().gap(2.0).child(head).children(rows.iter().map(|&i| child_row(s, &children[i], false, k))));
        }
        if !rest.is_empty() {
            list = list.child(col().w_full().gap(2.0)
                .child(text("Other").font(style::GROUP).color(k.muted_fg).px(8.0).pt(4.0))
                .children(rest.iter().map(|&i| child_row(s, &children[i], false, k))));
        }
    } else {
        list = list.children(children.iter().map(|c| child_row(s, c, at_root, k)));
    }
    Some(vec![style::separator(k), list])
}

fn child_row(s: &AppState, c: &Child, project: bool, k: &Ink) -> El {
    let id = c.space.id.clone();
    let is_project = project || c.space.kind == Some(SpaceKind::Project) && s.route.is_root();
    let mut names = col().min_w(0.0).grow(1.0).child(text(c.space.name.clone()).font(style::XS).color(k.fg).truncate());
    if is_project {
        if let Some(p) = &c.path {
            names = names.child(text(tilde_path(p)).font(style::MONO).color(k.muted_fg).truncate().help(p.clone()));
        }
    }
    let button = style::focusable(row(), k).key(format!("insp-space-{}", c.space.id)).role("button").label(c.space.name.clone())
        .grow(1.0).min_w(0.0).px(8.0).py(6.0).gap(8.0).center_y()
        .hover_group()
        .on_press(move |s: &mut AppState| s.enter_space(&id, None))
        .child(icons::draw(if is_project { &FOLDER_GIT_2 } else { &LAYERS }, 14.0, k.muted_fg))
        .child(names)
        .child(text(c.count.to_string()).font(style::XS_TNUM).color(k.muted_fg))
        .child(icons::draw(&ARROW_RIGHT, 12.0, k.fg).group_opacity(0.0, 1.0));
    let pod = if is_project { c.path.as_deref().and_then(|p| pod_for(s, p)) } else { None };
    row().key(format!("insp-space-row-{}", c.space.id)).w_full().radius(style::R_MD).center_y().hover(k.muted)
        .child(button)
        .children(pod.map(|pod| pod_link(&format!("insp-pod-{}", c.space.id), &pod, &c.space.name, k).mr(6.0)))
}

fn pod_link(key: &str, pod: &PodRecord, name: &str, k: &Ink) -> El {
    let doc = pod.doc.clone();
    style::focusable(el(), k).key(key.to_string()).role("link").label(format!("Open {name} on the Solid pod"))
        .help(format!("{} in {}", pod.subject, pod.doc))
        .padding(2.0).radius(4.0).hover(k.muted)
        .on_press_tx(move |_s: &mut AppState, tx: &mut Tx| {
            let _ = tx.open_url(&doc);
        })
        .child(icons::draw(&DATABASE, 12.0, k.muted_fg))
}
