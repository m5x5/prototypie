//! The generation entry points on `Ai`: variations jobs (codex or agent queue, incl. imports), in-place edit jobs,
//! spokes and decisions. Ports of `api/variations/route.ts`, `lib/server/ai-edit.ts` (LLM/queue part),
//! `api/spokes/route.ts` and `api/decisions/route.ts`.

use std::collections::HashSet;

use futures_util::future::join_all;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::context::{clean_components, describe_guidelines, truncate_chars};
use crate::jobs::{Emitter, JobId};
use crate::llm::{Reasoning, generate_object, schema};
use crate::prompts;
use crate::queue::{QueueItem, QueueMode};
use crate::types::*;
use crate::{Ai, AiError};

#[derive(Deserialize)]
struct Plan {
    directions: Vec<Direction>,
}

#[derive(Deserialize)]
struct Built {
    title: String,
    rationale: String,
    html: String,
}

#[derive(Deserialize)]
struct Edited {
    rationale: String,
    html: String,
}

#[derive(Deserialize)]
struct Spokes {
    components: Vec<SpokeComponent>,
}

#[derive(Deserialize)]
struct Decisions {
    decisions: Vec<SuggestedDecision>,
}

fn non_empty<T>(v: &Option<Vec<T>>) -> Option<Vec<T>>
where
    T: Clone,
{
    v.as_ref().filter(|l| !l.is_empty()).cloned()
}

impl Ai {
    /// `POST /api/variations`: start a detached job (plan → variation/error per direction → done) and return its id.
    /// `guidelines` = the server's `guidelinesFor(req.space_id)`.
    pub fn start_variations(&self, req: VariationRequest, guidelines: Option<Guidelines>) -> JobId {
        let ai = self.clone();
        self.jobs().start(move |emit, cancel, job_id| async move { ai.run_variations(req, guidelines, emit, cancel, job_id).await })
    }

    async fn run_variations(
        &self,
        raw: VariationRequest,
        guidelines: Option<Guidelines>,
        emit: Emitter,
        cancel: CancellationToken,
        job_id: JobId,
    ) -> Result<(), AiError> {
        let body = VariationRequest { components: clean_components(raw.components.as_deref()), ..raw };
        let count = body.clamped_count();
        let agent = body.agent.unwrap_or_default();
        if let Some(from) = body.import_from.clone() {
            if agent == AgentChoice::Claude {
                return self.run_import(&body, &from, guidelines.as_ref(), &emit, &cancel, &job_id).await;
            }
            // Recreating an existing element means reading its source code: only the Claude agent can do that.
            let direction = Direction { title: truncate_chars(from.element.trim(), 200).to_string(), brief: from.source };
            emit.send(VariationEvent::Plan { directions: vec![direction] });
            emit.send(VariationEvent::Error {
                index: Some(0),
                message: "Importing from the app needs the Claude agent (switch the inspector's agent to Claude).".into(),
            });
            return Ok(());
        }
        if agent == AgentChoice::Claude {
            return self.run_via_queue(&body, count, guidelines.as_ref(), &emit, &cancel, &job_id).await;
        }

        let provider = self.provider()?;
        let task = prompts::describe_task(&body);
        let system = prompts::design_system(&describe_guidelines(guidelines.as_ref()));
        // 1. Plan N distinct directions (fast), so the parallel generations don't converge. A retry knows its direction.
        let directions: Vec<Direction> = match &body.direction {
            Some(d) => vec![d.clone()],
            None => {
                let plan: Plan = generate_object(
                    provider,
                    &system,
                    &prompts::plan_prompt(&task, count),
                    schema::plan(),
                    "Submit the design directions.",
                    Reasoning::Low,
                    &cancel,
                )
                .await?;
                plan.directions.into_iter().take(count).collect()
            }
        };
        emit.send(VariationEvent::Plan { directions: directions.clone() });

        // 2. Generate every direction in parallel; each result is emitted as soon as it lands.
        join_all(directions.iter().enumerate().map(|(index, dir)| {
            let (emit, cancel, system, task) = (emit.clone(), cancel.clone(), &system, &task);
            async move {
                let built: Result<Built, AiError> = generate_object(
                    provider,
                    system,
                    &prompts::build_prompt(task, dir),
                    schema::variation(),
                    "Submit the finished design.",
                    Reasoning::Medium,
                    &cancel,
                )
                .await;
                emit.send(match built {
                    Ok(b) => VariationEvent::Variation {
                        index,
                        variation: Variation { title: b.title, rationale: b.rationale, html: b.html, viewport: None, viewport_height: None },
                    },
                    Err(e) => VariationEvent::Error { index: Some(index), message: e.to_string() },
                });
            }
        }))
        .await;
        Ok(())
    }

