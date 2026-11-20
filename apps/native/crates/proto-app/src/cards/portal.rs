//! The project / exploration card (docs/spec-cards.md §3): header, hero preview, counts.
//! Double-press anywhere enters the space (the canvas' node frame calls `on_node_double_press`).

use proto_core::Space;
use rune::{El, Icon, LoopSpec, Spring, col, el, icon, row, spacer, text};

use super::controls::icon_btn;
use super::facts::PortalFacts;
use super::icons::*;
use super::preview::{Picture, PreviewBox, thumb};
use super::theme::{CardTheme, MONO11, PORTAL_W, SM_MED, T11, XS, XS_NUM_MED, shadow_lg, shadow_sm};
use super::CardAction as A;

pub fn node(sp: &Space, f: &PortalFacts, selected: bool, hero_pic: Option<Picture>, hero_vw: f32, t: &CardTheme) -> El {
    let kind = if f.project { "Project" } else { "Exploration" };
    let mut card = col().key(format!("p:{}:card", sp.id)).w_full().radius(14.0).border(1.0, t.border).fill(t.card)
        .hover_group()
        .motion(Spring::new(0.15, 0.0));
    let shadows = if selected { shadow_lg() } else { shadow_sm() };
    for s in shadows {
        card = card.box_shadow(s);
    }
    if selected {
        card = card.outline(2.0, 0.0, t.primary);
    }
    col().key(format!("p:{}:node", sp.id)).w(PORTAL_W)
        .role("group").label(format!("{kind} {}, double-click to open", sp.name))
        .child(card.child(header(sp, f, t)).child(hero(sp, f, hero_pic, hero_vw, t)).child(footer(sp, f, t)))
}

fn header(sp: &Space, f: &PortalFacts, t: &CardTheme) -> El {
    let id = sp.id.as_str();
    let k = |n: &str| format!("p:{id}:{n}");
    let chip = row().key(k("chip")).size(24.0, 24.0).radius(8.0).fill(t.primary.alpha(0.1)).center()
        .child(icon(if f.project { &FOLDER_GIT_2 } else { &LAYERS }, 14.0).color(t.primary));
    let names = col().key(k("names")).shrink().min_w(0.0)
        .child(text(sp.name.clone()).font(SM_MED).color(t.fg).single_line().truncate().help(sp.name.clone()))
        .children(f.local_path.as_ref().map(|p| text(p.clone()).font(MONO11).color(t.muted_fg).single_line().truncate().help(p.clone())));
    let dot = (f.generating > 0).then(|| {
        el().key(k("gen-dot")).size(6.0, 6.0).radius(3.0).fill(t.blue_500).help("Generating").label("Generating")
            .child(el().key(k("gen-ping")).pin(Some(-4.0), Some(-4.0), Some(-4.0), Some(-4.0)).radius(7.0)
                .border(1.0, t.blue_500).looping(LoopSpec::opacity(1.0, 0.0, 0.0, 0.6)))
    });
    let new = (f.new_count > 0).then(|| {
        let (bg, fg) = if t.dark { (t.blue_400.alpha(0.15), t.blue_300) } else { (t.blue_600.alpha(0.1), t.blue_700) };
        row().key(k("new")).px(6.0).py(2.0).radius(9.0).fill(bg)
            .child(text(format!("{} new", f.new_count)).font(T11).color(fg))
    });
    let label = if f.project { "Delete project" } else { "Delete exploration" };
    let delete = icon_btn(k("delete"), label, &TRASH, 28.0, 16.0, t.muted_fg, t, Some(A::DeleteSpace(id.into())))
        .group_opacity(0.6, 1.0);
    row().key(k("header")).w_full().py(10.0).pl(12.0).pr(10.0).gap(8.0).center_y()
        .child(chip).child(names).children(dot).children(new).child(spacer()).child(delete)
}

