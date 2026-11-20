//! The Annotate layer (spec 01 §3–§13): capture surface, hover outline and chip, selection outlines, pins, the
//! note popover, a note's thread, the list, the settings and the toolbar, bottom to top (AN-UI-106).

use rune::color::hex;
use rune::{Color, Cursor, Cx, El, Phase, TextStyle, col, el, icon, row, text, text_input};

use super::icons;
use super::logic::{self, Conn};
use super::model::{Note, Status, Target};
use super::place::{self, PANEL_W, TOOLBAR, TOOLBAR_HOME};
use super::{AnnotateState, UI_PREFIX};
use crate::app::state::{AppState, ThemePref};
use crate::app::style::{self, Palette};

const SMALL: TextStyle = TextStyle::new(11.0, 500.0);
const BODY: TextStyle = TextStyle::new(12.5, 400.0);
const LABEL: TextStyle = TextStyle::new(12.5, 600.0);
const CHIP: TextStyle = TextStyle::new(11.0, 600.0);
const DIGIT: TextStyle = TextStyle::new(10.5, 700.0);

fn c(s: &str) -> Color {
    hex(s).expect("hex")
}

fn blue() -> Color {
    c("#3b82f6")
}

fn green() -> Color {
    c("#10b981")
}

fn ui_key(name: &str) -> String {
    format!("{UI_PREFIX}-{name}")
}

/// Live web views drawn over everything: fullscreen, compare, review (AN-UI-032).
fn live_rects(s: &AppState) -> Vec<[f32; 4]> {
    [s.fullscreen.live().shown(), s.features.compare.live.shown(), s.features.review.live.shown()]
        .into_iter().flatten().map(|(_, _, r)| r).filter(|r| r[2] > 0.0 && r[3] > 0.0).collect()
}

/// Everything panels must keep clear of: the live views, and the fullscreen view's own controls while it's open.
fn taken(s: &AppState, cx: &Cx<AppState>, live: &[[f32; 4]]) -> Vec<[f32; 4]> {
    let mut t = live.to_vec();
    if let Some(id) = crate::fullscreen::open_id(s) {
        let keys = [
            "fs:header".to_string(),
            "fs:close".to_string(),
            "fs:preview".to_string(),
            format!("fs:neighbours:{id}"),
            format!("fs:note-box:{id}"),
            format!("edit-box-{id}"),
            "notes-panel".to_string(),
            format!("fs:rationale:{id}"),
        ];
        for k in keys {
            if let Some(r) = cx.window_layout(&k).filter(|r| r[2] > 0.0 && r[3] > 0.0) {
                t.push(r);
            }
        }
    }
    t
}

/// A key's rect: as last drawn, else as laid out.
fn key_rect(a: &AnnotateState, cx: &Cx<AppState>, key: &str) -> Option<[f32; 4]> {
    a.drawn_keys.get(key).copied().or_else(|| cx.layout(key))
}

/// The rect a note's pin sits on: its first target is a native one with a key whose rect is known.
fn pin_rect(a: &AnnotateState, cx: &Cx<AppState>, n: &Note) -> Option<[f32; 4]> {
    let t = n.targets.first().filter(|t| !t.is_web())?;
    key_rect(a, cx, t.key.as_deref()?)
}