    /// "claude" agent: each direction becomes a queue item answered by a Claude Code session.
    async fn run_via_queue(
        &self,
        body: &VariationRequest,
        count: usize,
        guidelines: Option<&Guidelines>,
        emit: &Emitter,
        cancel: &CancellationToken,
        job_id: &str,
    ) -> Result<(), AiError> {
        let directions: Vec<Direction> = match &body.direction {
            Some(d) => vec![d.clone()],
            None => (0..count)
                .map(|i| Direction {
                    title: format!("Direction {}", i + 1),
                    brief: if body.instruction.is_empty() {
                        "Explore a meaningfully different direction.".into()
                    } else {
                        body.instruction.clone()
                    },
                })
                .collect(),
        };
        emit.send(VariationEvent::Plan { directions: directions.clone() });
        let bases = non_empty(&body.bases);
        join_all(directions.into_iter().enumerate().map(|(index, direction)| {
            let item = QueueItem {
                job_id: job_id.to_string(),
                index,
                instruction: body.instruction.clone(),
                focus: body.focus.clone(),
                scope: body.scope,
                annotations: body.annotations.clone(),
                direction,
                base_title: body.base_title.clone(),
                base_html: if bases.is_some() { None } else { body.base_html.clone() },
                bases: bases.clone(),
                pod_context: non_empty(&body.pod_context),
                components: body.components.clone(),
                guidelines: guidelines.map(Into::into),
                ..Default::default()
            };
            let (emit, cancel) = (emit.clone(), cancel.clone());
            async move {
                emit.send(match self.queue().enqueue_and_wait(item, cancel).await {
                    Ok(variation) => VariationEvent::Variation { index, variation },
                    Err(e) => VariationEvent::Error { index: Some(index), message: e.to_string() },
                });
            }
        }))
        .await;
        Ok(())
    }

    /// "+ → Import from app…" with the Claude agent: one queue item in import mode.
    async fn run_import(
        &self,
        body: &VariationRequest,
        from: &ImportFrom,
        guidelines: Option<&Guidelines>,
        emit: &Emitter,
        cancel: &CancellationToken,
        job_id: &str,
    ) -> Result<(), AiError> {
        let element = truncate_chars(from.element.trim(), 200).to_string();
        let source = truncate_chars(from.source.trim(), 2000).to_string();
        let direction = Direction { title: element.clone(), brief: format!("Recreate \"{element}\" exactly as it is in {source}.") };
        emit.send(VariationEvent::Plan { directions: vec![direction.clone()] });
        let shot = match from.screenshot.as_deref() {
            Some(s) if !s.is_empty() => self.queue().save_screenshot(job_id, s).await,
            _ => None,
        };
        let instruction = if body.instruction.is_empty() {
            format!("Import the existing \"{element}\" from the app at {source}: recreate it faithfully from its source code (structure, tokens, real copy, real sizes and viewport). Don't redesign or improve it.")
        } else {
            body.instruction.clone()
        };
        let item = QueueItem {
            job_id: job_id.to_string(),
            index: 0,
            mode: Some(QueueMode::Import),
            element: Some(element.clone()),
            import_source: Some(source),
            import_screenshot: shot.map(|p| p.to_string_lossy().into_owned()),
            instruction,
            focus: Some(element),
            scope: Some(Scope::Component),
            direction,
            guidelines: guidelines.map(Into::into),
            ..Default::default()
        };
        emit.send(match self.queue().enqueue_and_wait(item, cancel.clone()).await {
            Ok(variation) => VariationEvent::Variation { index: 0, variation },
            Err(e) => VariationEvent::Error { index: Some(0), message: e.to_string() },
        });
        Ok(())
    }

    /// In-place edit of `design` (POST /api/edit after the server's checks). Job events: plan → variation (title =
    /// the card's title) or error (message "Cancelled" when the job was cancelled) → done. The server writes
    /// `editing` before and runs finish/fail from the events.
    pub fn start_edit(&self, design: EditTarget, raw: EditRequest, guidelines: Option<Guidelines>) -> JobId {
        let ai = self.clone();
        self.jobs().start(move |emit, cancel, job_id| async move {
            let req = EditRequest { components: clean_components(raw.components.as_deref()), ..raw };
            emit.send(VariationEvent::Plan {
                directions: vec![Direction { title: design.title.clone(), brief: req.instruction.clone() }],
            });
            let result = ai.run_edit(&design, &req, guidelines.as_ref(), &cancel, &job_id).await;
            emit.send(match result {
                Ok(variation) => VariationEvent::Variation { index: 0, variation },
                Err(e) => {
                    let message = if cancel.is_cancelled() { "Cancelled".to_string() } else { e.to_string() };
                    VariationEvent::Error { index: Some(0), message }
                }
            });
            Ok(())
        })
    }

