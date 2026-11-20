# proto-server: HTTP API (route table)

Owner: proto-worker-2. Crate `proto-server`, axum 0.8 on tokio, bound to **127.0.0.1:3124**.
Goal: the same paths, status codes and JSON shapes as the reference app's `src/app/api/**` on :3123, so
`scripts/agent-queue.md` and the worker agents work against this server by changing only the port.
Source of truth for every shape below: the reference route file plus the `src/lib/server/*` module named in
each section. Where this server deliberately differs, it says **Differs**.

## Embedding

```rust
// proto-server public API (lib.rs)
pub struct ServerConfig { pub addr: SocketAddr /* 127.0.0.1:3124 */, pub data_dir: PathBuf, pub notes_url: String /* DEFAULT_NOTES_URL = http://127.0.0.1:3124 */, pub app_url: String /* http://localhost:3124/ */, pub codex: bool }
pub struct ServerHandle { pub addr: SocketAddr, /* shutdown on drop */ }
pub async fn serve(cfg: ServerConfig, store: proto_core::Store, ai: Option<proto_ai::Ai>) -> anyhow::Result<ServerHandle>;
pub fn router(state: AppState) -> axum::Router; // for tests (tower::ServiceExt::oneshot)
```

The app starts it on its tokio runtime and shares the `proto-core` Store. All writes to shared state go
through the Store (one version per batch, change events, append log), never through files directly.
Without `ai` (no API key) the LLM routes answer 503 `{ error }`; the agent-queue mode works without it.

## Files (all under the data dir, `~/Library/Application Support/Prototypie`, `PROTOTYPIE_DATA` overrides)

| reference (`<repo>/data/…`) | here | owner |
|---|---|---|
| `state.json`, `state-log.ndjson` | same names | proto-core Store |
| `agent-queue/<jobId>-<index>.json`, `<jobId>-screenshot.<ext>` | same | proto-server |
| `polls/<id>.json` | same | proto-server |
| `implement/<designId>.html` | same | proto-server |
| `pod-catalog.json` (read-only input) | same; first run copies the reference file if missing | proto-server |
| `public/imports/*`, `public/vendor/*` | `public/imports/*`, `public/vendor/*` (copied by proto-core import), served at `/imports/*`, `/vendor/*` | proto-core copies, proto-server serves |

Never write into `apps/web/data` (PLAN rule).

## Common conventions

- Bodies are JSON; a body that isn't JSON counts as `null` and fails validation (400) unless noted.
- Validation errors: the reference returns zod issues. Here `issues` is an array of
  `{ path: (string|number)[], message: string, code: string }` (same keys zod uses; `code` best effort).
- Ids: `Id = ^[\w-]{1,64}$`. New ids are nanoid-style `[A-Za-z0-9_-]`: designs/jobs/polls/votes 10 chars, spaces 8.
- `Cache-Control: no-store` where the reference sets it (marked *no-store*).
- Times: `createdAt` on records = epoch ms (number); queue/task times = ISO strings. Keep both as the reference.

---

## 1. Shared state — `state-store.ts`

### `GET /api/state?since=N&epoch=E` *no-store*
`200 { epoch: string, version: number, records: { "<key>/<id>": { rev: number, value?: any, deleted?: true } } }`
All records (tombstones included) when `N<=0` or `E` ≠ server epoch; else only records with `rev > N`.

### `PUT /api/state`
Body `{ clientId?: string, reason?: string, changes: [{ path: string, value: any } | { path: string, deleted: true }] }`
- 400 `{ error: "Body must be JSON." }` · 400 `{ error: "`changes` must be an array." }`
- 400 `{ error: "Invalid record path (not a shared store key): <path>" }` — path must be `<key>/<id>`, `< 512` chars,
  key ∈ SHARED_KEYS: `spaces designs runs portals promotedFrom seededFrom notes reviewedIds lanes spokeFocus publishedPolls pollWinners`.
- 400 `{ error: "Missing value for <path>" }` (not deleted and no value)
- 200 `{ epoch, version }`. All changes share one version; empty `changes` returns the current cursor without bumping.
- `clientId`/`reason` truncated to 200 chars; User-Agent goes to the log.
- Side effect: deleted `designs/<id>` cancels waiting queue items whose placeholder is that id (§3).
- Log line (NDJSON, per change): `{ ts, rev, clientId, path, op: create|update|delete, before: absent|deleted|live, prevRev, title, reason, ua }`.