fn hero(sp: &Space, f: &PortalFacts, pic: Option<Picture>, vw: f32, t: &CardTheme) -> El {
    let id = sp.id.as_str();
    let k = |n: &str| format!("p:{id}:{n}");
    let w = PORTAL_W - 2.0 - 20.0;
    let h = w * 10.0 / 16.0;
    let wrap = col().key(k("hero-wrap")).w_full().px(10.0);
    if f.designs == 0 || (f.hero.is_none() && !f.designing) {
        return wrap.child(
            col().key(k("empty")).w_full().h(h).radius(9.0).border(1.0, t.border).fill(t.muted.alpha(0.3)).center().gap(6.0)
                .child(icon(&FOLDER_TREE, 20.0).color(t.muted_fg))
                .child(text(if f.project { "Empty project" } else { "Empty exploration" }).font(XS).color(t.muted_fg)),
        );
    }
    if f.hero.is_none() {
        return wrap.child(
            el().key(k("designing")).w_full().h(h).radius(9.0).fill(t.muted)
                .border(1.0, t.blue_500.alpha(0.5)).border_dash(4.0, 3.0)
                .looping(LoopSpec::opacity(2.0, 0.0, 0.5, 1.0)).help("Designing…"),
        );
    }
    let b = PreviewBox { w, h, centred: false, transparent: false };
    let mut frame = el().key(k("hero")).w_full().h(h).radius(9.0).clip_rounded(9.0).border(1.0, t.border).fill(t.muted.alpha(0.3))
        .child(thumb(k("hero-thumb"), b, pic, vw, t));
    if f.hero_starred {
        frame = frame.child(
            row().key(k("hero-star")).absolute(w - 8.0 - 18.0, 8.0).size(18.0, 18.0).radius(9.0).fill(t.card)
                .box_shadow(shadow_sm()[0]).center().help("Starred")
                .child(icon(&STAR_FILLED, 10.0).color(t.amber_400)),
        );
    }
    wrap.child(frame)
}

fn footer(sp: &Space, f: &PortalFacts, t: &CardTheme) -> El {
    let id = sp.id.as_str();
    let k = |n: &str| format!("p:{id}:{n}");
    let count = |name: &str, glyph: &'static Icon, n: usize, help: &str| {
        row().key(k(name)).gap(4.0).center_y().help(help.to_string())
            .child(icon(glyph, 12.0).color(t.muted_fg.alpha(0.8)))
            .child(text(n.to_string()).font(XS_NUM_MED).color(t.fg))
    };
    let mut counts = row().key(k("counts")).gap(10.0).center_y()
        .label(summary(f))
        .child(count("n-designs", &LAYOUT_GRID, f.designs, "Designs"));
    if f.starred > 0 {
        counts = counts.child(count("n-starred", &STAR, f.starred, "Starred"));
    }
    if f.promoted > 0 {
        counts = counts.child(count("n-promoted", &ARROW_UP_FROM_LINE, f.promoted, "Promoted"));
    }
    if f.nested > 0 {
        counts = counts.child(count("n-nested", &LAYERS, f.nested, "Nested explorations"));
    }
    let generating = (f.generating > 0).then(|| {
        row().key(k("generating")).px(8.0).py(2.0).radius(9.0).fill(t.muted)
            .child(text(format!("{} generating", f.generating)).font(T11).color(if t.dark { t.blue_400 } else { t.blue_600 }))
    });
    row().key(k("footer")).w_full().py(10.0).pl(14.0).pr(10.0).gap(8.0).center_y()
        .child(counts).child(spacer()).children(generating)
}

/// The counts for screen readers: "N designs, S starred, P promoted, K nested".
fn summary(f: &PortalFacts) -> String {
    let mut s = format!("{} {}", f.designs, if f.designs == 1 { "design" } else { "designs" });
    if f.starred > 0 {
        s += &format!(", {} starred", f.starred);
    }
    if f.promoted > 0 {
        s += &format!(", {} promoted", f.promoted);
    }
    if f.nested > 0 {
        s += &format!(", {} nested", f.nested);
    }
    s
}
