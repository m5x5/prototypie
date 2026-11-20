//! The one LLM seam (reference `src/lib/server/llm.ts`): ask the model for one JSON object by having it call a
//! single strict `submit` tool, validate it, retry once. Providers implement `Provider`; only Codex exists.

use futures_util::future::BoxFuture;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::AiError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reasoning {
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
}

impl Reasoning {
    pub fn as_str(self) -> &'static str {
        match self {
            Reasoning::Minimal => "minimal",
            Reasoning::Low => "low",
            Reasoning::Medium => "medium",
            Reasoning::High => "high",
            Reasoning::Xhigh => "xhigh",
        }
    }
}

/// `llmStatus()`: what the inspector shows about the LLM.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmStatus {
    pub provider: String,
    pub model: String,
    pub logged_in: bool,
}

/// A function tool the model must call.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments (strict: every property required, `additionalProperties: false`).
    pub parameters: Value,
}

/// One model call: system prompt, a single user message, one tool.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolRequest {
    pub system: String,
    pub prompt: String,
    pub tool: ToolSpec,
    pub reasoning: Reasoning,
}

/// How a call ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Completion {
    /// The model called the tool; its parsed arguments (`Value::Null` when they weren't valid JSON).
    ToolCall(Value),
    /// Output hit the token limit without a tool call.
    CutOff,
    /// The model answered without calling the tool.
    NoToolCall,
}

pub trait Provider: Send + Sync {
    fn status(&self) -> LlmStatus;
    /// One request. Errors: `AiError::Llm` for API/model/auth problems, `AiError::Cancelled` when `cancel` trips.
    fn complete<'a>(&'a self, req: &'a ToolRequest, cancel: &'a CancellationToken) -> BoxFuture<'a, Result<Completion, AiError>>;
}

pub const NOT_LOGGED_IN: &str = "Not logged in to Codex. Run `codex login` in a terminal (ChatGPT account), then retry.";

/// Ask the model for one object of type `T` (schema `schema`). Up to two attempts: invalid arguments are re-asked
/// with the parse error, a plain-text answer with a reminder; output cut off fails at once.
pub async fn generate_object<T: DeserializeOwned>(
    provider: &dyn Provider,
    system: &str,
    prompt: &str,
    schema: Value,
    description: &str,
    reasoning: Reasoning,
    cancel: &CancellationToken,
) -> Result<T, AiError> {
    if !provider.status().logged_in {
        return Err(AiError::Llm(NOT_LOGGED_IN.into()));
    }
    let mut req = ToolRequest {
        system: format!("{system}\n\nAlways deliver your answer by calling the `submit` tool exactly once. Do not answer in plain text."),
        prompt: prompt.to_string(),
        tool: ToolSpec { name: "submit".into(), description: description.to_string(), parameters: schema },
        reasoning,
    };
    for _ in 0..2 {
        if cancel.is_cancelled() {
            return Err(AiError::Cancelled);
        }
        match provider.complete(&req, cancel).await? {
            Completion::ToolCall(args) => match serde_json::from_value::<T>(args) {
                Ok(v) => return Ok(v),
                Err(e) => {
                    req.prompt = format!(
                        "{prompt}\n\nYour previous submit call was invalid: {e}. Call submit again with valid arguments."
                    );
                }
            },
            Completion::CutOff => return Err(AiError::Llm("Output was cut off (too long).".into())),
            Completion::NoToolCall => req.prompt = format!("{prompt}\n\nReminder: you must call the submit tool."),
        }
    }
    Err(AiError::Llm("The model did not return a valid result.".into()))
}

/// Strict JSON Schema helpers (all properties required, no extras).
pub mod schema {
    use super::*;

    pub fn string() -> Value {
        json!({ "type": "string" })
    }

    pub fn array(items: Value) -> Value {
        json!({ "type": "array", "items": items })
    }

    pub fn object(props: &[(&str, Value)]) -> Value {
        let properties: serde_json::Map<String, Value> = props.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let required: Vec<&str> = props.iter().map(|(k, _)| *k).collect();
        json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
    }

    /// `{ directions: [{ title, brief }] }`
    pub fn plan() -> Value {
        object(&[("directions", array(object(&[("title", string()), ("brief", string())])))])
    }

    /// `{ title, rationale, html }`
    pub fn variation() -> Value {
        object(&[("title", string()), ("rationale", string()), ("html", string())])
    }

    /// `{ rationale, html }`
    pub fn edit() -> Value {
        object(&[("rationale", string()), ("html", string())])
    }

