use serde_json::json;

use super::*;

fn body() -> Value {
    let opt = |id: &str| json!({ "id": id, "designId": format!("d{id}"), "label": format!(" {id} "), "title": "T", "html": "<p>x</p>" });
    json!({ "title": " Poll ", "decisions": [{ "id": "q1", "question": "Which?", "options": [opt("a"), opt("b")] }] })
}

fn poll() -> Poll {
    let (title, intro, decisions) = parse_create(&body()).unwrap();
    Poll { id: "abcdef1234".into(), title, intro, created_at: 1, decisions, votes: vec![] }
}

fn vote(voter_id: &str, voter: &str, option: &str) -> VoteRequest {
    VoteRequest { voter_id: voter_id.into(), voter: voter.into(), decision_id: "q1".into(), option_id: option.into(), comment: None }
}

#[test]
fn create_applies_defaults_and_trims() {
    let p = poll();
    assert_eq!(p.title, "Poll");
    assert_eq!(p.intro, "");
    assert_eq!(p.decisions[0]["why"], "");
    assert_eq!(p.decisions[0]["options"][0]["label"], "a");
    assert_eq!(p.decisions[0]["options"][0]["rationale"], "");
}

#[test]
fn create_reports_first_issue_like_issue_message() {
    let mut b = body();
    b["decisions"][0]["options"] = json!([b["decisions"][0]["options"][0].clone()]);
    let issues = parse_create(&b).err().unwrap();
    assert_eq!(util::issue_message(&issues, "Invalid poll."), "decisions.0.options: Every decision needs at least two options.");
    b["decisions"][0]["question"] = json!("  ");
    let issues = parse_create(&b).err().unwrap();
    assert_eq!(util::issue_message(&issues, ""), "decisions.0.question: Every decision needs a question.");
}

#[test]
fn revote_replaces_and_renames() {
    let mut p = poll();
    cast(&mut p, vote("voter1", "Ann", "a"), 10).unwrap();
    cast(&mut p, vote("voter2", "Bob", "a"), 11).unwrap();
    cast(&mut p, vote("voter1", "Anna", "b"), 12).unwrap();
    assert_eq!(p.votes.len(), 2);
    assert_eq!(p.votes[0].option_id, "b");
    assert_eq!(p.votes[0].voter, "Anna");
    assert_eq!(p.votes[0].created_at, 10);
    assert_eq!(cast(&mut p, VoteRequest { decision_id: "x".into(), ..vote("voter1", "A", "a") }, 1), Err("Unknown decision."));
    assert_eq!(cast(&mut p, vote("voter1", "A", "zz"), 1), Err("Unknown option."));
    let r = results(&p);
    assert_eq!(r["tallies"][0]["total"], 2);
    assert_eq!(r["tallies"][0]["leaders"], json!(["a", "b"]));
    assert_eq!(r["tallies"][0]["options"][1]["voters"], json!([{ "name": "Anna" }]));
}

#[test]
fn no_votes_means_no_leaders() {
    assert_eq!(results(&poll())["tallies"][0]["leaders"], json!([]));
}

#[test]
fn vote_validation() {
    assert!(parse_vote(&json!({ "voterId": "abc", "voter": "A", "decisionId": "q", "optionId": "o" })).is_err());
    let e = parse_vote(&json!({ "voterId": "abcd", "voter": " ", "decisionId": "q", "optionId": "o" })).err().unwrap();
    assert_eq!(util::issue_message(&e, ""), "voter: Please tell us your name.");
    let v = parse_vote(&json!({ "voterId": "abcd", "voter": " A ", "decisionId": "q", "optionId": "o", "comment": "  " })).ok().unwrap();
    assert_eq!(v.voter, "A");
    assert_eq!(v.comment.as_deref(), Some(""));
}

#[tokio::test]
async fn store_roundtrip_and_concurrent_votes() {
    let dir = tempfile::tempdir().unwrap();
    let polls = Polls::new(dir.path());
    polls.write(&poll()).await.unwrap();
    assert!(polls.exists("abcdef1234").await);
    assert!(!polls.exists("../etc").await);
    let tasks: Vec<_> = (0..20)
        .map(|i| {
            let polls = polls.clone();
            tokio::spawn(async move { polls.update("abcdef1234", |p| cast(p, vote(&format!("voter{i}"), "x", "a"), i)).await })
        })
        .collect();
    for t in tasks {
        t.await.unwrap().unwrap();
    }
    assert_eq!(polls.read("abcdef1234").await.unwrap().unwrap().votes.len(), 20);
    assert!(polls.update("missing123", |_| ()).await.unwrap().is_none());
}
