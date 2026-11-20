//! proto-ai: generation for Prototypie (see docs/api-ai.md). Port of the reference app's LLM seam (OpenAI Codex
//! via the ChatGPT login), its prompts, detached jobs and the Claude agent-queue mode. No proto-core dependency:
//! store reads/writes stay in proto-server, which passes in what is needed.

use std::path::PathBuf;
use std::sync::Arc;

pub mod codex;
pub mod codex_auth;
pub mod context;
pub mod jobs;
pub mod llm;
pub mod ndjson;
mod ops;
pub mod prompts;
pub mod queue;
pub mod types;

pub use codex::{CodexConfig, CodexProvider};
pub use jobs::{Emitter, JobId, Jobs};
pub use llm::{LlmStatus, Provider, Reasoning};
pub use queue::{AgentQueue, NoNotifier, NoteStatus, Notifier, QueueItem, QueueMode, QueueStatus, announce_comment, infer_viewport, infer_viewport_height};
pub use tokio_util::sync::CancellationToken;
pub use types::*;

/// Errors of the generation entry points. HTTP mapping used by the reference: `Llm` → 503 (not logged in / model
/// problem), `Cancelled` → 499, `Empty` → 422/502 (route-specific), `Other` → 500.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiError {
    Llm(String),
    Cancelled,
    /// The model answered but nothing usable was left (message for the user).
    Empty(String),
    Other(String),
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiError::Llm(m) | AiError::Empty(m) | AiError::Other(m) => f.write_str(m),
            AiError::Cancelled => f.write_str("Cancelled"),
        }
    }
}

impl std::error::Error for AiError {}

/// The generation service: jobs, the agent queue and the LLM provider. Cheap to clone.
#[derive(Clone)]
pub struct Ai {
    inner: Arc<Inner>,
}

struct Inner {
    provider: Option<Arc<dyn Provider>>,
    queue: AgentQueue,
    jobs: Jobs,
}

impl Ai {
    /// `data_dir` holds `agent-queue/`. Without a provider the LLM entry points fail with the not-logged-in error;
    /// the agent-queue mode works regardless.
    pub fn new(data_dir: PathBuf, provider: Option<Arc<dyn Provider>>, notifier: Arc<dyn Notifier>) -> Ai {
        let queue = AgentQueue::new(&data_dir, notifier);
        Ai { inner: Arc::new(Inner { provider, queue, jobs: Jobs::new() }) }
    }

    /// With `CodexProvider::from_env()` ($CODEX_HOME, $PROTOTYPIE_MODEL).
    pub fn with_codex(data_dir: PathBuf, notifier: Arc<dyn Notifier>) -> Ai {
        Ai::new(data_dir, Some(Arc::new(CodexProvider::from_env())), notifier)
    }

    pub fn jobs(&self) -> &Jobs {
        &self.inner.jobs
    }

    pub fn queue(&self) -> &AgentQueue {
        &self.inner.queue
    }

    pub fn status(&self) -> LlmStatus {
        match &self.inner.provider {
            Some(p) => p.status(),
            None => LlmStatus { provider: codex::PROVIDER_ID.into(), model: codex::DEFAULT_MODEL.into(), logged_in: false },
        }
    }

    fn provider(&self) -> Result<&dyn Provider, AiError> {
        self.inner.provider.as_deref().ok_or_else(|| AiError::Llm(llm::NOT_LOGGED_IN.into()))
    }
}

const ID_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";

/// nanoid-style random id (`[A-Za-z0-9_-]`).
pub fn nanoid(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    getrandom::fill(&mut bytes).expect("OS randomness");
    bytes.iter().map(|b| ID_ALPHABET[(b & 63) as usize] as char).collect()
}

/// ISO-8601 with milliseconds and `Z`, like JavaScript's `toISOString()`.
pub fn iso_now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids_and_times() {
        let id = super::nanoid(10);
        assert_eq!(id.len(), 10);
        assert!(id.bytes().all(|b| super::ID_ALPHABET.contains(&b)));
        assert_ne!(super::nanoid(10), id);
        let t = super::iso_now();
        assert_eq!(t.len(), 24);
        assert!(t.ends_with('Z'));
    }
}
