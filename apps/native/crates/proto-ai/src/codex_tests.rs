//! CodexProvider against a local mock server: never the real API, never the real ~/.codex.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::*;
use crate::codex_auth::tests::jwt;
use crate::llm::{Reasoning, ToolSpec, schema};

/// (status, extra headers, body)
type Reply = (u16, Vec<(&'static str, &'static str)>, String);

#[derive(Default)]
struct Mock {
    /// Responses for /codex/responses, served in order.
    replies: Vec<Reply>,
    requests: Vec<(HeaderMap, Value)>,
    token_requests: Vec<String>,
}

type Shared = Arc<Mutex<Mock>>;

async fn responses(State(m): State<Shared>, headers: HeaderMap, body: String) -> Response {
    let mut m = m.lock().unwrap();
    m.requests.push((headers, serde_json::from_str(&body).unwrap()));
    let (status, extra, body) = if m.replies.is_empty() { (500, vec![], "{}".into()) } else { m.replies.remove(0) };
    let mut res = (StatusCode::from_u16(status).unwrap(), body).into_response();
    res.headers_mut().insert("content-type", "text/event-stream".parse().unwrap());
    for (k, v) in extra {
        res.headers_mut().insert(k, v.parse().unwrap());
    }
    res
}

async fn token(State(m): State<Shared>, body: String) -> Response {
    m.lock().unwrap().token_requests.push(body);
    let access = jwt(json!({"exp": 4_000_000_000i64}));
    axum::Json(json!({"access_token": access, "refresh_token": "r-new", "expires_in": 3600, "id_token": "ignored"})).into_response()
}

