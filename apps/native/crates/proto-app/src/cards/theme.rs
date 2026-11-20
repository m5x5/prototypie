//! The cards' look on top of the shell palette: the reference's shadcn tokens (docs/spec-cards.md §1),
//! text styles, shadows and sizes. Dark is the reference's default theme.

use rune::color::hex;
use rune::{Color, Shadow, TextStyle};

use crate::app::style::Palette;

fn c(s: &str) -> Color {
    hex(s).expect("valid hex")
}

/// Card width in world units (the reference's `NODE_W`).
pub const NODE_W: f32 = 380.0;
/// Layout estimate of a design card's height (`NODE_H`).
pub const NODE_H: f32 = 420.0;
/// Project/exploration card width (`PORTAL_W`) and layout estimate of its height (`PORTAL_H`).
pub const PORTAL_W: f32 = 300.0;
pub const PORTAL_H: f32 = 330.0;
/// The title label above a design card's surface; the top edge handle sits at this y.
pub const TITLE_H: f32 = 28.0;

/// Colours the cards use beyond the shell palette.
#[derive(Clone, Copy, Debug)]
pub struct CardTheme {
    pub dark: bool,
    pub fg: Color,
    pub muted_fg: Color,
    pub border: Color,
    /// Page background (`--background`): the fullscreen page.
    pub background: Color,
    /// Card surface (`--card`).
    pub card: Color,
    /// `--muted` / `--secondary` / `--accent`.
    pub muted: Color,
    /// `--primary`: near-white in dark, near-black in light.
    pub primary: Color,
    pub primary_fg: Color,
    /// `--secondary-foreground`: text on `secondary` fills (#fafafa dark, #171717 light).
    pub secondary_fg: Color,
    pub ring: Color,
    pub input: Color,
    pub destructive: Color,
    /// Ghost button hover overlay (`hover:bg-muted/50` dark, `hover:bg-muted` light).
    pub ghost_hover: Color,
    /// Secondary button hover overlay (secondary mixed 5% toward the foreground).
    pub secondary_hover: Color,
    pub press: Color,
    pub blue_300: Color,
    pub blue_400: Color,
    pub blue_500: Color,
    pub blue_600: Color,
    pub blue_700: Color,
    pub amber_400: Color,
    pub emerald: Color,
}

impl CardTheme {
    pub fn new(p: &Palette) -> Self {
        let dark = p.dark;
        let (card, muted, primary, primary_fg, ring, input, destructive) = if dark {
            (c("#171717"), c("#262626"), c("#e5e5e5"), c("#171717"), c("#737373"), Color::WHITE.alpha(0.15), c("#ff6467"))
        } else {
            (Color::WHITE, c("#f5f5f5"), c("#171717"), c("#fafafa"), c("#a1a1a1"), c("#e5e5e5"), c("#e7000b"))
        };
        CardTheme {
            dark,
            fg: p.fg,
            muted_fg: p.muted_fg,
            border: p.border,
            background: if dark { c("#0a0a0a") } else { Color::WHITE },
            card,
            muted,
            primary,
            primary_fg,
            secondary_fg: if dark { p.fg } else { c("#171717") },
            ring,
            input,
            destructive,
            ghost_hover: if dark { muted.alpha(0.5) } else { muted },
            secondary_hover: p.fg.alpha(0.05),
            press: p.press,
            blue_300: c("#8ec5ff"),
            blue_400: c("#51a2ff"),
            blue_500: c("#2b7fff"),
            blue_600: c("#155dfc"),
            blue_700: c("#1447e6"),
            amber_400: c("#ffb900"),
            emerald: if dark { c("#00d492") } else { c("#007a55") },
        }
    }

    /// The design card's resting shadow (`0 8 30 black/.4`, light `/.1`).
    pub fn card_shadow(&self, selected: bool) -> Shadow {
        let (y, blur, a) = match (selected, self.dark) {
            (false, true) => (8.0, 30.0, 0.40),
            (false, false) => (8.0, 30.0, 0.10),
            (true, true) => (12.0, 36.0, 0.50),
            (true, false) => (12.0, 36.0, 0.16),
        };
        Shadow { x: 0.0, y, blur, spread: 0.0, color: Color::BLACK.alpha(a), inset: false }
    }
}

/// Tailwind `shadow-sm`.
pub fn shadow_sm() -> [Shadow; 2] {
    [
        Shadow { x: 0.0, y: 1.0, blur: 3.0, spread: 0.0, color: Color::BLACK.alpha(0.1), inset: false },
        Shadow { x: 0.0, y: 1.0, blur: 2.0, spread: -1.0, color: Color::BLACK.alpha(0.1), inset: false },
    ]
}

/// Tailwind `shadow-lg`.
pub fn shadow_lg() -> [Shadow; 2] {
    [
        Shadow { x: 0.0, y: 10.0, blur: 15.0, spread: -3.0, color: Color::BLACK.alpha(0.1), inset: false },
        Shadow { x: 0.0, y: 4.0, blur: 6.0, spread: -4.0, color: Color::BLACK.alpha(0.1), inset: false },
    ]
}

/// `text-[10px]` medium (kind badge).
pub const T10: TextStyle = TextStyle::new(10.0, 500.0);
/// `text-[11px]` medium (pills).
pub const T11: TextStyle = TextStyle::new(11.0, 500.0).features("'tnum' 1");
/// `text-[11.5px]` (pod provenance).
pub const T11_5: TextStyle = TextStyle::new(11.5, 400.0);
/// `font-mono text-[11px] leading-4` (local path).
pub const MONO11: TextStyle = TextStyle::new(11.0, 400.0).mono().lh(16.0);
/// `text-xs` (12/16).
pub const XS: TextStyle = TextStyle::new(12.0, 400.0).lh(16.0);
pub const XS_MED: TextStyle = TextStyle::new(12.0, 500.0).lh(16.0);
/// `text-xs` tabular (counts).
pub const XS_NUM: TextStyle = TextStyle::new(12.0, 400.0).lh(16.0).features("'tnum' 1");
pub const XS_NUM_MED: TextStyle = TextStyle::new(12.0, 500.0).lh(16.0).features("'tnum' 1");
/// Button `sm` label (`text-[0.8rem]` medium).
pub const BTN: TextStyle = TextStyle::new(12.8, 500.0);
/// `text-sm` medium (portal name).
pub const SM_MED: TextStyle = TextStyle::new(14.0, 500.0).lh(20.0);
/// `text-sm` (descriptions, note box).
pub const SM: TextStyle = TextStyle::new(14.0, 400.0).lh(20.0);
/// `text-base` medium, leading 1 (fullscreen title).
pub const TITLE: TextStyle = TextStyle::new(16.0, 500.0).lh(16.0);
