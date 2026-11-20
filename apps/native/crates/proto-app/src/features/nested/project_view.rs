//! The "New project" dialog's look: a centred modal with a search combobox and the project list.

use rune::{Color, El, ScrollSpec, TextStyle, col, el, row, text, text_input};

use super::project::{self, Phase};
use crate::app::state::AppState;
use crate::app::style::{self, Palette};

const TITLE: TextStyle = TextStyle::new(16.0, 600.0);
const BODY: TextStyle = TextStyle::new(13.0, 400.0);
const ROW: TextStyle = TextStyle::new(13.5, 500.0);
const SUB: TextStyle = TextStyle::new(12.0, 400.0);

pub fn view(s: &AppState, p: &Palette) -> Option<El> {
    let d = &s.features.project;
    if !d.is_open() {
        return None;
    }
    let busy = matches!(d.phase, Phase::Creating);
    let input = text_input(&d.query, BODY).color(p.fg)
        .key("project-search")
        .label("Search projects")
        .placeholder("Search projects, or type a name or folder path…", p.muted_fg)
        .single_line()
        .w_full()
        .on_edit(|s: &mut AppState| &mut s.features.project.query)
        .on_input_change(|s: &mut AppState, _| s.features.project.active = 0)
        .on_input_key(project::on_key);
    let field = row().key("project-search-box").h(40.0).px(12.0).radius(10.0).center_y()
        .border(1.0, p.border).fill(p.hover).child(input);

    let options = d.options();
    let active = d.active.min(options.len().saturating_sub(1));
    let mut list = col().key("project-options").w_full().min_w(0.0).gap(2.0);
    for (i, opt) in options.iter().enumerate() {
        let (label, sub) = match opt {
            Some(pr) => {
                let mut parts = pr.keywords.clone();
                if parts.is_empty() {
                    parts.push("No keywords".to_string());
                }
                // Where it comes from, after the keywords.
                parts.extend(pr.source.clone());
                (pr.name.clone(), parts.join(" · "))
            }
            None => {
                let (name, path) = super::from_query(&d.query.text);
                let sub = path.unwrap_or_else(|| "Type a name or an absolute folder path".into());
                (if d.query.text.trim().is_empty() { "New project".into() } else { format!("New project „{name}“") }, sub)
            }
        };
        list = list.child(row().key(format!("project-opt-{i}")).role("option").label(label.clone())
            .w_full().min_w(0.0).gap(10.0).px(10.0).py(8.0).radius(8.0).items_start()
            .fill(if i == active { p.press } else { Color::TRANSPARENT })
            .hover(p.hover)
            .on_hover(move |s: &mut AppState| s.features.project.active = i)
            .on_press_tx(move |s: &mut AppState, tx| project::choose(s, i, tx))
            .child(col().grow(1.0).min_w(0.0).gap(2.0)
                .child(text(label).font(ROW).color(p.fg).truncate())
                .child(text(sub).font(SUB).color(p.muted_fg).truncate())));
    }
    match &d.list {
        None => list = list.child(row().px(10.0).py(8.0).child(text("Loading projects…").font(SUB).color(p.muted_fg))),
        Some(l) if !l.available => {
            list = list.child(row().px(10.0).py(8.0)
                .child(text("No project list found (private-projects.ttl). You can still start a new project.").font(SUB).color(p.muted_fg).wrap()));
        }
        _ => {}
    }

    let card = col().key("project-dialog").role("dialog").label("New project").w(520.0).gap(14.0).padding(20.0).radius(16.0)
        .child(col().gap(4.0)
            .child(text("New project").font(TITLE).color(p.fg))
            .child(text("Pick a project: it gets its name, folder and a project info card.").font(BODY).color(p.muted_fg).wrap()))
        .child(field)
        .child(el().key("project-scroll").w_full().max_h(380.0).scroll(ScrollSpec { stick_bottom: false }).child(list))
        .opacity(if busy { 0.6 } else { 1.0 });
    let scrim = el().key("project-scrim").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).fill(Color::BLACK.alpha(0.45))
        .on_press(project::close);
    Some(el().key("project-modal").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center()
        .child(scrim)
        .child(style::glass(card, p, 0.96)))
}
