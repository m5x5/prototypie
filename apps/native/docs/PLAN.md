# Prototypie in Rune: full standalone rewrite (macOS first)

Goal: a native macOS Prototypie built with Rune (Rust, GPU-drawn), feature-equivalent to the Next.js app in
`apps/web` (the "reference app", ~19k lines TS), backend included. The reference app keeps
running on :3123 meanwhile; this app must never write the reference app's `data/`.

Rune: pinned at rev 4a2e437 in `Cargo.toml` (plus Rune's `[patch.crates-io]`). Upstream for reading
docs, examples and source: https://github.com/anomalyco/rune. Read `crates/rune/README.md`, `AGENTS.md`, `examples/`
(glass-panel, frosted-bar, sketch, canvas input, overlay, kit-gallery) and `crates/rune-kit` before writing UI.
Toolchain: `rust-toolchain.toml` pins 1.99.1. Use rustup's cargo: `PATH="$HOME/.cargo/bin:$PATH" cargo …`
(Homebrew's cargo 1.98 comes first on PATH otherwise).

## Crates (each owner edits only their crate unless agreed with the lead)

| crate | owner | what |
|---|---|---|
| `proto-core` | proto-worker-1 | Domain types and the record store. Types serde-compatible with the reference `data/state.json` (`{version, epoch, records: {"<type>/<id>": {rev, value} \| {rev, deleted}}}`; types: designs, spaces, portals, runs, lanes, promotedFrom, reviewedIds, seededFrom, spokeFocus). Store: load/save atomically (tmp+rename, off the UI thread), versioned writes, change events (channel), the append log. Queries: children of a space, project of a space (`kind:"project"`, `localPath` + ancestor inheritance), hub of a spoke, main variant, promoted/reviewed sets. Data dir: `~/Library/Application Support/Prototypie` (env `PROTOTYPIE_DATA` overrides). `import_reference(path)` copies a reference state.json once. Unit tests. |
| `proto-server` | proto-worker-2 | Embedded axum HTTP API on **127.0.0.1:3124**, same paths and JSON shapes as the reference `src/app/api/**` (state + SSE events, agent-queue + delivery, agent/cards, agent/explorations, agent/canvas, projects, pod-catalog, polls/votes, decisions, spokes, variations, edit, implement, agent-tasks, bundles), so `scripts/agent-queue.md` workflows and the worker agents work against it by changing only the port. Runs on a tokio runtime started by the app; shares the `proto-core` Store. Notes service: queue items are filed as notes in Prototypie's own notes service (see `docs/api-server.md`). |
| `proto-ai` | proto-worker-7 | Generation, parity with the reference: port `src/lib/server/llm.ts` (OpenAI Codex Responses via the ChatGPT login in `~/.codex/auth.json`, same refresh, model `$PROTOTYPIE_MODEL` > `~/.codex/config.toml` > `gpt-6-astra`; no Anthropic API) behind a `Provider` trait, the variations/edit/spokes/decisions prompts, detached jobs (`jobs.ts`) and the "Claude agent queue" mode (queue files + waiters in the data dir instead of an LLM call). Agent per request like the inspector toggle (`codex` \| `claude`). Exposes async fns plus an event stream the app and server consume; no proto-core dependency. See `docs/api-ai.md`. |
| `proto-web` | proto-worker-3 | HTML rendering on macOS via WKWebView (objc2 crates): (a) offscreen snapshots of a design's html at a viewport width into RGBA `rune::Pixels`, a worker thread + bounded LRU cache (entries and decoded bytes), invalidated by design rev; (b) a live interactive WKWebView child NSView placed over a rectangle of the Rune window (fullscreen view, live views, comparing), positioned and clipped from Rune layout rects, with an API to show, move and hide it. No file I/O on the UI thread. |
| `proto-app` shell + canvas engine | proto-worker-4 | `main.rs`, app state, routing between spaces (main canvas = projects only, project → explorations), header (pitch-black per-group gradient fades over the canvas, logo button → main canvas, breadcrumb with centred ›, New menu, agent capacity), the canvas engine: camera (pan/zoom/pinch via Rune's world/live values), node layer, edges, selection/marquee, dragging, fitView with insets, glass minimap (`GLASS` look), keyboard. Owns `src/app/`, `src/shell/`, `src/canvas/`. |
| `proto-app` cards + fullscreen | proto-worker-5 | Design card (title label above, elevated surface, preview image from proto-web, action row: Vary, Compare, Explore, Promote, Spokes, Poll, Implement in brand blue; status states), project/portal card (path, preview, counts; the pod link stays in the inspector, as in the reference), fullscreen dialog with the morph transition and live WKWebView, Esc handling. Owns `src/cards/`, `src/fullscreen/`. |
| `proto-app` inspector + composer | proto-worker-6 | Floating glass inspector panel (`GLASS_PANEL`, 340px, insets), design details (title, made from, rationale), explorations/projects list grouped by project with path and pod link, the prompt composer (brand-blue send, agent picker, @pod mentions, #component mentions, focus/annotations), the ⋯ menu with Decisions. Owns `src/inspector/`, `src/composer/`. |
| `proto-app` features | proto-worker-8 | Spokes and lanes, review rounds, polls/decisions UI, multi-select bar + compare + combine, imports (bundles), agent tasks/implement, pod layouts and mentions, live views (slots). Owns `src/features/`. |

Visual reference: the reference app at http://localhost:3123 (dark theme) and its source. Brand blue `#1447e6`
with inset highlight `inset 4px 4px 17px rgba(255,255,255,.28)`; canvas background `#000` with dots `#4d4d4d`.

## Interfaces (agree before coding across crates)

- `proto-core` publishes the types and `Store` API first (types + `Store::open/get/put/subscribe`). Everyone else
  codes against it; worker-1 announces the API in `docs/api-core.md` and keeps it stable.
- `proto-web` publishes `docs/api-web.md`: `Snapshots::request(design_id, rev, html, viewport) -> Option<Pixels>`
  (poll-based, Latest-wins) and `LiveView::show(rect, html)/move_to(rect)/hide()`.
- `proto-ai` publishes `docs/api-ai.md`. `proto-server` publishes the route table in `docs/api-server.md`.
- `proto-app` modules talk through `src/app/state.rs` (owned by worker-4); others add their own state structs
  and register them with one line, following Rune's "a feature owns its state" rule.

## Milestones

1. **M1 (skeleton):** the workspace builds; the app opens a window with the black dotted canvas and header; the core
   loads an imported copy of the reference state; the server answers GET /api/state on :3124.
2. **M2 (read-only parity):** main canvas shows the 11 project cards; entering a project shows its explorations and
   design cards with real HTML previews; pan/zoom/minimap; inspector shows details; fullscreen with the live webview.
3. **M3 (editing):** create projects/explorations, vary/compare/promote/explore/spokes/polls, the composer runs
   generation (Claude API and agent-queue mode), workers can deliver to :3124.
4. **M4 (polish + parity check):** side-by-side checklist against the reference app, motion, accessibility, perf.

## Rules

- Rune's AGENTS.md rules apply (explicit state machines, no file I/O on the UI thread, bounded caches,
  files < 800 lines, `poll` returns true only on visible change).
- Never write to `apps/web/data` (read-only import).
  The lead commits at milestones; workers don't run git.
- Build/check: `PATH="$HOME/.cargo/bin:$PATH" cargo check -p <your crate>`; one cargo build at a time per crate,
  and avoid `cargo clean`. The shared `target/` is locked per build, so long builds queue.
- Headless checks: Rune apps render to PNG (`--snap`), so verify UI with snapshots, not by opening windows that steal
  focus. No visible windows unless the user asks.
- Report to the lead "prototypie-3a [9a4e6b]" via SendMessage: done/blocked + files + what was verified.
