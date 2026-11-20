//! Entry points end to end with a scripted provider (no HTTP) and a temp data dir for the queue.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::json;

use super::*;
use crate::llm::Completion;
use crate::llm::mock::MockProvider;
use crate::queue::{NoNotifier, QueueStatus};

fn ai(dir: &std::path::Path, replies: Vec<Result<Completion, AiError>>) -> (Ai, Arc<MockProvider>) {
    let p = Arc::new(MockProvider::new(replies));
    (Ai::new(dir.to_path_buf(), Some(p.clone()), Arc::new(NoNotifier)), p)
}

async fn all_events(ai: &Ai, id: &str) -> Vec<VariationEvent> {
    tokio::time::timeout(Duration::from_secs(5), ai.jobs().subscribe(id, 0).unwrap().collect()).await.expect("job finished")
}

fn tool(v: serde_json::Value) -> Result<Completion, AiError> {
    Ok(Completion::ToolCall(v))
}

#[tokio::test]
async fn codex_variations_plan_then_build_each() {
    let dir = tempfile::tempdir().unwrap();
    let (ai, p) = ai(
        dir.path(),
        vec![
            tool(json!({"directions":[{"title":"Dock","brief":"b1"},{"title":"Rail","brief":"b2"},{"title":"Extra","brief":"x"}]})),
            tool(json!({"title":"Dock","rationale":"r","html":"<p>1"})),
            Err(AiError::Llm("rate limited".into())),
        ],
    );
    let g = Guidelines { card_id: "c".into(), title: "Rules".into(), from_space: "Main canvas".into(), text: "Blue".into() };
    let req = VariationRequest { instruction: "toolbar".into(), count: 2.0, base_html: Some("<b>".into()), ..Default::default() };
    let id = ai.start_variations(req, Some(g));
    let ev = all_events(&ai, &id).await;
    assert_eq!(
        ev[0],
        VariationEvent::Plan {
            directions: vec![Direction { title: "Dock".into(), brief: "b1".into() }, Direction { title: "Rail".into(), brief: "b2".into() }]
        }
    );
    // The two builds run in parallel: one success and one error, in either order, then done.
    assert_eq!(ev.len(), 4);
    assert!(ev[1..3].iter().any(|e| matches!(e, VariationEvent::Variation { variation, .. } if variation.html == "<p>1")));
    assert!(ev[1..3].iter().any(|e| matches!(e, VariationEvent::Error { index: Some(_), message } if message == "rate limited")));
    assert_eq!(ev[3], VariationEvent::Done);

    let reqs = p.requests.lock().unwrap();
    assert!(reqs[0].system.contains("<design_guidelines>\nBlue\n</design_guidelines>"));
    assert!(reqs[0].prompt.contains("Propose exactly 2 clearly distinct design directions"));
    assert_eq!(reqs[0].reasoning, Reasoning::Low);
    assert!(reqs[1].prompt.contains("Build this direction:\nTitle: "));
    assert_eq!(reqs[1].reasoning, Reasoning::Medium);
}

#[tokio::test]
async fn retry_with_direction_skips_planning_and_plan_failure_is_job_error() {
    let dir = tempfile::tempdir().unwrap();
    let (ai, p) = ai(dir.path(), vec![tool(json!({"title":"T","rationale":"r","html":"h"}))]);
    let d = Direction { title: "Only".into(), brief: "b".into() };
    let id = ai.start_variations(VariationRequest { direction: Some(d.clone()), count: 5.0, ..Default::default() }, None);
    let ev = all_events(&ai, &id).await;
    assert_eq!(ev[0], VariationEvent::Plan { directions: vec![d] });
    assert!(matches!(ev[1], VariationEvent::Variation { index: 0, .. }));
    assert_eq!(p.requests.lock().unwrap().len(), 1);

    let (ai, _) = ai_without_provider(dir.path());
    let id = ai.start_variations(VariationRequest::default(), None);
    let ev = all_events(&ai, &id).await;
    assert_eq!(ev, vec![VariationEvent::Error { index: None, message: crate::llm::NOT_LOGGED_IN.into() }, VariationEvent::Done]);
}