pub fn layer(s: &AppState, cx: &mut Cx<AppState>, _shell: &Palette) -> Vec<El> {
    let a = &s.features.annotate;
    let dark = match s.theme {
        ThemePref::System => cx.system_dark(),
        ThemePref::Dark => true,
        ThemePref::Light => false,
    };
    a.light.set(!dark);
    let p = style::palette(dark);
    let win = cx.layout_size("root").filter(|w| w[0] > 0.0 && w[1] > 0.0).unwrap_or([1280.0, 800.0]);
    a.window.set(win);
    let live = live_rects(s);
    let taken = taken(s, cx, &live);
    let space = s.route.space.as_str();
    let mut out = Vec::new();

    if a.on {
        out.push(capture());
        if let Some(h) = &a.hover {
            let krect = h.key.as_deref().and_then(|k| key_rect(a, cx, k));
            a.key_rect.set(krect);
            let rect = a.drawn_paths.get(&h.path).copied().or(krect);
            if let (Some(r), None) = (rect, &a.draft) {
                out.push(outline("annotate-hover", r, blue(), 1.5, 0.08));
                let src = h.key.as_deref().and_then(super::source::find).map(|(s, _)| s);
                out.push(chip(&logic::chip_text(&logic::key_path(&h.path), src.as_deref()), r, win));
            }
        } else {
            a.key_rect.set(None);
        }
        for (i, t) in a.selection.iter().enumerate() {
            out.push(outline(&format!("annotate-selected-{i}"), t.rect, green(), 2.0, 0.12));
        }
    }
    if a.on || a.list_open {
        for (num, n) in logic::numbered(&a.notes, space) {
            let Some(r) = pin_rect(a, cx, n) else { continue };
            let at = [r[0] + r[2] - 10.0, r[1] - 10.0, 20.0, 20.0];
            if !place::under_any(at, &live) {
                out.push(pin(n, num, at));
            }
        }
    }
    if let (Some(d), Some(last)) = (&a.draft, a.selection.last()) {
        let at = place::at_anchor(last.rect, 170.0, &taken, win);
        out.push(popover(a, d, last, &p, at));
    }
    if let Some(n) = a.open_note.as_deref().and_then(|id| a.note(id)) {
        let h = 220.0 + 50.0 * n.thread.len() as f32;
        let anchor = pin_rect(a, cx, n).unwrap_or([16.0, win[1] - 300.0, 0.0, 0.0]);
        let at = place::at_anchor(anchor, h, &taken, win);
        out.push(detail(a, n, logic::number_of(&a.notes, &n.id, space), &p, at));
    }
    let wish = a.drag.or_else(|| super::saved_pos(s)).unwrap_or(TOOLBAR_HOME);
    let bar = place::free_spot(wish, TOOLBAR, &taken, win);
    a.toolbar_at.set(bar);
    let open = logic::numbered(&a.notes, space);
    if a.list_open {
        let h = (90.0 + open.len().min(8) as f32 * 46.0).min(win[1] - 32.0);
        let at = place::by_toolbar(bar, h, &taken, win);
        out.push(list(a, &open, h, &p, at));
    }
    if a.settings_open {
        let at = place::by_toolbar(bar, 130.0, &taken, win);
        out.push(settings(a, &p, at));
    }
    let ep = super::endpoint(s);
    out.push(toolbar(a, open.len(), &ep, &p, bar));
    out
}

// ---- capture, outlines, chip, pins ----

fn capture() -> El {
    el().key("annotate-capture").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(Color::TRANSPARENT)
        .cursor(Cursor::Crosshair)
        .on_pointer_move_local(|_s: &mut AppState, e, tx| super::pointer_moved(tx, e.window_x, e.window_y))
        .press_on_down()
        .on_background_press_tx(|s: &mut AppState, tx| {
            let shift = tx.mods().shift;
            super::click(s, shift);
        })
}

fn outline(key: &str, r: [f32; 4], color: Color, width: f32, fill: f32) -> El {
    el().key(key.to_string()).pin(Some(r[0] - 2.0), Some(r[1] - 2.0), None, None).size(r[2] + 4.0, r[3] + 4.0)
        .radius(4.0).border(width, color).fill(color.alpha(fill)).inert()
}

fn chip(label: &str, r: [f32; 4], win: [f32; 2]) -> El {
    let (top, bottom) = (r[1], r[1] + r[3]);
    let y = if top > 28.0 { top - 26.0 } else { (bottom + 6.0).min(win[1] - 24.0) };
    let x = r[0].clamp(4.0, (win[0] - 320.0).max(4.0));
    row().key("annotate-chip").pin(Some(x), Some(y), None, None).h(20.0).px(7.0).radius(5.0).center_y().max_w(420.0)
        .fill(blue())
        .child(text(label.to_string()).font(CHIP).color(Color::WHITE).truncate())
        .inert()
}

