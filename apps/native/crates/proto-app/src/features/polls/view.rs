//! The Decisions popover (from the ⋯ menu) and the Decisions panel: Build poll (tray, agent, decision editors,
//! publish) and Results (published polls, votes, winners).

use proto_core::PublishedPoll;
use rune::color::hex;
use rune::{Color, El, ScrollSpec, TextEdit, TextStyle, col, el, row, text, text_input};

use super::{DraftDecision, DraftOption, PollsState, Tab};
use crate::app::insets::HEADER_H;
use crate::app::state::AppState;
use crate::app::style::{self, Palette};
use crate::cards::preview::{self, Picture};
use crate::cards::theme::CardTheme;
use crate::features::icons;

const H1: TextStyle = TextStyle::new(17.0, 600.0);
const TITLE: TextStyle = TextStyle::new(14.0, 600.0);
const BODY: TextStyle = TextStyle::new(13.0, 400.0);
const LABEL: TextStyle = TextStyle::new(12.5, 500.0);
const SMALL: TextStyle = TextStyle::new(11.5, 400.0);
const LETTERS: [&str; 4] = ["A", "B", "C", "D"];

/// Pins a closure to the higher-ranked `for<'a> Fn(&'a mut AppState) -> &'a mut TextEdit` (text-field binders).
fn hr<F: Fn(&mut AppState) -> &mut TextEdit>(f: F) -> F {
    f
}

fn pl(s: &mut AppState) -> &mut PollsState {
    &mut s.features.polls
}

fn button(key: String, label: String, primary: bool, enabled: bool, p: &Palette) -> El {
    let (fill, fg) = if primary { (p.fg, p.canvas_bg) } else { (Color::TRANSPARENT, p.fg) };
    row().key(key).role("button").label(label.clone()).disabled(!enabled).h(30.0).px(12.0).radius(8.0).center()
        .fill(fill).border(1.0, if primary { fill } else { p.border }).opacity(if enabled { 1.0 } else { 0.5 })
        .hover(p.hover).press(p.press).child(text(label).font(LABEL).color(fg))
}

fn icon_btn(key: String, label: &str, glyph: El, enabled: bool, p: &Palette) -> El {
    row().key(key).role("button").label(label.to_string()).disabled(!enabled).size(26.0, 26.0).radius(6.0).center()
        .opacity(if enabled { 1.0 } else { 0.35 }).hover(p.hover).press(p.press).child(glyph)
}

fn field(key: String, edit: &TextEdit, placeholder: &str, font: TextStyle, p: &Palette, bind: impl Fn(&mut AppState) -> &mut TextEdit + 'static) -> El {
    row().key(format!("{key}-box")).w_full().min_h(32.0).px(8.0).py(6.0).radius(6.0).border(1.0, p.border).center_y()
        .child(text_input(edit, font).color(p.fg).key(key).label(placeholder.to_string()).placeholder(placeholder.to_string(), p.muted_fg).single_line().w_full().on_edit(bind))
}

fn picture(s: &AppState, id: &str, w: f32, t: &CardTheme, key: String) -> El {
    let h = w * 0.75;
    let Some(d) = s.snap().design(id) else {
        return col().key(key).size(w, h).center().fill(t.muted).child(text("Design deleted").font(SMALL).color(t.muted_fg));
    };
    let pic = s.cards.thumbs.get(id).map(|sn| Picture { pixels: &sn.pixels, rect: sn.rect, cropped: sn.crop.is_some() });
    preview::thumb(key, preview::PreviewBox { w, h, centred: false, transparent: false }, pic, d.viewport.unwrap_or(1280.0) as f32, t)
}

/// A read-only share link with Copy.
fn share_link(key: &str, url: &str, p: &Palette) -> El {
    let u = url.to_string();
    row().key(format!("{key}-share")).w_full().gap(6.0).center_y()
        .child(row().grow(1.0).min_w(0.0).h(28.0).px(8.0).radius(6.0).border(1.0, p.border).center_y()
            .child(text(url.to_string()).font(SMALL).color(p.fg).truncate()))
        .child(button(format!("{key}-copy"), "Copy".into(), false, true, p).on_press_tx(move |_: &mut AppState, tx| tx.copy(u.clone())))
}

