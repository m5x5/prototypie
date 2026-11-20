//! The inspector's and composer's look: the reference app's dark/light tokens resolved to colours
//! (shadcn `--muted`, `--input`, `--popover`…), its type scale and radii, and the `GLASS_PANEL`
//! surface. `app::style::Palette` carries the shared ones (canvas, fg, muted fg, border).

use rune::color::hex;
use rune::{Color, El, Shadow, TextStyle};

use crate::app::style::Palette;

fn c(s: &str) -> Color {
    hex(s).expect("valid hex")
}

/// Radii (`--radius` 10px: sm 6, md 8, lg 10, xl 14, 2xl 18).
pub const R_SM: f32 = 6.0;
pub const R_MD: f32 = 8.0;
pub const R_LG: f32 = 10.0;
pub const R_XL: f32 = 14.0;
pub const R_2XL: f32 = 18.0;

/// Panel padding and the gap between its sections (`p-4`, `space-y-4`).
pub const PAD: f32 = 16.0;

const TNUM: &str = "'tnum' 1";

/// text-sm semibold (title).
pub const TITLE: TextStyle = TextStyle::new(14.0, 600.0).lh(20.0);
/// Label (text-sm medium).
pub const LABEL: TextStyle = TextStyle::new(14.0, 500.0).lh(20.0);
/// text-xs.
pub const XS: TextStyle = TextStyle::new(12.0, 400.0).lh(16.0);
pub const XS_MEDIUM: TextStyle = TextStyle::new(12.0, 500.0).lh(16.0);
pub const XS_TNUM: TextStyle = TextStyle::new(12.0, 400.0).lh(16.0).features(TNUM);
/// The composer's text (13.5px, leading-snug).
pub const PROMPT: TextStyle = TextStyle::new(13.5, 400.0).lh(18.5);
/// Menu rows (text-sm).
pub const MENU: TextStyle = TextStyle::new(14.0, 400.0).lh(20.0);
/// Composer tabs (12.5px medium).
pub const TAB: TextStyle = TextStyle::new(12.5, 500.0).lh(16.0);
/// Group labels in lists (11.5px medium).
pub const GROUP: TextStyle = TextStyle::new(11.5, 500.0).lh(16.0);
/// Monospaced paths (11px).
pub const MONO: TextStyle = TextStyle::new(11.0, 400.0).lh(16.0).mono();
pub const SMALL: TextStyle = TextStyle::new(11.0, 400.0).lh(16.0);
/// Badge numbers and the count picker (tabular).
pub const COUNT: TextStyle = TextStyle::new(11.5, 400.0).lh(14.0).features(TNUM);
pub const COUNT_ON: TextStyle = TextStyle::new(11.5, 600.0).lh(14.0).features(TNUM);
pub const PILL_NUM: TextStyle = TextStyle::new(10.5, 400.0).lh(14.0).features(TNUM);
/// Stat tiles: value (text-sm medium, tabular) and caption (10px).
pub const STAT: TextStyle = TextStyle::new(14.0, 500.0).lh(20.0).features(TNUM);
pub const STAT_LABEL: TextStyle = TextStyle::new(10.0, 400.0).lh(15.0);
/// Mention list rows (13px medium) and chips (12px medium).
pub const ROW: TextStyle = TextStyle::new(13.0, 500.0).lh(18.0);
pub const BADGE: TextStyle = TextStyle::new(10.5, 600.0).lh(16.0);

/// The inspector's colours for one theme, on top of the shared `Palette`.
#[derive(Clone, Copy, Debug)]
pub struct Ink {
    pub fg: Color,
    pub muted_fg: Color,
    pub border: Color,
    /// `--input` (field borders).
    pub input: Color,
    /// `--ring` (focused field border).
    pub ring: Color,
    /// `--muted` / `--accent` (hover rows, active list row).
    pub muted: Color,
    /// `--popover` (menus, mention lists).
    pub popover: Color,
    /// `--primary` / `--primary-foreground` (count picker's pick, default button).
    pub primary: Color,
    pub primary_fg: Color,
    pub destructive: Color,
    /// The composer box and its active tab (`dark:bg-[#151515]`, light `bg-muted/40`).
    pub field: Color,
    /// `bg-foreground/5`.
    pub fg5: Color,
    pub hover: Color,
    pub press: Color,
    pub dark: bool,
}

