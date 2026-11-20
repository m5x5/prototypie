//! The multi-select bar (the reference's `multi/selection-bar.tsx`): with 2+ designs selected, a slim pill at
//! the bottom of the canvas (count + clear, View, Live, a one-line prompt, send) that expands into the full
//! composer (Revise each / Combine, prompt, count, send). Plus `generate_from_sources` (Combine and drag-to-mix),
//! `create_live_view` and Alt+D delete.

use std::sync::Arc;

use proto_core::{Change, Design, DesignKind, Point, RecordKind, VariationRequest, ops};
use rune::{Color, El, TextEdit, TextStyle, col, el, row, text, text_input};
use serde_json::json;

use crate::app::insets::INSPECTOR_OVERLAY;
use crate::app::state::AppState;
use crate::app::style::{self, Palette};
use crate::features::launch::{self, Launch, NODE_H, NODE_W};
use crate::features::{icons, nav};

const MAX_REVISIONS: u32 = 3;
const MAX_COMBINATIONS: u32 = 4;
const SIBLING_GAP: f64 = 160.0;
const BODY: TextStyle = TextStyle::new(13.5, 400.0);
const LABEL: TextStyle = TextStyle::new(12.5, 500.0);
const SMALL: TextStyle = TextStyle::new(11.5, 400.0);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Mode {
    /// Revise every selected design on its own.
    #[default]
    Each,
    /// Merge all of them into new designs.
    Combine,
}

#[derive(Default)]
pub struct MultiState {
    pub open: bool,
    pub mode: Mode,
    /// Revisions per design / combinations (clamped per mode).
    pub count: u32,
    pub message: TextEdit,
}

/// Selected designs that exist, in canvas order (rows by y within 160, then x).
pub fn selected(s: &AppState) -> Vec<Arc<Design>> {
    let mut v: Vec<Arc<Design>> = s.route.selected.iter().filter_map(|id| s.snap().design(id)).collect();
    v.sort_by(|a, b| {
        if (a.position.y - b.position.y).abs() < 160.0 { a.position.x.total_cmp(&b.position.x) } else { a.position.y.total_cmp(&b.position.y) }
    });
    v
}

fn agent(s: &AppState) -> String {
    match s.prefs.agent_choice {
        crate::app::prefs::AgentChoice::Claude => "claude".into(),
        crate::app::prefs::AgentChoice::Codex => "codex".into(),
    }
}

fn count(s: &AppState) -> u32 {
    let max = if s.features.multi.mode == Mode::Combine { MAX_COMBINATIONS } else { MAX_REVISIONS };
    s.features.multi.count.clamp(1, max)
}

/// One ready source → variations of it; two or more → ONE request with all of them as `bases`. Results land
/// in a row at `origin` (default: centred below the sources). False when nothing is ready.
pub fn generate_from_sources(s: &mut AppState, source_ids: &[String], instruction: &str, count: u32, origin: Option<Point>) -> bool {
    let ready: Vec<Arc<Design>> = source_ids.iter().filter_map(|id| s.snap().design(id)).filter(|d| d.is_ready()).collect();
    let Some(first) = ready.first().cloned() else { return false };
    let count = count.max(1);
    let text = instruction.trim();
    let width = count as f64 * NODE_W + (count as f64 - 1.0) * SIBLING_GAP;
    let origin = origin.unwrap_or_else(|| {
        let centre = ready.iter().map(|d| d.position.x).sum::<f64>() / ready.len() as f64 + NODE_W / 2.0;
        let bottom = ready.iter().map(|d| d.position.y).fold(f64::MIN, f64::max);
        Point { x: centre - width / 2.0, y: bottom + NODE_H + 220.0 }
    });
    let agent = agent(s);
    let mut request = VariationRequest { count, agent: Some(agent), space_id: Some(first.space_id.clone()), ..Default::default() };
    let label = if ready.len() == 1 {
        request.instruction = if text.is_empty() { "Explore a meaningfully different direction.".into() } else { text.into() };
        request.base_html = Some(first.html.clone());
        request.base_title = Some(first.title.clone());
        format!("{}: {}", if text.is_empty() { "Variation".to_string() } else { text.chars().take(40).collect() }, first.title)
    } else {
        request.instruction = if text.is_empty() { "Combine them into one design.".into() } else { text.into() };
        request.extra.insert("bases".into(), json!(ready.iter().map(|d| json!({"id": d.id, "title": d.title, "html": d.html})).collect::<Vec<_>>()));
        let titles = ready.iter().map(|d| d.title.as_str()).collect::<Vec<_>>().join(" + ");
        request.base_title = Some(titles.clone());
        format!("Mix: {titles}").chars().take(60).collect()
    };
    launch::launch(s, Launch {
        space_id: first.space_id.clone(),
        parent_id: Some(first.id.clone()),
        kind: DesignKind::Variation,
        origin: Some(origin),
        label,
        request,
        lane: None,
    });
    true
}