/* ------------------------------------------------------------------ popover */

pub fn dock(s: &AppState, p: &Palette) -> Option<El> {
    let pls = &s.features.polls;
    if !pls.dock_open {
        return None;
    }
    let titles: Vec<String> = pls.candidates.iter().filter_map(|id| s.snap().design(id)).map(|d| d.title.clone()).collect();
    let n = titles.len();
    let latest = super::published(s).into_iter().next();
    let mut body = col().w_full().gap(10.0).padding(14.0);
    if let Some(l) = &latest {
        let id = l.id.clone();
        body = body.child(col().w_full().gap(6.0)
            .child(row().w_full().justify_between().child(text(l.title.clone()).font(LABEL).color(p.fg).truncate())
                .child(text(format!("{} decision{}", l.decisions.len(), if l.decisions.len() == 1 { "" } else { "s" })).font(SMALL).color(p.muted_fg)))
            .child(share_link("dock", &l.share_url, p))
            .child(button("dock-results".into(), "See results".into(), false, true, p).on_press(move |s: &mut AppState| super::show_results(s, &id))));
    }
    if n > 0 {
        let mut list = col().w_full().gap(4.0);
        for t in titles.iter().take(4) {
            list = list.child(row().w_full().px(8.0).py(4.0).radius(6.0).fill(p.hover).child(text(t.clone()).font(SMALL).color(p.fg).truncate()));
        }
        if n > 4 {
            list = list.child(text(format!("+{} more", n - 4)).font(SMALL).color(p.muted_fg));
        }
        body = body.child(list);
    } else {
        body = body.child(text("Add two or more designs with “Poll” on a card, then build the poll.").font(SMALL).color(p.muted_fg).wrap());
    }
    if let Some(shared) = pls.sharing {
        let (label, c) = if shared { ("Geteilt im LAN", hex("#10b981").expect("hex")) } else { ("nur lokal", p.muted_fg) };
        body = body.child(row().key("dock-sharing").gap(6.0).center_y().child(el().size(6.0, 6.0).radius(3.0).fill(c)).child(text(label).font(SMALL).color(c)));
    }
    let card = col().key("decisions-dock").role("dialog").label("Share a poll").w(320.0).radius(14.0)
        .child(row().w_full().px(14.0).pt(12.0).justify_between().child(text("Share a poll").font(TITLE).color(p.fg))
            .child(text(format!("{n} candidate{}", if n == 1 { "" } else { "s" })).font(SMALL).color(p.muted_fg)))
        .child(body)
        .child(row().w_full().padding(10.0).child(button("dock-build".into(), if latest.is_some() { "Build a new poll".into() } else { "Build poll".into() }, true, true, p)
            .w_full().on_press(|s: &mut AppState| super::open_panel(s, Tab::Build))));
    Some(el().key("decisions-dock-anchor").pin(None, Some(HEADER_H), Some(16.0), None)
        .modal(true, |s: &mut AppState| pl(s).dock_open = false)
        .child(style::glass(card, p, 0.94)))
}

/* ------------------------------------------------------------------ panel */

