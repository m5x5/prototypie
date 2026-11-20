//! Stakeholder polls (reference `src/features/poll/{schema,server-store}.ts`): `<data>/polls/<id>.json`, atomic
//! writes, read-modify-write serialised per poll.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::util::{self, Check, Issue, p};

const MAX_HTML: usize = 2_000_000;

pub fn is_poll_id(id: &str) -> bool {
    (6..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Vote {
    pub id: String,
    pub voter_id: String,
    pub voter: String,
    pub decision_id: String,
    pub option_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A poll file. Decisions stay JSON (they carry full HTML snapshots and are only validated on create).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Poll {
    pub id: String,
    pub title: String,
    pub intro: String,
    pub created_at: i64,
    pub decisions: Vec<Value>,
    pub votes: Vec<Vote>,
}

/// A validated vote request.
pub struct VoteRequest {
    pub voter_id: String,
    pub voter: String,
    pub decision_id: String,
    pub option_id: String,
    pub comment: Option<String>,
}

#[derive(Clone)]
pub struct Polls {
    dir: PathBuf,
    locks: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

impl Polls {
    pub fn new(data_dir: &Path) -> Self {
        Self { dir: data_dir.join("polls"), locks: Arc::default() }
    }

    fn file(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    pub async fn read(&self, id: &str) -> anyhow::Result<Option<Poll>> {
        if !is_poll_id(id) {
            return Ok(None);
        }
        match tokio::fs::read_to_string(self.file(id)).await {
            Ok(txt) => Ok(Some(serde_json::from_str(&txt)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn exists(&self, id: &str) -> bool {
        is_poll_id(id) && tokio::fs::try_exists(self.file(id)).await.unwrap_or(false)
    }

    pub async fn write(&self, poll: &Poll) -> anyhow::Result<()> {
        anyhow::ensure!(is_poll_id(&poll.id), "Invalid poll id");
        tokio::fs::create_dir_all(&self.dir).await?;
        let file = self.file(&poll.id);
        let tmp = self.dir.join(format!("{}.{}.{}.tmp", poll.id, std::process::id(), util::nanoid(6)));
        tokio::fs::write(&tmp, serde_json::to_vec(poll)?).await?;
        tokio::fs::rename(&tmp, &file).await?;
        Ok(())
    }

    /// Read, change and write one poll with no other change to it in between. None when the poll is missing.
    pub async fn update<T>(&self, id: &str, f: impl FnOnce(&mut Poll) -> T) -> anyhow::Result<Option<(Poll, T)>> {
        let lock = self.locks.lock().unwrap().entry(id.to_string()).or_default().clone();
        let _guard = lock.lock().await;
        let Some(mut poll) = self.read(id).await? else { return Ok(None) };
        let out = f(&mut poll);
        self.write(&poll).await?;
        Ok(Some((poll, out)))
    }
}

/// `{ title, intro, decisions }` of POST /api/polls, with zod defaults applied.
pub fn parse_create(v: &Value) -> Result<(String, String, Vec<Value>), Vec<Issue>> {
    let mut c = Check::default();
    if !v.is_object() {
        c.push(&[], "invalid_type", "Expected object, received null");
        return Err(c.issues);
    }
    let title = c.string(&v["title"], &p(&[&"title"]), true, 1, 200);
    let intro = c.opt_string(&v["intro"], &p(&[&"intro"]), false, 0, 2000).unwrap_or_default();
    let decisions = list(&mut c, &v["decisions"], &p(&[&"decisions"]), 1, 8, None, |c, d, at| parse_decision(c, d, at));
    match title {
        Some(t) if c.ok() => Ok((t, intro, decisions)),
        _ => Err(c.issues),
    }
}

fn list(
    c: &mut Check,
    v: &Value,
    at: &[Value],
    min: usize,
    max: usize,
    min_msg: Option<&str>,
    mut each: impl FnMut(&mut Check, &Value, &[Value]) -> Option<Value>,
) -> Vec<Value> {
    let Some(arr) = v.as_array() else {
        c.push(at, "invalid_type", if v.is_null() { "Required" } else { "Expected array" });
        return Vec::new();
    };
    if arr.len() < min {
        c.push(at, "too_small", min_msg.map(str::to_string).unwrap_or_else(|| format!("Array must contain at least {min} element(s)")));
    }
    if arr.len() > max {
        c.push(at, "too_big", format!("Array must contain at most {max} element(s)"));
    }
    arr.iter()
        .enumerate()
        .filter_map(|(i, x)| {
            let mut path = at.to_vec();
            path.push(json!(i));
            each(c, x, &path)
        })
        .collect()
}

fn sub(at: &[Value], k: &str) -> Vec<Value> {
    let mut x = at.to_vec();
    x.push(json!(k));
    x
}

fn parse_decision(c: &mut Check, v: &Value, at: &[Value]) -> Option<Value> {
    let id = c.string(&v["id"], &sub(at, "id"), false, 1, 64);
    let question = match v["question"].as_str() {
        Some(q) if q.trim().is_empty() => {
            c.push(&sub(at, "question"), "too_small", "Every decision needs a question.");
            None
        }
        _ => c.string(&v["question"], &sub(at, "question"), true, 1, 300),
    };
    let why = c.opt_string(&v["why"], &sub(at, "why"), false, 0, 1000).unwrap_or_default();
    let options = list(c, &v["options"], &sub(at, "options"), 2, 4, Some("Every decision needs at least two options."), parse_option);
    Some(json!({ "id": id?, "question": question?, "why": why, "options": options }))
}

fn parse_option(c: &mut Check, v: &Value, at: &[Value]) -> Option<Value> {
    let id = c.string(&v["id"], &sub(at, "id"), false, 1, 64);
    let design_id = c.string(&v["designId"], &sub(at, "designId"), false, 1, 64);
    let label = c.string(&v["label"], &sub(at, "label"), true, 1, 120);
    let title = c.string(&v["title"], &sub(at, "title"), false, 0, 300);
    let rationale = c.opt_string(&v["rationale"], &sub(at, "rationale"), false, 0, 4000).unwrap_or_default();
    let html = c.string(&v["html"], &sub(at, "html"), false, 1, MAX_HTML);
    Some(json!({ "id": id?, "designId": design_id?, "label": label?, "title": title?, "rationale": rationale, "html": html? }))
}

pub fn parse_vote(v: &Value) -> Result<VoteRequest, Vec<Issue>> {
    let mut c = Check::default();
    if !v.is_object() {
        c.push(&[], "invalid_type", "Expected object, received null");
        return Err(c.issues);
    }
    let voter_id = c.string(&v["voterId"], &p(&[&"voterId"]), false, 4, 64);
    let voter = match v["voter"].as_str() {
        Some(s) if s.trim().is_empty() => {
            c.push(&p(&[&"voter"]), "too_small", "Please tell us your name.");
            None
        }
        _ => c.string(&v["voter"], &p(&[&"voter"]), true, 1, 80),
    };
    let decision_id = c.string(&v["decisionId"], &p(&[&"decisionId"]), false, 1, 64);
    let option_id = c.string(&v["optionId"], &p(&[&"optionId"]), false, 1, 64);
    let comment = c.opt_string(&v["comment"], &p(&[&"comment"]), true, 0, 2000);
    match (voter_id, voter, decision_id, option_id) {
        (Some(voter_id), Some(voter), Some(decision_id), Some(option_id)) if c.ok() => Ok(VoteRequest { voter_id, voter, decision_id, option_id, comment }),
        _ => Err(c.issues),
    }
}

/// Apply a vote: one per voter per decision (a re-vote replaces it); the voter's name is kept the same across
/// their votes. Err = "Unknown decision." / "Unknown option.".
pub fn cast(poll: &mut Poll, v: VoteRequest, now: i64) -> Result<(), &'static str> {
    let decision = poll.decisions.iter().find(|d| d["id"].as_str() == Some(&v.decision_id)).ok_or("Unknown decision.")?;
    let known = decision["options"].as_array().is_some_and(|o| o.iter().any(|o| o["id"].as_str() == Some(&v.option_id)));
    if !known {
        return Err("Unknown option.");
    }
    let comment = v.comment.filter(|c| !c.is_empty());
    match poll.votes.iter_mut().find(|x| x.voter_id == v.voter_id && x.decision_id == v.decision_id) {
        Some(x) => {
            x.voter = v.voter.clone();
            x.option_id = v.option_id;
            x.comment = comment;
            x.updated_at = now;
        }
        None => poll.votes.push(Vote {
            id: util::nanoid(10),
            voter_id: v.voter_id.clone(),
            voter: v.voter.clone(),
            decision_id: v.decision_id,
            option_id: v.option_id,
            comment,
            created_at: now,
            updated_at: now,
        }),
    }
    for x in poll.votes.iter_mut().filter(|x| x.voter_id == v.voter_id) {
        x.voter = v.voter.clone();
    }
    Ok(())
}

/// `{ votes, tallies }`.
pub fn results(poll: &Poll) -> Value {
    let tallies: Vec<Value> = poll.decisions.iter().map(|d| tally(d, &poll.votes)).collect();
    json!({ "votes": poll.votes, "tallies": tallies })
}

fn tally(decision: &Value, votes: &[Vote]) -> Value {
    let id = decision["id"].as_str().unwrap_or_default();
    let mine: Vec<&Vote> = votes.iter().filter(|v| v.decision_id == id).collect();
    let options: Vec<(String, Value, usize)> = decision["options"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|o| {
            let oid = o["id"].as_str().unwrap_or_default().to_string();
            let mut vs: Vec<&&Vote> = mine.iter().filter(|v| v.option_id == oid).collect();
            vs.sort_by_key(|v| v.created_at);
            let voters: Vec<Value> = vs
                .iter()
                .map(|v| match &v.comment {
                    Some(c) => json!({ "name": v.voter, "comment": c }),
                    None => json!({ "name": v.voter }),
                })
                .collect();
            let n = vs.len();
            (oid.clone(), json!({ "optionId": oid, "count": n, "voters": voters }), n)
        })
        .collect();
    let max = options.iter().map(|o| o.2).max().unwrap_or(0);
    let leaders: Vec<String> = if max > 0 { options.iter().filter(|o| o.2 == max).map(|o| o.0.clone()).collect() } else { Vec::new() };
    json!({
        "decisionId": id,
        "total": mine.len(),
        "options": options.into_iter().map(|o| o.1).collect::<Vec<_>>(),
        "leaders": leaders,
    })
}

#[cfg(test)]
#[path = "polls_tests.rs"]
mod tests;
