//! A design's thumbnail on its card: the box the reference sizes it to (docs/spec-cards.md §2.3)
//! and the element that draws it. Pixels come from proto-web's `Snapshots` once wired; until then
//! the box shows its backdrop only.

use rune::{Align, El, Pixels, SurfaceFit, el};

use super::theme::CardTheme;

/// Height of a portrait or phone screen on a card.
const SCREEN_H: f32 = 420.0;
/// Phone width threshold (`viewport < 600`).
const PHONE_W: f64 = 600.0;

/// Where the thumbnail sits inside the card's inner width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewBox {
    pub w: f32,
    pub h: f32,
    /// Centred horizontally (portrait/phone screens); else it runs edge to edge.
    pub centred: bool,
    /// Phone pages draw without a backdrop (the screen sits on the card).
    pub transparent: bool,
}

/// The reference's thumbnail box for a design laid out at `viewport` (default 1280) with an optional
/// fixed screen height, inside `inner_w`. `tall` = the snapshot's content crop `[w, h]` when the
/// content is a narrow tall element (h / w > 1.5), which makes the box taller.
pub fn preview_box(viewport: Option<f64>, viewport_h: Option<f64>, inner_w: f32, tall: Option<[f32; 2]>) -> PreviewBox {
    let vw = viewport.filter(|v| *v > 0.0).unwrap_or(1280.0);
    let fixed = viewport_h.filter(|h| *h > 0.0);
    let phone = vw < PHONE_W;
    match fixed {
        Some(vh) if vh > vw => {
            let w = (SCREEN_H * (vw / vh) as f32).min(inner_w);
            PreviewBox { w, h: SCREEN_H, centred: true, transparent: phone }
        }
        Some(vh) => PreviewBox { w: inner_w, h: inner_w * (vh / vw) as f32, centred: false, transparent: phone },
        None if phone => PreviewBox { w: (SCREEN_H * 9.0 / 17.0).min(inner_w), h: SCREEN_H, centred: true, transparent: true },
        None => match tall {
            Some([fw, fh]) if fw > 0.0 && fh / fw > 1.5 => {
                PreviewBox { w: inner_w, h: inner_w * fh.min(fw * 3.0) / fw, centred: false, transparent: false }
            }
            _ => PreviewBox { w: inner_w, h: inner_w * 0.75, centred: false, transparent: false },
        },
    }
}

/// A snapshot to draw: its pixels, the viewport area they cover (CSS px), and whether that area is
/// a content crop (proto-web's `Snapshot`, decoupled so the box math stays testable).
pub struct Picture<'a> {
    pub pixels: &'a Pixels,
    pub rect: [f32; 4],
    pub cropped: bool,
}

/// Where a picture of `rect` (CSS px) sits in a box `[w, h]` for a page laid out at `vw`: the
/// reference's scale rule. Uncropped: fill the width from the top. Cropped: show the whole crop
/// when that keeps it at least half the fill scale, else fill the width from the top; never above
/// 1:1 unless the page itself is shown above 1:1. Returns `[x, y, w, h]` in box points.
pub fn place(rect: [f32; 4], cropped: bool, b: [f32; 2], vw: f32) -> [f32; 4] {
    let (rw, rh) = (rect[2].max(1.0), rect[3].max(1.0));
    let by_width = b[0] / rw;
    if !cropped {
        return [0.0, 0.0, b[0], rh * by_width];
    }
    let base = b[0] / vw.max(1.0);
    let whole = by_width.min(b[1] / rh);
    let scale = (if whole >= by_width * 0.5 { whole } else { by_width }).min(base.max(1.0));
    let (w, h) = (rw * scale, rh * scale);
    let y = if scale == whole { (b[1] - h) / 2.0 } else { 0.0 };
    [(b[0] - w) / 2.0, y.max(0.0), w, h]
}

