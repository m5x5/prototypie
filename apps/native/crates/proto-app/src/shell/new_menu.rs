//! The header's "+" New menu, as the reference's tile menu: a 26rem popover with a 2-column grid
//! of tiles (icon square, label, 2-line hint). Entries come from `crate::features::create_items`
//! (worker-8), in the reference's order; on the main canvas only Project is enabled, and the
//! others say "Open a project first".

use rune::{Color, El, TextStyle, Tx, col, el, row, text};

use crate::app::insets::HEADER_H;
use crate::app::state::AppState;
use crate::app::style::Palette;
use crate::features::CreateItem;

const LABEL: TextStyle = TextStyle::new(14.0, 500.0);
const HINT: TextStyle = TextStyle::new(12.0, 400.0);
/// The reference's `w-[26rem]`, `p-2`, `gap-1.5`.
const WIDTH: f32 = 416.0;
const PAD: f32 = 8.0;
const GAP: f32 = 6.0;
/// The menu's right edge sits under the New button's: right padding 16 + ⋯ 28 + gap 8 +
/// + sidebar toggle 28 + gap 8, plus the features' header actions after "+" (the reference's order). (Full screen is
/// the window's green button: no header button.)
const RIGHT: f32 = 16.0 + 28.0 + 8.0 + 28.0 + 8.0;

fn hex(s: &str) -> Color {
    rune::color::hex(s).unwrap()
}

/// shadcn's popover, muted and accent tokens.
fn popover(p: &Palette) -> (Color, Color, Color) {
    if p.dark { (hex("#171717"), hex("#262626"), hex("#262626")) } else { (Color::WHITE, hex("#f5f5f5"), hex("#f5f5f5")) }
}

fn tile(it: &CreateItem, p: &Palette, root: bool) -> El {
    let (_, muted, accent) = popover(p);
    let hint = if it.enabled { it.hint } else if root { "Open a project first" } else { it.hint };
    let square = el().size(40.0, 40.0).radius(8.0).border(1.0, p.border).fill(muted).center()
        .child(rune::icon(it.icon, 20.0).color(p.fg));
    let t = col().key(format!("new-{}", it.key)).role("menuitem").label(it.label).w((WIDTH - 2.0 * PAD - GAP) / 2.0)
        .padding(12.0).gap(10.0).radius(12.0).border(1.0, p.border)
        .child(square)
        .child(text(it.label).font(LABEL).color(p.fg))
        .child(text(hint).font(HINT).color(p.muted_fg).wrap());
    if it.enabled {
        let run = it.run;
        t.focusable(true).hover(accent).press(accent).on_press_tx(move |s: &mut AppState, tx: &mut Tx| {
            s.shell.new_menu_open = false;
            run(s, tx);
        })
    } else {
        t.opacity(0.5)
    }
}

/// The open menu, under "+" (8pt below the header controls). Escape or a click outside closes it.
pub fn view(s: &AppState, p: &Palette, actions_w: f32) -> El {
    let (bg, ..) = popover(p);
    let items = crate::features::create_items(s);
    let root = s.route.is_root();
    let mut grid = col().key("new-menu-grid").gap(GAP).focus_group();
    for pair in items.chunks(2) {
        grid = grid.child(pair.iter().fold(row().gap(GAP), |r, it| r.child(tile(it, p, root))));
    }
    let menu = col().key("new-menu").role("menu").label("Create new").w(WIDTH).padding(PAD).radius(12.0)
        .fill(bg).border(1.0, p.border)
        .box_shadow(rune::Shadow { x: 0.0, y: 8.0, blur: 24.0, spread: -4.0, color: Color::BLACK.alpha(0.35), inset: false })
        .modal(true, |s: &mut AppState| s.shell.new_menu_open = false)
        .child(grid);
    el().key("new-menu-anchor").pin(None, Some(HEADER_H - 4.0 + 8.0 - 4.0), Some(RIGHT + actions_w), None).child(menu)
}