pub fn panel(s: &AppState, p: &Palette, t: &CardTheme, window: [f32; 2]) -> Option<El> {
    let pls = &s.features.polls;
    if !pls.panel_open {
        return None;
    }
    let published = super::published(s).len();
    let tab = |key: &'static str, label: String, which: Tab| {
        let on = pls.tab == which;
        row().key(key).role("tab").label(label.clone()).checked(on).h(28.0).px(12.0).radius(6.0).center()
            .fill(if on { p.canvas_bg } else { Color::TRANSPARENT })
            .on_press(move |s: &mut AppState| {
                pl(s).tab = which;
                if which == Tab::Results
                    && let Some(id) = pl(s).results_poll.clone().or_else(|| super::published(s).first().map(|x| x.id.clone()))
                {
                    super::load_results(s, &id);
                }
            })
            .child(text(label).font(LABEL).color(if on { p.fg } else { p.muted_fg }))
    };
    let head = col().w_full().gap(10.0).px(20.0).pt(18.0).pb(12.0)
        .child(row().w_full().gap(8.0).center_y()
            .child(icons::vote(18.0, p.fg)).child(text("Decisions").font(H1).color(p.fg).grow(1.0))
            .child(icon_btn("decisions-close".into(), "Close", icons::x(16.0, p.fg), true, p).on_press(|s: &mut AppState| pl(s).panel_open = false)))
        .child(text("Don't share the messy file. Package the competing directions into a few clear questions and let stakeholders vote on live prototypes.").font(BODY).color(p.muted_fg).wrap())
        .child(row().gap(2.0).padding(2.0).radius(8.0).fill(p.hover)
            .child(tab("tab-build", "Build poll".into(), Tab::Build))
            .child(tab("tab-results", if published > 0 { format!("Results · {published}") } else { "Results".into() }, Tab::Results)));
    let body = match pls.tab {
        Tab::Build => build(s, p, t),
        Tab::Results => results(s, p, t),
    };
    let card = col().key("decisions").role("dialog").label("Decisions").size((window[0] * 0.92).min(1150.0), window[1] * 0.9).radius(14.0).clip_soft(0.0)
        .fill(p.canvas_bg).border(1.0, p.border)
        .child(head).child(el().w_full().h(1.0).fill(p.border)).child(body);
    Some(el().key("decisions-modal").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).center().fill(Color::BLACK.alpha(0.5))
        .modal(false, |s: &mut AppState| pl(s).panel_open = false).child(card))
}

