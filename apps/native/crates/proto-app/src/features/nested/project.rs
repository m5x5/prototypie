//! "+ → Project" (the reference's `project-space.tsx`): pick one of the user's projects (`GET /api/projects`) and
//! get a project on the main canvas named after it, with a "Projekt-Info" card and an agent note
//! (`POST /api/projects/:id`). A project already on the canvas (same folder) is opened instead. The first row
//! makes one from what was typed: a name, or an absolute folder path.

use std::time::Duration;

use proto_core::{Change, Design, DesignKind, DesignStatus, Point, RecordKind};
use rune::{Latest, TextEdit, Tx, Waker};
use serde::Deserialize;

use crate::app::state::AppState;
use crate::features::http::{request as http, server_addr};
use crate::features::{nav, notify};

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub repo: Option<String>,
    pub local_path: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    pub readme: Option<String>,
    /// Where the entry comes from ("private-projects.ttl", "Folder", "Solid pod").
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ProjectList {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub projects: Vec<Project>,
}

/// The dialog: closed, or open with what the list fetch returned, or creating a listed project.
#[derive(Default)]
pub enum Phase {
    #[default]
    Closed,
    Open,
    /// Fetching a listed project's details; the dialog shows busy.
    Creating,
}

#[derive(Default)]
pub struct ProjectDialog {
    pub phase: Phase,
    pub query: TextEdit,
    pub active: usize,
    /// None while loading; Some(list) (unavailable when the fetch failed).
    pub list: Option<ProjectList>,
    list_req: Latest<ProjectList>,
    detail_req: Latest<Result<Project, String>>,
    note_req: Latest<(String, bool)>,
}

impl ProjectDialog {
    pub fn setup(&mut self, waker: Waker) {
        self.list_req.set_waker(waker.clone());
        self.detail_req.set_waker(waker.clone());
        self.note_req.set_waker(waker);
    }

    pub fn is_open(&self) -> bool {
        !matches!(self.phase, Phase::Closed)
    }

    /// Rows: `None` = the "new project" row, then matching projects (name or keyword, case-insensitive).
    pub fn options(&self) -> Vec<Option<&Project>> {
        let q = self.query.text.trim().to_lowercase();
        let hits = self.list.iter().flat_map(|l| l.projects.iter()).filter(|p| {
            q.is_empty() || p.name.to_lowercase().contains(&q) || p.keywords.iter().any(|k| k.to_lowercase().contains(&q))
        });
        std::iter::once(None).chain(hits.map(Some)).collect()
    }
}

pub fn open(s: &mut AppState) {
    let d = &mut s.features.project;
    d.phase = Phase::Open;
    d.query = TextEdit::new("");
    d.query.focused = true;
    d.active = 0;
    d.list = None;
    let addr = server_addr();
    d.list_req.request(addr, |a| {
        http(&a, "GET", "/api/projects", None)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    });
}

/// `--snap` scene: the dialog open over `projects`.
pub fn scene(s: &mut AppState, projects: Vec<Project>) {
    let d = &mut s.features.project;
    d.phase = Phase::Open;
    d.query = TextEdit::new("");
    d.list = Some(ProjectList { available: true, projects });
}

pub fn close(s: &mut AppState) {
    s.features.project.phase = Phase::Closed;
    s.features.project.detail_req.cancel();
}

/// Arrow keys, Home/End and Enter on the search field; Esc closes. True when handled.
pub fn on_key(s: &mut AppState, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
    use rune::Key;
    let n = s.features.project.options().len();
    let active = s.features.project.active.min(n.saturating_sub(1));
    if ev.key == Key::Enter {
        choose(s, active, tx);
        return true;
    }
    if ev.key == Key::Escape {
        close(s);
        return true;
    }
    let d = &mut s.features.project;
    match ev.key {
        Key::Down => d.active = (d.active + 1).min(n.saturating_sub(1)),
        Key::Up => d.active = d.active.saturating_sub(1),
        Key::Home => d.active = 0,
        Key::End => d.active = n.saturating_sub(1),
        _ => return false,
    }
    // The list scrolls with the arrows: keep the active row in view.
    tx.scroll_into_view_in("project-scroll", format!("project-opt-{}", d.active));
    true
}