fn pin(n: &Note, num: u32, at: [f32; 4]) -> El {
    let id = n.id.clone();
    row().key(ui_key(&format!("pin-{}", n.id))).role("button").label(format!("Note {num}: {}", n.comment))
        .pin(Some(at[0]), Some(at[1]), None, None).size(20.0, 20.0).radius(10.0).center()
        .fill(c(logic::status_hex(n.status))).border(1.5, Color::WHITE)
        .on_press(move |s: &mut AppState| {
            let a = &mut s.features.annotate;
            a.open_note = if a.open_note.as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) };
            a.reply = rune::TextEdit::default();
        })
        .child(text(num.to_string()).font(DIGIT).color(Color::WHITE))
}

// ---- shared parts (§12) ----

fn card(e: El, p: &Palette, at: [f32; 2]) -> El {
    style::glass(e.pin(Some(at[0]), Some(at[1]), None, None).w(PANEL_W).padding(12.0).gap(8.0).radius(12.0).role("dialog"), p, 0.94)
}

fn location(t: &Target, p: &Palette) -> El {
    col().gap(2.0).min_w(0.0).w_full()
        .child(text(t.element.clone()).font(LABEL).color(p.fg).truncate())
        .children(logic::location_line(t).map(|l| text(l).font(SMALL).color(p.muted_fg).truncate()))
}

fn primary(key: &str, label: &str, enabled: bool) -> El {
    row().key(ui_key(key)).role("button").label(label.to_string()).h(28.0).px(12.0).radius(8.0).center()
        .fill(if enabled { blue() } else { blue().alpha(0.4) }).press(Color::BLACK.alpha(0.15))
        .child(text(label.to_string()).font(LABEL).color(Color::WHITE))
}

fn secondary(key: &str, label: &str, p: &Palette) -> El {
    row().key(ui_key(key)).role("button").label(label.to_string()).h(28.0).px(10.0).radius(8.0).center()
        .hover(p.hover).press(p.press)
        .child(text(label.to_string()).font(LABEL).color(p.fg))
}

fn bubble(num: u32, status: Status) -> El {
    row().size(18.0, 18.0).radius(9.0).center().fill(c(logic::status_hex(status)))
        .child(text(num.to_string()).font(DIGIT).color(Color::WHITE))
}

// ---- the note popover (§5) ----

fn popover(a: &AnnotateState, d: &rune::TextEdit, last: &Target, p: &Palette, at: [f32; 2]) -> El {
    let n = a.selection.len();
    let input = text_input(d, BODY).color(p.fg).key("annotate-note-input").label("Note")
        .placeholder("What should change?", p.muted_fg)
        .w_full().min_h(56.0).padding(8.0).radius(8.0).border(1.0, p.border).fill(p.hover)
        .on_edit(|s: &mut AppState| s.features.annotate.draft.get_or_insert_with(rune::TextEdit::default))
        .on_input_key(|s: &mut AppState, ev, _| match ev.key {
            rune::Key::Enter if !ev.mods.shift => {
                super::save_draft(s);
                true
            }
            rune::Key::Escape => {
                super::cancel_draft(s);
                true
            }
            _ => false,
        });
    let can = !d.text.trim().is_empty();
    let save = primary("note-save", "Save", can);
    let save = if can { save.on_press(super::save_draft) } else { save };
    card(col().key(ui_key("popover")).label("New note"), p, at)
        .child(location(last, p))
        .children((n > 1).then(|| text(format!("{n} elements (⇧-click to add or remove)")).font(SMALL).color(p.muted_fg)))
        .child(input)
        .child(row().w_full().justify_end().gap(6.0)
            .child(secondary("note-cancel", "Cancel", p).on_press(super::cancel_draft))
            .child(save))
}

// ---- a note's thread (§11) ----

