//! The app's generation host: a small tokio runtime and the one `proto_ai::Ai` (jobs + agent queue + Codex).
//! The embedded proto-server must reuse this `Ai` (`s.runner.host()`), so agent-queue deliveries to :3124
//! resolve the waiters of jobs the runner started.

use std::path::PathBuf;
use std::sync::Arc;

use proto_ai::{Ai, CodexProvider, NoNotifier, Notifier, Provider};

/// What the host is built from; defaults: the store's data dir, Codex from the environment, no notes service.
#[derive(Default, Clone)]
pub struct HostConfig {
    pub data_dir: Option<PathBuf>,
    pub provider: Option<Arc<dyn Provider>>,
    pub notifier: Option<Arc<dyn Notifier>>,
}

pub struct AiHost {
    rt: tokio::runtime::Runtime,
    ai: Ai,
    /// The throwaway data dir of a store without one (headless tooling, scenes, tests): queue items and
    /// screenshots go here and are deleted with the host. Never the user's data dir.
    scratch: Option<tempfile::TempDir>,
}

impl AiHost {
    /// `store_dir` = the store's data dir. An in-memory store has none: the host then works in a temp dir it
    /// deletes on drop, so a headless run never writes the user's data dir (no `default_data_dir` fallback).
    pub fn start(cfg: &HostConfig, store_dir: Option<PathBuf>) -> std::io::Result<AiHost> {
        let (dir, scratch) = match cfg.data_dir.clone().or(store_dir) {
            Some(dir) => (dir, None),
            None => {
                let tmp = tempfile::Builder::new().prefix("prototypie-ai-").tempdir()?;
                (tmp.path().to_path_buf(), Some(tmp))
            }
        };
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("proto-ai").enable_all().build()?;
        let provider: Arc<dyn Provider> = cfg.provider.clone().unwrap_or_else(|| Arc::new(CodexProvider::from_env()));
        let notifier: Arc<dyn Notifier> = cfg.notifier.clone().unwrap_or_else(|| Arc::new(NoNotifier));
        Ok(AiHost { rt, ai: Ai::new(dir, Some(provider), notifier), scratch })
    }

    /// True when the host works in its own throwaway dir (the store has no data dir).
    pub fn is_scratch(&self) -> bool {
        self.scratch.is_some()
    }

    pub fn ai(&self) -> &Ai {
        &self.ai
    }

    /// For spawning on the runtime (the embedded server, event forwarders).
    pub fn handle(&self) -> &tokio::runtime::Handle {
        self.rt.handle()
    }

    /// Run `f` inside the runtime's context (proto-ai's `start_*` spawn their job there).
    pub fn enter<R>(&self, f: impl FnOnce(&Ai) -> R) -> R {
        let _guard = self.rt.enter();
        f(&self.ai)
    }
}