### `GET /api/state/events` (SSE)
Headers `Content-Type: text/event-stream; charset=utf-8`, `Cache-Control: no-store, no-transform`, `Connection: keep-alive`, `X-Accel-Buffering: no`.
First message `data: {"epoch","version"}\n\n`, then one `data: {"epoch","version","clientId"?}\n\n` per applied batch,
`: ping\n\n` every 20 s.

## 2. Generation jobs — `jobs.ts`, `variations/route.ts`

Jobs run detached (survive client disconnects), kept 1 h after finishing for replay.
Stream format: NDJSON, `Content-Type: application/x-ndjson; charset=utf-8`, `Cache-Control: no-cache`, header `X-Job-Id`.
First line `{"type":"job","id"}`, then events from `from`:
```
{ type: "plan", directions: [{ title, brief }] }
{ type: "variation", index: number, variation: { title, rationale, html, viewport?, viewportHeight? } }
{ type: "error", index: number | null, message: string }
{ type: "done" }            // always last
```
(`from` counts events excluding the `job` line.)

### `POST /api/variations` → job stream (200)
Body `VariationRequest`:
`{ instruction: string, count: number, baseHtml?, baseTitle?, bases?: [{id,title,html}], focus?, scope?: "page"|"component",
direction?: {title,brief}, annotations?: [{element?, comment}], agent?: "codex"|"claude", podContext?: PodContextEntry[],
spaceId?, importFrom?: { element, source, screenshot? }, components?: [{id,title,html}] }`
- `count` = clamp(round(count||3), 1, 8). `components` cleaned: ≤12, title ≤200, html ≤120 000, empty html dropped.
- Guidelines: `guidelinesFor(spaceId)` (see §9) become part of the system prompt / queue item.
- `importFrom` + claude: one queue item `mode:"import"` (§3). `importFrom` + codex: `plan` with one direction then
  `error {index:0, message:"Importing from the app needs the Claude agent (switch the inspector's agent to Claude)."}`.
- `agent:"claude"`: directions = `[direction]` or `Direction 1..N` with brief = instruction
  (or "Explore a meaningfully different direction."); emit `plan`; each direction → `enqueueAndWait` → `variation`/`error`.
- else (codex): proto-ai plans N directions (skipped when `direction` given), then generates each in parallel.
  Prompts/system text: proto-ai owns them (port of `variations/route.ts` DESIGN_SYSTEM/describeTask).

### `GET /api/variations/:id?from=N` → job stream, or 404 `{ error: "Unknown or expired job" }`
### `HEAD /api/variations/:id` → 200 known · 404 unknown
### `DELETE /api/variations/:id` → 204 (aborts the job; waiting queue items become cancelled)

## 3. Agent queue — `agent-queue.ts`

QueueItem (file `agent-queue/<jobId>-<index>.json`, pretty JSON):
`{ jobId, index, instruction, focus?, scope?, annotations?, direction:{title,brief}, baseTitle?, baseHtml?, bases?,
podContext?, components?, guidelines?: {cardId,title,text}, mode?: "import"|"edit", targetDesignId?, element?,
importSource?, importScreenshot?, createdAt: ISO, status: "pending"|"done"|"cancelled", annotationId?, placeholderId?,
context? }`
`context` (computed on list, not stored): `{ spaceId, spaceName, spacePath: string[], baseDesignId?, placeholderId?,
cards: [{id,title}], seedDesign?: {id,title,originalId} }` — from the `runs/*` record whose `serverJobId == jobId`
(`placeholderId = run.designIds[index]`, `baseDesignId = run.parentId`), or for edit items without a run from the target card.
`spacePath` always starts with "Main canvas"; root's name is "Main canvas".

### `GET /api/agent-queue`
`200 { items: QueueItem[] }` — status pending **and** a live waiter in this process, oldest first, with `context`.
Remembers `context.placeholderId` on disk as `placeholderId` when missing.

### `POST /api/agent-queue/:jobId/:index/claim` (Rune addition)
**Claim the item before working on it.** Body `{ agent: string(1..100) }` (worker or session name) → `200 { ok, agent, claimedAt }`.
Stored as `agent` + `claimedAt` in the item file (survives restarts); re-claiming overwrites. 404 unknown item · 409 not
pending (done/cancelled) · 400 issues. `GET /api/agent-queue` and `GET /api/agent-tasks` return `agent` (+ `claimedAt`).