fn detail(a: &AnnotateState, n: &Note, num: u32, p: &Palette, at: [f32; 2]) -> El {
    let mut c = card(col().key(ui_key("thread")).label(format!("Note {num}")), p, at)
        .child(row().w_full().gap(8.0).center_y()
            .child(bubble(num, n.status))
            .child(text(n.status.word()).font(SMALL).color(p.muted_fg)))
        .child(text(n.comment.clone()).font(BODY).color(p.fg).wrap());
    if let Some(t) = n.targets.first() {
        c = c.child(location(t, p));
    }
    for (i, m) in n.thread.iter().enumerate() {
        let agent = m.role == "agent";
        c = c.child(col().key(ui_key(&format!("message-{i}"))).w_full().padding(8.0).radius(8.0).gap(2.0)
            .fill(if agent { blue().alpha(0.12) } else { p.hover })
            .child(text(if agent { "Agent" } else { "You" }).font(SMALL).color(p.muted_fg))
            .child(text(m.content.clone()).font(BODY).color(p.fg).wrap()));
    }
    let reply = text_input(&a.reply, BODY).color(p.fg).key("annotate-reply").label("Reply")
        .placeholder("Reply to the agent…", p.muted_fg)
        .w_full().padding(8.0).radius(8.0).border(1.0, p.border)
        .on_edit(|s: &mut AppState| &mut s.features.annotate.reply)
        .on_input_key(|s: &mut AppState, ev, _| {
            if ev.key == rune::Key::Enter && !ev.mods.shift {
                super::send_reply(s);
                return true;
            }
            false
        });
    let (del, res) = (n.id.clone(), n.id.clone());
    c.child(reply)
        .child(row().w_full().justify_end().gap(6.0)
            .child(secondary("thread-delete", "Delete", p).on_press(move |s: &mut AppState| super::set_status(s, &del, Status::Dismissed)))
            .child(primary("thread-resolve", "Resolve", true).on_press(move |s: &mut AppState| super::set_status(s, &res, Status::Resolved))))
}

// ---- the list (§10) ----

fn list(a: &AnnotateState, open: &[(u32, &Note)], h: f32, p: &Palette, at: [f32; 2]) -> El {
    let head = row().w_full().justify_between().center_y()
        .child(text(format!("Notes · {}", open.len())).font(LABEL).color(p.fg))
        .child(text(logic::list_status(a.reach)).font(SMALL).color(p.muted_fg));
    let mut rows = col().key(ui_key("list-rows")).w_full().gap(2.0).max_h((h - 60.0).max(0.0))
        .scroll(rune::ScrollSpec { stick_bottom: false });
    for (num, n) in open {
        let sub = match n.targets.first() {
            Some(t) if t.is_web() => {
                let d = t.design_id.clone().unwrap_or_default();
                if a.missing_web.contains(&n.id) { format!("design {d} · not on this page") } else { format!("design {d}") }
            }
            Some(t) => t.source.clone().unwrap_or_else(|| t.element_path.clone()),
            None => String::new(),
        };
        let id = n.id.clone();
        rows = rows.child(row().key(ui_key(&format!("row-{}", n.id))).role("button").label(n.comment.clone())
            .w_full().min_w(0.0).px(6.0).py(5.0).radius(8.0).gap(8.0).items_start()
            .hover(p.hover).press(p.press)
            .on_press(move |s: &mut AppState| {
                let a = &mut s.features.annotate;
                a.open_note = Some(id.clone());
                a.reply = rune::TextEdit::default();
            })
            .child(bubble(*num, n.status))
            .child(col().grow(1.0).min_w(0.0)
                .child(text(n.comment.clone()).font(BODY).color(p.fg).truncate())
                .child(text(sub).font(SMALL).color(p.muted_fg).truncate())));
    }
    let mut c = card(col().key(ui_key("list")).label("Notes"), p, at).child(head);
    if open.is_empty() {
        c = c.child(text("No open notes here. Turn on annotate (⌘⇧A) and click an element.").font(SMALL).color(p.muted_fg).wrap());
    }
    c.child(rows)
}

// ---- settings (§13) ----

fn settings(a: &AnnotateState, p: &Palette, at: [f32; 2]) -> El {
    let field = text_input(&a.endpoint_edit, BODY).color(p.fg).key("annotate-endpoint").label("Server URL")
        .w_full().padding(8.0).radius(8.0).border(1.0, p.border)
        .on_edit(|s: &mut AppState| &mut s.features.annotate.endpoint_edit)
        .on_input_key(|s: &mut AppState, ev, _| {
            if ev.key == rune::Key::Enter {
                super::save_endpoint(s);
                return true;
            }
            false
        });
    card(col().key(ui_key("settings")).label("Annotate settings"), p, at)
        .child(text("Notes service").font(LABEL).color(p.fg))
        .child(field)
        .child(row().w_full().justify_between().center_y()
            .child(text(format!("Default {}", logic::DEFAULT_ENDPOINT)).font(SMALL).color(p.muted_fg))
            .child(primary("settings-save", "Save", true).on_press(super::save_endpoint)))
}