/// Send from the bar: Revise each needs a prompt; Combine needs 2+ ready designs.
pub fn send(s: &mut AppState) {
    let text = s.features.multi.message.text.trim().to_string();
    let n = count(s);
    let sel = selected(s);
    match s.features.multi.mode {
        Mode::Combine => {
            let ready: Vec<String> = sel.iter().filter(|d| d.is_ready()).map(|d| d.id.clone()).collect();
            if ready.len() < 2 {
                return;
            }
            generate_from_sources(s, &ready, &text, n, None);
        }
        Mode::Each => {
            if text.is_empty() {
                return;
            }
            let agent = agent(s);
            for d in sel.iter().filter(|d| d.is_ready()) {
                launch::launch(s, Launch {
                    space_id: d.space_id.clone(),
                    parent_id: Some(d.id.clone()),
                    kind: DesignKind::Variation,
                    origin: None,
                    label: format!("{}: {}", text.chars().take(40).collect::<String>(), d.title),
                    request: VariationRequest {
                        instruction: text.clone(),
                        count: n,
                        base_html: Some(d.html.clone()),
                        base_title: Some(d.title.clone()),
                        agent: Some(agent.clone()),
                        space_id: Some(d.space_id.clone()),
                        ..Default::default()
                    },
                    lane: None,
                });
            }
        }
    }
    s.prefs_mut().remember_prompt(&text);
    let m = &mut s.features.multi;
    m.message = TextEdit::new("");
    m.open = false;
}

/// "Live": a new card whose slots reference the selected cards (stacked), placed centred below them.
pub fn create_live_view(s: &mut AppState, ids: &[String]) -> Option<String> {
    let ready: Vec<Arc<Design>> = ids.iter().filter_map(|id| s.snap().design(id)).filter(|d| d.is_ready()).collect();
    let first = ready.first()?.clone();
    let viewport = ready.iter().map(|d| d.viewport.unwrap_or(1280.0)).fold(0.0, f64::max);
    let x = ready.iter().map(|d| d.position.x).sum::<f64>() / ready.len() as f64;
    let y = ready.iter().map(|d| d.position.y).fold(f64::MIN, f64::max) + NODE_H + 220.0;
    let id = ops::new_id(10);
    let refs: Vec<&str> = ready.iter().map(|d| d.id.as_str()).collect();
    let titles: Vec<&str> = ready.iter().map(|d| d.title.as_str()).collect();
    let v = json!({
        "id": id, "spaceId": first.space_id, "parentId": first.id, "kind": "page", "title": "Live view",
        "rationale": format!("Shows the current main variant of: {}.", titles.join(", ")),
        "html": proto_core::live_view::default_live_html(&refs, viewport as u32), "status": "ready",
        "position": {"x": x, "y": y}, "viewport": viewport, "createdAt": ops::now_ms(),
        "request": {"instruction": "Live view", "count": 1, "baseTitle": titles.join(" + "),
                    "bases": ready.iter().map(|d| json!({"id": d.id, "title": d.title, "html": ""})).collect::<Vec<_>>()},
    });
    nav::apply(s, vec![Change::set(RecordKind::Designs.path(&id), v)], "live view").then_some(id)
}

/// Alt+D: delete the selected designs.
pub fn delete_selected(s: &mut AppState) -> bool {
    let ids: Vec<String> = s.route.selected.iter().filter(|id| s.snap().design(id).is_some()).cloned().collect();
    if ids.is_empty() {
        return false;
    }
    let changes = ids.iter().map(|id| Change::delete(RecordKind::Designs.path(id))).collect();
    nav::apply(s, changes, "delete selected");
    s.route.selected.clear();
    true
}

