//! The 1–6 variations picker: press a number, or press and drag across the row to scrub
//! (ew-resize); ←/↓ and →/↑ step, Home/End jump.

use rune::{Cursor, Drag, El, Key, KeyEvent, Phase, Tx, row, text};

use crate::app::state::AppState;
use crate::inspector::style::{self, Ink};

pub const MIN: u8 = 1;
pub const MAX: u8 = 6;
/// Button width, gap and the group's padding.
const W: f32 = 20.0;
const GAP: f32 = 1.0;
const PAD: f32 = 2.0;

/// The count under `x` (points from the group's left edge): the nearest button.
pub fn count_at(x: f32) -> u8 {
    let i = ((x - PAD + GAP / 2.0) / (W + GAP)).floor();
    (i.clamp(0.0, f32::from(MAX - 1)) as u8) + MIN
}

/// The count after a key, if the key steps it.
pub fn step(count: u8, ev: &KeyEvent) -> Option<u8> {
    match ev.key {
        Key::Right | Key::Up => Some((count + 1).min(MAX)),
        Key::Left | Key::Down => Some(count.saturating_sub(1).max(MIN)),
        Key::Home => Some(MIN),
        Key::End => Some(MAX),
        _ => None,
    }
}

pub fn view(count: u8, k: &Ink) -> El {
    let buttons = (MIN..=MAX).map(|n| {
        let on = n == count;
        let b = row().key(format!("composer-count-{n}")).size(W, 22.0).radius(5.0).center().cursor(Cursor::ResizeColumn)
            .role("button").label(format!("{n} variation{}", if n == 1 { "" } else { "s" }))
            .child(text(n.to_string()).font(if on { style::COUNT_ON } else { style::COUNT }).color(if on { k.primary_fg } else { k.muted_fg }));
        if on { b.fill(k.primary) } else { b }
    });
    row().key("composer-count").gap(GAP).padding(PAD).radius(style::R_MD).fill(k.fg5).cursor(Cursor::ResizeColumn)
        .role("group").label("Variations (drag, click or use arrow keys)").help("Drag, click or use ←/→")
        .focusable(true).focus_ring(3.0, 0.0, k.ring.alpha(0.5))
        .on_drag(|s: &mut AppState, d: &mut Drag, _tx: &mut Tx| {
            if d.phase != Phase::Up || d.dx.abs() < 1.0 {
                s.composer.count = count_at(d.x);
            }
        })
        .on_key(|s: &mut AppState, ev: &KeyEvent, _tx: &mut Tx| match step(s.composer.count, ev) {
            Some(n) => {
                s.composer.count = n;
                true
            }
            None => false,
        })
        .children(buttons)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_maps_to_the_nearest_button() {
        assert_eq!(count_at(0.0), 1);
        assert_eq!(count_at(12.0), 1);
        assert_eq!(count_at(PAD + W + GAP + 1.0), 2);
        assert_eq!(count_at(PAD + 5.0 * (W + GAP) + 10.0), 6);
        assert_eq!(count_at(500.0), 6);
        assert_eq!(count_at(-30.0), 1);
    }

    #[test]
    fn keys_step_and_clamp() {
        let k = |key| KeyEvent { key, mods: Default::default() };
        assert_eq!(step(3, &k(Key::Right)), Some(4));
        assert_eq!(step(6, &k(Key::Up)), Some(6));
        assert_eq!(step(1, &k(Key::Left)), Some(1));
        assert_eq!(step(4, &k(Key::Home)), Some(1));
        assert_eq!(step(2, &k(Key::End)), Some(6));
        assert_eq!(step(2, &k(Key::Tab)), None);
    }
}
