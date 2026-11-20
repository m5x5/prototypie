//! The "Break into spokes" picker, the "New page" (block of clay) dialog and the inspector's hub/spoke box.

use proto_core::{Design, DesignKind};
use rune::{Color, El, ScrollSpec, TextStyle, col, el, row, text, text_input};

use super::{Analysis, Picker};
use crate::app::state::AppState;
use crate::app::style::{self, Palette};

const TITLE: TextStyle = TextStyle::new(16.0, 600.0);
const BODY: TextStyle = TextStyle::new(13.0, 400.0);
const LABEL: TextStyle = TextStyle::new(12.5, 500.0);
const SMALL: TextStyle = TextStyle::new(11.5, 400.0);
const MONO: TextStyle = TextStyle::new(10.5, 400.0);

fn modal(key: &'static str, card: El, p: &Palette, close: fn(&mut AppState)) -> El {
    el().key(key).pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center().fill(Color::BLACK.alpha(0.45))
        .modal(true, close)
        .child(style::glass(card, p, 0.96))
}

fn button(key: String, label: String, primary: bool, enabled: bool, p: &Palette) -> El {
    let (fill, fg) = if primary { (p.fg, p.canvas_bg) } else { (Color::TRANSPARENT, p.fg) };
    row().key(key).role("button").label(label.clone()).disabled(!enabled).h(32.0).px(12.0).radius(8.0).center()
        .fill(fill).border(1.0, if primary { fill } else { p.border }).opacity(if enabled { 1.0 } else { 0.5 })
        .hover(p.hover).press(p.press)
        .child(text(label).font(LABEL).color(fg))
}

fn field(key: &'static str, edit: &rune::TextEdit, placeholder: &str, p: &Palette, bind: fn(&mut AppState) -> &mut rune::TextEdit) -> El {
    row().key(format!("{key}-box")).w_full().min_h(36.0).px(10.0).py(8.0).radius(8.0).border(1.0, p.border).center_y()
        .child(text_input(edit, BODY).color(p.fg).key(key).label(placeholder.to_string()).placeholder(placeholder.to_string(), p.muted_fg).single_line().w_full().on_edit(bind))
}

fn picker_mut(s: &mut AppState) -> &mut Picker {
    s.features.spokes.picker.as_mut().expect("picker open")
}

