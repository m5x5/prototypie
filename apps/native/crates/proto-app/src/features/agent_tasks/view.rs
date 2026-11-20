//! The header's agent capacity pill (`Agenten: n von m`) and its task panel (German copy, as the reference).

use rune::color::hex;
use rune::{Align, Color, Cx, El, ScrollSpec, Side, TextStyle, col, el, row, text};

use super::data::{AgentTask, AgentTasksData, JobGroup, TaskStatus, job_groups};
use crate::app::state::AppState;
use crate::app::style::{self, Palette};
use crate::features::icons;

const PILL: TextStyle = TextStyle::new(12.0, 500.0);
const SMALL: TextStyle = TextStyle::new(11.0, 500.0);
/// The connect commands (a long app path must fit the panel without a break point).
const MONO: TextStyle = TextStyle::new(10.0, 500.0).mono();
const BODY: TextStyle = TextStyle::new(12.5, 400.0);
const TITLE: TextStyle = TextStyle::new(13.0, 600.0);
const BADGE: TextStyle = TextStyle::new(10.0, 600.0);
const GROUP: TextStyle = TextStyle::new(10.5, 600.0);
/// Most capacity slots the pill draws before "+N".
const MAX_SLOTS: u32 = 12;
const PANEL_W: f32 = 380.0;

/// Claude's orange, for working agents.
pub fn claude() -> Color {
    hex("#D97757").expect("hex")
}

fn status_dot(status: TaskStatus, p: &Palette) -> Color {
    match status {
        TaskStatus::Working => hex("#f59e0b").expect("hex"),
        TaskStatus::Waiting => hex("#0ea5e9").expect("hex"),
        TaskStatus::Delivered => hex("#10b981").expect("hex"),
        TaskStatus::Resolved => p.muted_fg.alpha(0.5),
        TaskStatus::Cancelled => hex("#ef4444").expect("hex").alpha(0.7),
    }
}

/// The pill's accessible label.
pub fn pill_label(data: Option<&AgentTasksData>, failed: bool) -> String {
    match data.and_then(|d| d.known().map(|k| (k, d.open_count()))) {
        Some(((w, t), open)) => format!("Agenten: {w} von {t} arbeiten · {open} offene Aufgabe{}", if open == 1 { "" } else { "n" }),
        None if failed => "Agenten-Status nicht erreichbar".into(),
        None => "Agenten-Status wird geladen".into(),
    }
}

pub fn pill(s: &AppState, _cx: &mut Cx<AppState>, p: &Palette) -> El {
    let t = &s.features.tasks;
    let data = t.poller.data.as_ref();
    let known = data.and_then(AgentTasksData::known);
    let working = known.map_or(0, |k| k.0);
    let open_count = data.map_or(0, AgentTasksData::open_count);
    let mut pill = row().key("agent-capacity").role("button").tab_index(1).label(pill_label(data, t.poller.failed))
        .expanded(t.panel_open)
        .h(32.0).px(10.0).gap(8.0).radius(16.0).center_y();
    pill = style::glass_pill(pill, p)
        .hover(p.hover).press(p.press)
        .on_press_tx(|s: &mut AppState, tx| s.features.tasks.toggle_panel(tx))
        .child(icons::minion(16.0, if working > 0 { claude() } else { p.fg }));
    pill = match known {
        Some((w, total)) => {
            let slots = total.min(MAX_SLOTS);
            // No agents: just the icon, with the same padding on all sides (no empty dot row and its gap).
            if slots == 0 && open_count == 0 {
                return pill.px(7.0);
            }
            let mut dots = row().key("slots").gap(3.0).center_y();
            for i in 0..slots {
                let on = i < w;
                dots = dots.child(el().key(format!("slot-{i}")).size(6.0, 6.0).radius(2.0)
                    .border(1.0, if on { claude() } else { p.fg.alpha(0.6) })
                    .fill(if on { claude() } else { Color::TRANSPARENT }));
            }
            if total > slots {
                dots = dots.child(text(format!("+{}", total - slots)).font(SMALL).color(p.muted_fg));
            }
            pill.child(dots)
        }
        None => pill.child(text(if t.poller.failed { "–" } else { "…" }).font(PILL).color(p.muted_fg)),
    };
    if open_count > 0 {
        pill = pill.child(row().key("open-badge").h(16.0).min_w(16.0).px(4.0).radius(8.0).center().fill(p.fg)
            .child(text(open_count.to_string()).font(BADGE).color(p.canvas_bg)));
    }
    pill
}