/// Pick row `index` (0 = new project from the query).
pub fn choose(s: &mut AppState, index: usize, tx: &mut Tx) {
    if matches!(s.features.project.phase, Phase::Creating) {
        return;
    }
    let picked = s.features.project.options().get(index).cloned().flatten().cloned();
    match picked {
        None => {
            let q = s.features.project.query.text.clone();
            close(s);
            super::create_project_from_query(s, &q);
        }
        Some(p) => {
            s.features.project.phase = Phase::Creating;
            let (addr, path) = (server_addr(), format!("/api/projects/{}", url_escape(&p.id)));
            s.features.project.detail_req.request((addr, path, p), |(a, path, fallback)| {
                // A failed detail fetch still creates the project from the list entry.
                Ok(http(&a, "GET", &path, None).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(fallback))
            });
            tx.wake_after(Duration::ZERO);
        }
    }
}

/// Land fetches: the list, a listed project's details (then create it), the agent note's outcome.
pub fn poll(s: &mut AppState) -> bool {
    let mut changed = false;
    if let Some(list) = s.features.project.list_req.take() {
        s.features.project.list = Some(list);
        changed = true;
    }
    if let Some(Ok(p)) = s.features.project.detail_req.take() {
        create_listed(s, &p);
        changed = true;
    }
    if let Some((name, ok)) = s.features.project.note_req.take() {
        if ok {
            crate::features::notify(s, &format!("Space „{name}“ angelegt"), "Ein Agent sammelt weitere Projekt-Infos.");
        } else {
            crate::features::notify(s, &format!("Space „{name}“ angelegt"), "Kein Agent erreichbar: die Projekt-Info bleibt bei der ersten Karte.");
        }
        changed = true;
    }
    changed
}

fn create_listed(s: &mut AppState, p: &Project) {
    close(s);
    let now = proto_core::ops::now_ms();
    let (created, is_new) = proto_core::ops::create_project(s.snap(), &p.name, p.local_path.as_deref());
    let space = created.space_id.clone();
    if !is_new {
        let name = s.snap().space(&space).map_or(p.name.clone(), |sp| sp.name.clone());
        nav::enter_space(s, &space, None);
        notify(s, &format!("„{name}“ is already on the canvas"), "");
        return;
    }
    let card = info_card(p, &space, now);
    let card_id = card.id.clone();
    let mut changes = created.changes;
    changes.push(Change::put(RecordKind::Designs, &card.id, &card).expect("design serialises"));
    if !nav::apply(s, changes, "create project") {
        notify(s, "Space konnte nicht angelegt werden", "");
        return;
    }
    nav::enter_space(s, &space, Some(&card_id));
    let body = serde_json::json!({ "spaceId": space }).to_string();
    let (addr, path, name) = (server_addr(), format!("/api/projects/{}", url_escape(&p.id)), p.name.clone());
    s.features.project.note_req.request((addr, path, body, name), |(a, path, body, name)| {
        let ok = http(&a, "POST", &path, Some(&body)).is_ok();
        (name, ok)
    });
}

fn info_card(p: &Project, space: &str, now: f64) -> Design {
    let v = serde_json::json!({
        "id": proto_core::ops::new_id(10), "spaceId": space, "kind": "page",
        "title": format!("Projekt-Info · {}", p.name),
        "rationale": "Belegte Felder aus private-projects.ttl (Name, Repository, Keywords) und, falls das Repo lokal liegt, der erste README-Absatz.",
        "html": info_card_html(p), "status": "ready",
        "instruction": format!("Project {} from private-projects.ttl", p.name),
        "position": {"x": 0, "y": 0}, "createdAt": now,
    });
    let d: Design = serde_json::from_value(v).expect("design json");
    debug_assert!(d.kind == DesignKind::Page && d.status == DesignStatus::Ready && d.position == Point { x: 0.0, y: 0.0 });
    d
}

fn esc(s: &str) -> String {
    s.chars().map(|c| match c {
        '&' | '<' | '>' | '"' | '\'' => format!("&#{};", c as u32),
        c => c.to_string(),
    }).collect()
}

