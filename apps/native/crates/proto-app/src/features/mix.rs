//! Drag-to-mix (the reference's `multi/mix.tsx`): drag from a card's handle onto empty canvas to start a draft
//! card there (a variation of that card); drop onto another card, or onto a draft, to make it a MIX of several.
//! Sending a draft generates through `multi::generate_from_sources` at the draft's position.
//! The canvas (worker-4) places the drafts as extra nodes and draws the extra edges.

use proto_core::Point;
use rune::{Color, El, TextEdit, TextStyle, col, el, row, text, text_input};

use crate::app::state::AppState;
use crate::app::style::{self, Palette};
use crate::features::icons;
use crate::features::launch::{NODE_H, NODE_W};

pub const MIX_PREFIX: &str = "mix:";
pub const MIX_W: f32 = 340.0;
/// Layout height of a draft card (for fit, minimap, anchors).
pub const MIX_H: f32 = 236.0;
const MAX_COUNT: u32 = 4;
const TITLE: TextStyle = TextStyle::new(13.5, 600.0);
const BODY: TextStyle = TextStyle::new(13.0, 400.0);
const SMALL: TextStyle = TextStyle::new(11.5, 400.0);

/// A card that doesn't exist yet (per device, like the reference's local `mixDrafts`).
#[derive(Clone, Debug)]
pub struct MixDraft {
    pub id: String,
    pub space_id: String,
    pub position: [f32; 2],
    pub sources: Vec<String>,
    pub text: TextEdit,
    pub count: u32,
}

#[derive(Default)]
pub struct MixState {
    pub drafts: Vec<MixDraft>,
}

/// An extra canvas node a feature draws (worker-4 wraps it in its node frame).
pub struct ExtraNode {
    pub id: String,
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub content: El,
}

/// An extra edge (ids are design ids or extra-node ids), drawn bottom → top.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtraEdge {
    pub from: String,
    pub to: String,
    pub dashed: bool,
    pub animated: bool,
}

/// Where a handle drag from a card was released.
#[derive(Clone, Debug, PartialEq)]
pub enum DropTarget {
    /// Empty canvas at this world point.
    Empty([f32; 2]),
    Design(String),
    Extra(String),
}

fn draft_mut<'a>(s: &'a mut AppState, id: &str) -> Option<&'a mut MixDraft> {
    s.features.mix.drafts.iter_mut().find(|d| d.id == id)
}

/// Start a draft at `pos` (top-left) from `sources`; returns its id.
pub fn start(s: &mut AppState, space: &str, pos: [f32; 2], sources: Vec<String>) -> String {
    let id = proto_core::ops::new_id(8);
    let mut uniq: Vec<String> = Vec::new();
    for x in sources {
        if !uniq.contains(&x) {
            uniq.push(x);
        }
    }
    let mut text = TextEdit::new("");
    text.focused = true;
    s.features.mix.drafts.push(MixDraft { id: id.clone(), space_id: space.into(), position: pos, sources: uniq, text, count: 1 });
    id
}

/// A draft between two cards: centred under them.
fn start_between(s: &mut AppState, a: &str, b: &str) {
    let snap = s.store().snapshot();
    let (Some(da), Some(db)) = (snap.design(a), snap.design(b)) else { return };
    let x = (da.position.x + db.position.x) / 2.0 + NODE_W / 2.0 - MIX_W as f64 / 2.0;
    let y = da.position.y.max(db.position.y) + NODE_H + 120.0;
    start(s, &da.space_id, [x as f32, y as f32], vec![a.into(), b.into()]);
}

pub fn add_source(s: &mut AppState, draft: &str, design: &str) {
    if let Some(d) = draft_mut(s, draft)
        && !d.sources.iter().any(|x| x == design)
    {
        d.sources.push(design.into());
    }
}

pub fn remove_source(s: &mut AppState, draft: &str, design: &str) {
    if let Some(d) = draft_mut(s, draft) {
        d.sources.retain(|x| x != design);
    }
}

pub fn close(s: &mut AppState, draft: &str) {
    s.features.mix.drafts.retain(|d| d.id != draft);
}

/// Handle released: on empty canvas start a draft there; on another card start a mix of both; on a draft add
/// the source. Only designs start a drag.
pub fn on_handle_drop(s: &mut AppState, from: &str, drop: DropTarget, _tx: &mut rune::Tx) {
    let Some(src) = s.snap().design(from) else { return };
    match drop {
        DropTarget::Empty(p) => {
            start(s, &src.space_id.clone(), [p[0] - MIX_W / 2.0, p[1] - 16.0], vec![from.into()]);
        }
        DropTarget::Design(id) if id != from && s.snap().design(&id).is_some() => start_between(s, from, &id),
        DropTarget::Extra(id) => {
            if let Some(draft) = id.strip_prefix(MIX_PREFIX) {
                add_source(s, draft, from);
            }
        }
        DropTarget::Design(_) => {}
    }
}

