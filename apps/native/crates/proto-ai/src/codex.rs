//! OpenAI Codex Responses provider (pi-ai `openai-codex`, SSE transport), authenticated with the Codex CLI's
//! ChatGPT login. Port of what the reference uses through `llm.ts`.

use std::path::PathBuf;
use std::time::Duration;

use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::AiError;
use crate::codex_auth::{self, Credential};
use crate::llm::{Completion, LlmStatus, Provider, ToolRequest};

pub const PROVIDER_ID: &str = "openai-codex";
pub const DEFAULT_BASE_URL: &str = "https://chatgpt.com/backend-api";
pub const DEFAULT_MODEL: &str = "gpt-6-astra";
const USER_AGENT: &str = "prototypie/0.1";
/// Retries for 429/5xx before giving up, and the longest wait honoured.
const MAX_RETRIES: usize = 2;
const MAX_RETRY_DELAY: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
pub struct CodexConfig {
    /// `$CODEX_HOME` (auth.json, config.toml).
    pub codex_home: PathBuf,
    /// `$PROTOTYPIE_MODEL`; wins over config.toml.
    pub model_override: Option<String>,
    pub base_url: String,
    pub token_url: String,
}

impl CodexConfig {
    pub fn from_env() -> Self {
        Self {
            codex_home: codex_auth::default_codex_home(),
            model_override: std::env::var("PROTOTYPIE_MODEL").ok().filter(|m| !m.is_empty()),
            base_url: DEFAULT_BASE_URL.into(),
            token_url: codex_auth::TOKEN_URL.into(),
        }
    }
}

pub struct CodexProvider {
    cfg: CodexConfig,
    http: reqwest::Client,
    /// Serializes refreshes in-process (a refresh rotates the refresh token).
    refresh_lock: Mutex<()>,
}