pub fn picker(s: &AppState, p: &Palette) -> Option<El> {
    let pk = s.features.spokes.picker.as_ref()?;
    let hub = s.snap().design(&pk.hub_id)?;
    let mut list = col().key("spokes-rows").w_full().gap(4.0);
    match &pk.analysis {
        Analysis::Loading => {
            list = list.child(text("Looking for components…").font(SMALL).color(p.muted_fg));
            for i in 0..4 {
                list = list.child(el().key(format!("spokes-skel-{i}")).w_full().h(48.0).radius(8.0).fill(p.hover));
            }
        }
        Analysis::Error(msg) => {
            list = list.child(row().key("spokes-error").w_full().gap(8.0).padding(10.0).radius(8.0).border(1.0, rune::color::hex("#ef4444").expect("hex").alpha(0.4)).items_start()
                .child(col().grow(1.0).min_w(0.0).gap(2.0)
                    .child(text("Couldn't identify components").font(LABEL).color(rune::color::hex("#ef4444").expect("hex")))
                    .child(text(msg.clone()).font(SMALL).color(p.muted_fg).wrap())
                    .child(text("You can still add components by hand below.").font(SMALL).color(p.muted_fg)))
                .child(button("spokes-retry".into(), "Retry".into(), false, true, p).on_press(super::retry)));
        }
        Analysis::Ready(_) => {}
    }
    for r in pk.rows() {
        let on = pk.checked.get(&r.key).copied().unwrap_or(false);
        let key = r.key.clone();
        let mut rowel = row().key(format!("spoke-{}", r.key)).role("checkbox").label(r.name.clone()).checked(on)
            .w_full().gap(10.0).padding(10.0).radius(8.0).items_start()
            .border(1.0, if on { p.fg.alpha(0.3) } else { p.border }).fill(if on { p.hover } else { Color::TRANSPARENT })
            .on_press(move |s: &mut AppState| {
                let pk = picker_mut(s);
                let v = !pk.checked.get(&key).copied().unwrap_or(false);
                pk.checked.insert(key.clone(), v);
            })
            .child(row().size(16.0, 16.0).mt(2.0).radius(4.0).border(1.0, if on { p.fg } else { p.muted_fg }).fill(if on { p.fg } else { Color::TRANSPARENT }))
            .child(col().grow(1.0).min_w(0.0).gap(2.0)
                .child(text(r.name.clone()).font(LABEL).color(p.fg))
                .child(text(r.description.clone()).font(SMALL).color(p.muted_fg).wrap())
                .children(r.selector.clone().map(|sel| text(sel).font(MONO).color(p.muted_fg.alpha(0.8)).truncate())));
        if r.custom {
            let key = r.key.clone();
            rowel = rowel.child(row().key(format!("spoke-rm-{}", r.key)).role("button").label(format!("Remove {}", r.name)).size(20.0, 20.0).center()
                .on_press(move |s: &mut AppState| {
                    let pk = picker_mut(s);
                    pk.custom.retain(|c| c.key != key);
                    pk.checked.remove(&key);
                })
                .child(crate::features::icons::x(12.0, p.muted_fg)));
        }
        list = list.child(rowel);
    }
    let n = pk.picked().len();
    let add = row().w_full().gap(8.0).center_y()
        .child(el().grow(1.0).min_w(0.0).child(field("spokes-add", &pk.draft, "Add a component, e.g. Pricing card: the middle tier", p, |s| &mut picker_mut(s).draft)))
        .child(button("spokes-add-btn".into(), "Add".into(), false, !pk.draft.text.trim().is_empty(), p).on_press(super::add_custom));
    let mut counts = row().gap(2.0).padding(2.0).radius(8.0).border(1.0, p.border);
    for c in 1..=6u32 {
        let on = pk.count == c;
        counts = counts.child(row().key(format!("spokes-count-{c}")).role("radio").label(c.to_string()).checked(on).size(24.0, 24.0).radius(6.0).center()
            .fill(if on { p.fg } else { Color::TRANSPARENT })
            .on_press(move |s: &mut AppState| picker_mut(s).count = c)
            .child(text(c.to_string()).font(SMALL).color(if on { p.canvas_bg } else { p.muted_fg })));
    }
    let go_label = if n == 0 { "Explore spokes".to_string() } else { format!("Explore {n} spoke{} · {} designs", if n == 1 { "" } else { "s" }, n as u32 * pk.count) };
    let footer = row().w_full().justify_between().center_y()
        .child(row().gap(8.0).center_y().child(text("Per spoke").font(SMALL).color(p.muted_fg)).child(counts))
        .child(if n > 0 { button("spokes-go".into(), go_label.clone(), true, true, p).on_press(super::launch_picked) } else { button("spokes-go".into(), go_label, true, false, p) });
    let card = col().key("spokes-dialog").role("dialog").label("Break into spokes").w(560.0).gap(14.0).padding(20.0).radius(16.0)
        .child(col().gap(4.0)
            .child(text("Break into spokes").font(TITLE).color(p.fg))
            .child(text(format!("Pick the pieces of {} to iterate on in isolation. Each one runs as its own parallel lane.", hub.title)).font(BODY).color(p.muted_fg).wrap()))
        .child(el().key("spokes-scroll").w_full().max_h(360.0).scroll(ScrollSpec { stick_bottom: false }).child(list))
        .child(add)
        .child(col().w_full().gap(6.0)
            .child(text("Direction for every spoke (optional)").font(SMALL).color(p.muted_fg))
            .child(field("spokes-direction", &pk.direction, "e.g. more editorial, try bolder type", p, |s| &mut picker_mut(s).direction)))
        .child(footer);
    Some(modal("spokes-modal", card, p, super::close_picker))
}

pub fn clay(s: &AppState, p: &Palette) -> Option<El> {
    let e = s.features.spokes.clay.as_ref()?;
    let input = text_input(e, BODY).color(p.fg).key("clay-brief").label("Brief")
        .placeholder("e.g. Landing page for a climbing gym: hero, class schedule, pricing tiers, coach profiles, FAQ", p.muted_fg)
        .w_full().min_h(110.0)
        .on_edit(|s: &mut AppState| s.features.spokes.clay.as_mut().expect("clay open"))
        .on_input_key(|s: &mut AppState, ev, _| {
            if ev.key == rune::Key::Enter && ev.mods.cmd {
                super::submit_clay(s);
                return true;
            }
            false
        });
    let ok = !e.text.trim().is_empty();
    let card = col().key("clay-dialog").role("dialog").label("New page").w(520.0).gap(14.0).padding(20.0).radius(16.0)
        .child(col().gap(4.0)
            .child(text("New page").font(TITLE).color(p.fg))
            .child(text("Get a rough full-page prototype to break apart into spokes. Don't over-specify; it's clay.").font(BODY).color(p.muted_fg).wrap()))
        .child(el().w_full().padding(10.0).radius(8.0).border(1.0, p.border).child(input))
        .child(row().w_full().justify_end().gap(8.0)
            .child(button("clay-cancel".into(), "Cancel".into(), false, true, p).on_press(super::close_clay))
            .child(if ok { button("clay-go".into(), "Generate page ⌘↵".into(), true, true, p).on_press(super::submit_clay) } else { button("clay-go".into(), "Generate page ⌘↵".into(), true, false, p) }));
    Some(modal("clay-modal", card, p, super::close_clay))
}