pub fn view(s: &AppState, p: &Palette, window: [f32; 2]) -> Option<El> {
    let sel = selected(s);
    let n = sel.len();
    if n < 2 {
        return None;
    }
    let m = &s.features.multi;
    let combine = m.mode == Mode::Combine;
    let k = count(s);
    let ready = sel.iter().filter(|d| d.is_ready()).count();
    let can_send = if combine { ready >= 2 } else { !m.message.text.trim().is_empty() };
    let send_label = if combine { format!("Combine {n} into {k} design{}", if k == 1 { "" } else { "s" }) } else if k > 1 { format!("Revise {n} designs ×{k}") } else { format!("Revise {n} designs") };
    let overlay = if s.canvas.inspector_hidden { 0.0 } else { INSPECTOR_OVERLAY };
    let w = (window[0] - overlay - 32.0).clamp(280.0, 560.0);

    let chip = row().key("multi-chip").h(30.0).pl(4.0).pr(4.0).gap(6.0).radius(15.0).center_y().fill(p.hover)
        .child(row().size(22.0, 22.0).radius(11.0).center().fill(p.fg).child(text(n.to_string()).font(SMALL).color(p.canvas_bg)))
        .child(text("selected").font(LABEL).color(p.fg))
        .child(row().key("multi-clear").role("button").label("Clear selection").size(20.0, 20.0).radius(10.0).center()
            .hover(p.hover).on_press(|s: &mut AppState| s.route.selected.clear()).child(icons::x(11.0, p.muted_fg)));
    let ids: Vec<String> = sel.iter().filter(|d| d.is_ready()).map(|d| d.id.clone()).collect();
    let pill_btn = |key: &'static str, label: &'static str| row().key(key).role("button").label(label).h(30.0).px(8.0).radius(15.0).center_y()
        .hover(p.hover).press(p.press).child(text(label).font(LABEL).color(p.muted_fg));
    let mut pill = row().key("multi-pill").w_full().h(44.0).pl(8.0).pr(6.0).gap(6.0).center_y().child(chip);
    if ready > 1 {
        let (view_ids, live_ids) = (ids.clone(), ids.clone());
        pill = pill.child(pill_btn("multi-view", "View").on_press(move |s: &mut AppState| {
            if let Some(first) = view_ids.first() {
                crate::fullscreen::open(s, first, &view_ids);
            }
        }));
        if !m.open {
            pill = pill.child(pill_btn("multi-live", "Live").on_press(move |s: &mut AppState| {
                if let Some(id) = create_live_view(s, &live_ids) {
                    let space = s.route.space.clone();
                    nav::enter_space(s, &space, Some(&id));
                }
            }));
        }
    }
    let input = text_input(&m.message, BODY).color(p.fg).key("multi-input").label(if combine { "Combine the selected designs" } else { "Revise selected designs" })
        .placeholder(if combine { format!("Combine all {n}: steer it (optional)") } else { format!("Revise all {n}: what should change?") }, p.muted_fg)
        .w_full()
        .on_edit(|s: &mut AppState| &mut s.features.multi.message)
        .on_input_change(|s: &mut AppState, _| s.features.multi.open = true)
        .on_input_key(|s: &mut AppState, ev, _| {
            if ev.key == rune::Key::Enter && !ev.mods.shift {
                send(s);
                return true;
            }
            if ev.key == rune::Key::Escape && s.features.multi.open {
                s.features.multi.open = false;
                return true;
            }
            false
        });
    pill = pill.child(el().grow(1.0).min_w(0.0).child(input));
    let send_btn = row().key("multi-send").role("button").label(send_label.clone()).disabled(!can_send).size(32.0, 32.0).radius(16.0).center()
        .fill(style::brand()).opacity(if can_send { 1.0 } else { 0.5 }).child(icons::arrow_up(16.0, Color::WHITE));
    pill = pill.child(if can_send { send_btn.press(Color::BLACK.alpha(0.15)).on_press(send) } else { send_btn });

    let mut card = col().key("multi-bar").role("dialog").label("Revise selected designs").w(w).radius(if m.open { 14.0 } else { 22.0 });
    if m.open {
        let seg = |key: &'static str, label: &'static str, mode: Mode| {
            let on = s.features.multi.mode == mode;
            row().key(key).role("radio").label(label).checked(on).grow(1.0).h(28.0).radius(8.0).center()
                .fill(if on { p.canvas_bg } else { Color::TRANSPARENT })
                .on_press(move |s: &mut AppState| s.features.multi.mode = mode)
                .child(text(label).font(LABEL).color(if on { p.fg } else { p.muted_fg }))
        };
        let max = if combine { MAX_COMBINATIONS } else { MAX_REVISIONS };
        let mut counts = row().gap(2.0).padding(2.0).radius(8.0).fill(p.hover);
        for c in 1..=max {
            let on = k == c;
            counts = counts.child(row().key(format!("multi-count-{c}")).role("radio").label(c.to_string()).checked(on).size(26.0, 24.0).radius(6.0).center()
                .fill(if on { p.fg } else { Color::TRANSPARENT })
                .on_press(move |s: &mut AppState| s.features.multi.count = c)
                .child(text(c.to_string()).font(SMALL).color(if on { p.canvas_bg } else { p.muted_fg })));
        }
        card = card.child(col().w_full().gap(10.0).px(14.0).pt(14.0)
            .child(row().w_full().gap(2.0).padding(2.0).radius(10.0).fill(p.hover)
                .child(seg("multi-each", "Revise each", Mode::Each))
                .child(seg("multi-combine", "Combine", Mode::Combine)))
            .child(text(if combine { format!("Combine all {n}: what should the result be?") } else { format!("How should all {n} change?") }).font(LABEL).color(p.fg))
            .child(row().w_full().justify_between().center_y()
                .child(text(if combine { "Combinations" } else { "Revisions per design" }).font(SMALL).color(p.muted_fg))
                .child(counts)));
    }
    card = card.child(pill);
    Some(el().key("multi-anchor").pin(Some(0.0), None, Some(overlay), Some(24.0)).justify_center()
        .child(style::glass(card, p, 0.92).on_press(|s: &mut AppState| s.features.multi.open = true)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composer::Outgoing;

    fn state() -> AppState {
        let mut s = AppState::new();
        s.use_store(crate::features::scenes::fixture());
        s.route.selected = vec!["zzv1".into(), "zzv2".into(), "zzv3".into()];
        s
    }

    #[test]
    fn combine_sends_one_request_with_all_bases_centred_below() {
        let mut s = state();
        s.features.multi.mode = Mode::Combine;
        s.features.multi.count = 2;
        send(&mut s);
        let [Outgoing::Generate { parent_id, request, origin, .. }] = s.composer.outbox.as_slice() else { panic!("one generate") };
        assert_eq!(parent_id.as_deref(), Some("zzv1"));
        assert_eq!(request.extra["bases"].as_array().unwrap().len(), 3);
        assert_eq!(request.instruction, "Combine them into one design.");
        // Sources at x 0/420/840, y 640: centre 420 + 190; two cards are 380·2 + 160 wide.
        let o = origin.unwrap();
        assert_eq!((o.x, o.y), (610.0 - 460.0, 640.0 + NODE_H + 220.0));
    }

    #[test]
    fn revise_each_needs_a_prompt_and_sends_one_per_design() {
        let mut s = state();
        send(&mut s);
        assert!(s.composer.outbox.is_empty());
        s.features.multi.message = TextEdit::new("darker");
        send(&mut s);
        assert_eq!(s.composer.outbox.len(), 3);
    }

    #[test]
    fn alt_d_deletes_the_selection() {
        let mut s = state();
        let mut tx = rune::Tx::default();
        let ev = rune::KeyEvent { key: rune::Key::Text("∂".into()), mods: rune::Mods { alt: true, ..Default::default() } };
        assert!(crate::features::on_key(&mut s, &ev, &mut tx));
        assert!(s.snap().design("zzv1").is_none() && s.snap().design("zzbase").is_some());
        assert!(s.route.selected.is_empty());
    }

    #[test]
    fn live_view_references_the_selection() {
        let mut s = state();
        let sel = s.route.selected.clone();
        let id = create_live_view(&mut s, &sel).unwrap();
        let d = s.snap().design(&id).unwrap();
        assert_eq!(proto_core::live_view::slot_ids(&d.html), ["zzv1", "zzv2", "zzv3"]);
        assert_eq!(d.parent_id.as_deref(), Some("zzv1"));
    }
}