/// The panel under the pill, while open.
pub fn panel(s: &AppState, _cx: &mut Cx<AppState>, p: &Palette) -> Option<El> {
    let t = &s.features.tasks;
    if !t.panel_open {
        return None;
    }
    let data = t.poller.data.as_ref();
    let head_line = match data.and_then(AgentTasksData::known) {
        Some((w, total)) => format!("{w} von {total} Agenten arbeiten"),
        None if t.poller.failed => "Status nicht erreichbar".into(),
        None => "Lädt …".into(),
    };
    let mut head = col().key("tasks-head").gap(8.0).px(14.0).pt(12.0).pb(10.0)
        .child(row().w_full().gap(8.0).center_y()
            .child(icons::minion(12.0, p.fg))
            .child(text(head_line).font(TITLE).color(p.fg).grow(1.0).min_w(0.0))
            .child(row().key("tasks-close").role("button").label("Schließen").size(24.0, 24.0).radius(12.0).center()
                .hover(p.hover).press(p.press)
                .on_press(|s: &mut AppState| s.features.tasks.panel_open = false)
                .child(icons::x(14.0, p.muted_fg))));
    if let Some(agents) = data.map(|d| &d.capacity.agents).filter(|a| !a.is_empty()) {
        let mut chips = row().key("agent-chips").gap(4.0).flex_wrap();
        for (i, a) in agents.iter().enumerate() {
            let busy = a.status == "working" || a.status == "blocked";
            chips = chips.child(row().key(format!("agent-{i}")).label(format!("{}: {}", a.name, a.status))
                .h(20.0).px(8.0).gap(4.0).radius(10.0).center_y()
                .border(1.0, if busy { claude().alpha(0.5) } else { p.border })
                .fill(if busy { claude().alpha(0.1) } else { Color::TRANSPARENT })
                .child(el().size(6.0, 6.0).radius(3.0).fill(if busy { claude() } else { p.muted_fg.alpha(0.4) }))
                .child(text(a.name.clone()).font(SMALL).color(if busy { p.fg } else { p.muted_fg })));
        }
        head = head.child(chips);
    }
    // No agent connected (Rex counts none, or without Rex nobody waited for notes): the connect commands show;
    // the task list goes only while it has nothing to show (handed-over tasks stay visible, with their status).
    let no_agents = data.is_some_and(AgentTasksData::no_agent);
    if no_agents {
        head = head.child(connect_help(p, t.copied, t.exe.as_deref()));
    }
    let hide_list = no_agents && data.is_none_or(|d| d.tasks.is_empty());
    if data.is_some_and(|d| !d.notes_service) {
        head = head.child(text("Der Notizdienst ist nicht erreichbar: Status und Verlauf der Aufgaben fehlen.").font(BODY).color(hex("#d97706").expect("hex")).wrap());
    }
    let mut panel = col().key("tasks-panel").role("dialog").label("Agenten und Aufgaben")
        .w(PANEL_W).radius(14.0)
        .child(head);
    if !hide_list {
        let list = task_list(s, p);
        let body = el().key("tasks-scroll").w_full().max_h(560.0).scroll(ScrollSpec { stick_bottom: false }).child(list);
        panel = panel.child(el().h(1.0).w_full().fill(p.border)).child(body);
    }
    // Under the pill, right edges aligned (the reference's anchored popover); Esc / outside click close it.
    Some(style::glass(panel, p, 0.92)
        .anchored("agent-capacity", Side::Bottom, Align::End, 8.0)
        .modal(true, |s: &mut AppState| s.features.tasks.panel_open = false))
}