    /// `{ components: [{ name, description, selector }] }`
    pub fn spokes() -> Value {
        object(&[("components", array(object(&[("name", string()), ("description", string()), ("selector", string())])))])
    }

    /// `{ decisions: [{ question, why, options: [{ designId, label }] }] }`
    pub fn decisions() -> Value {
        let option = object(&[("designId", string()), ("label", string())]);
        object(&[("decisions", array(object(&[("question", string()), ("why", string()), ("options", array(option))])))])
    }
}

/// A scripted provider for tests: returns the queued completions in order and records the requests.
#[cfg(test)]
pub(crate) mod mock {
    use super::*;
    use std::sync::Mutex;

    pub struct MockProvider {
        pub logged_in: bool,
        pub replies: Mutex<Vec<Result<Completion, AiError>>>,
        pub requests: Mutex<Vec<ToolRequest>>,
    }

    impl MockProvider {
        pub fn new(replies: Vec<Result<Completion, AiError>>) -> Self {
            Self { logged_in: true, replies: Mutex::new(replies.into_iter().rev().collect()), requests: Mutex::default() }
        }
    }

    impl Provider for MockProvider {
        fn status(&self) -> LlmStatus {
            LlmStatus { provider: "mock".into(), model: "m".into(), logged_in: self.logged_in }
        }
        fn complete<'a>(&'a self, req: &'a ToolRequest, _cancel: &'a CancellationToken) -> BoxFuture<'a, Result<Completion, AiError>> {
            self.requests.lock().unwrap().push(req.clone());
            let next = self.replies.lock().unwrap().pop().unwrap_or(Err(AiError::Llm("no more replies".into())));
            Box::pin(async move { next })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock::MockProvider;
    use super::*;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Edit {
        rationale: String,
        html: String,
    }

    async fn run(p: &MockProvider) -> Result<Edit, AiError> {
        generate_object::<Edit>(p, "SYS", "PROMPT", schema::edit(), "Submit it.", Reasoning::Medium, &CancellationToken::new()).await
    }

    #[tokio::test]
    async fn returns_valid_tool_call() {
        let p = MockProvider::new(vec![Ok(Completion::ToolCall(json!({"rationale":"r","html":"h"})))]);
        assert_eq!(run(&p).await.unwrap(), Edit { rationale: "r".into(), html: "h".into() });
        let req = &p.requests.lock().unwrap()[0];
        assert_eq!(
            req.system,
            "SYS\n\nAlways deliver your answer by calling the `submit` tool exactly once. Do not answer in plain text."
        );
        assert_eq!(req.tool.name, "submit");
        assert_eq!(req.tool.parameters["required"], json!(["rationale", "html"]));
        assert_eq!(req.tool.parameters["additionalProperties"], json!(false));
        assert_eq!(req.reasoning, Reasoning::Medium);
    }

    #[tokio::test]
    async fn retries_invalid_args_then_no_call() {
        let p = MockProvider::new(vec![Ok(Completion::ToolCall(json!({"rationale":"r"}))), Ok(Completion::NoToolCall)]);
        match run(&p).await {
            Err(AiError::Llm(m)) => assert_eq!(m, "The model did not return a valid result."),
            other => panic!("{other:?}"),
        }
        let reqs = p.requests.lock().unwrap();
        assert!(reqs[1].prompt.starts_with("PROMPT\n\nYour previous submit call was invalid: missing field `html`"));
    }

    #[tokio::test]
    async fn reminder_then_success_and_cutoff() {
        let p = MockProvider::new(vec![Ok(Completion::NoToolCall), Ok(Completion::ToolCall(json!({"rationale":"r","html":"h"})))]);
        assert!(run(&p).await.is_ok());
        assert_eq!(p.requests.lock().unwrap()[1].prompt, "PROMPT\n\nReminder: you must call the submit tool.");

        let p = MockProvider::new(vec![Ok(Completion::CutOff)]);
        assert!(matches!(run(&p).await, Err(AiError::Llm(m)) if m == "Output was cut off (too long)."));
    }

    #[tokio::test]
    async fn not_logged_in_and_cancelled() {
        let mut p = MockProvider::new(vec![]);
        p.logged_in = false;
        assert!(matches!(run(&p).await, Err(AiError::Llm(m)) if m == NOT_LOGGED_IN));
        let p = MockProvider::new(vec![]);
        let c = CancellationToken::new();
        c.cancel();
        let r = generate_object::<Edit>(&p, "s", "p", schema::edit(), "d", Reasoning::Low, &c).await;
        assert!(matches!(r, Err(AiError::Cancelled)));
    }
}