/// The inspector box: "Hub" with "Break into spokes" for any non-spoke design; "Spoke" with its focus and
/// "Merge back into …" (with an optional note) for a spoke whose hub exists.
pub fn inspector_box(s: &AppState, p: &Palette, d: &Design) -> Option<El> {
    let snap = s.snap();
    if d.kind != DesignKind::Spoke {
        let spokes = snap.children_of(&d.id).iter().filter(|c| c.kind == DesignKind::Spoke).count();
        let id = d.id.clone();
        let mut head = row().gap(6.0).center_y()
            .child(crate::inspector::icons::draw(&crate::cards::icons::BOXES, 14.0, p.fg))
            .child(text(if d.kind == DesignKind::Page { "Hub page" } else { "Hub" }).font(LABEL).color(p.fg));
        if spokes > 0 {
            head = head.child(text(format!("· {spokes} spoke designs")).font(SMALL).color(p.muted_fg));
        }
        // Filled muted button with the spokes icon (the reference's `secondary sm`).
        let btn = row().key("hub-break").role("button").label("Break into spokes").disabled(!d.is_ready()).w_full().h(28.0).gap(6.0).radius(8.0).center()
            .fill(p.hover).opacity(if d.is_ready() { 1.0 } else { 0.5 }).press(p.press)
            .child(crate::inspector::icons::draw(&crate::cards::icons::BOXES, 14.0, p.fg))
            .child(text("Break into spokes").font(LABEL).color(p.fg));
        // CSS lays the 1px border out inside p-3; Rune doesn't, hence 13.
        return Some(col().key("hub-box").w_full().gap(8.0).padding(13.0).radius(10.0).border(1.0, p.border).fill(p.hover)
            .child(head)
            .child(text("Break it into components and explore each one in its own parallel lane.").font(SMALL).color(p.muted_fg).wrap())
            .child(if d.is_ready() { btn.on_press(move |s: &mut AppState| super::open_picker(s, &id)) } else { btn }));
    }
    let mut c = col().key("spoke-box").w_full().gap(8.0).padding(13.0).radius(10.0).border(1.0, p.border).fill(p.hover)
        .child(row().gap(6.0).center_y().child(crate::inspector::icons::draw(&crate::cards::icons::BOXES, 14.0, p.fg)).child(text("Spoke").font(LABEL).color(p.fg)));
    if let Some(f) = super::focus_of(s, &d.id) {
        c = c.child(text(f).font(SMALL).color(p.muted_fg).wrap());
    }
    if let Some(hub) = snap.hub_of(&d.id) {
        let id = d.id.clone();
        let empty = rune::TextEdit::default();
        let note = s.features.spokes.merge_notes.get(&d.id).unwrap_or(&empty);
        let bind_id = d.id.clone();
        let input = text_input(note, SMALL).color(p.fg).key(format!("merge-note-{}", d.id)).label("Merge note")
            .placeholder("Optional note, e.g. only merge the steps", p.muted_fg).w_full()
            .on_edit(move |s: &mut AppState| s.features.spokes.merge_notes.entry(bind_id.clone()).or_default())
            .on_input_key({
                let id = d.id.clone();
                move |s: &mut AppState, ev, _| {
                    if ev.key == rune::Key::Enter && ev.mods.cmd {
                        super::merge_into_hub(s, &id);
                        return true;
                    }
                    false
                }
            });
        let ok = d.is_ready() && hub.is_ready();
        let btn = button(format!("merge-{}", d.id), format!("Merge back into “{}”", hub.title), false, ok, p).w_full();
        c = c.child(el().w_full().padding(8.0).radius(8.0).border(1.0, p.border).child(input))
            .child(if ok { btn.on_press(move |s: &mut AppState| super::merge_into_hub(s, &id)) } else { btn });
    }
    Some(c)
}