    async fn run_edit(
        &self,
        d: &EditTarget,
        req: &EditRequest,
        guidelines: Option<&Guidelines>,
        cancel: &CancellationToken,
        job_id: &str,
    ) -> Result<Variation, AiError> {
        if req.agent.unwrap_or_default() == AgentChoice::Claude {
            let item = QueueItem {
                job_id: job_id.to_string(),
                index: 0,
                mode: Some(QueueMode::Edit),
                target_design_id: Some(d.id.clone()),
                instruction: req.instruction.clone(),
                focus: req.focus.clone(),
                scope: Some(Scope::Page),
                annotations: non_empty(&req.annotations),
                direction: Direction { title: format!("Edit in place: {}", d.title), brief: req.instruction.clone() },
                base_title: Some(d.title.clone()),
                base_html: Some(d.html.clone()),
                pod_context: non_empty(&req.pod_context),
                components: req.components.clone(),
                guidelines: guidelines.map(Into::into),
                ..Default::default()
            };
            return self.queue().enqueue_and_wait(item, cancel.clone()).await;
        }
        let out: Edited = generate_object(
            self.provider()?,
            &prompts::edit_system(&describe_guidelines(guidelines)),
            &prompts::describe_edit(d, req),
            schema::edit(),
            "Submit the edited document.",
            Reasoning::Medium,
            cancel,
        )
        .await?;
        Ok(Variation { title: d.title.clone(), rationale: out.rationale, html: out.html, viewport: None, viewport_height: None })
    }

    /// POST /api/spokes: the page's components (1..=8). `Empty("The model found no components.")` when none.
    pub async fn spokes(&self, html: &str, title: Option<&str>, cancel: CancellationToken) -> Result<Vec<SpokeComponent>, AiError> {
        let out: Spokes = generate_object(
            self.provider()?,
            prompts::SPOKES_SYSTEM,
            &prompts::spokes_prompt(html, title),
            schema::spokes(),
            "Submit the page's components.",
            Reasoning::Low,
            &cancel,
        )
        .await?;
        let components: Vec<SpokeComponent> = out.components.into_iter().take(8).collect();
        if components.is_empty() {
            return Err(AiError::Empty("The model found no components.".into()));
        }
        Ok(components)
    }

    /// POST /api/decisions: 1..=4 decisions with 2..=4 known, distinct options each.
    /// `Empty("The agent couldn't find a clear decision between these designs.")` when nothing is left.
    pub async fn decisions(
        &self,
        designs: &[DecisionCandidate],
        context: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<Vec<SuggestedDecision>, AiError> {
        let out: Decisions = generate_object(
            self.provider()?,
            prompts::DECISIONS_SYSTEM,
            &prompts::decisions_prompt(designs, context),
            schema::decisions(),
            "Submit the core decisions for the stakeholder poll.",
            Reasoning::Medium,
            &cancel,
        )
        .await?;
        Ok(filter_decisions(out.decisions, designs)).and_then(|d| {
            if d.is_empty() {
                Err(AiError::Empty("The agent couldn't find a clear decision between these designs.".into()))
            } else {
                Ok(d)
            }
        })
    }
}

/// Keep only real candidate ids, dedupe options, drop decisions that no longer compare anything.
fn filter_decisions(raw: Vec<SuggestedDecision>, designs: &[DecisionCandidate]) -> Vec<SuggestedDecision> {
    let known: HashSet<&str> = designs.iter().map(|d| d.id.as_str()).collect();
    raw.into_iter()
        .map(|d| {
            let mut seen = HashSet::new();
            let options = d
                .options
                .into_iter()
                .filter(|o| known.contains(o.design_id.as_str()) && seen.insert(o.design_id.clone()))
                .take(4)
                .map(|o| {
                    let label = o.label.trim();
                    DecisionOption { design_id: o.design_id.clone(), label: if label.is_empty() { "Option".into() } else { label.into() } }
                })
                .collect::<Vec<_>>();
            SuggestedDecision { question: d.question.trim().into(), why: d.why.trim().into(), options }
        })
        .filter(|d| !d.question.is_empty() && d.options.len() >= 2)
        .take(4)
        .collect()
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
