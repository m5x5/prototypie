//! Mention chips, the @/# buttons, the option list under the composer box, and "Flows into generation".

use rune::color::hex;
use rune::{Align, Color, Cx, El, ScrollSpec, Side, TextStyle, col, el, row, text};

use super::{Chip, component_options, kind_label, mentions, options};
use crate::app::state::AppState;
use crate::app::style::{self, Palette};
use crate::features::icons;

const LABEL: TextStyle = TextStyle::new(12.5, 500.0);
const SMALL: TextStyle = TextStyle::new(11.5, 400.0);
const TINY: TextStyle = TextStyle::new(10.5, 600.0);

fn kind_tone(kind: &str) -> Color {
    hex(match kind {
        "resource" => "#0ea5e9",
        "container" => "#f59e0b",
        "shape" => "#10b981",
        "component" => "#d946ef",
        _ => "#8b5cf6",
    })
    .expect("hex")
}

fn access_badge(access: &str, p: &Palette) -> El {
    let (label, c) = match access {
        "live" => ("live", hex("#10b981").expect("hex")),
        "401" => ("401", hex("#f59e0b").expect("hex")),
        "ref" => ("reference", p.muted_fg),
        _ => ("cache", p.muted_fg),
    };
    row().h(16.0).px(6.0).radius(4.0).center().fill(c.alpha(0.15)).child(text(label).font(TINY).color(c))
}

fn chip(key: String, label: String, tone: Color, p: &Palette, remove: Chip) -> El {
    let rm = remove.clone();
    row().key(key.clone()).h(24.0).pl(8.0).pr(4.0).gap(4.0).radius(12.0).center_y().fill(tone.alpha(0.14))
        .child(text(label.clone()).font(SMALL).color(p.fg).truncate())
        .child(row().key(format!("{key}-x")).role("button").label(format!("Remove {label}")).size(18.0, 18.0).center()
            .on_press(move |s: &mut AppState| super::remove(s, &rm)).child(icons::x(10.0, p.muted_fg)))
}

/// Pod and component chips above the text.
pub fn chips(s: &AppState, p: &Palette) -> Option<El> {
    let m = mentions(s).filter(|m| !m.chips.is_empty())?;
    let mut r = row().key("mention-chips").w_full().gap(4.0).flex_wrap();
    for c in &m.chips {
        match c {
            Chip::Pod(id) => {
                let Some(e) = s.features.mention.catalog.as_ref().and_then(|cat| cat.entry(id)) else { continue };
                r = r.child(chip(format!("chip-pod-{id}"), e.label.clone(), kind_tone(&e.kind), p, c.clone()));
            }
            Chip::Component(id) => {
                let Some(d) = s.snap().design(id) else { continue };
                r = r.child(chip(format!("chip-comp-{id}"), d.title.clone(), kind_tone("component"), p, c.clone()));
            }
        }
    }
    Some(r)
}

/// The @ and # buttons for the composer's bottom bar (they don't take focus from the field).
pub fn buttons(s: &AppState, p: &Palette) -> Vec<El> {
    let open = s.features.mention.open.as_ref().map(|o| o.trigger);
    let b = |key: &'static str, label: &'static str, glyph: &'static rune::Icon, trigger: char| {
        row().key(key).role("button").label(label).expanded(open == Some(trigger)).size(26.0, 26.0).radius(6.0).center()
            .preserve_focus_on_press(true)
            .fill(if open == Some(trigger) { p.hover } else { Color::TRANSPARENT }).hover(p.hover).press(p.press)
            .on_press_tx(move |s: &mut AppState, tx| super::open_from_button(s, trigger, tx))
            .child(crate::inspector::icons::draw(glyph, 14.0, p.muted_fg))
    };
    vec![
        b("mention-pod", "Mention pod data · @", &crate::inspector::icons::AT_SIGN, '@'),
        b("mention-comp", "Mention a component · #", &crate::inspector::icons::HASH, '#'),
    ]
}