/// The running binary, quoted for a shell when needed; `prototypie` when it can't be found. Found once.
fn exe_word() -> &'static str {
    static EXE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    EXE.get_or_init(|| match std::env::current_exe() {
        Ok(p) => shell_word(&p.to_string_lossy()),
        Err(_) => "prototypie".into(),
    })
}

/// `'…'` around anything but `A–Z a–z 0–9 / . _ -` (a `'` inside as `'\''`).
pub fn shell_word(s: &str) -> String {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// The two commands for the agent (spec-public/05 AN-CLI-401): install the skill with this binary, then wait.
pub fn connect_commands(exe: Option<&str>) -> String {
    let exe = exe.map(shell_word).unwrap_or_else(|| exe_word().to_string());
    format!("{exe} skill install\nprototypie notes watch")
}

/// Shown while no agent is connected: two commands any coding agent can run, and a button to copy them.
fn connect_help(p: &Palette, copied: bool, exe: Option<&str>) -> El {
    let commands = connect_commands(exe);
    let command_box = row().key("connect-prompt").w_full().px(8.0).py(6.0).radius(6.0).fill(p.hover)
        .child(text(commands.clone()).font(MONO).color(p.fg).wrap().grow(1.0).min_w(0.0));
    let label = if copied { "Kopiert" } else { "Befehle kopieren" };
    let copy = row().key("connect-copy-prompt").role("button").label(label).h(28.0).px(12.0).gap(6.0).radius(14.0).center_y()
        .fill(if copied { hex("#10b981").expect("hex") } else { p.fg }).press(Color::BLACK.alpha(0.15))
        .on_press_tx(move |s: &mut AppState, tx| {
            // No trailing newline: pasting into a terminal must not run anything yet.
            tx.copy(commands.clone());
            s.features.tasks.copied_until = s.features.annotate.clock + 1.5;
            s.features.tasks.copied = true;
            tx.wake_after(std::time::Duration::from_millis(1600));
        })
        .children(copied.then(|| icons::check(14.0, Color::WHITE)))
        .child(text(label).font(SMALL).color(if copied { Color::WHITE } else { p.canvas_bg }));
    col().key("tasks-connect").w_full().gap(8.0).pt(4.0)
        .child(text("Noch kein Agent verbunden").font(TITLE).color(p.fg))
        .child(text("Jeder Coding-Agent funktioniert (Claude Code, Codex, Cursor …). Gib deinem Agenten diese zwei Befehle:").font(BODY).color(p.muted_fg).wrap())
        .child(command_box)
        .child(text("Der erste richtet einmalig den Skill „prototypie“ ein, der zweite wartet auf Notizen und Aufträge aus Prototypie.").font(SMALL).color(p.muted_fg).wrap())
        .child(copy)
}

#[cfg(test)]
mod connect_tests {
    use super::*;

    #[test]
    fn commands_quote_the_binary_when_needed() {
        // AN-CLI-401
        assert_eq!(
            connect_commands(Some("/Applications/Prototypie.app/Contents/MacOS/prototypie")),
            "/Applications/Prototypie.app/Contents/MacOS/prototypie skill install\nprototypie notes watch"
        );
        assert_eq!(shell_word("/Users/me/My Apps/prototypie"), "'/Users/me/My Apps/prototypie'");
        assert_eq!(shell_word("/tmp/it's/prototypie"), "'/tmp/it'\\''s/prototypie'");
        assert!(!connect_commands(None).ends_with('\n'));
    }
}

fn task_list(s: &AppState, p: &Palette) -> El {
    let t = &s.features.tasks;
    let Some(data) = t.poller.data.as_ref() else {
        return col().padding(16.0).child(text("Lädt …").font(BODY).color(p.muted_fg));
    };
    if data.tasks.is_empty() {
        return col().padding(16.0).child(text("Keine Aufgaben.").font(BODY).color(p.muted_fg));
    }
    let mut list = col().key("task-groups").w_full().min_w(0.0).gap(10.0).padding(8.0);
    // Items of one queue job are one row (in the group of their most active status); single tasks stay rows.
    let jobs = job_groups(&data.tasks);
    let job_of = |task: &AgentTask| jobs.iter().position(|g| task.job_id.as_deref() == Some(g.job_id.as_str()));
    for status in TaskStatus::ORDER {
        enum Entry<'a> {
            Task(&'a AgentTask),
            Job(usize),
        }
        let mut entries: Vec<Entry> = Vec::new();
        for task in data.tasks.iter().filter(|x| x.status == status) {
            match job_of(task) {
                Some(j) if jobs[j].status() == status => {
                    if !entries.iter().any(|e| matches!(e, Entry::Job(k) if *k == j)) {
                        entries.push(Entry::Job(j));
                    }
                }
                Some(_) => {}
                None => entries.push(Entry::Task(task)),
            }
        }
        if entries.is_empty() {
            continue;
        }
        let count: usize = entries.iter().map(|e| match e {
            Entry::Task(_) => 1,
            Entry::Job(j) => jobs[*j].count(status),
        }).sum();
        let mut group = col().key(format!("group-{status:?}")).w_full().min_w(0.0).gap(2.0)
            .child(row().px(6.0).gap(6.0).center_y()
                .child(el().size(6.0, 6.0).radius(3.0).fill(status_dot(status, p)))
                .child(text(status.label().to_uppercase()).font(GROUP).color(p.muted_fg))
                .child(text(count.to_string()).font(SMALL).color(p.muted_fg.alpha(0.7))));
        for (i, e) in entries.iter().enumerate() {
            match e {
                Entry::Job(j) => group = group.child(job_row(s, &jobs[*j], p)),
                Entry::Task(task) => {
                    if i >= status.shown() && t.expanded.as_deref() != Some(task.id.as_str()) {
                        continue;
                    }
                    group = group.child(task_row(s, task, t.expanded.as_deref() == Some(task.id.as_str()), None, p));
                }
            }
        }
        list = list.child(group);
    }
    list
}

/// One row for a job's items: the prompt once, "3 von 5 geliefert · 2 in Arbeit", and a chip per direction with its
/// worker and state. Expanded, each item is today's row (Zur Karte, note composer).
fn job_row(s: &AppState, g: &JobGroup, p: &Palette) -> El {
    let t = &s.features.tasks;
    let open = t.expanded_jobs.contains(&g.job_id);
    let first = g.items[0];
    let job = g.job_id.clone();
    let head = row().key("job-head").w_full().min_w(0.0).role("button").label(format!("{}: {}", first.title, g.summary())).expanded(open)
        .gap(8.0).px(6.0).py(6.0).radius(8.0).items_start()
        .hover(p.hover).press(p.press)
        .on_press(move |s: &mut AppState| {
            let e = &mut s.features.tasks.expanded_jobs;
            if !e.remove(&job) {
                e.insert(job.clone());
            }
        })
        .child(el().mt(5.0).size(8.0, 8.0).radius(4.0).fill(status_dot(g.status(), p)))
        .child(col().grow(1.0).min_w(0.0).gap(2.0)
            .child(text(first.title.clone()).font(TITLE).color(p.fg).truncate())
            .child(text(format!("{} · {}", g.job_id, g.summary())).font(SMALL).color(p.muted_fg).truncate()));
    let mut chips = row().key("job-chips").gap(4.0).flex_wrap().pl(22.0).pr(6.0).pb(6.0);
    for item in &g.items {
        let n = item.index.unwrap_or(0);
        let worker = item.worker();
        let label = format!("Richtung {n}: {}{}", item.status.label(), worker.as_deref().map(|w| format!(", {w}")).unwrap_or_default());
        let (job, id) = (g.job_id.clone(), item.id.clone());
        let mut chip = row().key(format!("job-item-{n}")).role("button").label(label.clone()).help(label)
            .h(20.0).px(7.0).gap(4.0).radius(10.0).center_y().border(1.0, p.border)
            .hover(p.hover).press(p.press)
            .on_press(move |s: &mut AppState| {
                s.features.tasks.expanded_jobs.insert(job.clone());
                s.features.tasks.expanded = Some(id.clone());
            })
            .child(el().size(6.0, 6.0).radius(3.0).fill(status_dot(item.status, p)))
            .child(text(format!("#{n}")).font(BADGE).color(p.fg));
        if let Some(w) = item.worker_short() {
            chip = chip.child(text(w).font(BADGE).color(p.muted_fg));
        }
        chips = chips.child(chip);
    }
    let mut c = col().key(format!("job-{}", g.job_id)).w_full().min_w(0.0).radius(8.0).child(head).child(chips);
    if open {
        c = c.border(1.0, p.border);
        let mut items = col().key("job-items").w_full().min_w(0.0).gap(2.0).px(6.0).pb(6.0);
        for item in &g.items {
            let title = format!("#{} · {}{}", item.index.unwrap_or(0), item.status.label(), item.worker().map(|w| format!(" · {w}")).unwrap_or_default());
            items = items.child(task_row(s, item, t.expanded.as_deref() == Some(item.id.as_str()), Some(title), p));
        }
        c = c.child(items);
    }
    c
}

/// `title`: replaces the task's title (inside a job group, where the prompt is already shown once).
fn task_row(s: &AppState, task: &AgentTask, open: bool, title: Option<String>, p: &Palette) -> El {
    let id = task.id.clone();
    let mut meta = format!("{} · {} · {}", task.reference(), task.status.label(), crate::features::time::when(&task.updated_at));
    if !task.thread.is_empty() {
        meta.push_str(&format!(" · {} im Verlauf", task.thread.len()));
    }
    let title = title.unwrap_or_else(|| task.title.clone());
    let head = row().key("row-head").w_full().min_w(0.0).role("button").label(title.clone()).expanded(open)
        .gap(8.0).px(6.0).py(6.0).radius(8.0).items_start()
        .hover(p.hover).press(p.press)
        .on_press(move |s: &mut AppState| s.features.tasks.toggle_task(&id))
        .child(el().mt(5.0).size(8.0, 8.0).radius(4.0).fill(status_dot(task.status, p)))
        .child(col().grow(1.0).min_w(0.0).gap(2.0)
            .child(text(title).font(TITLE).color(p.fg).truncate())
            .child(text(meta).font(SMALL).color(p.muted_fg).truncate()));
    let mut row_el = col().key(format!("task-{}", task.id)).w_full().min_w(0.0).radius(8.0);
    if open {
        row_el = row_el.border(1.0, p.border).fill(p.hover);
    }
    row_el = row_el.child(head);
    if open {
        row_el = row_el.child(detail(s, task, p));
    }
    row_el
}

fn detail(s: &AppState, task: &AgentTask, p: &Palette) -> El {
    let mut d = col().key("detail").gap(8.0).px(10.0).pb(10.0);
    if !task.request.is_empty() {
        d = d.child(text(task.request.clone()).font(BODY).color(p.muted_fg).wrap().max_lines(6));
    }
    let mut steps = row().key("steps").gap(4.0).flex_wrap().center_y();
    for (i, st) in task.steps.iter().enumerate() {
        let cancelled = st.key == "cancelled";
        let c = if cancelled { hex("#ef4444").expect("hex") } else if st.done { p.fg } else { p.muted_fg };
        let label = match &st.at {
            Some(at) => format!("{} {}", st.label, crate::features::time::when(at)),
            None => st.label.clone(),
        };
        steps = steps.child(row().key(format!("step-{i}")).h(20.0).px(8.0).radius(10.0).center_y()
            .border(1.0, if st.done { c.alpha(0.4) } else { p.border })
            .child(text(label).font(SMALL).color(c)));
    }
    d = d.child(steps);
    if let Some(target) = &task.target {
        let path = target.space_path.join(" › ");
        let (space, design) = (target.space_id.clone(), target.design_id.clone());
        d = d.child(row().key("target").gap(8.0).center_y()
            .child(text(path).font(SMALL).color(p.muted_fg).truncate().grow(1.0).min_w(0.0))
            .child(row().key("to-card").role("button").label("Zur Karte").h(24.0).px(8.0).radius(6.0).center_y()
                .border(1.0, p.border).hover(p.hover).press(p.press)
                .on_press(move |s: &mut AppState| crate::features::nav::enter_space(s, &space, design.as_deref()))
                .child(text("Zur Karte").font(SMALL).color(p.fg))));
    }
    if !task.thread.is_empty() {
        let mut thread = col().key("thread").gap(6.0);
        for (i, m) in task.thread.iter().enumerate() {
            let (who, c) = match m.role.as_str() {
                "human" => ("Du".to_string(), hex("#0284c7").expect("hex")),
                "agent" => ("Agent".to_string(), hex("#C2643F").expect("hex")),
                other => (other.to_string(), p.muted_fg),
            };
            thread = thread.child(col().key(format!("msg-{i}")).gap(2.0)
                .child(row().gap(6.0)
                    .child(text(who).font(SMALL).color(c))
                    .child(text(crate::features::time::when(&m.at)).font(SMALL).color(p.muted_fg)))
                .child(text(m.content.clone()).font(BODY).color(p.fg).wrap()));
        }
        d = d.child(thread);
    }
    match &task.note_id {
        None => d = d.child(text("Ohne Notiz im Notizdienst kann hier keine Notiz angehängt werden.").font(SMALL).color(p.muted_fg).wrap()),
        Some(note) => d = d.child(note_composer(s, note, p)),
    }
    d
}

/// The card plus its promoted source and copies (they share a decision, so they share agent tasks).
pub fn linked_ids(s: &AppState, design_id: &str) -> Vec<String> {
    let snap = s.snap();
    let mut ids = vec![design_id.to_string()];
    if let Some(r) = snap.promoted_from(design_id) {
        ids.push(r.design_id.clone());
    }
    for (copy_id, e) in snap.live(proto_core::RecordKind::PromotedFrom) {
        if matches!(e.typed(), proto_core::Typed::PromotedFrom(r) if ids.contains(&r.design_id)) && !ids.iter().any(|x| x == copy_id) {
            ids.push(copy_id.to_string());
        }
    }
    ids
}

/// "Beim Agenten" in the inspector: up to 4 tasks of the selected card (or a linked copy), each with its step
/// dots, the agent's summary and "Im Aufgabenpanel öffnen". None for a card without tasks.
pub fn progress_section(s: &AppState, p: &Palette, design_id: &str) -> Option<El> {
    let data = s.features.tasks.poller.data.as_ref()?;
    let ids = linked_ids(s, design_id);
    let tasks: Vec<&AgentTask> = data.tasks.iter().filter(|t| t.design_id.as_ref().is_some_and(|d| ids.contains(d))).take(4).collect();
    if tasks.is_empty() {
        return None;
    }
    let mut c = col().key("agent-progress").w_full().gap(10.0).child(text("Beim Agenten").font(TITLE).color(p.fg));
    for t in tasks {
        let steps: Vec<_> = t.steps.iter().filter(|st| st.key != "announced").collect();
        let current = steps.iter().rev().find(|st| st.done);
        let kind = match (&t.job_id, t.kind.as_str()) {
            (_, "implement") => "Implement".to_string(),
            (Some(j), _) => format!("Job {j}#{}", t.index.unwrap_or(0)),
            _ => "Notiz".to_string(),
        };
        let line = match current.and_then(|st| st.at.as_ref().map(|at| (st, at))) {
            Some((st, at)) => format!("{kind} · {} {}", st.label, crate::features::time::when(at)),
            None => kind,
        };
        let mut dots = row().gap(4.0).center_y();
        for (i, st) in steps.iter().enumerate() {
            let is_cur = current.is_some_and(|c| std::ptr::eq(*c, *st));
            let fill = if st.key == "cancelled" { hex("#ef4444").expect("hex") } else if is_cur && t.status == TaskStatus::Working { claude() } else if st.done { p.fg } else { Color::TRANSPARENT };
            dots = dots.child(el().key(format!("dot-{}-{i}", t.id)).size(10.0, 10.0).radius(5.0).fill(fill).border(1.0, if st.done { fill } else { p.muted_fg.alpha(0.6) }));
        }
        let id = t.id.clone();
        let mut item = col().key(format!("progress-{}", t.id)).w_full().gap(6.0)
            .child(row().gap(8.0).center_y().child(el().size(8.0, 8.0).radius(4.0).fill(status_dot(t.status, p)))
                .child(text(t.title.clone()).font(TITLE).color(p.fg).truncate().grow(1.0).min_w(0.0)))
            .child(text(line).font(SMALL).color(p.muted_fg))
            .child(dots);
        if let Some(sum) = t.summary.as_ref().map(|x| x.trim_start_matches("Resolved: ").to_string()).filter(|x| !x.is_empty()) {
            item = item.child(text(sum).font(BODY).color(p.muted_fg).wrap().max_lines(4));
        }
        item = item.child(row().key(format!("progress-open-{}", t.id)).role("button").label("Im Aufgabenpanel öffnen").h(24.0).px(8.0).radius(6.0).center_y()
            .border(1.0, p.border).hover(p.hover).press(p.press)
            .on_press_tx(move |s: &mut AppState, tx| s.features.tasks.open_on(&id, tx))
            .child(text("Im Aufgabenpanel öffnen").font(SMALL).color(p.fg)));
        c = c.child(item);
    }
    Some(c)
}

fn note_composer(s: &AppState, note_id: &str, p: &Palette) -> El {
    let empty = rune::TextEdit::default();
    let edit = s.features.tasks.notes.get(note_id).unwrap_or(&empty);
    let (bind, send, key_send) = (note_id.to_string(), note_id.to_string(), note_id.to_string());
    let input = rune::text_input(edit, BODY).color(p.fg).key(format!("note-{note_id}")).label("Notiz an den Agenten")
        .placeholder("Notiz an den Agenten …", p.muted_fg).w_full().min_h(40.0)
        .on_edit(move |s: &mut AppState| s.features.tasks.notes.entry(bind.clone()).or_default())
        .on_input_key(move |s: &mut AppState, ev, _| {
            if ev.key == rune::Key::Enter && ev.mods.cmd {
                s.features.tasks.send_note(&key_send);
                return true;
            }
            false
        });
    let mut c = col().key(format!("note-box-{note_id}")).w_full().gap(6.0)
        .child(el().w_full().padding(8.0).radius(8.0).border(1.0, p.border).child(input))
        // The app's small brand button (as Implement on cards); ⌘↵ in the field still sends, named in the tooltip only.
        .child(row().w_full().justify_end().child(
            crate::cards::button_sm(format!("note-send-{note_id}"), "Senden", Some(&crate::cards::icons::SEND),
                crate::cards::Variant::Brand, None, &crate::cards::CardTheme::new(p))
                .label("Notiz senden").help("Notiz senden (⌘↵)")
                .on_press(move |s: &mut AppState| s.features.tasks.send_note(&send))));
    if let Some(e) = s.features.tasks.note_errors.get(note_id) {
        c = c.child(text(e.clone()).font(SMALL).color(hex("#ef4444").expect("hex")).wrap());
    }
    c
}