/// The thumbnail element at `b`: the backdrop plus the picture, placed per `place`.
pub fn thumb(key: String, b: PreviewBox, pic: Option<Picture>, vw: f32, t: &CardTheme) -> El {
    let mut e = el().key(key.clone()).size(b.w, b.h).clip_soft(0.0);
    if !b.transparent {
        e = e.fill(t.muted);
    }
    if let Some(p) = pic {
        let [x, y, w, h] = place(p.rect, p.cropped, [b.w, b.h], vw);
        e = e.child(
            el().key(format!("{key}:px")).absolute(x, y).size(w, h)
                .pixels_fit(p.pixels, SurfaceFit::Fill).surface_anchor(Align::Start, Align::Start),
        );
    }
    if b.centred { e.self_center() } else { e }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 378.0;

    #[test]
    fn desktop_is_four_by_three() {
        let b = preview_box(None, None, W, None);
        assert_eq!((b.w, b.h, b.centred, b.transparent), (W, W * 0.75, false, false));
    }

    #[test]
    fn phone_is_a_centred_nine_by_seventeen_screen() {
        let b = preview_box(Some(390.0), None, W, None);
        assert_eq!(b.h, 420.0);
        assert!((b.w - 420.0 * 9.0 / 17.0).abs() < 0.01 && b.centred && b.transparent);
    }

    #[test]
    fn fixed_portrait_keeps_its_ratio_at_screen_height() {
        let b = preview_box(Some(640.0), Some(960.0), W, None);
        assert_eq!((b.w, b.h, b.centred), (280.0, 420.0, true));
    }

    #[test]
    fn fixed_landscape_fills_the_width() {
        let b = preview_box(Some(1280.0), Some(720.0), W, None);
        assert_eq!((b.w, b.h, b.centred), (W, W * 720.0 / 1280.0, false));
    }

    #[test]
    fn uncropped_pictures_fill_the_width_from_the_top() {
        assert_eq!(place([0.0, 0.0, 1280.0, 960.0], false, [W, W * 0.75], 1280.0), [0.0, 0.0, W, W * 0.75]);
    }

    #[test]
    fn a_small_crop_is_capped_at_native_size_centred_horizontally_from_the_top() {
        // A 340×200 card on a 1280 page in a 378×283.5 box: whole = min(378/340, 283.5/200) > 1 → capped at 1,
        // and a capped scale isn't the "whole" one, so it sits at the top (the reference's ty = -fit.y).
        let [x, y, w, h] = place([470.0, 300.0, 340.0, 200.0], true, [W, W * 0.75], 1280.0);
        assert_eq!((w, h, y), (340.0, 200.0, 0.0));
        assert!((x - (W - 340.0) / 2.0).abs() < 0.01);
    }

    #[test]
    fn a_wide_crop_fits_whole_and_centred() {
        // 600×200 crop: whole = min(378/600, 283.5/200) = 0.63 = by_width → centred vertically.
        let [x, y, w, h] = place([0.0, 0.0, 600.0, 200.0], true, [W, W * 0.75], 1280.0);
        assert!((w - W).abs() < 0.01 && x.abs() < 0.01 && (y - (W * 0.75 - h) / 2.0).abs() < 0.01);
    }

    #[test]
    fn a_tall_narrow_crop_runs_from_the_top_at_most_native_size() {
        // whole (0.315) < half the fill scale (1.26) → fill width, capped at 1:1, centred, from the top.
        let [x, y, w, h] = place([0.0, 0.0, 300.0, 900.0], true, [W, W * 0.75], 1280.0);
        assert_eq!((y, w, h), (0.0, 300.0, 900.0));
        assert!((x - (W - 300.0) / 2.0).abs() < 0.01);
    }

    #[test]
    fn tall_content_grows_up_to_three_widths() {
        assert_eq!(preview_box(None, None, W, Some([340.0, 680.0])).h, W * 2.0);
        assert_eq!(preview_box(None, None, W, Some([300.0, 3000.0])).h, W * 3.0);
        assert_eq!(preview_box(None, None, W, Some([300.0, 400.0])).h, W * 0.75);
    }
}