impl CodexProvider {
    pub fn new(cfg: CodexConfig) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("reqwest client");
        Self { cfg, http, refresh_lock: Mutex::new(()) }
    }

    pub fn from_env() -> Self {
        Self::new(CodexConfig::from_env())
    }

    /// `$PROTOTYPIE_MODEL`, else `model = "…"` in `$CODEX_HOME/config.toml`, else gpt-6-astra (re-read per call).
    pub fn model(&self) -> String {
        if let Some(m) = &self.cfg.model_override {
            return m.clone();
        }
        std::fs::read_to_string(self.cfg.codex_home.join("config.toml"))
            .ok()
            .and_then(|toml| model_from_toml(&toml))
            .unwrap_or_else(|| DEFAULT_MODEL.to_string())
    }

    fn url(&self) -> String {
        let base = self.cfg.base_url.trim_end_matches('/');
        if base.ends_with("/codex/responses") {
            base.to_string()
        } else if base.ends_with("/codex") {
            format!("{base}/responses")
        } else {
            format!("{base}/codex/responses")
        }
    }

    /// A credential valid for at least 5 more minutes, refreshing (and writing back) when needed.
    async fn credential(&self) -> Result<Credential, AiError> {
        let home = &self.cfg.codex_home;
        let not_logged_in = || AiError::Llm(crate::llm::NOT_LOGGED_IN.into());
        let cred = codex_auth::read_credential(home)?.ok_or_else(not_logged_in)?;
        if !cred.needs_refresh(crate::now_ms()) {
            return Ok(cred);
        }
        let _guard = self.refresh_lock.lock().await;
        // Another task (or the CLI) may have refreshed meanwhile.
        let cred = codex_auth::read_credential(home)?.ok_or_else(not_logged_in)?;
        if !cred.needs_refresh(crate::now_ms()) {
            return Ok(cred);
        }
        let (access, refresh) = codex_auth::refresh(&self.http, &self.cfg.token_url, &cred.refresh).await?;
        codex_auth::write_credential(home, &access, &refresh, Some(&cred.account_id))
            .map_err(|e| AiError::Llm(format!("Couldn't save the refreshed Codex login: {e}")))?;
        let expires_ms = codex_auth::jwt_expiry_ms(&access);
        Ok(Credential { access, refresh, account_id: cred.account_id, expires_ms })
    }

    fn body(&self, req: &ToolRequest) -> Value {
        json!({
            "model": self.model(),
            "store": false,
            "stream": true,
            "instructions": req.system,
            "input": [{ "role": "user", "content": [{ "type": "input_text", "text": req.prompt }] }],
            "text": { "verbosity": "low" },
            "include": ["reasoning.encrypted_content"],
            "tool_choice": "auto",
            "parallel_tool_calls": true,
            "tools": [{
                "type": "function",
                "name": req.tool.name,
                "description": req.tool.description,
                "parameters": req.tool.parameters,
                "strict": true,
            }],
            "reasoning": { "effort": req.reasoning.as_str(), "summary": "auto" },
        })
    }

    async fn run(&self, req: &ToolRequest, cancel: &CancellationToken) -> Result<Completion, AiError> {
        let cred = self.credential().await?;
        let body = self.body(req);
        let mut attempt = 0;
        let res = loop {
            let send = self
                .http
                .post(self.url())
                .bearer_auth(&cred.access)
                .header("chatgpt-account-id", &cred.account_id)
                .header("OpenAI-Beta", "responses=experimental")
                .header("originator", "pi")
                .header("accept", "text/event-stream")
                .json(&body)
                .send();
            let res = tokio::select! {
                r = send => r.map_err(|e| AiError::Llm(format!("Codex request failed: {}", e.without_url())))?,
                _ = cancel.cancelled() => return Err(AiError::Cancelled),
            };
            let status = res.status();
            if status.is_success() {
                break res;
            }
            let retry_after = retry_after(res.headers());
            let text = res.text().await.unwrap_or_default();
            let message = error_message(&text).unwrap_or_else(|| status.canonical_reason().unwrap_or("error").to_string());
            let retryable = (status.as_u16() == 429 || status.is_server_error()) && !message.to_lowercase().contains("usage limit");
            if !retryable || attempt >= MAX_RETRIES {
                return Err(AiError::Llm(format!("Codex request failed ({}): {message}", status.as_u16())));
            }
            attempt += 1;
            let delay = retry_after.unwrap_or(Duration::from_millis(500 * (1 << attempt))).min(MAX_RETRY_DELAY);
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = cancel.cancelled() => return Err(AiError::Cancelled),
            }
        };
        let mut events = SseEvents::default();
        let mut stream = res.bytes_stream();
        let mut submit: Option<Value> = None;
        loop {
            let chunk = tokio::select! {
                c = stream.next() => c,
                _ = cancel.cancelled() => return Err(AiError::Cancelled),
            };
            let done = match chunk {
                Some(Ok(bytes)) => {
                    events.push(&bytes);
                    false
                }
                Some(Err(e)) => return Err(AiError::Llm(format!("Codex stream broke: {}", e.without_url()))),
                None => {
                    events.finish();
                    true
                }
            };
            while let Some(data) = events.next_data() {
                let event: Value =
                    serde_json::from_str(&data).map_err(|e| AiError::Llm(format!("Invalid Codex SSE JSON: {e}")))?;
                if let Some(end) = handle_event(&event, &req.tool.name, &mut submit)? {
                    return Ok(end);
                }
            }
            if done {
                return match submit {
                    Some(args) => Ok(Completion::ToolCall(args)),
                    None => Err(AiError::Llm("Codex stream ended before the response completed.".into())),
                };
            }
        }
    }
}

impl Provider for CodexProvider {
    fn status(&self) -> LlmStatus {
        LlmStatus { provider: PROVIDER_ID.into(), model: self.model(), logged_in: codex_auth::logged_in(&self.cfg.codex_home) }
    }

    fn complete<'a>(&'a self, req: &'a ToolRequest, cancel: &'a CancellationToken) -> BoxFuture<'a, Result<Completion, AiError>> {
        Box::pin(self.run(req, cancel))
    }
}

/// `model = "…"` at the start of a line in config.toml.
fn model_from_toml(toml: &str) -> Option<String> {
    toml.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix("model")?.trim_start().strip_prefix('=')?.trim_start();
        let rest = rest.strip_prefix('"')?;
        Some(rest[..rest.find('"')?].to_string())
    })
}

