//! Shared look: canvas colours, brand blue, the glass surface. One source of truth so the header
//! fades, the canvas, the minimap and the inspector can't drift (as `--canvas-bg` and `GLASS` in
//! the reference app).

use rune::color::hex;
use rune::{Color, El, Shadow};

fn c(s: &str) -> Color {
    hex(s).expect("valid hex")
}

/// Colours for one theme.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    /// The canvas background; header fades start from it.
    pub canvas_bg: Color,
    pub dots: Color,
    pub fg: Color,
    pub muted_fg: Color,
    /// Hairline borders (foreground at 10%).
    pub border: Color,
    /// Hover tint for ghost buttons.
    pub hover: Color,
    pub press: Color,
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            dark,
            canvas_bg: Color::BLACK,
            dots: c("#4d4d4d"),
            fg: c("#fafafa"),
            muted_fg: c("#a1a1a1"),
            border: Color::WHITE.alpha(0.10),
            hover: Color::WHITE.alpha(0.07),
            press: Color::WHITE.alpha(0.12),
        }
    } else {
        Palette {
            dark,
            canvas_bg: Color::WHITE,
            dots: c("#91919a"),
            fg: c("#0a0a0a"),
            muted_fg: c("#737373"),
            border: Color::BLACK.alpha(0.10),
            hover: Color::BLACK.alpha(0.05),
            press: Color::BLACK.alpha(0.09),
        }
    }
}

/// The brand blue, white text.
pub fn brand() -> Color {
    c("#1447e6")
}

/// The brand button's inset highlight (`inset 4px 4px 17px rgba(255,255,255,.28)`).
pub fn brand_highlight() -> Shadow {
    Shadow { x: 4.0, y: 4.0, blur: 17.0, spread: 0.0, color: Color::WHITE.alpha(0.28), inset: true }
}

/// The header controls' glass: the same translucent, blurred surface as `glass`, lighter (no drop shadow), for
/// pills that sit directly on the canvas.
pub fn glass_pill(e: El, p: &Palette) -> El {
    e.backdrop_blur(14.0).fill(p.canvas_bg.alpha(0.55)).border(1.0, p.border)
}

/// The glass surface for UI floating over the canvas (minimap, inspector): translucent fill at
/// `alpha` of the canvas colour, 14pt backdrop blur, a 1px foreground/10 border and a soft shadow.
/// `alpha` 0.5 is the minimap's look.
pub fn glass(e: El, p: &Palette, alpha: f32) -> El {
    e.backdrop_blur(14.0)
        .fill(p.canvas_bg.alpha(alpha))
        .border(1.0, p.border)
        .box_shadow(Shadow { x: 0.0, y: 10.0, blur: 30.0, spread: -12.0, color: Color::BLACK.alpha(0.6), inset: false })
}