pub fn ink(p: &Palette) -> Ink {
    if p.dark {
        Ink {
            fg: p.fg,
            muted_fg: p.muted_fg,
            border: p.border,
            input: Color::WHITE.alpha(0.15),
            ring: c("#737373"),
            muted: c("#262626"),
            popover: c("#171717"),
            primary: c("#e5e5e5"),
            primary_fg: c("#171717"),
            destructive: c("#ff6467"),
            field: c("#151515"),
            fg5: Color::WHITE.alpha(0.05),
            hover: p.hover,
            press: p.press,
            dark: true,
        }
    } else {
        Ink {
            fg: p.fg,
            muted_fg: p.muted_fg,
            border: p.border,
            input: c("#e5e5e5"),
            ring: c("#a1a1a1"),
            muted: c("#f5f5f5"),
            popover: Color::WHITE,
            primary: c("#171717"),
            primary_fg: c("#fafafa"),
            destructive: c("#e7000b"),
            field: c("#f5f5f5").alpha(0.4),
            fg5: Color::BLACK.alpha(0.05),
            hover: p.hover,
            press: p.press,
            dark: false,
        }
    }
}

/// `GLASS_PANEL`: canvas colour at 80%, 20pt backdrop blur, 1px foreground/10 border, soft shadow.
/// (The reference also lifts saturation 1.3×; Rune's backdrop blur has no saturation, and at 80%
/// fill it doesn't show.)
pub fn glass_panel(e: El, p: &Palette) -> El {
    e.backdrop_blur(20.0)
        .fill(p.canvas_bg.alpha(0.80))
        .border(1.0, p.border)
        .box_shadow(Shadow { x: 0.0, y: 10.0, blur: 30.0, spread: -12.0, color: Color::BLACK.alpha(0.6), inset: false })
}

/// The popover surface (menus, mention lists): popover fill, 1px border, `shadow-lg`.
pub fn popover(e: El, k: &Ink) -> El {
    e.fill(k.popover)
        .border(1.0, k.border)
        .radius(R_LG)
        .box_shadow(Shadow { x: 0.0, y: 10.0, blur: 15.0, spread: -3.0, color: Color::BLACK.alpha(0.25), inset: false })
        .box_shadow(Shadow { x: 0.0, y: 4.0, blur: 6.0, spread: -4.0, color: Color::BLACK.alpha(0.25), inset: false })
}

/// A keyboard stop with the reference's focus ring (`focus-visible:ring-3 ring-ring/50`):
/// drawn only when focus came from the keyboard.
pub fn focusable(e: El, k: &Ink) -> El {
    e.focusable(true).focus_ring(3.0, 0.0, k.ring.alpha(0.5))
}

/// A menu row: a keyboard stop in its menu's arrow-key group, highlighted on hover and focus.
pub fn menu_item(e: El, k: &Ink) -> El {
    e.focusable(true).focus_highlight(k.muted)
}

/// A 1px horizontal separator in the border colour.
pub fn separator(k: &Ink) -> El {
    rune::el().w_full().h(1.0).fill(k.border)
}

/// Pod kind / component tones (dark: `-500/12` background, `-300` text; light: `-700` text).
#[derive(Clone, Copy, Debug)]
pub struct Tone {
    pub bg: Color,
    pub fg: Color,
}

pub fn tone(dark: bool, hue: &str) -> Tone {
    let (base, light_fg, dark_fg, a) = match hue {
        "sky" => ("#0ea5e9", "#0369a1", "#7dd3fc", 0.12),
        "amber" => ("#f59e0b", "#b45309", "#fcd34d", 0.14),
        "emerald" => ("#10b981", "#047857", "#6ee7b7", 0.12),
        "violet" => ("#8b5cf6", "#6d28d9", "#c4b5fd", 0.12),
        _ => ("#d946ef", "#a21caf", "#f0abfc", 0.12), // fuchsia (components)
    };
    Tone { bg: c(base).alpha(a), fg: c(if dark { dark_fg } else { light_fg }) }
}

/// Status dots of the agent runs list.
pub fn blue_500() -> Color {
    c("#3b82f6")
}
pub fn emerald_500() -> Color {
    c("#10b981")
}