async fn serve(mock: Shared) -> String {
    let app = Router::new()
        .route("/backend-api/codex/responses", post(responses))
        .route("/oauth/token", post(token))
        .with_state(mock);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn login(home: &std::path::Path, exp: i64) {
    let access = jwt(json!({"exp": exp, "https://api.openai.com/auth": {"chatgpt_account_id": "acc-1"}}));
    std::fs::write(
        home.join("auth.json"),
        json!({"auth_mode":"chatgpt","tokens":{"access_token": access,"refresh_token":"r-old","id_token":"id"}}).to_string(),
    )
    .unwrap();
}

fn provider(home: &std::path::Path, base: &str, model: Option<&str>) -> CodexProvider {
    CodexProvider::new(CodexConfig {
        codex_home: home.to_path_buf(),
        model_override: model.map(str::to_string),
        base_url: format!("{base}/backend-api"),
        token_url: format!("{base}/oauth/token"),
    })
}

fn request() -> ToolRequest {
    ToolRequest {
        system: "SYS".into(),
        prompt: "PROMPT".into(),
        tool: ToolSpec { name: "submit".into(), description: "Submit it.".into(), parameters: schema::edit() },
        reasoning: Reasoning::Medium,
    }
}

fn sse(events: &[Value]) -> String {
    events.iter().map(|e| format!("event: {}\ndata: {}\n\n", e["type"].as_str().unwrap(), e)).collect()
}

fn tool_call_stream(args: &str) -> String {
    sse(&[
        json!({"type":"response.created","response":{"status":"in_progress"}}),
        json!({"type":"response.function_call_arguments.delta","delta":"{"}),
        json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","name":"submit","arguments":args}}),
        json!({"type":"response.completed","response":{"status":"completed"}}),
    ])
}

#[tokio::test]
async fn sends_reference_request_and_parses_tool_call() {
    let home = tempfile::tempdir().unwrap();
    login(home.path(), 4_000_000_000);
    std::fs::write(home.path().join("config.toml"), "# c\nmodel = \"gpt-test\"\nother = 1\n").unwrap();
    let mock: Shared = Arc::default();
    mock.lock().unwrap().replies.push((200, vec![], tool_call_stream(r#"{"rationale":"r","html":"<p>"}"#)));
    let base = serve(mock.clone()).await;
    let p = provider(home.path(), &base, None);

    assert_eq!(p.status(), LlmStatus { provider: "openai-codex".into(), model: "gpt-test".into(), logged_in: true });
    let out = p.complete(&request(), &CancellationToken::new()).await.unwrap();
    assert_eq!(out, Completion::ToolCall(json!({"rationale":"r","html":"<p>"})));

    let m = mock.lock().unwrap();
    let (headers, body) = &m.requests[0];
    assert!(headers["authorization"].to_str().unwrap().starts_with("Bearer ey"));
    assert_eq!(headers["chatgpt-account-id"], "acc-1");
    assert_eq!(headers["openai-beta"], "responses=experimental");
    assert_eq!(headers["originator"], "pi");
    assert_eq!(headers["user-agent"], "prototypie/0.1");
    assert_eq!(body["model"], "gpt-test");
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert_eq!(body["instructions"], "SYS");
    assert_eq!(body["input"], json!([{"role":"user","content":[{"type":"input_text","text":"PROMPT"}]}]));
    assert_eq!(body["reasoning"], json!({"effort":"medium","summary":"auto"}));
    assert_eq!(body["tools"][0]["name"], "submit");
    assert_eq!(body["tools"][0]["strict"], true);
    assert_eq!(body["tool_choice"], "auto");
    assert!(m.token_requests.is_empty(), "fresh token: no refresh");
}

#[tokio::test]
async fn refreshes_expiring_token_and_writes_back() {
    let home = tempfile::tempdir().unwrap();
    login(home.path(), 1_000); // long expired
    let mock: Shared = Arc::default();
    mock.lock().unwrap().replies.push((200, vec![], tool_call_stream("{}")));
    let base = serve(mock.clone()).await;
    let p = provider(home.path(), &base, Some("gpt-env"));
    assert_eq!(p.model(), "gpt-env");
    p.complete(&request(), &CancellationToken::new()).await.unwrap();

    let m = mock.lock().unwrap();
    assert_eq!(m.token_requests.len(), 1);
    let form = &m.token_requests[0];
    assert!(form.contains("grant_type=refresh_token") && form.contains("refresh_token=r-old"));
    assert!(form.contains(&format!("client_id={}", codex_auth::CLIENT_ID)));
    let file: Value = serde_json::from_str(&std::fs::read_to_string(home.path().join("auth.json")).unwrap()).unwrap();
    assert_eq!(file["tokens"]["refresh_token"], "r-new");
    assert_eq!(file["tokens"]["account_id"], "acc-1");
    assert_eq!(file["tokens"]["id_token"], "id");
    assert_eq!(file["auth_mode"], "chatgpt");
    // The request used the new token.
    let auth = m.requests[0].0["authorization"].to_str().unwrap().to_string();
    assert_eq!(auth, format!("Bearer {}", file["tokens"]["access_token"].as_str().unwrap()));
}

#[tokio::test]
async fn retries_429_then_succeeds_and_reports_errors() {
    let home = tempfile::tempdir().unwrap();
    login(home.path(), 4_000_000_000);
    let mock: Shared = Arc::default();
    {
        let mut m = mock.lock().unwrap();
        m.replies.push((429, vec![("retry-after-ms", "10")], r#"{"error":{"message":"slow down"}}"#.into()));
        m.replies.push((200, vec![], tool_call_stream("{}")));
        m.replies.push((429, vec![], r#"{"detail":"You've hit your usage limit."}"#.into()));
        m.replies.push((400, vec![], r#"{"error":{"message":"bad model"}}"#.into()));
    }
    let base = serve(mock.clone()).await;
    let p = provider(home.path(), &base, None);
    assert!(p.complete(&request(), &CancellationToken::new()).await.is_ok());
    let err = p.complete(&request(), &CancellationToken::new()).await.unwrap_err();
    assert_eq!(err.to_string(), "Codex request failed (429): You've hit your usage limit.");
    let err = p.complete(&request(), &CancellationToken::new()).await.unwrap_err();
    assert_eq!(err.to_string(), "Codex request failed (400): bad model");
    assert_eq!(mock.lock().unwrap().requests.len(), 4);
}

#[tokio::test]
async fn stream_outcomes() {
    let home = tempfile::tempdir().unwrap();
    login(home.path(), 4_000_000_000);
    let mock: Shared = Arc::default();
    {
        let mut m = mock.lock().unwrap();
        m.replies.push((200, vec![], sse(&[json!({"type":"response.completed","response":{"status":"completed"}})])));
        m.replies.push((
            200,
            vec![],
            sse(&[json!({"type":"response.incomplete","response":{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"}}})]),
        ));
        m.replies.push((200, vec![], sse(&[json!({"type":"error","message":"overloaded"})])));
        m.replies.push((200, vec![], sse(&[json!({"type":"response.failed","response":{"error":{"message":"policy"}}})])));
        m.replies.push((200, vec![], tool_call_stream("not json")));
        // CRLF framing, residual frame without trailing blank line, [DONE] marker.
        let crlf = "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"name\":\"submit\",\"arguments\":\"{\\\"a\\\":1}\"}}\r\n\r\ndata: [DONE]\r\n\r\ndata: {\"type\":\"response.done\",\"response\":{}}";
        m.replies.push((200, vec![], crlf.into()));
        m.replies.push((200, vec![], String::new()));
    }
    let base = serve(mock).await;
    let p = provider(home.path(), &base, None);
    let c = CancellationToken::new();
    assert_eq!(p.complete(&request(), &c).await.unwrap(), Completion::NoToolCall);
    assert_eq!(p.complete(&request(), &c).await.unwrap(), Completion::CutOff);
    assert_eq!(p.complete(&request(), &c).await.unwrap_err().to_string(), "Codex error: overloaded");
    assert_eq!(p.complete(&request(), &c).await.unwrap_err().to_string(), "policy");
    assert_eq!(p.complete(&request(), &c).await.unwrap(), Completion::ToolCall(Value::Null));
    assert_eq!(p.complete(&request(), &c).await.unwrap(), Completion::ToolCall(json!({"a":1})));
    assert_eq!(
        p.complete(&request(), &c).await.unwrap_err().to_string(),
        "Codex stream ended before the response completed."
    );
}

#[tokio::test]
async fn not_logged_in_never_calls_the_api() {
    let home = tempfile::tempdir().unwrap();
    let mock: Shared = Arc::default();
    let base = serve(mock.clone()).await;
    let p = provider(home.path(), &base, None);
    assert!(!p.status().logged_in);
    assert_eq!(p.status().model, DEFAULT_MODEL);
    let err = p.complete(&request(), &CancellationToken::new()).await.unwrap_err();
    assert_eq!(err.to_string(), crate::llm::NOT_LOGGED_IN);
    assert!(mock.lock().unwrap().requests.is_empty());
}

#[test]
fn sse_framing_handles_split_utf8() {
    let mut s = SseEvents::default();
    let bytes = "data: {\"t\":\"ü\"}\n\n".as_bytes();
    let split = bytes.iter().position(|&b| b == 0xc3).unwrap() + 1;
    s.push(&bytes[..split]);
    assert_eq!(s.next_data(), None);
    s.push(&bytes[split..]);
    assert_eq!(s.next_data().as_deref(), Some("{\"t\":\"ü\"}"));
}

#[test]
fn toml_model_line() {
    assert_eq!(model_from_toml("model_reasoning = \"x\"\n  model = \"gpt-a\"\n").as_deref(), Some("gpt-a"));
    assert_eq!(model_from_toml("[profiles]\nfoo = 1"), None);
}
