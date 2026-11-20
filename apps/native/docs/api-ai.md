# proto-ai: generation API

Owner: proto-worker-7. Crate `proto-ai` (tokio + reqwest). Port of the reference app's generation path:
`src/lib/server/llm.ts` + `codex-credentials.ts` (LLM seam), `jobs.ts` (detached jobs), `agent-queue.ts` (Claude
agent queue, file part), the prompts in `api/variations`, `lib/server/ai-edit.ts`, `api/spokes`, `api/decisions`,
and the helpers `components.ts`, `guidelines.ts` (`describeGuidelines`, `htmlToText`), `pod-catalog.ts`
(`describePodContext`). Client side `src/lib/generate.ts` (NDJSON reader) is covered by `Jobs::subscribe` in-process
and by `ndjson::Reader` for HTTP clients.

**No `proto-core` dependency.** Everything that reads or writes shared state (guidelines lookup, queue item
context, placeholder fill, edit finish/fail, deleted-card checks) stays in `proto-server`; proto-ai takes the
values it needs as arguments.

## Agents and provider (parity with the reference)

Two ways to build a design, picked per request exactly like the reference (`agent` field, inspector toggle):

| `AgentChoice` | what happens | default |
|---|---|---|
| `Codex` | an LLM call through the `Provider` (OpenAI Codex Responses, ChatGPT login) | server default when `agent` is omitted |
| `Claude` | no LLM call: one queue item per direction in `<data>/agent-queue/`, answered by a Claude Code session via `POST /api/agent-queue/<jobId>/<index>` | the inspector's default |

`Provider` is a trait so another backend can be added later; only `CodexProvider` exists (no Anthropic API).

### CodexProvider (port of pi-ai `openai-codex` + `CodexCliCredentialStore`)

- **Auth:** `$CODEX_HOME/auth.json` (default `~/.codex/auth.json`), re-read on every call so a `codex login` is
  picked up live. Logged in = `tokens.access_token` and `tokens.refresh_token` present. Account id =
  `tokens.account_id`, else the access JWT claim `https://api.openai.com/auth`.`chatgpt_account_id`.
- **Refresh:** when the access JWT `exp` is within 5 min (or unreadable): `POST https://auth.openai.com/oauth/token`
  (form: `grant_type=refresh_token`, `refresh_token`, `client_id=app_EMoamEEZ73f0CkXaXp7hrann`), then write the
  new `access_token`/`refresh_token` (+`account_id`) back into auth.json in the CLI's format, other fields kept,
  `last_refresh` = now ISO, tmp file (mode 0600) + rename. Refreshes are serialized in-process. Tokens are never
  logged, printed or put into error messages.
- **Model:** `$PROTOTYPIE_MODEL`, else `model = "…"` in `$CODEX_HOME/config.toml`, else `gpt-6-astra`.
- **Request:** `POST https://chatgpt.com/backend-api/codex/responses` (SSE), headers `Authorization: Bearer …`,
  `chatgpt-account-id`, `OpenAI-Beta: responses=experimental`, `originator: pi`, `User-Agent: prototypie/0.1`,
  `accept: text/event-stream`. Body `{ model, store:false, stream:true, instructions, input:[{role:"user",
  content:[{type:"input_text",text}]}], text:{verbosity:"low"}, include:["reasoning.encrypted_content"],
  tool_choice:"auto", parallel_tool_calls:true, tools:[submit], reasoning:{effort, summary:"auto"} }`.
  Structured output = one strict `submit` function tool (JSON schema, `additionalProperties:false`).
- **Stream:** `response.output_item.done` with a `function_call` named `submit` carries the arguments;
  `response.completed|done|incomplete` ends it (`incomplete` + `max_output_tokens` → "Output was cut off (too long).");
  `error` / `response.failed` → error. 429/5xx are retried twice (honouring `retry-after`, capped at 10 s) unless
  the message is a usage limit.
- `generate_object` = the reference's `generateObject`: system + "Always deliver your answer by calling the `submit`
  tool exactly once…", up to 2 attempts (invalid arguments → re-ask with the parse error; no tool call → reminder).

## Public API (`proto_ai::…`)