fn build(s: &AppState, p: &Palette, t: &CardTheme) -> El {
    let pls = &s.features.polls;
    let ready = pls.candidates.iter().filter(|id| s.snap().design(id).is_some_and(|d| d.is_ready())).count();
    // Tray.
    let mut tray = col().w_full().gap(10.0);
    if pls.candidates.is_empty() {
        tray = tray.child(text("Use Poll on a design card to collect the competing variations here.").font(SMALL).color(p.muted_fg).wrap());
    }
    for id in &pls.candidates {
        let title = s.snap().design(id).map_or("Missing design".into(), |d| d.title.clone());
        let rid = id.clone();
        tray = tray.child(col().key(format!("tray-{id}")).w_full().radius(8.0).clip_soft(0.0).border(1.0, p.border)
            .child(picture(s, id, 254.0, t, format!("tray-pic-{id}")))
            .child(row().w_full().px(8.0).py(6.0).gap(4.0).center_y().child(text(title.clone()).font(SMALL).color(p.fg).truncate().grow(1.0).min_w(0.0))
                .child(icon_btn(format!("tray-rm-{id}"), &format!("Remove {title} from poll"), icons::x(12.0, p.muted_fg), true, p)
                    .on_press(move |s: &mut AppState| super::toggle_candidate(s, &rid)))));
    }
    let mut agent = col().w_full().gap(8.0).padding(14.0)
        .child(field("poll-context".into(), &pls.context, "Optional: what are you torn about?", BODY, p, |s| &mut pl(s).context));
    let find = button("poll-find".into(), if pls.finding { "Finding decisions…".into() } else { "Let the agent find the decisions".into() }, true, ready >= 2 && !pls.finding, p).w_full();
    agent = agent.child(if ready >= 2 && !pls.finding { find.on_press(super::find_decisions) } else { find });
    if ready < 2 {
        agent = agent.child(text("Needs at least two finished designs.").font(SMALL).color(p.muted_fg));
    }
    if let Some(e) = &pls.find_error {
        agent = agent.child(col().w_full().gap(2.0).padding(8.0).radius(8.0).border(1.0, hex("#ef4444").expect("hex").alpha(0.4))
            .child(text("Couldn't reach the agent").font(LABEL).color(hex("#ef4444").expect("hex")))
            .child(text(e.clone()).font(SMALL).color(p.muted_fg).wrap())
            .child(text("You can still build decisions by hand.").font(SMALL).color(p.muted_fg)));
    }
    let left = col().key("poll-tray").w(290.0).h_full().min_h(0.0).fill(p.hover)
        .child(row().w_full().px(14.0).pt(12.0).justify_between().child(text("POLL TRAY").font(SMALL).color(p.muted_fg))
            .child(text(format!("{} designs", pls.candidates.len())).font(SMALL).color(p.muted_fg)))
        .child(el().grow(1.0).min_h(0.0).w_full().scroll(ScrollSpec { stick_bottom: false }).child(col().w_full().padding(14.0).child(tray)))
        .child(el().w_full().h(1.0).fill(p.border)).child(agent);

    // Decisions.
    let mut main = col().w_full().gap(14.0).padding(20.0)
        .child(text("Poll title").font(SMALL).color(p.muted_fg))
        .child(field("poll-title".into(), &pls.title, "e.g. Checkout redesign - pick a direction", TITLE, p, |s| &mut pl(s).title));
    if pls.decisions.is_empty() {
        main = main.child(col().w_full().py(40.0).gap(4.0).center().radius(12.0).border(1.0, p.border).border_dash(5.0, 4.0)
            .child(text("No decisions yet").font(TITLE).color(p.fg))
            .child(text(if ready >= 2 { "Let the agent find the core decisions in your candidates, or add one yourself." } else { "Add two or more designs to the poll from the canvas (the “Poll” button on a design), then let the agent find the decisions." }).font(SMALL).color(p.muted_fg).wrap().max_w(420.0)));
    }
    let n = pls.decisions.len();
    for (i, d) in pls.decisions.iter().enumerate() {
        main = main.child(decision(s, d, i, n, p, t));
    }
    main = main.child(button("poll-add-decision".into(), "+ Add a decision manually".into(), false, true, p).w_full().on_press(super::add_decision));
    let issue = super::validate(s);
    let mut bar = col().w_full().gap(8.0).px(20.0).py(12.0).fill(p.hover);
    if let Some(id) = &pls.just_published
        && let Some(pp) = s.snap().published_poll(id)
    {
        let rid = id.clone();
        bar = bar.child(col().w_full().gap(6.0).padding(10.0).radius(8.0).border(1.0, hex("#10b981").expect("hex").alpha(0.4))
            .child(row().w_full().gap(8.0).center_y().child(text("Published. Share this link with your stakeholders:").font(LABEL).color(p.fg).grow(1.0))
                .child(button("published-results".into(), "See results".into(), false, true, p).on_press(move |s: &mut AppState| super::show_results(s, &rid)))
                .child(icon_btn("published-dismiss".into(), "Dismiss", icons::x(12.0, p.muted_fg), true, p).on_press(|s: &mut AppState| pl(s).just_published = None)))
            .child(share_link("published", &pp.share_url, p)));
    }
    let ok = issue.is_none() && !pls.publishing;
    let status = issue.clone().unwrap_or_else(|| format!("{n} decision{} · {} candidates · snapshots the current HTML", if n == 1 { "" } else { "s" }, pls.candidates.len()));
    let pub_btn = button("poll-publish".into(), if pls.publishing { "Publishing…".into() } else { "Publish poll".into() }, true, ok, p);
    bar = bar.child(row().w_full().gap(10.0).center_y().child(text(status).font(SMALL).color(p.muted_fg).grow(1.0).min_w(0.0).truncate())
        .child(if ok { pub_btn.on_press(super::publish) } else { pub_btn }));
    let right = col().grow(1.0).min_w(0.0).h_full().min_h(0.0)
        .child(el().grow(1.0).min_h(0.0).w_full().scroll(ScrollSpec { stick_bottom: false }).child(main))
        .child(bar);
    row().key("poll-build").w_full().grow(1.0).min_h(0.0).child(left).child(right)
}

