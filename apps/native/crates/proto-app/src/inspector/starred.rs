//! The exploration's starred designs (in place of the old stat tiles): a small picture and the
//! title per design. A press selects the card and centres the canvas on it; a double-press opens
//! it fullscreen (ready designs only, as on the card).

use std::sync::Arc;

use proto_core::Design;
use rune::{Cx, El, Tx, col, row, text};

use super::style::{self, Ink};
use crate::app::state::AppState;
use crate::cards::preview::{self, PreviewBox};
use crate::cards::CardTheme;

/// Thumbnail size (4:3, like the cards' desktop preview).
const THUMB_W: f32 = 48.0;
const THUMB_H: f32 = 36.0;

/// The starred designs of the active space, in the store's order.
pub fn starred(s: &AppState) -> Vec<Arc<Design>> {
    s.snap().designs_in(&s.route.space).filter(|d| d.starred == Some(true)).collect()
}

/// From `inspector::poll`: ask for the rows' pictures (2× backing). True when one arrived.
pub fn poll(s: &mut AppState) -> bool {
    let ids: Vec<String> = starred(s).iter().map(|d| d.id.clone()).collect();
    if ids.is_empty() {
        return false;
    }
    let snap = s.store().snapshot();
    let mut changed = false;
    for id in ids {
        changed |= s.cards.thumbs.request_extra(&snap, &id, (THUMB_W * 2.0) as u32);
    }
    changed
}

/// Select `id` and centre the canvas on its card, keeping the zoom.
fn reveal(s: &mut AppState, id: &str) {
    s.route.selected = vec![id.to_string()];
    let Some((_, r)) = s.canvas.nodes.rects.get(id).copied() else { return };
    let c = &mut s.canvas;
    let size = if c.size[0] > 0.0 { c.size } else { [1280.0, 800.0] };
    let [l, t, rt, b] = crate::app::insets::canvas_insets(true);
    let centre = [l + (size[0] - l - rt) / 2.0, t + (size[1] - t - b) / 2.0];
    let zoom = c.camera.zoom;
    let mid = [r[0] + r[2] / 2.0, r[1] + r[3] / 2.0];
    c.camera.set([centre[0] - mid[0] * zoom, centre[1] - mid[1] * zoom], zoom);
}

pub fn view(s: &AppState, cx: &mut Cx<AppState>, list: &[Arc<Design>], k: &Ink) -> El {
    let t = crate::cards::theme(s, cx);
    col().key("insp-starred").role("list").label("Starred designs").w_full().gap(2.0).focus_group()
        .children(list.iter().map(|d| item(s, d, k, &t)))
}

fn item(s: &AppState, d: &Design, k: &Ink, t: &CardTheme) -> El {
    let selected = s.route.selected.iter().any(|x| x == &d.id);
    let pic = s.cards.thumbs.get(&d.id).map(|sn| preview::Picture { pixels: &sn.pixels, rect: sn.rect, cropped: sn.crop.is_some() });
    let thumb = preview::thumb(format!("insp-starred-pic-{}", d.id),
        PreviewBox { w: THUMB_W - 2.0, h: THUMB_H - 2.0, centred: false, transparent: false }, pic, d.viewport.unwrap_or(1280.0) as f32, t);
    let (id, open) = (d.id.clone(), d.id.clone());
    let r = style::menu_item(row(), k).key(format!("insp-starred-{}", d.id)).role("button").label(d.title.clone())
        .w_full().px(4.0).py(4.0).gap(10.0).radius(style::R_MD).center_y().hover(k.muted)
        .on_press(move |s: &mut AppState| reveal(s, &id))
        .on_double_press_tx(move |s: &mut AppState, tx: &mut Tx| {
            crate::cards::on_node_double_press(s, crate::canvas::NodeKind::Design, &open, tx)
        })
        .child(el_clip(thumb, k))
        .child(text(d.title.clone()).font(style::XS).color(k.fg).truncate().grow(1.0).min_w(0.0));
    if selected { r.fill(k.muted.alpha(0.5)) } else { r }
}

/// The picture with a hairline border and small radius.
fn el_clip(thumb: El, k: &Ink) -> El {
    // Rune's border takes no space and the picture would cover it: inset the picture by 1pt.
    rune::el().size(THUMB_W, THUMB_H).padding(1.0).radius(4.0).border(1.0, k.border)
        .child(rune::el().radius(3.0).clip_rounded(3.0).child(thumb))
}
