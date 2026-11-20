//! The reference's shadcn button and badge, sized as in docs/spec-cards.md §1, for card and
//! fullscreen controls. Other modules' card controls use these too, so every row reads the same.

use rune::{Color, El, Icon, TextStyle, icon, row, text};

use super::theme::{BTN, CardTheme};
use super::CardAction;
use crate::app::state::AppState;

/// Button looks (`variant` in the reference).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Ghost,
    Secondary,
    Outline,
    /// Brand blue, white text, inset highlight.
    Brand,
}

/// A press target: the action queued for its owner (see `CardAction`), or none when disabled.
fn on_action(e: El, action: Option<CardAction>) -> El {
    match action {
        Some(a) => e.on_press_tx(move |s: &mut AppState, tx: &mut rune::Tx| super::dispatch_tx(s, a.clone(), tx)),
        None => e.dim(0.5),
    }
}

fn styled(mut e: El, v: Variant, t: &CardTheme) -> El {
    e = match v {
        Variant::Ghost => e.hover(t.ghost_hover),
        Variant::Secondary => e.fill(t.muted).hover(t.secondary_hover),
        // `dark:bg-input/30`: input (white 15%) at 30% → white 4.5%. `Color::alpha` replaces the
        // alpha rather than scaling it, so multiply explicitly.
        Variant::Outline => e.fill(if t.dark { t.input.alpha(t.input.a * 0.3) } else { Color::TRANSPARENT })
            .border(1.0, t.input).hover(t.ghost_hover),
        Variant::Brand => e
            .fill(crate::app::style::brand())
            .box_shadow(crate::app::style::brand_highlight())
            .hover(Color::WHITE.alpha(0.08)),
    };
    e.press(t.press).focusable(true).focus_ring(3.0, 0.0, t.ring.alpha(0.5))
}

fn label_color(v: Variant, t: &CardTheme) -> Color {
    match v {
        Variant::Brand => Color::WHITE,
        Variant::Secondary => t.secondary_fg,
        Variant::Ghost | Variant::Outline => t.fg,
    }
}

/// Button `size="sm"` without a handler, for controls other modules own: add `.on_press(..)`
/// when enabled, `.dim(0.5)` when disabled. h 28, px 10, gap 4, 14pt icon, 12.8 medium label,
/// radius 8. `color` overrides the label/icon colour (e.g. `text-primary` for "Promoted").
pub fn button_sm(key: String, label: &str, glyph: Option<&'static Icon>, v: Variant, color: Option<Color>, t: &CardTheme) -> El {
    let c = color.unwrap_or_else(|| label_color(v, t));
    let e = row().key(key).role("button").label(label.to_string())
        .h(28.0).px(10.0).gap(4.0).radius(8.0).center_y().shrink()
        .children(glyph.map(|g| icon(g, 14.0).color(c)))
        .child(text(label.to_string()).font(BTN).color(c).single_line());
    styled(e, v, t)
}

/// `button_sm` that dispatches a card action (or shows disabled with `None`).
pub fn btn_sm(
    key: String,
    label: &str,
    glyph: Option<&'static Icon>,
    v: Variant,
    color: Option<Color>,
    t: &CardTheme,
    action: Option<CardAction>,
) -> El {
    on_action(button_sm(key, label, glyph, v, color, t), action)
}

/// Icon-only button (`icon-xs` 24, `icon-sm` 28): ghost, radius 8, `help` as tooltip.
pub fn icon_btn(
    key: String,
    help: &str,
    glyph: &'static Icon,
    box_size: f32,
    glyph_size: f32,
    color: Color,
    t: &CardTheme,
    action: Option<CardAction>,
) -> El {
    let e = row().key(key).role("button").label(help.to_string()).help(help.to_string())
        .size(box_size, box_size).radius(8.0).center()
        .child(icon(glyph, glyph_size).color(color));
    on_action(styled(e, Variant::Ghost, t), action)
}

/// Badge looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeLook {
    /// `secondary`: muted fill.
    Secondary,
    /// `outline`: hairline border in `border_color`.
    Outline,
}

/// A pill badge: h 20 (or `h`), px 8 (or `px`), `font`, 12pt icon, gap 4.
pub struct Badge<'a> {
    pub key: String,
    pub label: &'a str,
    pub glyph: Option<&'static Icon>,
    pub look: BadgeLook,
    pub color: Color,
    pub border_color: Option<Color>,
    pub h: f32,
    pub px: f32,
    pub font: TextStyle,
    /// May shrink and truncate its label (the "from {space}" badge); others keep their width.
    pub shrinks: bool,
}

impl<'a> Badge<'a> {
    pub fn new(key: String, label: &'a str, look: BadgeLook, t: &CardTheme) -> Self {
        let color = if look == BadgeLook::Secondary { t.secondary_fg } else { t.fg };
        Badge { key, label, glyph: None, look, color, border_color: None, h: 20.0, px: 8.0, font: super::theme::XS_MED, shrinks: false }
    }

    pub fn build(self, t: &CardTheme) -> El {
        let label = text(self.label.to_string()).font(self.font).color(self.color).single_line();
        let mut e = row().key(self.key).h(self.h).px(self.px).gap(4.0).radius(self.h / 2.0).center_y()
            .children(self.glyph.map(|g| icon(g, 12.0).color(self.color)))
            .child(if self.shrinks { label.truncate().shrink().min_w(0.0) } else { label });
        if self.shrinks {
            e = e.shrink().min_w(0.0);
        }
        e = match self.look {
            BadgeLook::Secondary => e.fill(t.muted),
            BadgeLook::Outline => e.border(1.0, self.border_color.unwrap_or(t.border)),
        };
        e
    }
}