fn ai_without_provider(dir: &std::path::Path) -> (Ai, ()) {
    (Ai::new(dir.to_path_buf(), None, Arc::new(NoNotifier)), ())
}

async fn pending(ai: &Ai, n: usize) -> Vec<QueueItem> {
    for _ in 0..400 {
        let items = ai.queue().list_pending().await;
        if items.len() >= n {
            return items;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("queue never had {n} items");
}

#[tokio::test]
async fn claude_variations_go_through_the_queue() {
    let dir = tempfile::tempdir().unwrap();
    let (ai, p) = ai(dir.path(), vec![]);
    let req = VariationRequest {
        instruction: "make it pop".into(),
        count: 2.0,
        agent: Some(AgentChoice::Claude),
        base_html: Some("<base>".into()),
        bases: Some(vec![]),
        pod_context: Some(vec![]),
        ..Default::default()
    };
    let g = Guidelines { card_id: "c".into(), title: "Rules".into(), from_space: "S".into(), text: "t".into() };
    let id = ai.start_variations(req, Some(g));
    let items = pending(&ai, 2).await;
    assert_eq!(items[0].direction.brief, "make it pop");
    assert!(items.iter().any(|i| i.direction.title == "Direction 2"));
    assert_eq!(items[0].base_html.as_deref(), Some("<base>"), "empty bases don't replace baseHtml");
    assert!(items[0].bases.is_none() && items[0].pod_context.is_none());
    assert_eq!(items[0].guidelines.as_ref().unwrap().title, "Rules");

    let v = Variation { title: "T".into(), rationale: "R".into(), html: "<p>".into(), viewport: Some(640), viewport_height: None };
    assert!(ai.queue().submit(&id, 0, v.clone()).await);
    assert!(ai.queue().cancel_item(&id, 1, "Card deleted").await);
    let ev = all_events(&ai, &id).await;
    assert!(ev.contains(&VariationEvent::Variation { index: 0, variation: v }));
    assert!(ev.contains(&VariationEvent::Error { index: Some(1), message: "Card deleted".into() }));
    assert!(p.requests.lock().unwrap().is_empty(), "queue mode never calls the LLM");
}

#[tokio::test]
async fn cancelling_a_queue_job_cancels_its_items() {
    let dir = tempfile::tempdir().unwrap();
    let (ai, _) = ai(dir.path(), vec![]);
    let id = ai.start_variations(VariationRequest { count: 1.0, agent: Some(AgentChoice::Claude), ..Default::default() }, None);
    pending(&ai, 1).await;
    ai.jobs().cancel(&id);
    let ev = all_events(&ai, &id).await;
    assert!(ev.contains(&VariationEvent::Error { index: Some(0), message: "Cancelled".into() }));
    assert_eq!(ai.queue().load(&id, 0).await.unwrap().status, QueueStatus::Cancelled);
}

#[tokio::test]
async fn imports_need_claude_and_build_an_import_item() {
    let dir = tempfile::tempdir().unwrap();
    let (ai, _) = ai(dir.path(), vec![]);
    let from = ImportFrom { element: "  Toolbar ".into(), source: "/repo".into(), screenshot: Some("data:image/png;base64,iVBORw0K".into()) };
    let id = ai.start_variations(VariationRequest { import_from: Some(from.clone()), ..Default::default() }, None);
    let ev = all_events(&ai, &id).await;
    assert_eq!(ev[0], VariationEvent::Plan { directions: vec![Direction { title: "Toolbar".into(), brief: "/repo".into() }] });
    assert!(matches!(&ev[1], VariationEvent::Error { index: Some(0), message } if message.starts_with("Importing from the app needs the Claude agent")));

    let id = ai.start_variations(
        VariationRequest { import_from: Some(from), agent: Some(AgentChoice::Claude), ..Default::default() },
        None,
    );
    let item = pending(&ai, 1).await.remove(0);
    assert_eq!(item.mode, Some(QueueMode::Import));
    assert_eq!(item.element.as_deref(), Some("Toolbar"));
    assert_eq!(item.focus.as_deref(), Some("Toolbar"));
    assert_eq!(item.scope, Some(Scope::Component));
    assert_eq!(item.direction.brief, "Recreate \"Toolbar\" exactly as it is in /repo.");
    assert!(item.instruction.starts_with("Import the existing \"Toolbar\" from the app at /repo:"));
    assert!(item.import_screenshot.as_deref().unwrap().ends_with(&format!("{id}-screenshot.png")));
    ai.jobs().cancel(&id);
}

#[tokio::test]
async fn edit_codex_and_claude() {
    let dir = tempfile::tempdir().unwrap();
    let (ai, p) = ai(dir.path(), vec![tool(json!({"rationale":"Made it blue","html":"<p class=blue>"}))]);
    let d = EditTarget { id: "d1".into(), title: "Card".into(), html: "<p>".into(), space_id: "s".into() };
    let req = EditRequest { design_id: "d1".into(), instruction: "blue".into(), ..Default::default() };
    let id = ai.start_edit(d.clone(), req.clone(), None);
    let ev = all_events(&ai, &id).await;
    assert_eq!(ev[0], VariationEvent::Plan { directions: vec![Direction { title: "Card".into(), brief: "blue".into() }] });
    assert_eq!(
        ev[1],
        VariationEvent::Variation {
            index: 0,
            variation: Variation { title: "Card".into(), rationale: "Made it blue".into(), html: "<p class=blue>".into(), viewport: None, viewport_height: None }
        }
    );
    assert!(p.requests.lock().unwrap()[0].system.starts_with("You are a senior product designer who edits HTML prototypes in place."));

    let id = ai.start_edit(d, EditRequest { agent: Some(AgentChoice::Claude), annotations: Some(vec![]), ..req }, None);
    let item = pending(&ai, 1).await.remove(0);
    assert_eq!(item.mode, Some(QueueMode::Edit));
    assert_eq!(item.target_design_id.as_deref(), Some("d1"));
    assert_eq!(item.base_html.as_deref(), Some("<p>"));
    assert_eq!(item.direction.title, "Edit in place: Card");
    assert!(item.annotations.is_none());
    ai.jobs().cancel(&id);
    let ev = all_events(&ai, &id).await;
    assert_eq!(ev[1], VariationEvent::Error { index: Some(0), message: "Cancelled".into() });
}

#[tokio::test]
async fn spokes_and_decisions() {
    let dir = tempfile::tempdir().unwrap();
    let comps: Vec<_> = (0..10).map(|i| json!({"name":format!("C{i}"),"description":"d","selector":"s"})).collect();
    let (ai, _) = ai(
        dir.path(),
        vec![
            tool(json!({ "components": comps })),
            tool(json!({"components": []})),
            tool(json!({"decisions":[
                {"question":"  Pricing? ","why":"w","options":[{"designId":"a","label":" Up front "},{"designId":"a","label":"dup"},{"designId":"zzz","label":"x"},{"designId":"b","label":""}]},
                {"question":"Tone?","why":"w","options":[{"designId":"a","label":"Only one"}]}
            ]})),
            tool(json!({"decisions":[]})),
        ],
    );
    let c = CancellationToken::new();
    assert_eq!(ai.spokes("<p>", None, c.clone()).await.unwrap().len(), 8);
    assert_eq!(ai.spokes("<p>", None, c.clone()).await, Err(AiError::Empty("The model found no components.".into())));
    let designs: Vec<DecisionCandidate> = ["a", "b"]
        .iter()
        .map(|id| DecisionCandidate { id: id.to_string(), title: "T".into(), rationale: "".into(), html: "<p>".into() })
        .collect();
    let d = ai.decisions(&designs, None, c.clone()).await.unwrap();
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].question, "Pricing?");
    assert_eq!(
        d[0].options,
        vec![
            DecisionOption { design_id: "a".into(), label: "Up front".into() },
            DecisionOption { design_id: "b".into(), label: "Option".into() }
        ]
    );
    assert!(matches!(ai.decisions(&designs, None, c).await, Err(AiError::Empty(_))));
}