```rust
pub struct Ai { /* cheap Clone: Arc inside */ }
impl Ai {
    /// data_dir = the app's data dir (queue files go to data_dir/agent-queue). provider None = codex not usable
    /// (LLM entry points return AiError::Llm("Not logged in to Codex…")); the queue mode still works.
    pub fn new(data_dir: PathBuf, provider: Option<Arc<dyn Provider>>, notifier: Arc<dyn Notifier>) -> Ai;
    pub fn with_codex(data_dir: PathBuf, notifier: Arc<dyn Notifier>) -> Ai; // CodexProvider::from_env()
    pub fn jobs(&self) -> &Jobs;
    pub fn queue(&self) -> &AgentQueue;
    pub fn status(&self) -> LlmStatus;                 // { provider:"openai-codex", model, logged_in }

    /// POST /api/variations. Starts a detached job and returns its id at once. `guidelines` = the server's
    /// guidelinesFor(req.space_id). Clamps count to 1..=8 and cleans components like the reference.
    pub fn start_variations(&self, req: VariationRequest, guidelines: Option<Guidelines>) -> JobId;

    /// In-place edit (POST /api/edit). Detached job emitting plan → variation|error → done. The caller (server)
    /// writes `editing`, then on the job's variation/error does finish()/fail() in the store.
    /// Codex: EDIT_SYSTEM prompt, title = design.title. Claude: a queue item mode "edit".
    pub fn start_edit(&self, design: EditTarget, req: EditRequest, guidelines: Option<Guidelines>) -> JobId;

    /// POST /api/spokes: 1..=8 components (Err(AiError::Empty(..)) when none).
    pub async fn spokes(&self, html: &str, title: Option<&str>, cancel: CancellationToken) -> Result<Vec<SpokeComponent>, AiError>;
    /// POST /api/decisions: filtered like the reference (known ids, deduped, ≥2 options, ≤4 decisions).
    pub async fn decisions(&self, designs: &[DecisionCandidate], context: Option<&str>, cancel: CancellationToken) -> Result<Vec<SuggestedDecision>, AiError>;
}

pub enum AiError { Llm(String) /* not logged in / model problem → 503 */, Cancelled /* → 499 */,
                  Empty(String) /* spokes 502 "The model found no components." · decisions 422 */, Other(String) /* → 500 */ }
```

### Jobs (`jobs.ts`)

```rust
pub type JobId = String;                               // nanoid(10)
impl Jobs {
    pub fn start<F, Fut>(&self, run: F) -> JobId
        where F: FnOnce(Emitter /* .send(VariationEvent) */, CancellationToken, JobId) -> Fut,
              Fut: Future<Output = Result<(), AiError>> + Send + 'static;
    pub fn exists(&self, id: &str) -> bool;            // running, or finished < 1 h ago (sweeps on start/exists)
    pub fn cancel(&self, id: &str);                    // trips the job's CancellationToken
    /// Replay from `from` then follow live; ends after Done. None = unknown/expired (→ 404).
    pub fn subscribe(&self, id: &str, from: usize) -> Option<impl Stream<Item = VariationEvent> + Send + 'static>;
    pub fn events(&self, id: &str, from: usize) -> Option<(Vec<VariationEvent>, bool /* done */)>;
}
```
A subscriber attached after the job ended still gets the full replay plus `Done`. A job always ends with exactly one `Done`; an `Err` from `run` becomes `Error { index: None, message }` first.
Emitting after `Done` is ignored. The HTTP stream (server) = `{"type":"job","id"}` line + one JSON line per event.

### Events and types (serde shapes = the reference JSON, camelCase)

```rust
#[serde(tag = "type", rename_all = "lowercase")]
pub enum VariationEvent {
    Job { id: String },
    Plan { directions: Vec<Direction> },
    Variation { index: usize, variation: Variation },
    Error { index: Option<usize>, message: String },
    Done,
}
pub struct Direction { title, brief }
pub struct Variation { title, rationale, html, viewport: Option<u32>, viewport_height: Option<u32> }
pub struct VariationRequest { base_html, bases: Option<Vec<BaseDesign>>, base_title, instruction, count, focus,
    scope: Option<Scope /* page|component */>, direction, annotations: Option<Vec<Annotation>>, agent: Option<AgentChoice>,
    pod_context: Option<Vec<PodContextEntry>>, space_id, import_from: Option<ImportFrom>, components: Option<Vec<ComponentRef>> }
pub struct EditRequest { design_id, instruction, focus, annotations, agent, pod_context, components }
pub struct EditTarget { id, title, html, space_id }
pub struct Guidelines { card_id, title, from_space, text }
pub struct SpokeComponent { name, description, selector }
pub struct DecisionCandidate { id, title, rationale, html }
pub struct SuggestedDecision { question, why, options: Vec<DecisionOption { design_id, label }> }
```
`PodContextEntry`, `PodField`, `ComponentRef`, `ImportFrom`, `Annotation` mirror `src/lib/types.ts` 1:1.