fn decision(s: &AppState, d: &DraftDecision, i: usize, n: usize, p: &Palette, t: &CardTheme) -> El {
    let id = d.id.clone();
    let bind = |id: String, which: u8| hr(move |s: &mut AppState| {
        let dd = pl(s).decisions.iter_mut().find(|x| x.id == id).expect("decision");
        if which == 0 { &mut dd.question } else { &mut dd.why }
    });
    let (up, down, rm) = (id.clone(), id.clone(), id.clone());
    let head = row().w_full().gap(10.0).items_start()
        .child(row().size(24.0, 24.0).mt(4.0).radius(12.0).center().fill(p.fg).child(text((i + 1).to_string()).font(SMALL).color(p.canvas_bg)))
        .child(col().grow(1.0).min_w(0.0).gap(4.0)
            .child(field(format!("q-{id}"), &d.question, "The question, e.g. How prominent should pricing be?", TITLE, p, bind(id.clone(), 0)))
            .child(field(format!("why-{id}"), &d.why, "Why it matters (one line)", BODY, p, bind(id.clone(), 1))))
        .child(icon_btn(format!("dup-{id}"), "Move up", icons::arrow_up(14.0, p.fg), i > 0, p).on_press(move |s: &mut AppState| super::move_decision(s, &up, -1)))
        .child(icon_btn(format!("ddn-{id}"), "Move down", icons::arrow_down(14.0, p.fg), i + 1 < n, p).on_press(move |s: &mut AppState| super::move_decision(s, &down, 1)))
        .child(icon_btn(format!("drm-{id}"), "Remove decision", icons::x(14.0, p.fg), true, p).on_press(move |s: &mut AppState| pl(s).decisions.retain(|x| x.id != rm)));
    let mut opts = row().w_full().gap(10.0).flex_wrap();
    for (j, o) in d.options.iter().enumerate() {
        opts = opts.child(option(s, &id, o, j, d.options.len(), p, t));
    }
    if d.options.len() < 4 {
        let unused: Vec<String> = s.features.polls.candidates.iter().filter(|c| !d.options.iter().any(|o| &o.design_id == *c)).cloned().collect();
        let mut add = col().key(format!("add-opt-{id}")).w(180.0).gap(4.0).padding(8.0).radius(8.0).border(1.0, p.border).border_dash(4.0, 3.0)
            .child(text(if unused.is_empty() { "No more candidates" } else { "Add option" }).font(SMALL).color(p.muted_fg));
        for c in unused {
            let title = s.snap().design(&c).map_or(c.clone(), |x| x.title.clone());
            let (did, cid) = (id.clone(), c.clone());
            add = add.child(row().key(format!("add-{id}-{c}")).role("button").label(title.clone()).w_full().h(24.0).px(6.0).radius(5.0).center_y()
                .hover(p.hover).press(p.press)
                .on_press(move |s: &mut AppState| {
                    let title = s.snap().design(&cid).map_or(String::new(), |x| x.title.clone());
                    if let Some(dd) = pl(s).decisions.iter_mut().find(|x| x.id == did) {
                        dd.options.push(DraftOption { design_id: cid.clone(), label: TextEdit::new(title) });
                    }
                })
                .child(text(format!("+ {title}")).font(SMALL).color(p.fg).truncate()));
        }
        opts = opts.child(add);
    }
    col().key(format!("decision-{id}")).w_full().gap(12.0).padding(14.0).radius(12.0).border(1.0, p.border).child(head).child(opts)
}