/// The open list, anchored under "composer-box".
pub fn listbox(s: &AppState, cx: &mut Cx<AppState>, p: &Palette) -> Option<El> {
    let o = s.features.mention.open.as_ref()?;
    let w = cx.layout_size("composer-box").map_or(306.0, |sz| sz[0]);
    let ids = options(s);
    let active = o.active.min(ids.len().saturating_sub(1));
    let used: Vec<String> = mentions(s).map(|m| m.chips.iter().map(|c| match c { Chip::Pod(x) | Chip::Component(x) => x.clone() }).collect()).unwrap_or_default();
    let mut list = col().key("mention-list").w_full().gap(1.0).padding(4.0);
    let row_for = |i: usize, id: &str, label: String, sub: String, right: Option<El>, tone: Color| {
        let pick = id.to_string();
        row().key(format!("mention-opt-{id}")).role("option").label(label.clone()).checked(i == active).w_full().gap(8.0).px(8.0).py(6.0).radius(6.0).center_y()
            .fill(if i == active { p.hover } else { Color::TRANSPARENT }).opacity(if used.contains(&id.to_string()) { 0.55 } else { 1.0 })
            .preserve_focus_on_press(true)
            .on_hover(move |s: &mut AppState| {
                if let Some(o) = &mut s.features.mention.open {
                    o.active = i;
                }
            })
            .on_press_tx(move |s: &mut AppState, tx| super::pick(s, &pick, tx))
            .child(el().size(8.0, 8.0).radius(4.0).fill(tone))
            .child(col().grow(1.0).min_w(0.0).child(text(label).font(LABEL).color(p.fg).truncate()).children((!sub.is_empty()).then(|| text(sub).font(SMALL).color(p.muted_fg).truncate())))
            .children(right)
    };
    if o.trigger == '@' {
        match &s.features.mention.catalog {
            None => list = list.child(text(s.features.mention.catalog_error.clone().unwrap_or_else(|| "Loading pod catalog…".into())).font(SMALL).color(p.muted_fg).px(8.0).py(6.0)),
            Some(cat) => {
                let mut last_kind = "";
                for (i, id) in ids.iter().enumerate() {
                    let Some(e) = cat.entry(id) else { continue };
                    if e.kind != last_kind {
                        last_kind = &e.kind;
                        list = list.child(text(kind_label(&e.kind)).font(TINY).color(p.muted_fg).px(8.0).pt(6.0));
                    }
                    list = list.child(row_for(i, id, e.label.clone(), cat.source_of(e), Some(access_badge(&e.access, p)), kind_tone(&e.kind)));
                }
            }
        }
    } else {
        let opts = component_options(s.snap(), &s.route.space, &o.query);
        let mut shown_header = (false, false);
        for (i, c) in opts.iter().enumerate() {
            let header = if c.component { &mut shown_header.0 } else { &mut shown_header.1 };
            if !*header {
                *header = true;
                list = list.child(text(if c.component { "Components" } else { "Other cards" }).font(TINY).color(p.muted_fg).px(8.0).pt(6.0));
            }
            list = list.child(row_for(i, &c.id, c.title.clone(), c.from.clone().unwrap_or_default(), None, kind_tone(if c.component { "component" } else { "pod" })));
        }
    }
    if ids.is_empty() && (o.trigger == '#' || s.features.mention.catalog.is_some()) {
        list = list.child(text("Nothing matches").font(SMALL).color(p.muted_fg).px(8.0).py(6.0));
    }
    let card = el().key("mention-listbox").role("listbox").w(w).max_h(300.0).radius(10.0).scroll(ScrollSpec { stick_bottom: false }).child(list);
    Some(style::glass(card, p, 0.96).anchored("composer-box", Side::Bottom, Align::Start, 6.0))
}