// ---- the toolbar (§3) ----

fn round_button(key: &str, label: &str, glyph: &'static rune::Icon, active: bool, p: &Palette) -> El {
    row().key(ui_key(key)).role("button").label(label.to_string()).size(30.0, 30.0).radius(15.0).center()
        .fill(if active { blue() } else { Color::TRANSPARENT })
        .hover(if active { blue() } else { p.hover }).press(p.press)
        .child(icon(glyph, 15.0).color(if active { Color::WHITE } else { p.fg }))
}

fn toolbar(a: &AnnotateState, open: usize, endpoint: &str, p: &Palette, at: [f32; 2]) -> El {
    let grip = row().key(ui_key("grip")).role("button").label("Move toolbar").size(16.0, 30.0).center().cursor(Cursor::Grab)
        .child(icon(&icons::GRIP_VERTICAL, 14.0).color(p.muted_fg))
        .on_drag(|s: &mut AppState, d, _tx| {
            let a = &mut s.features.annotate;
            match d.phase {
                Phase::Down => {
                    a.drag_origin = a.toolbar_at.get();
                    a.drag = Some(a.drag_origin);
                }
                Phase::Move => a.drag = Some([a.drag_origin[0] + d.dx, a.drag_origin[1] + d.dy]),
                Phase::Up => {
                    let at = a.drag.take().unwrap_or(a.drag_origin);
                    super::save_pos(s, at);
                }
            }
        });
    let toggle = round_button("toggle", if a.on { "Stop annotating (⌘⇧A)" } else { "Annotate (⌘⇧A)" }, &icons::MOUSE_POINTER_CLICK, a.on, p)
        .expanded(a.on)
        .on_press(super::toggle);
    let list = row().key(ui_key("list-button")).role("button").label(format!("Notes: {open} open")).expanded(a.list_open)
        .h(30.0).px(8.0).gap(4.0).radius(15.0).center_y()
        .fill(if a.list_open { p.hover } else { Color::TRANSPARENT }).hover(p.hover).press(p.press)
        .on_press(|s: &mut AppState| {
            let a = &mut s.features.annotate;
            a.list_open = !a.list_open;
            a.settings_open = false;
        })
        .child(icon(&icons::LIST, 15.0).color(p.fg))
        .child(text(open.to_string()).font(LABEL).color(p.fg));
    let copied = a.copied_until > a.clock;
    let copy = round_button("copy", if copied { "Copied" } else { "Copy notes as markdown" }, if copied { &icons::CHECK } else { &icons::COPY }, false, p)
        .on_press_tx(super::copy_markdown);
    let gear = round_button("settings-button", "Annotate settings", &icons::SETTINGS, a.settings_open, p)
        .on_press(|s: &mut AppState| {
            let a = &mut s.features.annotate;
            a.settings_open = !a.settings_open;
            a.list_open = false;
        });
    let state = logic::conn(a.reach, logic::waiting(&a.notes), a.clock, a.last_ok, a.active());
    let col_ = c(logic::conn_hex(state));
    let label = logic::conn_text(state, endpoint);
    let count = match state {
        Conn::Offline(k) | Conn::Syncing(k) if k > 0 => Some(k),
        _ => None,
    };
    let conn = row().key(ui_key("connection")).role("status").label(label.clone()).help(label)
        .h(30.0).px(6.0).gap(4.0).center_y()
        .child(el().size(8.0, 8.0).radius(4.0).fill(col_))
        .children(matches!(state, Conn::Offline(_) | Conn::Unreachable).then(|| icon(&icons::WIFI_OFF, 13.0).color(col_)))
        .children(count.map(|k| text(k.to_string()).font(DIGIT).color(col_)));
    style::glass(row().key(ui_key("toolbar")).role("toolbar").label("Annotate")
        .pin(Some(at[0]), Some(at[1]), None, None).h(40.0).pl(4.0).pr(6.0).gap(2.0).radius(20.0).center_y(), p, 0.88)
        .child(grip)
        .child(toggle)
        .child(list)
        .child(copy)
        .child(gear)
        .child(conn)
}