fn retry_after(h: &reqwest::header::HeaderMap) -> Option<Duration> {
    let get = |name: &str| h.get(name)?.to_str().ok()?.trim().parse::<f64>().ok();
    if let Some(ms) = get("retry-after-ms") {
        return Some(Duration::from_millis(ms.max(0.0) as u64));
    }
    get("retry-after").map(|s| Duration::from_millis((s.max(0.0) * 1000.0) as u64))
}

/// The human message in an error body (`{error:{message}}`, `{detail}`, `{message}`), else the raw text.
fn error_message(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if let Ok(v) = serde_json::from_str::<Value>(text) {
        let msg = v
            .pointer("/error/message")
            .or_else(|| v.get("detail"))
            .or_else(|| v.get("message"))
            .and_then(Value::as_str);
        if let Some(m) = msg {
            return Some(m.to_string());
        }
    }
    Some(crate::context::truncate_chars(text, 300).to_string())
}

/// One stream event. Some(completion) ends the stream; errors map to `AiError::Llm`.
fn handle_event(event: &Value, tool: &str, submit: &mut Option<Value>) -> Result<Option<Completion>, AiError> {
    match event.get("type").and_then(Value::as_str).unwrap_or("") {
        "error" => {
            let msg = event
                .get("message")
                .or_else(|| event.pointer("/error/message"))
                .and_then(Value::as_str)
                .or_else(|| event.get("code").and_then(Value::as_str))
                .unwrap_or("unknown error");
            Err(AiError::Llm(format!("Codex error: {msg}")))
        }
        "response.failed" => {
            let msg = event.pointer("/response/error/message").and_then(Value::as_str).unwrap_or("Codex response failed");
            Err(AiError::Llm(msg.to_string()))
        }
        "response.output_item.done" => {
            let item = &event["item"];
            if item["type"] == "function_call" && item["name"] == tool && submit.is_none() {
                let args = item["arguments"].as_str().unwrap_or("");
                *submit = Some(serde_json::from_str(args).unwrap_or(Value::Null));
            }
            Ok(None)
        }
        "response.completed" | "response.done" | "response.incomplete" => {
            if let Some(args) = submit.take() {
                return Ok(Some(Completion::ToolCall(args)));
            }
            let status = event.pointer("/response/status").and_then(Value::as_str);
            let reason = event.pointer("/response/incomplete_details/reason").and_then(Value::as_str);
            if status == Some("incomplete") && reason.is_none_or(|r| r == "max_output_tokens") {
                return Ok(Some(Completion::CutOff));
            }
            Ok(Some(Completion::NoToolCall))
        }
        _ => Ok(None),
    }
}

/// Incremental SSE framing: frames are separated by a blank line; `data:` lines of a frame are joined.
#[derive(Default)]
struct SseEvents {
    buf: String,
    raw: Vec<u8>,
}

impl SseEvents {
    fn push(&mut self, bytes: &[u8]) {
        self.raw.extend_from_slice(bytes);
        // Keep an incomplete UTF-8 sequence for the next chunk.
        let valid = match std::str::from_utf8(&self.raw) {
            Ok(s) => s.len(),
            Err(e) => e.valid_up_to(),
        };
        let text = String::from_utf8_lossy(&self.raw[..valid]).replace("\r\n", "\n");
        self.buf.push_str(&text);
        self.raw.drain(..valid);
    }

    /// EOF terminates the residual frame.
    fn finish(&mut self) {
        if !self.buf.trim().is_empty() {
            self.buf.push_str("\n\n");
        }
    }

    fn next_data(&mut self) -> Option<String> {
        loop {
            let idx = self.buf.find("\n\n")?;
            let frame: String = self.buf.drain(..idx + 2).collect();
            let data: Vec<&str> =
                frame.lines().filter_map(|l| l.strip_prefix("data:")).map(str::trim).collect();
            let data = data.join("\n");
            let data = data.trim();
            if !data.is_empty() && data != "[DONE]" {
                return Some(data.to_string());
            }
        }
    }
}

#[cfg(test)]
#[path = "codex_tests.rs"]
mod tests;