/// "Flows into generation": each mentioned pod entry with its fields (switchable), children and note.
pub fn context_panel(s: &AppState, p: &Palette) -> Option<El> {
    let cat = s.features.mention.catalog.as_ref()?;
    let m = mentions(s)?;
    let entries: Vec<_> = m.chips.iter().filter_map(|c| if let Chip::Pod(id) = c { cat.entry(id) } else { None }).collect();
    if entries.is_empty() {
        return None;
    }
    let on_fields: usize = entries.iter().map(|e| (0..e.fields.len()).filter(|i| !m.off.contains(&format!("{}:{i}", e.id))).count()).sum();
    let mut c = col().key("pod-context").w_full().gap(10.0).padding(12.0).radius(10.0).border(1.0, p.border)
        .child(row().w_full().justify_between().child(text("Flows into generation").font(LABEL).color(p.fg))
            .child(text(format!("{on_fields} field{}", if on_fields == 1 { "" } else { "s" })).font(SMALL).color(p.muted_fg)));
    for e in entries {
        let mut block = col().w_full().gap(4.0)
            .child(row().w_full().gap(6.0).center_y().child(text(e.label.clone()).font(LABEL).color(p.fg).truncate().grow(1.0).min_w(0.0)).child(access_badge(&e.access, p)))
            .child(text(format!("{}{}", cat.source_of(e), e.stand.as_ref().map(|x| format!(" · {x}")).unwrap_or_default())).font(SMALL).color(p.muted_fg).truncate());
        for (i, f) in e.fields.iter().enumerate() {
            let k = format!("{}:{i}", e.id);
            let on = !m.off.contains(&k);
            let kk = k.clone();
            block = block.child(row().key(format!("pf-{k}")).role("checkbox").label(f.p.clone()).checked(on).w_full().gap(6.0).center_y()
                .on_press(move |s: &mut AppState| super::toggle_field(s, &kk))
                .child(row().size(12.0, 12.0).radius(3.0).border(1.0, p.muted_fg).fill(if on { p.fg } else { Color::TRANSPARENT }))
                .child(text(f.p.clone()).font(SMALL).color(if on { p.fg } else { p.muted_fg }))
                .child(text(f.v.clone().or_else(|| f.ty.clone()).unwrap_or_default()).font(SMALL).color(p.muted_fg).truncate().grow(1.0).min_w(0.0)));
        }
        if let Some(n) = &e.note {
            block = block.child(text(n.clone()).font(SMALL).color(hex("#d97706").expect("hex")).wrap());
        }
        c = c.child(block);
    }
    Some(c)
}

/// The strip under a card's preview for designs made with @-mentions: "N fields from the pod" and one chip per
/// entry (tooltip: source · access · stand). Reads only the design's stored `request.podContext`.
pub fn pod_provenance(d: &proto_core::Design, p: &Palette) -> Option<El> {
    let ctx = d.request.as_ref()?.extra.get("podContext")?.as_array()?;
    if ctx.is_empty() {
        return None;
    }
    let fields: usize = ctx.iter().map(|e| e["fields"].as_array().map_or(0, |f| f.len())).sum();
    let mut chips = row().key(format!("d:{}:pod-chips", d.id)).w_full().gap(4.0).flex_wrap();
    for (i, e) in ctx.iter().enumerate() {
        let label = e["label"].as_str().unwrap_or("").to_string();
        let kind = e["kind"].as_str().unwrap_or("resource");
        let help = format!("{} · {}{}", e["source"].as_str().unwrap_or(""), e["access"].as_str().unwrap_or(""), e["stand"].as_str().map(|x| format!(" · {x}")).unwrap_or_default());
        chips = chips.child(row().key(format!("d:{}:pod-{i}", d.id)).h(20.0).px(6.0).radius(10.0).center_y().fill(kind_tone(kind).alpha(0.14)).help(help)
            .child(text(label).font(SMALL).color(p.fg).truncate()));
    }
    Some(col().key(format!("d:{}:pod", d.id)).w_full().gap(4.0).px(10.0).py(6.0)
        .child(text(format!("{fields} field{} from the pod", if fields == 1 { "" } else { "s" })).font(SMALL).color(p.muted_fg))
        .child(chips))
}
