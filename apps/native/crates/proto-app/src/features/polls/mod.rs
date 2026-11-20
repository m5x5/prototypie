//! Polls and decisions, designer side (the reference's `features/poll`): collect competing designs in the poll
//! tray (card "Poll"), let the agent find the decisions (`POST /api/decisions`) or build them by hand, publish a
//! poll of HTML snapshots (`POST /api/polls`, sharing on the LAN first via `POST /api/polls/share`), read the
//! votes (`GET /api/polls/:id/votes`) and mark winners (stars the design). The voting page itself is served by
//! proto-server. Tray, title and draft decisions are per device; published polls and winners are shared records.

pub mod view;

use std::collections::BTreeMap;

use proto_core::{Change, PollDecision, PollOption, PublishedPoll, RecordKind, ops};
use rune::{Latest, TextEdit, Waker};
use serde::Deserialize;
use serde_json::json;

use crate::app::state::AppState;
use crate::features::{http, nav, notify};

#[derive(Clone, Debug, Default)]
pub struct DraftOption {
    pub design_id: String,
    pub label: TextEdit,
}

#[derive(Clone, Debug, Default)]
pub struct DraftDecision {
    pub id: String,
    pub question: TextEdit,
    pub why: TextEdit,
    pub options: Vec<DraftOption>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Tab {
    #[default]
    Build,
    Results,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct OptionTally {
    #[serde(rename = "optionId")]
    pub option_id: String,
    pub count: u32,
    #[serde(default)]
    pub voters: Vec<Voter>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Voter {
    pub name: String,
    pub comment: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Tally {
    #[serde(rename = "decisionId")]
    pub decision_id: String,
    pub total: u32,
    pub options: Vec<OptionTally>,
    #[serde(default)]
    pub leaders: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct VoteRow {
    #[serde(rename = "voterId")]
    pub voter_id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Results {
    #[serde(default)]
    pub votes: Vec<VoteRow>,
    #[serde(default)]
    pub tallies: Vec<Tally>,
}

#[derive(Clone, Debug, Deserialize)]
struct Suggested {
    question: String,
    #[serde(default)]
    why: String,
    options: Vec<SuggestedOption>,
}

#[derive(Clone, Debug, Deserialize)]
struct SuggestedOption {
    #[serde(rename = "designId")]
    design_id: String,
    label: String,
}

#[derive(Default)]
pub struct PollsState {
    /// Design ids in the tray, in order added.
    pub candidates: Vec<String>,
    pub title: TextEdit,
    pub decisions: Vec<DraftDecision>,
    pub context: TextEdit,
    /// The Decisions popover (from the ⋯ menu) and the big panel.
    pub dock_open: bool,
    pub panel_open: bool,
    pub tab: Tab,
    pub results_poll: Option<String>,
    /// The last poll published from Build (its share banner).
    pub just_published: Option<String>,
    pub finding: bool,
    pub find_error: Option<String>,
    pub publishing: bool,
    /// Votes per poll id, as last loaded (`Err` = why it failed).
    pub results: BTreeMap<String, Result<Results, String>>,
    find_req: Latest<Result<Vec<Suggested>, String>>,
    publish_req: Latest<Result<(String, String, String), String>>,
    results_req: Latest<(String, Result<Results, String>)>,
    /// LAN sharing as proto-server last reported it (`GET /api/polls/share`): Some(true) = shared.
    pub sharing: Option<bool>,
    share_req: Latest<Option<bool>>,
}

impl PollsState {
    pub fn setup(&mut self, waker: Waker) {
        self.find_req.set_waker(waker.clone());
        self.publish_req.set_waker(waker.clone());
        self.results_req.set_waker(waker.clone());
        self.share_req.set_waker(waker);
    }

    pub fn in_poll(&self, id: &str) -> bool {
        self.candidates.iter().any(|c| c == id)
    }
}

/// Card "Poll" / "In poll". Removing a candidate also takes it out of every draft decision.
pub fn toggle_candidate(s: &mut AppState, id: &str) {
    let p = &mut s.features.polls;
    if p.in_poll(id) {
        p.candidates.retain(|c| c != id);
        for d in &mut p.decisions {
            d.options.retain(|o| o.design_id != id);
        }
    } else if s.snap().design(id).is_some_and(|d| d.is_ready()) {
        s.features.polls.candidates.push(id.to_string());
    }
}

/// "N in poll · M shared" or "Share a poll".
pub fn status(s: &AppState) -> String {
    let n = s.features.polls.candidates.iter().filter(|id| s.snap().design(id).is_some()).count();
    let shared = s.snap().published_polls().count();
    let parts: Vec<String> = [(n > 0).then(|| format!("{n} in poll")), (shared > 0).then(|| format!("{shared} shared"))].into_iter().flatten().collect();
    if parts.is_empty() { "Share a poll".into() } else { parts.join(" · ") }
}

/// Published polls, newest first.
pub fn published(s: &AppState) -> Vec<std::sync::Arc<PublishedPoll>> {
    let mut v: Vec<_> = s.snap().published_polls().collect();
    v.sort_by(|a, b| b.created_at.total_cmp(&a.created_at));
    v
}

/// Ask proto-server whether polls are shared on the LAN (for "Geteilt im LAN" / "nur lokal").
pub fn load_sharing(s: &mut AppState) {
    s.features.polls.share_req.request(http::server_addr(), |a| {
        http::request(&a, "GET", "/api/polls/share", None).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()).and_then(|v| v["sharing"].as_bool())
    });
}

pub fn open_panel(s: &mut AppState, tab: Tab) {
    let p = &mut s.features.polls;
    p.dock_open = false;
    p.panel_open = true;
    p.tab = tab;
}

pub fn show_results(s: &mut AppState, poll_id: &str) {
    open_panel(s, Tab::Results);
    s.features.polls.results_poll = Some(poll_id.to_string());
    load_results(s, poll_id);
}

pub fn add_decision(s: &mut AppState) {
    s.features.polls.decisions.push(DraftDecision { id: ops::new_id(8), ..Default::default() });
}

pub fn move_decision(s: &mut AppState, id: &str, delta: isize) {
    let d = &mut s.features.polls.decisions;
    let Some(i) = d.iter().position(|x| x.id == id) else { return };
    let j = i as isize + delta;
    if j >= 0 && (j as usize) < d.len() {
        d.swap(i, j as usize);
    }
}

/// The first problem that blocks publishing, or None.
pub fn validate(s: &AppState) -> Option<String> {
    let ds = &s.features.polls.decisions;
    if ds.is_empty() {
        return Some("Add at least one decision.".into());
    }
    for (i, d) in ds.iter().enumerate() {
        let n = format!("Decision {}", i + 1);
        if d.question.text.trim().is_empty() {
            return Some(format!("{n} needs a question."));
        }
        if d.options.len() < 2 {
            return Some(format!("{n} needs at least two options."));
        }
        if d.options.iter().any(|o| !s.snap().design(&o.design_id).is_some_and(|x| x.is_ready())) {
            return Some(format!("{n} has an option that isn't ready."));
        }
    }
    None
}

/// "Let the agent find the decisions" (needs 2+ ready candidates).
pub fn find_decisions(s: &mut AppState) {
    let designs: Vec<serde_json::Value> = s
        .features
        .polls
        .candidates
        .iter()
        .filter_map(|id| s.snap().design(id))
        .filter(|d| d.is_ready() && !d.html.is_empty())
        .map(|d| json!({"id": d.id, "title": d.title, "rationale": d.rationale, "html": d.html}))
        .collect();
    if designs.len() < 2 {
        return;
    }
    let context = s.features.polls.context.text.trim().to_string();
    let mut body = json!({ "designs": designs });
    if !context.is_empty() {
        body["context"] = json!(context);
    }
    let p = &mut s.features.polls;
    p.finding = true;
    p.find_error = None;
    p.find_req.request((http::server_addr(), body.to_string()), |(a, body)| {
        #[derive(Deserialize)]
        struct Resp {
            decisions: Vec<Suggested>,
        }
        http::request(&a, "POST", "/api/decisions", Some(&body)).and_then(|b| serde_json::from_slice::<Resp>(&b).map(|r| r.decisions).map_err(|e| e.to_string()))
    });
}

/// "Publish poll": share on the LAN first (best effort), then POST the poll with HTML snapshots and record it.
pub fn publish(s: &mut AppState) {
    if validate(s).is_some() || s.features.polls.publishing {
        return;
    }
    let p = &s.features.polls;
    let title = if p.title.text.trim().is_empty() { "Which direction should we take?".to_string() } else { p.title.text.trim().to_string() };
    let decisions: Vec<serde_json::Value> = p
        .decisions
        .iter()
        .map(|d| {
            let options: Vec<serde_json::Value> = d
                .options
                .iter()
                .filter_map(|o| {
                    let x = s.snap().design(&o.design_id)?;
                    let label = if o.label.text.trim().is_empty() { x.title.clone() } else { o.label.text.trim().to_string() };
                    Some(json!({"id": ops::new_id(8), "designId": x.id, "label": label, "title": x.title, "rationale": x.rationale, "html": x.html}))
                })
                .collect();
            json!({"id": d.id, "question": d.question.text.trim(), "why": d.why.text.trim(), "options": options})
        })
        .collect();
    let body = json!({"title": title, "intro": "", "decisions": decisions});
    s.features.polls.publishing = true;
    s.features.polls.publish_req.request((http::server_addr(), body.to_string()), |(a, body)| {
        // Sharing may fail (no LAN interface): publish anyway, the link is then local only.
        let _ = http::request(&a, "POST", "/api/polls/share", Some("{}"));
        let resp = http::request(&a, "POST", "/api/polls", Some(&body))?;
        let v: serde_json::Value = serde_json::from_slice(&resp).map_err(|e| e.to_string())?;
        let id = v["id"].as_str().ok_or("no poll id")?.to_string();
        let path = v["path"].as_str().unwrap_or("").to_string();
        let origin = v["lanOrigins"].as_array().and_then(|o| o.first()).and_then(|o| o.as_str()).map(str::to_string).unwrap_or_else(|| format!("http://{a}"));
        Ok((id, format!("{origin}{path}"), body))
    });
}

pub fn load_results(s: &mut AppState, poll_id: &str) {
    let (a, id) = (http::server_addr(), poll_id.to_string());
    s.features.polls.results_req.request((a, id), |(a, id)| {
        let r = http::request(&a, "GET", &format!("/api/polls/{id}/votes"), None).and_then(|b| serde_json::from_slice::<Results>(&b).map_err(|e| e.to_string()));
        (id, r)
    });
}

/// "Forget this poll": remove it from the list (the shared link keeps working).
pub fn forget(s: &mut AppState, poll_id: &str) {
    nav::apply(s, vec![Change::delete(RecordKind::PublishedPolls.path(poll_id))], "forget poll");
    notify(s, "Removed from your list", "The shared link keeps working.");
}

/// "Mark winner" / unmark: records the winner and stars its design; the previous winner is unstarred unless
/// it wins another decision.
pub fn toggle_winner(s: &mut AppState, poll: &PublishedPoll, decision_id: &str, option: &PollOption) {
    let key = format!("{}/{decision_id}", poll.id);
    let current = s.snap().poll_winner(&key).map(|w| w.to_string());
    let decision = poll.decisions.iter().find(|d| d.id == decision_id);
    let mut changes = Vec::new();
    if let Some(prev) = current.as_deref().and_then(|w| decision.and_then(|d| d.options.iter().find(|o| o.id == w)))
        && !wins_elsewhere(s, &key, &prev.design_id)
        && let Some(c) = ops::set_starred(s.snap(), &prev.design_id, false)
    {
        changes.push(c);
    }
    if current.as_deref() == Some(option.id.as_str()) {
        changes.push(ops::set_poll_winner(&poll.id, decision_id, None));
        nav::apply(s, changes, "poll winner");
        return;
    }
    changes.push(ops::set_poll_winner(&poll.id, decision_id, Some(&option.id)));
    let exists = s.snap().design(&option.design_id).is_some();
    if let Some(c) = ops::set_starred(s.snap(), &option.design_id, true) {
        changes.push(c);
    }
    nav::apply(s, changes, "poll winner");
    if exists {
        notify(s, "Winner starred on the canvas", "");
    } else {
        notify(s, "Winner marked", "The original design is no longer on the canvas.");
    }
}

fn wins_elsewhere(s: &AppState, except_key: &str, design_id: &str) -> bool {
    for p in s.snap().published_polls() {
        for d in &p.decisions {
            let key = format!("{}/{}", p.id, d.id);
            if key == except_key {
                continue;
            }
            if let Some(w) = s.snap().poll_winner(&key)
                && d.options.iter().any(|o| o.id == *w && o.design_id == design_id)
            {
                return true;
            }
        }
    }
    false
}

/// Land answers; true when something visible changed.
pub fn poll(s: &mut AppState) -> bool {
    let mut changed = false;
    if let Some(r) = s.features.polls.find_req.take() {
        changed = true;
        s.features.polls.finding = false;
        match r {
            Ok(found) => {
                let candidates = s.features.polls.candidates.clone();
                let ds: Vec<DraftDecision> = found
                    .into_iter()
                    .map(|d| DraftDecision {
                        id: ops::new_id(8),
                        question: TextEdit::new(d.question),
                        why: TextEdit::new(d.why),
                        options: d.options.into_iter().filter(|o| candidates.contains(&o.design_id)).map(|o| DraftOption { design_id: o.design_id, label: TextEdit::new(o.label) }).collect(),
                    })
                    .collect();
                let n = ds.len();
                s.features.polls.decisions = ds;
                if s.features.polls.title.text.trim().is_empty() {
                    s.features.polls.title = TextEdit::new("Which direction should we take?");
                }
                notify(s, &format!("The agent found {n} decision{}", if n == 1 { "" } else { "s" }), "");
            }
            Err(e) => s.features.polls.find_error = Some(e),
        }
    }
    if let Some(r) = s.features.polls.publish_req.take() {
        changed = true;
        s.features.polls.publishing = false;
        match r {
            Ok((id, share_url, body)) => {
                let req: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                let decisions = req["decisions"].as_array().cloned().unwrap_or_default().iter().map(|d| PollDecision {
                    id: d["id"].as_str().unwrap_or("").into(),
                    question: d["question"].as_str().unwrap_or("").into(),
                    options: d["options"].as_array().cloned().unwrap_or_default().iter().map(|o| PollOption {
                        id: o["id"].as_str().unwrap_or("").into(),
                        design_id: o["designId"].as_str().unwrap_or("").into(),
                        label: o["label"].as_str().unwrap_or("").into(),
                        title: o["title"].as_str().unwrap_or("").into(),
                        extra: Default::default(),
                    }).collect(),
                    extra: Default::default(),
                }).collect();
                let poll = PublishedPoll { id: id.clone(), title: req["title"].as_str().unwrap_or("").into(), created_at: ops::now_ms(), share_url, decisions, extra: Default::default() };
                if let Ok(c) = Change::put(RecordKind::PublishedPolls, &id, &poll) {
                    nav::apply(s, vec![c], "publish poll");
                }
                s.features.polls.just_published = Some(id);
                load_sharing(s);
                notify(s, "Poll published", "");
            }
            Err(e) => notify(s, "Couldn't publish the poll", &e),
        }
    }
    if let Some(sh) = s.features.polls.share_req.take() {
        s.features.polls.sharing = sh;
        changed = true;
    }
    if let Some((id, r)) = s.features.polls.results_req.take() {
        s.features.polls.results.insert(id, r);
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        let mut s = AppState::new();
        s.use_store(crate::features::scenes::fixture());
        s
    }

    #[test]
    fn tray_and_validation() {
        let mut s = state();
        toggle_candidate(&mut s, "zzv1");
        toggle_candidate(&mut s, "zzv2");
        assert_eq!(status(&s), "2 in poll");
        assert_eq!(validate(&s).as_deref(), Some("Add at least one decision."));
        add_decision(&mut s);
        assert_eq!(validate(&s).as_deref(), Some("Decision 1 needs a question."));
        s.features.polls.decisions[0].question = TextEdit::new("Which steps?");
        s.features.polls.decisions[0].options = vec![DraftOption { design_id: "zzv1".into(), ..Default::default() }];
        assert_eq!(validate(&s).as_deref(), Some("Decision 1 needs at least two options."));
        s.features.polls.decisions[0].options.push(DraftOption { design_id: "zzv2".into(), ..Default::default() });
        assert_eq!(validate(&s), None);
        toggle_candidate(&mut s, "zzv2");
        assert_eq!(s.features.polls.decisions[0].options.len(), 1, "removing a candidate drops its options");
    }

    #[test]
    fn winners_star_and_unstar() {
        let mut s = state();
        let opt = |id: &str, d: &str| PollOption { id: id.into(), design_id: d.into(), label: d.into(), title: d.into(), extra: Default::default() };
        let poll = PublishedPoll { id: "pl1".into(), title: "t".into(), created_at: 1.0, share_url: "u".into(),
            decisions: vec![PollDecision { id: "d1".into(), question: "q".into(), options: vec![opt("o2", "zzv2"), opt("o3", "zzv3")], extra: Default::default() }], extra: Default::default() };
        toggle_winner(&mut s, &poll, "d1", &poll.decisions[0].options[0]);
        assert!(s.snap().design("zzv2").unwrap().is_starred());
        toggle_winner(&mut s, &poll, "d1", &poll.decisions[0].options[1]);
        assert!(!s.snap().design("zzv2").unwrap().is_starred() && s.snap().design("zzv3").unwrap().is_starred());
        assert_eq!(s.snap().poll_winner("pl1/d1").as_deref(), Some("o3"));
    }
}