pub fn on_extra_node_move(s: &mut AppState, id: &str, pos: [f32; 2]) {
    let Some(draft) = id.strip_prefix(MIX_PREFIX) else { return };
    if let Some(d) = draft_mut(s, draft) {
        d.position = pos;
    }
}

/// Generate from the draft at its position, then drop it.
pub fn send(s: &mut AppState, draft: &str) {
    let Some(d) = s.features.mix.drafts.iter().find(|d| d.id == draft).cloned() else { return };
    let origin = Point { x: d.position[0] as f64, y: d.position[1] as f64 };
    if crate::features::multi::generate_from_sources(s, &d.sources, &d.text.text, d.count, Some(origin)) {
        close(s, draft);
    }
}

/// Drafts in the active space, as canvas nodes.
pub fn extra_nodes(s: &AppState, p: &Palette) -> Vec<ExtraNode> {
    s.features
        .mix
        .drafts
        .iter()
        .filter(|d| d.space_id == s.route.space)
        .map(|d| ExtraNode { id: format!("{MIX_PREFIX}{}", d.id), pos: d.position, size: [MIX_W, MIX_H], content: card(s, d, p) })
        .collect()
}

/// Dashed edges source → draft, and from every combined base (other than `parentId`) to the design it made.
pub fn extra_edges(s: &AppState) -> Vec<ExtraEdge> {
    let space = &s.route.space;
    let snap = s.snap();
    let mut out = Vec::new();
    for d in s.features.mix.drafts.iter().filter(|d| &d.space_id == space) {
        for src in d.sources.iter().filter(|x| snap.design(x).is_some_and(|sd| &sd.space_id == space)) {
            out.push(ExtraEdge { from: src.clone(), to: format!("{MIX_PREFIX}{}", d.id), dashed: true, animated: true });
        }
    }
    for d in snap.designs_in(space) {
        let Some(bases) = d.request.as_ref().and_then(|r| r.extra.get("bases")).and_then(|b| b.as_array()) else { continue };
        if bases.len() < 2 {
            continue;
        }
        for b in bases.iter().filter_map(|b| b.get("id").and_then(|i| i.as_str())) {
            if Some(b) == d.parent_id.as_deref() || !snap.design(b).is_some_and(|x| &x.space_id == space) {
                continue;
            }
            out.push(ExtraEdge { from: b.into(), to: d.id.clone(), dashed: false, animated: d.status == proto_core::DesignStatus::Generating });
        }
    }
    out
}