### `POST /api/agent-queue/:jobId/:index`
Body `{ title: string(min 1), rationale: string, html: string(min 1), viewport?: int 200..4000, viewportHeight?: int 200..4000 }`
- 400 `{ error: issues[] }` (note: `error` holds the issues array, like the reference)
- viewport/viewportHeight default from `<meta name="viewport" content="width=N,height=M">` when in 200..4000.
- 409 `{ error: "The placeholder card for this item was deleted in the app, so the item is cancelled and nothing was placed. If the design is still wanted, create a card with POST /api/agent/cards." }`
- 404 `{ error: "No job is waiting for this item (done, cancelled or expired)" }`
- 200 `{ ok: true }`: resolves the waiter, status → done, closes the item's note in the notes service
  (resolved, closed by the agent; cancelled → dismissed), and fills the placeholder card in shared
  state when it is still `generating` (or `error` without html): `title, rationale, html, viewport?, viewportHeight?, status:"ready", error: undefined`
  (clientId `server:agent-queue`, reason `queue delivery <jobId>-<index>`).
- Edit items (`mode:"edit"`) resolve the edit job instead (§5).

### Notes service (`notifyAgent`)
The server files a note (comment, element, element path) under `APP_URL` in Prototypie's notes service
(`proto_server::NotesService::create`) and gets its id back. Short timeouts; failure → no note (never blocks the caller).
The notes service itself is served by this crate under `/api/notes` (section "Notes service" below).
Queue comment: `[prototypie job <jobId>#<index>] <instruction|brief>` + the same suffixes as the reference
(COMBINE, POD DATA, REUSE COMPONENTS AS-IS, FOLLOW DESIGN GUIDELINES, IMPORT, EDIT IN PLACE, context + skills, rules);
element `agent-queue item <jobId>-<index>`, element path `data/agent-queue/<jobId>-<index>.json`.
**Differs:** `APP_URL = http://localhost:3124/` so notes for this app don't land in the reference app's session
(the lead's watchers filter by the `[prototypie …]` tag, not the URL). The element path stays relative.

## 4. Agent canvas skills — `agent-canvas.ts`

Layout constants: NODE_W 380, NODE_H 420 (560 when viewport < 600 or viewportHeight > viewport), GAP 160,
PORTAL 300×330, collision margin 40. Placement: row right of `near` (x = near.x + 380 + 160, same y), else
below everything (x = min x, y = max bottom + 160; 0,0 when empty); step right by w+160 while colliding.
CardSchema: `{ title: trim 1..200, rationale?: ≤4000, html: trim min 1 ("html is empty"), viewport?, viewportHeight?,
kind?: page|variation|spoke, parentId?: Id, component?: bool }`.

### `GET /api/agent/canvas`
`200 { spaces: [{ id, name, parentSpaceId?, kind?, localPath?, cards: [{ id, title, status, kind, position:{x,y} }] }] }`
Root first: `{ id:"root", name:"Main canvas" }`.

### `POST /api/agent/cards`
Body `{ spaceId?: Id, nearId?: Id, cards: Card[1..50] }` → `200 { spaceId, ids: string[] }`
- 400 `{ error: "Invalid body", issues }` · 404 `Unknown design <nearId>` · 400 root rejected
  (`"The main canvas holds only projects: pass the spaceId of a project or one of its explorations (GET /api/agent/canvas lists them; kind \"project\")."`)
  · 404 `Unknown space <id>` · 404 `cards[i].parentId: unknown design`. Errors are `{ error }`.
- Space defaults to nearId's space. Design written: `{ id, spaceId, parentId?, kind: kind??"variation", title, rationale: ""?, html,
  status:"ready", position, createdAt: now+i, viewport?, viewportHeight?, component?: true }`; clientId `agent`, reason `agent-cards`.

### `POST /api/agent/explorations`
Body `{ name: trim 1..120, parentSpaceId?: Id, seedId?: Id, cards?: Card[≤50] }` → `200 { spaceId, portal:{x,y}, seedId?, cardIds }`
- Same errors (seed unknown → 404 `Unknown design <seedId>`). One batch: `spaces/<id> {id,name,parentSpaceId,createdAt}`,
  `portals/<id> {x,y}`, seed copy `designs/<new> {...seed, id, spaceId, parentId: undefined, starred:false, position:{0,0}, createdAt}`
  + `seededFrom/<new> = seed.id`, then cards (row right of 0,0 when seeded). clientId `agent`, reason `agent-exploration`.

## 5. In-place edit — `ai-edit.ts`

### `POST /api/edit`
Body `{ designId: Id, instruction: trim 1..4000, focus?: ≤2000, annotations?: [{element?, comment}] ≤50, agent?: "codex"|"claude",
podContext?: any[], components?: [{id ≤64, title ≤400, html}] ≤20 }` → `200 { jobId }`
- 400 `{ error: issues[] }` · 404 `{ error: "No such design" }` · 409 `"Only a finished design can be edited"` ·
  409 `"This design is already being edited"`.
- Writes `editing: { jobId, instruction, agent, startedAt }`, `editError: undefined` on the card (reason `ai edit start <jobId>`).
- Job events: `plan [{title: d.title, brief: instruction}]` then `variation`/`error` at index 0.
- claude: queue item `mode:"edit"`, `targetDesignId`, `scope:"page"`, `direction.title = "Edit in place: <title>"`, `baseHtml`.
  codex: proto-ai edit call. Default agent **codex** (reference default).
- Done (only if `editing.jobId` still matches): html, `rationale || old`, viewport?, `versions = [...versions, {html,title,rationale,viewport?,viewportHeight?,replacedAt,instruction}]` last 5, clear editing/editError.
  Fail: clear editing, `editError = message` (not on cancel). clientId `server:ai-edit`.

### `DELETE /api/edit/:designId` → 204 cancelled (also clears a stale flag) · 404 not editing

## 6. Implement — `implement/route.ts`, `implement-dedupe.ts`, `implement-links.ts`

### `POST /api/implement`
Body `{ designId: ^[\w-]+$, title?, rationale?, html?, force?: bool }`
- 400 `{ error: "designId required" }`
- Design = body (when html non-empty and title is a string) else stored card: 404 `"Unknown design"`, 409 `"Design isn't ready"`.
- Linked ids = designId + promotedFrom links both ways, transitively.
- Without force: any linked card with `implementRequestedAt` → 409 `{ error: "This design (or a linked copy) was already sent to the agent. Send again with force: true.", done: true, designId }`.
- Serialized dedupe (one at a time): open note `[prototypie implement <id>]` in the notes service (pending/acknowledged) for a linked id →
  `200 { ok: true, duplicate: true, annotationId }`; same for a note this server posted < 2 min ago that isn't listed yet;
  resolved one without force → 409 `{ error: "This design was already implemented. Send again with force: true.", done: true, annotationId }`.
- Else write `implement/<designId>.html`, post note: comment `[prototypie implement <id>] Implement the design "<title>" in the product (this codebase). Why: <rationale> The design's HTML is in <file>.`,
  element `design <id>: <title>`, element path = file.
  500 `{ error: "Couldn't save the design: …" }` · 503 `{ error: "No coding agent reachable (the notes service did not take the note; agents read notes with \`prototypie notes\`). The design was saved to <file>.", file }`
  · 200 `{ ok: true, annotationId, file }`.