### Agent queue (`agent-queue.ts`, file + waiter part)

File format = the reference (`<data>/agent-queue/<jobId>-<index>.json`, pretty JSON, `QueueItem` exactly as in
`docs/api-server.md` §3 minus `context`, which the server computes and attaches when listing).

```rust
pub trait Notifier: Send + Sync {             // implemented by proto-server (notes service, 3 s timeouts)
    fn announce(&self, comment: String, element: String, element_path: String) -> BoxFuture<'static, Option<String>>;
    fn set_status(&self, note_id: String, status: NoteStatus /* Resolved | Dismissed */) -> BoxFuture<'static, ()>;
}
impl AgentQueue {
    /// Save the item (status pending, createdAt now), register the waiter, announce it (comment from
    /// announce_comment(), element "agent-queue item <k>", element path "data/agent-queue/<k>.json") and store the
    /// annotationId. Resolves with the delivered Variation; Err(Cancelled|Other("Card deleted")) when the token
    /// trips or cancel_item() is called (the item is then marked cancelled and its note dismissed).
    pub async fn enqueue_and_wait(&self, item: NewQueueItem, cancel: CancellationToken) -> Result<Variation, AiError>;
    pub async fn submit(&self, job_id: &str, index: usize, v: Variation) -> bool; // false = nobody waits (→ 404); marks done, resolves note
    pub async fn cancel_item(&self, job_id: &str, index: usize, reason: &str) -> bool; // e.g. "Card deleted" (→ 409 path); the job gets Error{index, reason}
    pub fn is_waiting(&self, job_id: &str, index: usize) -> bool;
    pub fn waiting(&self) -> Vec<(String, usize)>;
    pub async fn load(&self, job_id: &str, index: usize) -> Option<QueueItem>;
    pub async fn remember_placeholder(&self, job_id: &str, index: usize, placeholder_id: &str);
    /// Every item on disk in any status with its file mtime (for /api/agent-tasks `doneAt`).
    pub async fn read_all(&self) -> Vec<(QueueItem, SystemTime)>;
    /// Pending items with a live waiter, oldest first (without context).
    pub async fn list_pending(&self) -> Vec<QueueItem>;
    pub async fn save_screenshot(&self, job_id: &str, data_url: &str) -> Option<PathBuf>; // png/jpeg/webp/gif ≤ 8 MiB
}
pub fn announce_comment(item: &QueueItem) -> String;          // same text and suffixes as the reference
pub fn infer_viewport(html: &str) -> Option<u32>;              // <meta name=viewport content="width=N"> in 200..=4000
pub fn infer_viewport_height(html: &str) -> Option<u32>;
```
Delivery route order in the server (unchanged from the reference): validate → placeholder deleted? →
`queue.cancel_item(.., "Card deleted")` + 409 → else `queue.submit()` (false → 404) → fill the placeholder → 200.

Edit jobs: the error event's message is exactly `"Cancelled"` when the job was cancelled (the reference then
clears `editing` without setting `editError`).

### Prompts (`proto_ai::prompts`, pure, unit-tested against the reference strings)

`DESIGN_SYSTEM`, `TITLE_RULE`, `describe_task(&VariationRequest)`, `plan_prompt(task, count)`,
`build_prompt(task, &Direction)`, `EDIT_SYSTEM`, `describe_edit(&EditTarget, &EditRequest)`, `SPOKES_SYSTEM`,
`spokes_prompt(html, title)` (html capped at 150 000), `DECISIONS_SYSTEM`, `decisions_prompt(..)` (+ `condense`),
`describe_guidelines`, `html_to_text`, `describe_pod_context`, `describe_components`, `clean_components`.

### NDJSON client (`generate.ts`)

`proto_ai::ndjson::Reader` turns a byte stream into `VariationEvent`s (split on `\n`, skip blank lines), for
clients that follow `/api/variations` over HTTP. In-process, prefer `Ai::jobs().subscribe()`.

## Tests

`cargo test -p proto-ai`. The HTTP side is tested against a local mock server (no real API calls, no real
`~/.codex`: tests point `CODEX_HOME` at a temp dir via `CodexProvider::new(CodexConfig {..})`).