fn option(s: &AppState, did: &str, o: &DraftOption, j: usize, n: usize, p: &Palette, t: &CardTheme) -> El {
    let title = s.snap().design(&o.design_id).map_or("Missing design".into(), |x| x.title.clone());
    let key = format!("{did}-{}", o.design_id);
    let (d1, d2, d3) = (did.to_string(), did.to_string(), did.to_string());
    let lb = did.to_string();
    let label_bind = hr(move |s: &mut AppState| &mut pl(s).decisions.iter_mut().find(|x| x.id == lb).expect("decision").options[j].label);
    let mv = move |s: &mut AppState, did: &str, a: usize, b: usize| {
        if let Some(dd) = pl(s).decisions.iter_mut().find(|x| x.id == did) && b < dd.options.len() {
            dd.options.swap(a, b);
        }
    };
    col().key(format!("opt-{key}")).w(200.0).radius(8.0).clip_soft(0.0).border(1.0, p.border)
        .child(el().child(picture(s, &o.design_id, 200.0, t, format!("opt-pic-{key}")))
            .child(row().absolute(6.0, 6.0).px(5.0).radius(4.0).fill(p.canvas_bg.alpha(0.9)).child(text(LETTERS[j.min(3)]).font(SMALL).color(p.fg))))
        .child(col().w_full().gap(4.0).padding(6.0)
            .child(field(format!("label-{key}"), &o.label, "Short label", SMALL, p, label_bind))
            .child(row().w_full().gap(2.0).center_y().child(text(title).font(SMALL).color(p.muted_fg).truncate().grow(1.0).min_w(0.0))
                .child(icon_btn(format!("ol-{key}"), "Move option left", icons::arrow_left(12.0, p.fg), j > 0, p).on_press(move |s: &mut AppState| if j > 0 { mv(s, &d1, j, j - 1) }))
                .child(icon_btn(format!("or-{key}"), "Move option right", icons::arrow_right(12.0, p.fg), j + 1 < n, p).on_press(move |s: &mut AppState| mv(s, &d2, j, j + 1)))
                .child(icon_btn(format!("ox-{key}"), "Remove option", icons::x(12.0, p.fg), true, p).on_press(move |s: &mut AppState| {
                    if let Some(dd) = pl(s).decisions.iter_mut().find(|x| x.id == d3) && j < dd.options.len() {
                        dd.options.remove(j);
                    }
                }))))
}

fn results(s: &AppState, p: &Palette, t: &CardTheme) -> El {
    let polls = super::published(s);
    if polls.is_empty() {
        return col().w_full().grow(1.0).center().gap(6.0)
            .child(text("No published polls yet").font(TITLE).color(p.fg))
            .child(text("Publish a poll from the Build tab. Votes from stakeholders show up here.").font(SMALL).color(p.muted_fg));
    }
    let sel = s.features.polls.results_poll.as_deref().and_then(|id| polls.iter().find(|x| x.id == id)).unwrap_or(&polls[0]).clone();
    let mut list = col().w_full().gap(2.0).padding(10.0).child(text("PUBLISHED").font(SMALL).color(p.muted_fg));
    for x in &polls {
        let on = x.id == sel.id;
        let id = x.id.clone();
        list = list.child(col().key(format!("pub-{}", x.id)).role("button").label(x.title.clone()).w_full().px(8.0).py(6.0).radius(6.0)
            .fill(if on { p.canvas_bg } else { Color::TRANSPARENT }).hover(p.hover)
            .on_press(move |s: &mut AppState| {
                pl(s).results_poll = Some(id.clone());
                super::load_results(s, &id);
            })
            .child(text(x.title.clone()).font(LABEL).color(p.fg).truncate())
            .child(text(format!("{} · {} decisions", crate::features::time::when_ms(x.created_at), x.decisions.len())).font(SMALL).color(p.muted_fg)));
    }
    let left = el().w(240.0).h_full().min_h(0.0).fill(p.hover).scroll(ScrollSpec { stick_bottom: false }).child(list);
    row().key("poll-results").w_full().grow(1.0).min_h(0.0).child(left)
        .child(el().grow(1.0).min_w(0.0).h_full().min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).child(detail(s, &sel, p, t)))
}