- **Differs:** `file` is the absolute path under the data dir (the reference's relative `data/implement/…` only works from its cwd).
  The server does not set `implementRequestedAt`; the client does, as in the reference.

## 7. Agent tasks — `agent-tasks.ts`

### `GET /api/agent-tasks` *no-store*
`200 { capacity: { working: number|null, total: number|null, agents: [{name, status: working|idle|unknown}] }, tasks: AgentTask[], notesService: bool }` (`notesService`: the open notes could be read)
- capacity: `rex block ls -s $REX_SESSION(prototypie) --json` → blocks labelled `proto-*`; status "working" when the last 12 lines
  of `rex capture -b <id> --trim` contain `esc to interrupt`. Cached 3 s; `REX_BIN_PATH` overrides the binary; 4 s timeout.
- tasks: newest 80 queue files (without baseHtml; bases → {id,title}; `doneAt` = file mtime when not pending) with context
  + their note in the notes service, plus all open notes and Implement notes (all states, scanned ≤ once/min) that
  aren't queue items, belong to a localhost URL and don't start with `[prototypie job `. Sorted by `updatedAt` desc.
- AgentTask: `{ id, kind: queue|note|implement, designId?, summary?, title, request, jobId?, index?, baseTitle?, status:
  waiting|working|delivered|resolved|cancelled, createdAt, updatedAt, steps: [{key, label, at: ISO|null, done}], target?:
  {spaceId, spaceName, spacePath, designId?}, noteId?, noteStatus?, element?, thread: [{id, role, content, at}] }`
  Step keys/labels exactly as the reference (Eingang, Notiz, In Arbeit, Geliefert, Abgeschlossen, Abgebrochen;
  notes: Gesendet/Notiz, In Arbeit, Abgeschlossen, Verworfen). Status rules as `queueTask`/`noteTask`.

### `POST /api/agent-tasks/:id/notes`
Body `{ text: trim 1..2000 }` → `200 { ok: true, thread }`
- 400 `{ error: "Bitte einen Text eingeben (höchstens 2000 Zeichen)." }` · 404 `"Unbekannte Aufgabe"` (only listed notes)
  · 502 `"Notizdienst antwortet <status>"` / `"Notizdienst nicht erreichbar"`.
- Appends the text as a `human` message to the note's thread (`NotesService::reply`).

## 8. Projects — `projects.ts`

TTL file: `PROTOTYPIE_PROJECTS_TTL`, default ``projects.ttl` in the data directory` (read-only). Tiny Turtle reader;
subjects with `rdf:type schema:WebSite`; id = local name (`^[\w.-]+$`); `localPath` = repo when it is an existing absolute
dir, else `<ttl dir>/<basename(repo)>` when that exists; `pod` from `schema:producer` ending in `/clients/index.ttl`.

### `GET /api/projects` *no-store* → `200 { available: bool, file: string, projects: [{ id, name, repo?, localPath?, keywords: string[], pod?: {doc, subject} }] }` (sorted by name)
### `GET /api/projects?match=<name>` *no-store* → `200 { localPath?: string }`
### `GET /api/projects/:id` *no-store* → `200 { ...project, readme?: string }` (first prose paragraph ≤ 400 chars, secrets filtered) · 404 `{ error: "Unknown project" }`
### `POST /api/projects/:id`
Body `{ spaceId: ^[\w-]+$ }` → `200 { ok: true, annotationId }` · 400 `"spaceId required"` · 404 `"Unknown project"` ·
503 `"No coding agent reachable (the notes service did not take the note; agents read notes with \`prototypie notes\`)"`. Note text (German) built from the project as the reference.

## 9. Guidelines (internal, used by §2 and §5)
Card with the newest `guidelinesAt` among ready cards with html in the space, else nearest ancestor (… → root).
`{ cardId, title, fromSpace, text = htmlToText(html)[..12000] }`.

## 10. Pod catalog — `pod-catalog.ts`

### `GET /api/pod-catalog[?refresh=1]`
`200 { version, pods: [{id, label, display, note?}], entries: PodCatalogEntry[], checkedAt: ISO }` · 500 `{ error }`.
Entries with `live: true` are probed (GET, `Accept: <accept|text/turtle>`, 2.5 s timeout) at `new URL(path, POD_BASE_<ID> || base)`:
2xx → access "live" (+ `contains` names for containers), 401/403 → "401", else documented; `probe: {status|"error", checkedAt, contains?}`.
`live`/`accept` are stripped from the output. Cached 5 min.

## 11. Polls & decisions — `features/poll/*`

### `POST /api/polls`
Body `{ title: trim 1..200, intro?: ≤2000 = "", decisions: [{ id, question: trim 1..300, why?: ≤1000 = "", options: [{ id, designId, label: trim 1..120, title ≤300, rationale ≤4000 = "", html 1..2 000 000 }] 2..4 }] 1..8 }`
→ `201 { id, path: "/poll/<id>", lanOrigins: string[] }` (**Differs:** origins of the stakeholder listener, see below,
not of the request's port).
400 `{ error: "<path>: <message>" }` (first issue) · 500 `{ error: "Could not save the poll: …" }`.
Stored: `{ id, title, intro, createdAt, decisions, votes: [] }`.

### Stakeholder listener (LAN) — lead decision 2026-11-18
The full API stays on **127.0.0.1:3124 only**; it is never bound to 0.0.0.0 (it would expose state writes).
Colleagues vote through a **second, separate listener** on the LAN interface:
- Off by default. Started when a poll is shared: Rust `ServerHandle::share_polls(port) -> Vec<String /* origins */>`,
  or over loopback `POST /api/polls/share { port? }` → `200 { lanOrigins }` (`DELETE /api/polls/share` stops it);
  port configurable, default 3125; stopped with the app. Binds the first non-internal IPv4 (or a configured interface), never the loopback API router.
- Serves ONLY: `GET /poll/:id` (the static poll page, title "Design poll · Prototypie"), `GET /poll/assets/*`
  (its JS/CSS), `GET /api/polls/:id`, `GET /api/polls/:id/votes`, `POST /api/polls/:id/votes`. Everything else → 404
  (no state, queue, agent, project or LLM routes; no `POST /api/polls`).
- Scoped to published polls: only ids whose `polls/<id>.json` exists; anything else → 404 `{ error: "Poll not found." }`.
- Votes: same validation as below, body ≤ 16 KiB, and rate-limited per client IP (token bucket, 30 votes/min,
  burst 10 → 429 `{ error: "Too many votes, please wait a moment." }`). GETs ≤ 120/min per IP.
- `lanOrigins` in the `POST /api/polls` response = `http://<lan ip>:<stakeholder port>` once sharing is on (empty
  before); the app's share link is the first origin + `/poll/<id>`.
- The poll page is one static HTML+JS bundle in proto-server (no Next): intro (name, voter `{id,name}` in
  localStorage) → one decision per step (progress dots, Desktop/Mobile toggle, A–D option cards with live sandboxed
  iframes, sticky comment + submit) → done (thank-you, tallies with own pick marked); missing poll →
  "This poll doesn't exist" / "The link may be mistyped, or the poll was removed. Ask the designer for a fresh link."
  Option iframes use `sandbox="allow-scripts"` (no same-origin), `srcdoc` from the poll's html.

### `GET /api/polls/:id` *no-store* → the Poll · 404 `{ error: "Poll not found." }` (id must match `^[A-Za-z0-9_-]{6,32}$`)
### `GET /api/polls/:id/votes` *no-store* → `{ votes, tallies: [{ decisionId, total, options: [{optionId, count, voters: [{name, comment?}]}], leaders: string[] }] }`
### `POST /api/polls/:id/votes`
Body `{ voterId 4..64, voter trim 1..80, decisionId, optionId, comment?: trim ≤2000 }` → 200 `{ votes, tallies }`
· 400 `"Unknown decision."` / `"Unknown option."` / validation · 404 `"Poll not found."`. One vote per voter per decision
(re-vote replaces, keeps `createdAt`), voter name synced across their votes; per-poll write lock, atomic file replace.

### `POST /api/decisions` (LLM, via proto-ai)
Body `{ designs: [{id 1..64, title ≤300, rationale ≤4000 = "", html ≤2 000 000}] 2..16, context?: ≤2000 }`
→ `200 { decisions: [{ question, why, options: [{designId, label}] }] }` (≤4 decisions, 2..4 known options each)
· 400 `"Body must be JSON."`/first issue · 422 `"The agent couldn't find a clear decision between these designs."`
· 499 `"Request cancelled."` · 503 (LLM not configured) · 500.

## 12. Spokes — `spokes/route.ts` (LLM, via proto-ai)
### `POST /api/spokes`
Body `{ html: string, title?: string }` → `200 { components: [{ name, description, selector }] }` (≤ 8)
· 400 `"Invalid JSON body."` / `"No page HTML to analyse."` · 502 `"The model found no components."` / LLM error.

## 13. Bundles + static assets — `bundles/route.ts`, Next's `public/`
### `GET /api/bundles` → `200 [{ file: "/imports/<f>.json", name, count, updatedAt: ms }]` (newest first; unreadable files skipped), from `<data>/public/imports/`
### `GET /imports/*path`, `GET /vendor/*path` → files under `<data>/public/imports|vendor/` (proto-core's import copies the reference `public/` there).
Read-only, MIME by extension, `nosniff`, `Cache-Control: public, max-age=300`; no listings; `..`, absolute paths, hidden
segments, backslashes and symlinks leading outside the root → 404. proto-web renders designs with base URL `http://127.0.0.1:3124/`.

## 14. Rust API for the app
Embedded (lead decision: one Store per data dir, the app's):
```rust
let ai = proto_ai::Ai::with_codex(data_dir, proto_server::notifier(proto_server::DEFAULT_NOTES_URL, proto_server::DEFAULT_APP_URL));
let server = proto_server::start(store.clone(), ai.clone(), proto_server::ServeOpts { runtime: Some(rt.handle().clone()), ..Default::default() })?;
// sync; binds 127.0.0.1:3124 now (Err on "address in use"); poll LAN listener off; data dir = store.data_dir()
server.shutdown().await;   // graceful (or drop the handle)
```
Standalone/tests: `serve(cfg, store, ai: Option<Ai>) -> ServerHandle { addr, ai }` (example `serve`, temp data dir; same port, stop it when running the app); `ServerHandle::share_polls(port) -> ShareState { sharing, port, origins }`,
`stop_sharing()`, `sharing() -> watch::Receiver<ShareState>` (for "Geteilt im LAN" / "nur lokal"). Over HTTP (loopback):
`GET|POST|DELETE /api/polls/share` → `{ sharing, port, lanOrigins }`. `proto_server::notifier(notes_url, app_url)` gives the notes
service as notifier when the app builds its own `Ai`. `PROTOTYPIE_LAN_IP` overrides the detected LAN address.

---

## Implementation plan
1. M1: crate skeleton, `serve()`, `GET/PUT /api/state`, `/api/state/events` against proto-core's Store (`docs/api-core.md`).
2. Agent queue + jobs + variations (claude mode) + agent/canvas|cards|explorations + edit (claude mode): the worker workflow.
3. Projects, pod-catalog, bundles, polls, implement, agent-tasks.
4. LLM routes (codex mode of variations/edit, decisions, spokes) once proto-ai publishes `docs/api-ai.md`.
Tests: `router()` + `oneshot` per route against a temp data dir; a test notes service on an ephemeral port for `NotesService`.

## Notes service — `src/notes/`

Prototypie's own notes service (spec-public/05 §1). Stored in `<data dir>/notes/<collectionId>.json`
(`{format: 1, collection, notes}`; atomic write + fsync before the answer). Local only: loopback peer and `Host`
`127.0.0.1`/`localhost`/`[::1]` (else 403 `not_local`); `Origin` or `Sec-Fetch-Mode` → 403 `browser_request`; no
CORS. Bodies: `application/json` (415), JSON object (400 `invalid_json`), ≤ 1 MiB (413). Errors:
`{ error, code, field?, note? }`. Answers `Cache-Control: no-store`. Times `YYYY-MM-DDTHH:MM:SS.mmmZ`.

| Method | Path | Answer |
|---|---|---|
| GET | `/api/notes/info` | `{ service: "prototypie-notes", apiVersion: 1, appVersion, pid, cursor, counts, collections, watchers }` |
| GET | `/api/notes/collections[?location&state]` | `{ collections }` (oldest first) |
| POST | `/api/notes/collections` `{location, reuse?}` | 201/200 `{ collection }` (reuses the first active one) |
| GET | `/api/notes/collections/{id}` | `{ collection, notes }` (every state, by `seq`) |
| PATCH | `/api/notes/collections/{id}` `{state}` | `{ collection }` |
| POST | `/api/notes/collections/{id}/notes` | 201/200 `{ note }` (`clientRef` makes it idempotent) |
| POST | `/api/notes` `{location, …}` | 201/200 `{ note }` (finds or makes the collection, one lock) |
| GET | `/api/notes[?status&location&locationPrefix&collection&tag&limit]` | `{ notes, count, more }` (default: pending) |
| GET | `/api/notes/watch?after&timeout&batchWindow&by[&filters][&replies=1]` | long-poll `{ notes, count, cursor, timedOut[, replies] }` |
| GET | `/api/notes/{id}` | `{ note }` |
| POST | `/api/notes/{id}/status` `{status, by?, message?, ifStatus?}` | `{ note }`; 409 `status_conflict` with `note` |
| POST | `/api/notes/{id}/thread` `{role, content}` | 201 `{ note }` |

A note: `id, collectionId, location, seq, kind, status, closedBy?, closedAt?, tag?, comment, element, elementPath,
designId?, clientRef?, clientTimestamp?, createdAt, updatedAt?, target, thread[{id, role, content, createdAt,
createdAtMs}]`. `seq` is the wake-up number (new and reopened notes; a person's reply with `replies=1`).
Limits: 5000 notes per collection, 500 messages per note, 1000 collections (409 `limit_reached`), 32 waiting
watches (429). `ServerHandle::shutdown` ends waiting watches with 503 first. `NotesService` (`src/notes/mod.rs`) is
the HTTP client the queue, `/api/implement`, project-info and `/api/agent-tasks` use; `GET /api/agent-tasks`
also carries `notesWatchers`. The `prototypie` command (proto-app `src/cli/`) wraps these routes for agents.