fn card(s: &AppState, d: &MixDraft, p: &Palette) -> El {
    let snap = s.snap();
    let sources: Vec<_> = d.sources.iter().filter_map(|id| snap.design(id)).collect();
    let mix = sources.len() > 1;
    let ready = sources.iter().any(|x| x.is_ready());
    let id = d.id.clone();
    let (close_id, send_id, edit_id) = (id.clone(), id.clone(), id.clone());
    let mut chips = row().key(format!("mix-chips-{id}")).w_full().gap(4.0).flex_wrap();
    for sd in &sources {
        let mut chip = row().key(format!("mix-src-{id}-{}", sd.id)).h(24.0).pl(8.0).pr(4.0).gap(2.0).radius(12.0).center_y().fill(p.hover)
            .child(text(sd.title.clone()).font(SMALL).color(p.fg).truncate());
        if mix {
            let (did, sid) = (id.clone(), sd.id.clone());
            chip = chip.child(row().key(format!("mix-rm-{id}-{}", sd.id)).role("button").label(format!("Remove {}", sd.title)).size(18.0, 18.0).center()
                .on_press(move |s: &mut AppState| remove_source(s, &did, &sid)).child(icons::x(10.0, p.muted_fg)));
        }
        chips = chips.child(chip);
    }
    if !mix {
        chips = chips.child(text("Connect more cards to mix them").font(SMALL).color(p.muted_fg));
    }
    let input = text_input(&d.text, BODY).color(p.fg).key(format!("mix-text-{id}")).label(if mix { "How should they be combined?" } else { "What should it become?" })
        .placeholder(if mix { "Optional: steer the mix, e.g. layout of the first, colours of the second…" } else { "What should it become? e.g. a darker, denser version…" }, p.muted_fg)
        .w_full().min_h(56.0)
        .on_edit(move |s: &mut AppState| &mut draft_mut(s, &edit_id).expect("draft").text)
        .on_input_key({
            let id = id.clone();
            move |s: &mut AppState, ev, _| match ev.key {
                rune::Key::Enter if !ev.mods.shift => {
                    send(s, &id);
                    true
                }
                rune::Key::Escape => {
                    close(s, &id);
                    true
                }
                _ => false,
            }
        });
    let mut counts = row().gap(1.0).padding(2.0).radius(6.0).fill(p.hover);
    for c in 1..=MAX_COUNT {
        let on = d.count == c;
        let cid = id.clone();
        counts = counts.child(row().key(format!("mix-count-{id}-{c}")).role("radio").label(c.to_string()).checked(on).size(22.0, 22.0).radius(5.0).center()
            .fill(if on { style::brand() } else { Color::TRANSPARENT })
            .on_press(move |s: &mut AppState| {
                if let Some(d) = draft_mut(s, &cid) {
                    d.count = c;
                }
            })
            .child(text(c.to_string()).font(SMALL).color(if on { Color::WHITE } else { p.muted_fg })));
    }
    let label = if mix { format!("Mix {} into {} design{}", sources.len(), d.count, if d.count == 1 { "" } else { "s" }) } else { format!("Generate {} variation{}", d.count, if d.count == 1 { "" } else { "s" }) };
    let go = row().key(format!("mix-send-{id}")).role("button").label(label.clone()).disabled(!ready).h(30.0).px(10.0).radius(8.0).gap(6.0).center()
        .fill(style::brand()).box_shadow(style::brand_highlight()).opacity(if ready { 1.0 } else { 0.5 })
        .child(text(label).font(SMALL).color(Color::WHITE))
        .child(icons::arrow_up(13.0, Color::WHITE));
    let go = if ready { go.press(Color::BLACK.alpha(0.15)).on_press(move |s: &mut AppState| send(s, &send_id)) } else { go };
    col().key(format!("mix-card-{id}")).role("group").label(if mix { "New mix" } else { "New card" }).w(MIX_W).h(MIX_H).gap(8.0).padding(12.0).radius(12.0)
        .fill(p.canvas_bg).border(1.0, p.fg.alpha(0.35)).border_dash(5.0, 4.0)
        .child(row().w_full().gap(8.0).center_y()
            .child(text(if mix { format!("Mix of {}", sources.len()) } else { "New card".to_string() }).font(TITLE).color(p.fg).grow(1.0))
            .child(row().key(format!("mix-close-{id}")).role("button").label("Discard draft (Esc)").size(24.0, 24.0).radius(6.0).center()
                .hover(p.hover).press(p.press).on_press(move |s: &mut AppState| close(s, &close_id)).child(icons::x(13.0, p.muted_fg))))
        .child(chips)
        .child(el().w_full().grow(1.0).min_h(0.0).padding(8.0).radius(8.0).border(1.0, p.border).child(input))
        .child(row().w_full().justify_between().center_y().child(counts).child(go))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composer::Outgoing;

    fn state() -> AppState {
        let mut s = AppState::new();
        s.use_store(crate::features::scenes::fixture());
        s.enter_space("zzfx", None);
        s
    }

    #[test]
    fn drops_start_drafts_and_mixes() {
        let mut s = state();
        let mut tx = rune::Tx::default();
        on_handle_drop(&mut s, "zzv1", DropTarget::Empty([500.0, 1500.0]), &mut tx);
        assert_eq!(s.features.mix.drafts.len(), 1);
        assert_eq!(s.features.mix.drafts[0].position, [500.0 - MIX_W / 2.0, 1484.0]);
        let draft = s.features.mix.drafts[0].id.clone();
        on_handle_drop(&mut s, "zzv2", DropTarget::Extra(format!("{MIX_PREFIX}{draft}")), &mut tx);
        on_handle_drop(&mut s, "zzv2", DropTarget::Extra(format!("{MIX_PREFIX}{draft}")), &mut tx);
        assert_eq!(s.features.mix.drafts[0].sources, ["zzv1", "zzv2"]);
        on_handle_drop(&mut s, "zzv1", DropTarget::Design("zzv3".into()), &mut tx);
        assert_eq!(s.features.mix.drafts.len(), 2);
        assert_eq!(extra_edges(&s).iter().filter(|e| e.dashed).count(), 4);
        send(&mut s, &draft);
        assert_eq!(s.features.mix.drafts.len(), 1);
        let [Outgoing::Generate { origin, request, .. }] = s.composer.outbox.as_slice() else { panic!("one generate") };
        assert_eq!(request.extra["bases"].as_array().unwrap().len(), 2);
        assert_eq!(origin.unwrap().x, (500.0 - MIX_W / 2.0) as f64);
    }
}