fn detail(s: &AppState, poll: &PublishedPoll, p: &Palette, t: &CardTheme) -> El {
    let loaded = s.features.polls.results.get(&poll.id);
    let voters = match loaded {
        Some(Ok(r)) => {
            let mut v: Vec<&str> = r.votes.iter().map(|x| x.voter_id.as_str()).collect();
            v.sort_unstable();
            v.dedup();
            format!("{} {} voted", v.len(), if v.len() == 1 { "person has" } else { "people have" })
        }
        Some(Err(_)) => "Couldn't load the votes".into(),
        None => "Loading votes…".into(),
    };
    let (rid, fid) = (poll.id.clone(), poll.id.clone());
    let mut c = col().w_full().gap(14.0).padding(20.0)
        .child(row().w_full().gap(8.0).center_y()
            .child(col().grow(1.0).min_w(0.0).child(text(poll.title.clone()).font(TITLE).color(p.fg).truncate()).child(text(voters).font(SMALL).color(p.muted_fg)))
            .child(button("results-refresh".into(), "Refresh".into(), false, true, p).on_press(move |s: &mut AppState| super::load_results(s, &rid)))
            .child(icon_btn("results-forget".into(), "Forget this poll", icons::x(14.0, p.fg), true, p).on_press(move |s: &mut AppState| super::forget(s, &fid))))
        .child(share_link("results", &poll.share_url, p));
    if let Some(Err(e)) = loaded {
        c = c.child(text(e.clone()).font(SMALL).color(hex("#ef4444").expect("hex")).wrap());
    }
    let tallies = loaded.and_then(|r| r.as_ref().ok());
    for (i, d) in poll.decisions.iter().enumerate() {
        let tally = tallies.and_then(|r| r.tallies.iter().find(|x| x.decision_id == d.id));
        let total = tally.map_or(0, |x| x.total);
        let winner = s.snap().poll_winner(&format!("{}/{}", poll.id, d.id)).map(|w| w.to_string());
        let mut opts = row().w_full().gap(10.0).flex_wrap();
        for (j, o) in d.options.iter().enumerate() {
            let ot = tally.and_then(|x| x.options.iter().find(|y| y.option_id == o.id));
            let count = ot.map_or(0, |x| x.count);
            let pct = if total > 0 { (count as f32 / total as f32 * 100.0).round() } else { 0.0 };
            let leading = total > 0 && tally.is_some_and(|x| x.leaders.contains(&o.id));
            let is_winner = winner.as_deref() == Some(o.id.as_str());
            let (pp, did, oo) = (poll.clone(), d.id.clone(), o.clone());
            let mut card = col().key(format!("res-{}-{}", d.id, o.id)).w(210.0).radius(8.0).clip_soft(0.0).border(if is_winner { 2.0 } else { 1.0 }, if is_winner { hex("#fbbf24").expect("hex") } else { p.border })
                .child(picture(s, &o.design_id, 210.0, t, format!("res-pic-{}", o.id)))
                .child(col().w_full().gap(6.0).padding(8.0)
                    .child(row().w_full().gap(6.0).center_y().child(text(LETTERS[j.min(3)]).font(SMALL).color(p.muted_fg))
                        .child(text(o.label.clone()).font(LABEL).color(p.fg).truncate().grow(1.0).min_w(0.0))
                        .children(leading.then(|| row().h(18.0).px(6.0).radius(9.0).center_y().fill(p.hover).child(text("Leading").font(SMALL).color(p.fg)))))
                    .child(el().w_full().h(6.0).radius(3.0).fill(p.hover).child(el().w_frac(pct / 100.0).h_full().radius(3.0).fill(if leading { p.fg } else { p.muted_fg.alpha(0.4) })))
                    .child(text(format!("{count} vote{} · {pct}%", if count == 1 { "" } else { "s" })).font(SMALL).color(p.muted_fg)));
            if let Some(ot) = ot {
                for v in &ot.voters {
                    card = card.child(col().px(8.0).gap(1.0).child(text(v.name.clone()).font(SMALL).color(p.fg))
                        .children(v.comment.clone().map(|cm| text(cm).font(SMALL).color(p.muted_fg).wrap())));
                }
            }
            card = card.child(row().padding(8.0).w_full().child(button(format!("win-{}-{}", d.id, o.id), if is_winner { "Winner".into() } else { "Mark winner".into() }, false, true, p).w_full()
                .on_press(move |s: &mut AppState| super::toggle_winner(s, &pp, &did, &oo))));
            opts = opts.child(card);
        }
        c = c.child(col().key(format!("res-dec-{}", d.id)).w_full().gap(10.0).padding(14.0).radius(12.0).border(1.0, p.border)
            .child(row().w_full().gap(8.0).child(text((i + 1).to_string()).font(SMALL).color(p.muted_fg)).child(text(d.question.clone()).font(TITLE).color(p.fg).grow(1.0).min_w(0.0))
                .child(text(format!("{total} vote{}", if total == 1 { "" } else { "s" })).font(SMALL).color(p.muted_fg)))
            .child(opts));
    }
    c
}