/// The reference's "Projekt-Info" card: only the fields the project file (and README) has.
pub fn info_card_html(p: &Project) -> String {
    let row = |label: &str, value: String| format!("<div class=\"row\"><dt>{label}</dt><dd>{value}</dd></div>");
    let mut rows = String::new();
    if let Some(repo) = &p.repo {
        rows += &row("Repository", format!("<code>{}</code>", esc(repo)));
    }
    if let Some(lp) = p.local_path.as_ref().filter(|lp| Some(*lp) != p.repo.as_ref()) {
        rows += &row("Lokal", format!("<code>{}</code>", esc(lp)));
    }
    if !p.keywords.is_empty() {
        let chips: String = p.keywords.iter().map(|k| format!("<span class=\"chip\">{}</span>", esc(k))).collect();
        rows += &row("Keywords", format!("<div class=\"chips\">{chips}</div>"));
    }
    let readme = p.readme.as_ref().map(|r| format!("<p class=\"readme\">{}</p>", esc(r))).unwrap_or_default();
    let name = esc(&p.name);
    format!(r#"<!doctype html><html lang="de"><head><meta charset="utf-8"><title>Projekt-Info · {name}</title>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Google+Sans+Flex:wght@400;500;600;700&display=swap">
<style>
:root{{--bg:#fff;--fg:#18181b;--mfg:#71717a;--line:#e4e4e7;--chip:#f4f4f5}}
@media (prefers-color-scheme:dark){{:root{{--bg:#0a0a0a;--fg:#fafafa;--mfg:#a1a1aa;--line:#27272a;--chip:#18181b}}}}
*{{box-sizing:border-box;margin:0}}
body{{background:var(--bg);color:var(--fg);font:15px/1.5 "Google Sans Flex",system-ui,sans-serif;padding:40px}}
.k{{font-size:12px;font-weight:600;letter-spacing:.08em;text-transform:uppercase;color:var(--mfg)}}
h1{{font-size:34px;font-weight:650;margin:6px 0 18px;overflow-wrap:anywhere}}
p.readme{{max-width:62ch;font-size:16px;margin-bottom:22px}}
dl{{display:grid;gap:10px;border-top:1px solid var(--line);padding-top:16px;max-width:760px}}
.row{{display:grid;grid-template-columns:140px 1fr;gap:12px}}
dt{{color:var(--mfg)}}
dd{{overflow-wrap:anywhere}}
code{{font:13px ui-monospace,SFMono-Regular,Menlo,monospace}}
.chips{{display:flex;flex-wrap:wrap;gap:6px}}
.chip{{background:var(--chip);border:1px solid var(--line);border-radius:999px;padding:2px 10px;font-size:13px}}
</style></head><body>
<div class="k">Projekt-Info</div>
<h1>{name}</h1>
{readme}
<dl>
{rows}
</dl>
</body></html>"#)
}

/// Percent-encode a path segment.
pub fn url_escape(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        b => format!("%{b:02X}"),
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_card_has_only_present_fields() {
        let p = Project { id: "x".into(), name: "A<b>".into(), repo: Some("/r".into()), local_path: Some("/r".into()), keywords: vec!["k".into()], readme: None, source: None };
        let h = info_card_html(&p);
        assert!(h.contains("<h1>A&#60;b&#62;</h1>") && h.contains("Repository") && h.contains("chip"));
        assert!(!h.contains("Lokal") && !h.contains("readme\">"));
    }

    #[test]
    fn options_filter_by_name_and_keyword() {
        let mut d = ProjectDialog { list: Some(ProjectList { available: true, projects: vec![
            Project { id: "1".into(), name: "Shop".into(), keywords: vec!["retail".into()], ..Default::default() },
            Project { id: "2".into(), name: "Blog".into(), ..Default::default() },
        ] }), ..Default::default() };
        assert_eq!(d.options().len(), 3);
        d.query = TextEdit::new("RET");
        assert_eq!(d.options().iter().flatten().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["1"]);
        assert_eq!(url_escape("a b/c"), "a%20b%2Fc");
    }
}
