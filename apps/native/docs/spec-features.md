# proto-app features: inventory of the reference `src/features/**`

Owner: proto-worker-8. Source: `apps/web/src/features/**` (~9.9k lines TS) plus the API routes
and store pieces they call. Each section gives **UI**, **state** (persisted vs transient), **API calls** and
**behaviour**, so the Rune port can be built without re-reading the TS. Sections are ordered by importance for
M2 (read-only parity) and M3 (editing); a priority table is at the end.

Conventions used below:
- "persisted" = a record type in `state.json` (proto-core Store); "local" = per-device UI state
  (the reference keeps it in zustand/localStorage); "transient" = in-memory only.
- `generate(opts)` = the core generation entry point (proto-ai), `GenerateOptions = {spaceId, parentId?, kind?,
  request: VariationRequest, origin?, label?}`. It creates placeholder cards (`status:"generating"`) and a `Job`
  `{id, label, spaceId, status: running|done|error, designIds[], startedAt, error?}`; jobs are transient, a
  persisted `runs/<id>` lets a restart reattach.
- Card size for layout: `NODE_W = 380`, `NODE_H = 420` (world units).
- Annotate notes: open notes pinned on a design (see §10) are passed as `request.annotations`.

---

## 1. Spokes, lanes and review (`features/spokes`) — M3 core, review queue count is M2

The hub-and-spoke workflow: break any design into components ("spokes"), explore each in its own parallel
generation ("lane"), review results in a keyboard-driven queue, run follow-up rounds, merge a refined spoke back.

### State
- persisted `reviewedIds/<designId> = true`: designs taken out of the review queue.
- persisted `lanes/<jobId> = LaneMeta {jobId, type: "clay"|"spoke"|"round"|"merge", name, focus?, hubId?, count,
  startedAt}`. Only the newest **60** lanes are kept (prune on insert, sorted by `startedAt` desc).
- persisted `spokeFocus/<designId> = string`: component description a spoke design explores (set when its lane
  finishes, for every resulting id).
- transient UI: `clayOpen`, `spokesFor: designId|null` (picker dialog), `reviewOpen`, `reviewStartId`.
- local drafts (survive restart, cleared on submit): `clay:brief`, `merge:<designId>`, `review:feedback`.

### Derived queries (put these in a `spokes::query` module; pure fns over the store)
- `queue_ids` = designs with `status == ready` and not in `reviewedIds`, sorted by `createdAt` asc.
- `siblings_of(id)`: the designs of the job that produced `id` (job order). Fallback when the job is gone (after
  restart): designs in the same space with the same `parentId`, `kind`, `instruction` and `|createdAt Δ| < 10s`,
  sorted by `position.x`, then `createdAt`.
- `lane_of(id)`: `lanes[job.id]` for the job containing `id`.
- `focus_of(id)`: walk `id` → parents while `kind == spoke` (max 50), first `spokeFocus[x]` or `lane_of(x).focus`.
- `hub_of(id)`: for a spoke, the nearest ancestor that is not a spoke; for anything else the nearest `page`
  ancestor (max 50 steps).
- `lane_progress(designIds, requested)` → `{total: len || requested, ready, failed}`.

### Layout helpers (needed for every lane launch)
- `GAP = 40`, `ROW_GAP = 120`, `row_width(n) = n*NODE_W + (n-1)*GAP`.
- Occupied rects = all designs in the space (NODE_W×NODE_H); a candidate hits if it overlaps any with 30px padding.
- `spoke_row_origins(hub, rows, count)`: for each row, try candidates k = 0..400: even k right of the hub
  (`hub.x + NODE_W + 220`), odd k left (`hub.x - 220 - row_width`), `y = hub.y + floor(k/2)*(NODE_H+ROW_GAP)`;
  first free wins and is added to the occupied list.
- `below_origins(parents, count)`: per parent, candidates `x = p.x + NODE_W/2 - w/2 + shift`,
  shift = `[0, 1, -1, 2, -2][k%5] * (w+80)`, `y = p.y + NODE_H + ROW_GAP + floor(k/5)*(NODE_H+ROW_GAP)`.

### `launch_lane(opts, info)`
Calls `generate(opts)`, registers `LaneMeta` with the new job id (`count = request.count`, `startedAt = now`). On
completion: `setSpokeFocus(ids, focus)` if `info.focus`; toasts: "`<name>` cancelled" (abort), "`<name>` failed"
(+ error), "`<name>`: every direction failed" (all ids error). Does not block the UI.

### UI
1. **Card action "Spokes" / "Merge"** (footer of every design card): non-spoke or hub-less spoke → *Spokes*
   (Boxes icon, opens the picker, disabled unless `ready`); spoke with a hub → *Merge* (title "Merge this spoke back
   into "<hub>"", disabled unless both ready).
2. **Break-into-spokes dialog** (max-w xl): title "Break into spokes", description "Pick the pieces of **<hub>** to
   iterate on in isolation. Each one runs as its own parallel lane."
   - On open: `POST /api/spokes {html, title}` → `{components: [{name, description, selector?}]}` or `{error}`.
     Cache per hub id for the session (reopen doesn't refetch). Loading: spinner "Looking for components…" + 4
     skeleton rows. Error: red box "Couldn't identify components", message, "You can still add components by hand
     below.", **Retry** button.
   - Checklist rows (name, description, monospace selector); first **3** AI components pre-checked. Custom rows are
     added from an input ("Add a component, e.g. Pricing card: the middle tier", Enter or **Add**): split on
     `:`/`–`/`—`/`-` → name (≤40 chars) + description; custom rows have an × remove.
   - Optional "Direction for every spoke" input. Footer: count picker "Per spoke" 1–6 (default 3) and primary
     "Explore N spokes · N×count designs".
   - Launch: origins via `spoke_row_origins`; per pick `launch_lane({spaceId: hub.spaceId, parentId: hub.id,
     kind: "spoke", origin, label: "Spoke · <name>", request: {baseHtml, baseTitle, instruction: direction ||
     "Explore <count> meaningfully different takes on the <name>.", count, focus: "<name>: <description> (in the
     HTML: <selector>)", scope: "component"}}, {type: "spoke", name, focus, hubId})`. Toast "Exploring N spokes in
     parallel" / "Results join the review queue as each one lands."
3. **New page ("block of clay") dialog** — opened from the header's "+ → Page" (guarded: not on the main canvas,
   `canCreateIn`). Textarea (draft `clay:brief`, voice button), ⌘↵ submits. Launch `kind: "page"`, `count: 1`,
   `scope: "page"`, label "Block of clay", instruction = brief + "\n\nThis is a rough first "block of clay": a
   complete, plausible full page with every section a real version would need. Favour breadth and structure over
   polish; the designer will refine it component by component." Toast "Shaping a block of clay…".
4. **Merge into hub** (`merge_into_hub(spokeId, note?)`): `launch_lane({spaceId: hub.spaceId, parentId: hub.id,
   kind: hub.kind, origin: below_origins([hub],1)[0], label: "Merge · <spoke>", request: {baseHtml: hub.html,
   baseTitle, count: 1, focus, scope: "page", instruction: "Integrate the refined component below into this design,
   replacing its existing version of it (<focus>). Keep its design, copy and interactions, but adapt spacing,
   widths and tokens so it sits naturally in the design. Leave the rest of the design as it is." + optional
   "Designer's note on this merge (takes priority, e.g. merge only part of it): <note>" + `<refined_component
   title="…">html</refined_component>`}}, {type: "merge", …})`. Toast "Merging "<spoke>" into <hub>".
5. **Inspector section** (in the inspector for the selected design):
   - non-spoke: box "Hub page"/"Hub" · "N spoke designs" (count of spokes with `parentId == design`), text "Break it
     into components and explore each one in its own parallel lane.", button "Break into spokes".
   - spoke: box "Spoke", focus text, merge note textarea ("Optional note, e.g. only merge the steps", ⌘↵) +
     "Merge back into "<hub>"".
   - **Lanes panel** "Exploration lanes" (`N running` / `all settled`): jobs sorted running-first then newest, max 30;
     only the 3 newest + all running shown, rest behind "Show N archived". Row: type icon (clay=wand, spoke=boxes,
     round=refresh, merge=merge), name (click = reveal: switch space, select first design, fit view on the lane's
     designs, maxZoom 1), status chip (`ready/total` with spinner while running, "planning" when total is 0;
     "cancelled"/"failed" red; done green, amber if some failed), cancel (■) while running else Review (inbox,
     opens review at the first ready design), progress bar (blue running / pulse when 0 ready, green done, red
     error; min 6% while running), error text (2 lines).
6. **Review dock** (header activity dock entry "Review", inbox icon): status "N waiting" / "Nothing waiting" +
   " · N generating" in blue; compact = count badge + pulsing blue dot when jobs run; highlighted when queue > 0.
   Panel "Review queue": generating line, first 5 queue rows (index, star, title, lane name or kind), "+N more",
   empty "Every ready design has been reviewed.", primary "Start reviewing".
7. **Review mode** (near-fullscreen dialog 96vw×94vh, no close X in the corner):
   - Header: "Review", "i of N to review" / "N to review" / "All caught up", " · N still designing";
     "Mark all reviewed" (toast with Undo), close.
   - Left (240px): "Queue" list (index, star, title, lane/kind; active row ringed, scrolled into view), then
     "Designing" with spinners for `generating` designs.
   - Centre: live interactive preview (viewport 1024 for spokes, else 1280); "Still designing…" / error message
     states; sibling strip below when the round has > 1 design: thumbs 144–160px wide 4:3 with number badge (click
     toggles selection, amber when selected), star/reviewed marks, current ringed primary.
   - Empty: "Inbox zero" + generating hint.
   - Right (340px): number badge + title, badges (kind, lane, "Reviewed"), rationale, "Component: <focus>",
     "Hub: <hub>". Buttons Star (S), Discard (X), Done (↵, primary). Feedback textarea (draft `review:feedback`,
     voice with hotkey M, hint "Try "3 and 6, more playful""), "Round on" number toggles, count picker "Each" 1–5
     (default 3), primary "Another round on #a, #b" (⌘↵), "Show on canvas" (switch space, select, close, fit view
     padding .4 maxZoom 1). Shortcut legend footer.
   - Keys (not while typing, no modifiers): J/↓ next, K/↑ previous in queue, →/← through siblings (wraps), S star,
     X discard (removes the design; toast with Undo re-adds the snapshot), Enter done (unless focus on a control),
     F or / focus feedback, 1–9 toggle sibling.
   - Selection parsing (`parse_selection(text, max)`): only a *leading* clause counts: optional fillers
     (ok/okay/so/alright/right/um/uh), optional `number(s)/no./#`, numbers as digits or words one..ten /
     first..tenth separated by `, & + and plus or` or spaces. `strip_selection` removes that clause and a "do/try/
     run/give me another/one more/a new/a fresh round (on these…)(with…)" phrase. Manual toggles override parsed
     picks; reset when the round (sibling set) changes.
   - **Another round**: targets = selected ready siblings, else the current design. Instruction = stripped feedback
     or "Push this direction further: keep what works and explore fresh takes on it.". Per target: `launch_lane`
     with `parentId: t.id, kind: t.kind, origin: below_origins, label "<lane minus 'Spoke · '> #n → round",
     request {baseHtml, baseTitle, instruction, count, focus: focus_of(t), scope: component for spokes,
     annotations}`, lane `{type: "round", hubId: hub_of(t)}`. Then mark current + targets reviewed, advance, clear
     feedback.
   - Moving on after done/discard: next in queue, else previous.
8. **Voice dictation** (`VoiceButton`): Web Speech API (`lang = navigator.language`), interim results appended to
   the text present when dictation started; optional hotkey. Rune port: macOS `SFSpeechRecognizer` behind a
   `VoiceInput` trait, or defer (M4) — low priority.

---

## 2. Polls and decisions (`features/poll`, `app/poll/[id]`, `api/polls`, `api/decisions`) — M3

Package competing directions into 1–8 questions, publish HTML snapshots as a stakeholder poll, collect votes over
the LAN, mark winners (stars the design on the canvas).

### State
- local (per device, never synced): `pollCandidates: [designId]` (tray, insertion order), `pollTitle`,
  `pollDecisions: [DraftDecision {id (nanoid 8), question, why, options: [{designId, label}]}]`, draft `poll:context`.
- persisted **sharedList** `publishedPolls/<pollId> = PublishedPoll {id, title, createdAt, shareUrl, decisions:
  [{id, question, options: [{id, designId, label, title}]}]}` (no HTML), listed newest first.
- persisted `pollWinners/<pollId>/<decisionId> = optionId` (key is the literal `"<pollId>/<decisionId>"`).
- server files `data/polls/<id>.json` = `Poll {id, title, intro, createdAt, decisions: [{id, question, why,
  options: [{id, designId, label, title, rationale, html}]}], votes: [Vote {id, voterId, voter, decisionId,
  optionId, comment?, createdAt, updatedAt}]}`. Poll id regex `^[A-Za-z0-9_-]{6,32}$`; writes atomic tmp+rename,
  read-modify-write serialised per poll id (vote races).
- transient UI: panel `open`, `tab: build|results`, `resultsPollId`, `dockOpen`.

> ⚠ PLAN.md's record-type list misses `notes`, `publishedPolls`, `pollWinners`. Full shared key list from
> `store/key-policy.ts`: spaces, designs, runs, portals, promotedFrom, seededFrom, notes, reviewedIds, lanes,
> spokeFocus, publishedPolls (sharedList), pollWinners. Local-only: activeSpaceId, selectedId, drafts,
> promptHistory, agentChoice, pollCandidates, pollTitle, pollDecisions, mixDrafts.

### API (proto-server; same shapes)
- `POST /api/decisions {designs: [{id, title, rationale, html}] (2..16), context?}` → `{decisions: [{question, why,
  options: [{designId, label}]}]}`. LLM call (proto-ai) with a "design lead preparing a decision review" system
  prompt: surface 1–4 core decisions, only candidate ids, ordered by consequence; server strips scripts/data URIs,
  dedupes options, drops decisions with < 2 options.
- `POST /api/polls CreatePoll {title (1..200), intro, decisions (1..8): [{id, question, why, options (2..4):
  [{id, designId, label (1..120), title, rationale, html (≤2MB)}]}]}` → `201 {id (nanoid 10), path: "/poll/<id>",
  lanOrigins: ["http://<ipv4>:<port>"]}` (non-internal IPv4 interfaces).
- `GET /api/polls/<id>` → full Poll (no-store), 404 `{error: "Poll not found."}`.
- `GET /api/polls/<id>/votes` → `{votes, tallies: [{decisionId, total, options: [{optionId, count, voters:
  [{name, comment?}]}], leaders: [optionId]}]}`.
- `POST /api/polls/<id>/votes {voterId (4..64), voter (1..80), decisionId, optionId, comment?}` → same as GET.
  One vote per voter per decision (re-vote replaces); renames propagate to all that voter's votes; 400 "Unknown
  decision."/"Unknown option.".
- Errors are `{error: "<first issue as 'path: message'>"}`.

### Sharing in the Rune app (agreed with worker-2)
The full API stays on loopback `127.0.0.1:3124`. A separate stakeholder listener on the LAN IP (default port 3125)
is **off until a poll is shared**, and serves only `/poll/:id` + assets, `GET /api/polls/:id`, and
`GET`/`POST /api/polls/:id/votes` (votes rate-limited per IP). `lanOrigins` from `POST /api/polls` stays empty until
sharing is on. Publish flow in the app: if sharing is off, call `ServerHandle::share_polls(3125) -> Vec<origin>`
(or `POST /api/polls/share` on loopback), then publish; `shareUrl = lanOrigins[0] + path`. When sharing fails or no
LAN interface exists, publish anyway and show the loopback URL with the hint that colleagues can't reach it.
`/api/decisions` → proto-ai `Ai::decisions`.
Built (worker-2): `GET|POST|DELETE /api/polls/share` (POST `{port?}`) → `{sharing, port, lanOrigins}`; no LAN IPv4 → 503
`{error: "Couldn't share on the LAN: …"}` (= "nur lokal"). In-process: `handle.share_polls(3125)`, `stop_sharing()`,
`sharing() -> watch::Receiver<ShareState>`. Page `/poll/<id>` on both listeners; voter key `prototypie:voter`;
LAN rate limits: votes 30/min/IP (burst 10), reads 120/min. See api-server.md §11, §14.

### UI (designer side)
1. **Card action "Poll" / "In poll"** (footer): toggles tray membership (green check when in); disabled unless ready
   (removing always allowed). Removing a candidate also removes it from every draft decision.
2. **More menu item "Decisions"** with status `"N in poll · M shared"` / `"Share a poll"` → opens the
   **Decisions popover** (dock panel anchored to the More button): head "Share a poll · N candidates"; latest
   published poll (title, decision count, share link row, "See results"); up to 4 candidate titles, "+N more";
   empty hint "Add two or more designs with "Poll" on a card, then build the poll."; primary "Build poll" /
   "Build a new poll".
3. **Decisions panel** (dialog ~96vw × 90vh, max 6xl): title "Decisions" + description "Don't share the messy file.
   Package the competing directions into a few clear questions and let stakeholders vote on live prototypes.";
   tabs **Build poll** / **Results (count badge)**.
   - Build, left **Poll tray** (288px): candidate thumbs 4:3 with title + ×; context textarea "Optional: what are you
     torn about?"; button "Let the agent find the decisions" (needs ≥ 2 ready; spinner "Finding decisions…") →
     `/api/decisions`, replaces drafts (toast "The agent found N decisions" with Undo restoring previous drafts),
     sets title to "Which direction should we take?" when empty; error box "Couldn't reach the agent".
   - Build, main: "Poll title" input; decision editors (numbered circle, borderless question input + "why" input,
     move up/down, delete); option grid (2 cols, 4 on wide) with A/B/C/D letter, label input, design title, move
     left/right, remove; "Add option" dropdown "From the poll tray" (max 4 options, only unused candidates);
     dashed "Add a decision manually"; empty state "No decisions yet".
   - Publish bar: validation message (first failing: "Add at least one decision." / "Decision N needs a question." /
     "…needs at least two options." / "…has an option that isn't ready.") or "N decisions · M candidates ·
     snapshots the current HTML"; **Publish poll** → builds CreatePoll (title default "Which direction should we
     take?", label default design title, option ids nanoid 8, html snapshot) → adds PublishedPoll with `shareUrl`
     = LAN origin + path when running on localhost; green "Published." banner with share link + "See results".
   - Share link row: read-only mono input (select on focus), Copy (→ "Copied" 1.5s), open-in-browser icon.
   - Results: left "Published" list (title, date+time, decision count); detail: title, "N people have voted ·
     updated <time>", Refresh, "Forget this poll" (removes from list only; toast "The shared link keeps working."),
     share link; per decision: question, total votes, option cards (preview of the live design or its title, letter,
     label, "Leading" crown badge for leaders, bar + "n votes · p%", voter names + comments, **Mark winner** /
     "Winner"). Mark winner: sets `pollWinners`, stars the design (toast "Winner starred on the canvas" or "Winner
     marked … no longer on the canvas"); switching/unmarking unstars the previous winner unless it wins another
     decision.

### Stakeholder page `/poll/<id>` (web, served by proto-server — not native UI)
Server-rendered page + client player: intro (title, intro, name input "So the designer knows who voted", voter
`{id, name}` in localStorage), one decision per step (header with progress dots + "Voting as <name>", "Decision i
of n", question, why, Desktop/Mobile preview toggle, live option iframes A–D with select + expand, sticky footer
comment "Why this one? (optional)" + submit), done page "Thank you, <name>!" with current tallies, review/refresh.
404 page "This poll doesn't exist". Recommendation: port as one static HTML/JS bundle embedded in proto-server
(no Next); the native app only needs the designer side.

---

## 3. Multi-select bar, combine, drag-to-mix (`features/multi`) — selection bar M3, View/Live M2-ish

### State
- local `mixDrafts/<id> = MixDraft {id (nanoid 8), spaceId, position, sourceIds (unique), createdAt, text?, count?
  (1–4)}`: draft cards on the canvas, per device.
- local drafts: `selection:<spaceId>` (message), `selection-opts:<spaceId>` = JSON `{revisions, mode:
  "each"|"combine"}` (empty string = default `{1, each}`).
- shared prefs: `agentChoice: "claude"|"codex"` (local; inspector default "claude"), `promptHistory` (local).
- Agents: `claude` = "Claude" / "Queued for Claude Code via the watcher"; `codex` = "Codex" / "Codex CLI
  generates the designs directly".

### `generate_from_sources({sourceIds, instruction, count, origin?}) -> bool`
Ready sources only (false if none). `count = max(1, round(count))`. Default origin: row of width
`count*NODE_W + (count-1)*SIBLING_GAP` (SIBLING_GAP = 160) centred on the mean source x (+NODE_W/2), at
`max(source y) + NODE_H + 220`.
- 1 source → `generate({spaceId, parentId: src, kind: "variation", origin, label: "<text[..40] or 'Variation'>:
  <title>", request: {instruction: text || "Explore a meaningfully different direction.", count, baseHtml,
  baseTitle, agent}})`.
- ≥ 2 → one request: `parentId: first`, `label: "Mix: A + B + …"[..60]`, `request: {instruction: text || "Combine
  them into one design.", count, bases: [{id, title, html}], baseTitle: "A + B", agent}`.
- Canvas edges: a combined design (≥ 2 `request.bases`) gets extra edges from every base other than `parentId`
  (animated while generating). Proto-app canvas (worker-4) must ask features for extra edges.

### Selection bar ("pill that expands") — shown when ≥ 2 design nodes are selected
- Placement: bottom-centre of the canvas area left of the inspector (inspector overlay 372px → shift −186px),
  width min(560, avail − 2rem), 24px from the bottom. Rounded 22px pill (44px tall) at rest, 14px radius expanded.
- Pill row: chip "[n] selected" with × (clear selection); **View** (if ≥ 2 ready: opens fullscreen on the ready
  selected designs in canvas order, ←/→ steps; canvas order = rows by y within 160px, then x); **Live** (collapsed
  only, ≥ 2 ready: `create_live_view(ids)`, §8); one-line input ("Revise all n: what should change?" / "Combine all
  n: steer it (optional)") — focus/click/typing expands; agent picker icon; round brand send button.
- Expanded (grows upward, 300ms `cubic-bezier(.2,.8,.2,1)`): segmented **Revise each** / **Combine**; label "How
  should all n change?" / "Combine all n: what should the result be?" + recent prompts menu (history, 10); textarea
  (3 rows; Enter sends, Shift+Enter newline, IME-safe); slider "Revisions per design" 1–3 / "Combinations" 1–4;
  full-width split button: "Revise n designs ×k" / "Combine n into k designs" + agent picker ("Who works on it").
  Esc collapses (keeps text, refocuses pill input without re-expanding). New selection < 2 collapses.
- Send: Revise each needs text; per ready selected design `generate({parentId: d, kind: variation, label:
  "<text[..40]>: <title>", request: {instruction, count, baseHtml, baseTitle, agent}})`. Combine needs ≥ 2 ready →
  `generate_from_sources`. Clears message and collapses.
- **Alt/Option+D** deletes all selected designs (else the inspector's selected one), not while typing.

### Drag-to-mix draft cards (canvas extra nodes, id prefix `mix:`)
- Every design card has a connection handle. Drag from it and drop:
  - on empty canvas → new draft at the drop point (`x - 170, y - 16`) with that source;
  - on another design (handle or body) → draft between them (`x = mid(xa, xb) + NODE_W/2 - 170`,
    `y = max(ya, yb) + NODE_H + 120`) with both;
  - on a draft (handle or body) → add the source (no duplicates, designs only).
- Draft node (340px, dashed ring border, shadow): icon + "New card" / "Mix of n", × discard (Esc); source chips
  (removable when > 1), hint "Connect more cards to mix them"; textarea (autofocus; "What should it become? e.g. a
  darker, denser version…" / "Optional: steer the mix, …"; Enter sends); count 1–4 segmented; brand button "Generate
  k variations" / "Mix n into k designs" with agent icon. Send = `generate_from_sources(origin: draft.position)`
  then close. Animated dashed edges source → draft. Dragging a draft moves it (`moveMix`).

## 4. Compare (`features/compare`) — M2 (read-only)
- Card action **Compare (n)** shown only when the design has n > 0 ready children (`parentId == id`).
- Full-window dialog "<title>: side by side": horizontal scroll row of columns: original first ("Original · title"),
  then ready children oldest first. Column width fixed (no post-load fitting): phone designs (`viewport < 600`)
  400px; ≤ 2 designs → half width (min 480); else 560px. Column header: title, star toggle, open fullscreen (closes
  compare). Body: live interactive webview at the design's viewport (default 1280), no frame.
- Rune: N live WKWebViews at once (proto-web `LiveView` must support several instances) or snapshots + one live
  view on hover/focus; recommend several live views (max ~6) since it's the point of the dialog.

---

## 5. Agent tasks, capacity and Implement (`features/agent-tasks`, `api/agent-tasks`, `api/implement`) — M2 read, M3 send

UI copy here is **German** in the reference (keep it). Header pill overlaps worker-4's header: worker-4 owns the slot,
`features::agent_tasks` provides the pill widget + panel.

### Data (server-computed, polled; nothing persisted client-side except `implementRequestedAt` on designs)
`GET /api/agent-tasks` → `{capacity: {working: n|null, total: n|null, agents: [{name, status}]}, tasks: [AgentTask],
notesService: bool}` (`notesService`: whether the server could read the notes service).
- capacity: proto-* blocks in the Rex session (`rex block ls -s prototypie --json`; a block is "working" when its
  `rex capture` tail shows "esc to interrupt"). null when Rex is unreachable.
- `AgentTask {id (note id or queue key), kind: queue|note|implement, designId?, summary?, title, request,
  jobId?, index?, baseTitle?, status: waiting|working|delivered|resolved|cancelled, createdAt, updatedAt (ISO),
  steps: [{key: received|announced|working|delivered|resolved|cancelled, label, at: ISO|null, done}], target?:
  {spaceId, spaceName, spacePath[], designId?}, noteId?, noteStatus?, element?, thread: [{id, role, content, at}]}`.
  Queue steps: Eingang, Notiz, In Arbeit, Geliefert, Abgeschlossen (+ Abgebrochen). Note steps:
  Gesendet/Notiz, In Arbeit, Abgeschlossen (+ Verworfen).
- `POST /api/agent-tasks/<noteId>/notes {text}` → `{thread}`: appends a `role: "human"` message to the note's thread in the notes service.
- Polling: one shared poller (Rune: one background task publishing into a feature state), every 4s while the panel is
  open, 8s closed, 30s when the window is hidden/occluded; reload immediately on open / after a send. Dedupe in-flight.

### Header capacity pill (`AgentCapacityButton`)
Rounded-full bordered button, h 32: Claude "minion" icon (animated while working > 0); `working/total` (narrow) or up
to 12 square slots (filled `#D97757` for working, "+N" overflow); dark count badge of open tasks (waiting+working).
Unknown: "…" loading, "–" failed. aria label "Agenten: w von t arbeiten · n offene Aufgabe(n)".
Click toggles the **panel** (anchored below-right, max-h min(78vh, 720)): head "w von t Agenten arbeiten" / "Status
nicht erreichbar" / "Lädt …", close ×, agent chips (orange when working/blocked), warning "Der Notizdienst ist nicht
erreichbar: Status und Verlauf der Aufgaben fehlen." Task list grouped in order working (In Arbeit, amber), waiting
(Wartet, sky), delivered (Geliefert, green), resolved (Abgeschlossen, grey, max 8), cancelled (Abgebrochen, red,
max 3); empty "Keine Aufgaben.". Row: status dot (minion when working), title, "<jobId#index | Implement | Notiz> ·
<status> · <time>" (+ " · n im Verlauf"); time format "14:05" today else "31.10. 14:05". Expanded detail: request
text, step chain chips (done solid, open dashed, tooltip with time), target space path + **Zur Karte** (enter space
and select the design), thread (Du / Agent / role + time + content), note composer ("Notiz an den Agenten …",
⌘↵, draft `note:<noteId>`, toast "Notiz angehängt", error "Bitte einen Text eingeben.") or "Ohne Notiz im Notizdienst
kann hier keine Notiz angehängt werden." Esc / outside click closes; `open_task_panel(taskId)` expands and scrolls
to a task.

### Inspector "Beim Agenten" (`AgentProgress`)
For the selected design, tasks whose `designId` is the design or a promoted link (`linkedDesignIds(promotedFrom,
id)`: source + copies), max 4: kind label ("Implement" / "Job <jobId>#<index>" / "Notiz") + current step + time;
step dots (announced hidden; current orange while working); summary (minus "Resolved: ", "Mehr/Weniger" over 160
chars); "Im Aufgabenpanel öffnen".

### Implement button (card action, brand blue when idle) — shown on promoted designs
- Phase from the newest non-cancelled implement task of the linked ids (open wins over resolved): idle
  "Implement" (code icon) → sent "Gesendet" → working "In Arbeit" (minion) → done "Erledigt" (check + chevron menu
  "Erneut senden …" → confirm dialog "Erneut an den Agenten senden?" / "Der Agent hat „title“ schon umgesetzt. Ein
  neuer Auftrag lässt ihn das Design noch einmal einbauen." Abbrechen / Erneut senden → force).
  Without notes service data, `design.implementRequestedAt` counts as sent.
- Click when not idle opens the task panel on that task. Tooltips per phase (see reference).
- `POST /api/implement {designId, title, rationale, html, force?}`: writes `data/implement/<id>.html`, files
  the note "[prototypie implement <id>] Implement the design "<title>" in the product (this codebase). Why:
  … The design's HTML is in <file>." Dedupe over linked ids: open note → `{ok, duplicate: true}`; already sent
  (`implementRequestedAt` on any linked design) without force → 409 `{done: true}`. Client sets
  `implementRequestedAt = now` on success unless duplicate, then reloads tasks.

## 6. Imports (`features/imports`, `api/bundles`) — M3

### Import bundle (More menu, section "Import bundle")
- On menu open: `GET /api/bundles` → `[{file: "/imports/<f>.json", name, count, updatedAt}]` newest first (JSON files
  in `public/imports/`; Rune: `<data dir>/imports/`). States "Loading…", "No bundles yet", rows (folder icon, name,
  count).
- `import_bundle(file)`: bundle `{name, designs: [{key?, parentKey?, group, title, rationale, html, viewport?,
  source?, instruction?, kind?, starred?, position?}]}`. On the main canvas → `createProject({name})`, else
  `createExploration({parentSpaceId: active, name})`. Designs grouped by `group` (first-seen order): row = group
  index, col = index in group; position default `{col*(NODE_W+160), row*620}`; `kind` default page; status ready;
  instruction default "Imported from <source>". Second pass sets `parentId` from `parentKey → key`. Enter the new
  space; toast "Imported N designs from <name>". The menu trigger shows a spinner while busy; errors toast.

### Import from app ("+ → Import from app…" dialog, max-w md)
- Title "Import from app", description "Recreate an element that already exists in the app, as a card to improve
  on." Fields: **Element** (autofocus, "e.g. Toolbar", ≤200), **Source** (mono, "Repo path or URL", ≤2000,
  prefilled once per open from `GET /api/projects` → project whose name or id matches the top-level space name
  (case-insensitive): `localPath ?? repo`; not overwritten after the user types), optional **Screenshot** (PNG/JPEG/
  WebP/GIF ≤ 8 MB, as data URL; chip with thumbnail + remove). Rune: NSOpenPanel + paste/drag image.
- Requires agent `claude` (else hint "Needs the Claude agent: switch the inspector's agent to Claude.") and both
  fields; `canCreateIn(space)` guard.
- Submit: `generate({spaceId, kind: "variation", label: "Import · <element>", request: {instruction: "", count: 1,
  focus: element, scope: "component", agent, importFrom: {element, source, screenshot?}}})`; toast "Importing
  <element>…" / "The agent recreates it from the app's source."; on error toast "<label> failed". Clears element
  and screenshot (keeps source).

---

## 7. Pod data: @-mentions, #-components, Layout-Finder (`features/pod-mention`, `features/pod-layouts`) — M3

Shared seam with worker-6 (composer): `features::mention` provides the **mention state machine, listbox, chips and
the "flows into generation" panel** as widgets; the composer embeds them and sends their output
(`podContext`, `components`) in the `VariationRequest`.

### Catalog
`GET /api/pod-catalog` → `PodCatalog {version, pods: [{id, label, display, note?}], entries: [PodCatalogEntry {id,
kind: pod|container|shape|resource, pod, label, path, access: live|401|ref|cache, stand?, types?, fields: [{p, v?,
type?}], relations?: [{p, target}], children?: [id], note?, provenance, probe?}], checkedAt}`. Fetched once per app
run (retry on next open after failure); cache in feature state.
- `source_of(entry)`: shapes `"<pod display> · <path>"`, else pod display + "/" + path (no double slash).
- Group order resource, container, shape, pod; labels "Resources", "Containers", "Shapes · types", "Pods".
  Access badges: live (green), 401 (amber), "reference"/"cache" (muted). Kind tones: resource sky, container amber,
  shape emerald, pod violet; icons file / folder / shapes / server.
- `match_entries(query)`: case-insensitive substring of label, path or group label; empty query = all.
- `to_pod_context(ids, off)` → `[{id, kind, label, source, access, stand?, fields (minus toggled-off
  "<entryId>:<fieldIndex>"), relations?, note?}]`.

### Mention state machine (shared by `@` pod data and `#` components)
- Opens when the text before the caret matches `(^|\s)@([^\s@]*)$` (resp. `#`); the captured query filters.
  Active index resets when the query changes. ↑/↓ cycle, Enter/Tab pick, Esc closes (each consumes the key so the
  composer doesn't send). Picking removes "@query" from the text, puts the caret at its start, appends the id to
  `mentions` (no dupes), refocuses the text. Backspace at caret 0 with no selection removes the last chip.
- Trigger buttons ("Mention pod data · @", "Mention a component · #") insert the trigger at the caret (with a leading
  space if needed) and open the list; must not steal focus from the text field.
- Listbox: grouped, rows `[icon | label + source | access badge]`, used rows at 55% opacity, active row
  highlighted, scrolled into view.
- Chips above the text with × remove.
- **PodContextPanel** "Flows into generation · F fields · R resources · S shapes": per mentioned entry: label,
  access badge, source · stand, field rows with on/off toggles (`p`, value/type), child chips, note. Off-state map is
  per composer draft.
- **PodProvenance** strip on result cards with `request.podContext`: "N fields from the pod" + entry chips (tooltip
  source · access · stand). (Card owned by worker-5, strip provided here.)

### `#` components
- Options: ready cards with html in the current space and its ancestors up to (excluding) the main canvas; on the
  main canvas just it. Sort: components first (`component` flag, `request.scope == component`, or `kind == spoke`),
  then nearer spaces, then title. Cards from parent spaces carry `from: <space name>`. Groups "Components" / "Other
  cards". Chips in fuchsia; chips of deleted cards drop out automatically.
- Request: `components: [{id, title, html (current)}]`; stored requests keep id/title only.

### Layout-Finder ("+ → Layout finden…", German UI) — M3/M4
- Builds a `LayoutModel` from the catalog: one group per shape (members = its resource children; duplicate labels
  get " · <pod label>"), plus a group per unclaimed resource; group name from the container (`tasks/`→Aufgaben,
  `events/`→Termine, `courses/`→Kurse, `profile/`→Profile) else the label; groups with items first; colours cycle
  `#d97757 #2563eb #7c3aed #0d9488 #ca8a04 #db2777 #525252`. Items: label + start/end parsed from
  `ical:due|start|dtstart|schema:startDate` / `end|dtend|schema:endDate` (format "YYYY-MM-DDTHH:MM[:SS] (Zone)" as
  wall time in that zone, or ISO). Relations: direct edges per (group, predicate, group) skipping "←" reverse edges,
  access = weakest (401 < ref < cache < live); plus implied edges when two groups point at the same target
  (`"p1 ↔ p2"`, `via`).
- Dialog "Layout finden": two shape chip radiogroups (A, B; empty shapes and the other side disabled; arrow keys
  rove), relation picker when several, four mini previews (Matrix, Timeline mit Bändern, Swimlanes, Karten mit
  Verweisen) each disabled with a reason ("Keine belegte Beziehung", "braucht Zeit · ical:due / Termin", "Keine
  belegten Einträge"); pairs without a relation show partners that have one; name input (default "A × B · Layout");
  target toggle **Neuer Space** (default) / **Karte hier**; **Übernehmen**.
- Apply: card `{kind: page, title, rationale: German provenance sentence (catalog Stand, sources, access, relation,
  Beleg, "Nur belegte Einträge und Kanten; nichts ergänzt."), html: layout_card_html(...), status: ready,
  instruction: "Layout-Finder: A × B · predicate · layout"}`. Neuer Space → `createExploration` + card at 0,0, enter
  it; Karte hier → right of the right-most card (`x + NODE_W + 80`). Toasts "Space „name“ angelegt" / "Karte „name“
  angelegt".
- `scene_html(layout, a, b, rel, w, h, k)` and `layout_card_html` are pure string builders (port 1:1 to Rust, with
  `SCENE_CSS`); the card html is 1120×520 stage at k = 2.6. Mini previews: render the same scene html via proto-web
  snapshots (small viewport) rather than re-implementing natively.

---

## 8. Live views / slots (`features/live-view`) — M2 (rendering), M3 (create, Main toggle)

A live view is a card whose html places other cards by reference:
`<prototypie-slot data-design="<id>" class style data-fit="fill"? data-height="N"?></prototypie-slot>`.
**M2 needs `compose_live`** wherever a design's html is rendered (card snapshot, fullscreen, compare, review), or live
views show empty. Proposed seam: proto-app calls `features::live_view::render_html(design, &store) -> Cow<str>` before
handing html to proto-web; snapshot cache key must include the composed parts (see `compose_live_cached`).

**Implemented in proto-core as `proto_core::live_view`** (worker-1; names in docs/api-core.md): `has_slots`,
`slot_ids`, `slot_lineage(&Snapshot)` (memoise by snapshot version), `Snapshot::main_variant(id)`,
`compose_live(html, &Snapshot, wrap)`, `ComposeCache::new(200).compose(card_id, html, &Snapshot, wrap) -> Arc<str>` +
`.fingerprint(..) -> u64` (proto-web snapshot key), `default_live_html(ids, viewport)`, `SIZER`. Features only adds
`create_live_view` and the Main toggle. Reference semantics below for review:
- `has_slots(html)`: `/<prototypie-slot\b/i`. `slot_ids(html)`: regex `<prototypie-slot\b([^>]*)>(?:\s*</prototypie-slot>)?`
  (case-insensitive), id from `\bdata-design\s*=\s*["']?([\w-]+)`.
- `main_variant(designs, id)`: DFS over `id` and its descendants (children index by `parentId`), **skipping** any
  descendant whose html has slots (and its branch); pick the ready design with the largest `mainAt`; else `id`.
- `slot_lineage(designs)`: every slot target plus its descendants (stopping at live-view descendants that are not
  targets) — decides where the **Main** toggle appears. Memoise per store version.
- `compose_live(html, depth=0)`: replace each slot with `<div class="pt-slot <class>" <other attrs>><iframe
  title="<title>" srcdoc="<escaped html>" [style="height:Npx"]></iframe></div>` (empty div when the target isn't
  ready); recurse into the component's html up to depth 3; escape `& " < >` in attributes; optional `wrap` hook per
  component (thumbnails freeze animations); inject the SIZER `<style>`+`<script>` before the last `</body>` (else
  append): iframes auto-grow to content height via ResizeObserver unless `data-fit=fill` (100% height) or
  `data-height`. Copy the SIZER string verbatim from the reference.
- `compose_live_cached(key, …)`: memo keyed by card id; reuse when the list `[html, part1, part2 (recursive)…]` is
  unchanged; LRU 200 entries.
- `default_live_html(ids, viewport)`: `<!doctype html>…<meta name="viewport" content="width=V">` + white body, `main`
  flex column with one slot per id.
- **create_live_view(ids)** (selection bar "Live"): ready designs only; new design `{spaceId: first.spaceId,
  parentId: first.id, kind: page, title: "Live view", rationale: "Shows the current main variant of: A, B.", html:
  default_live_html, status: ready, position: {mean x, max y + NODE_H + 220}, viewport: max viewport (default 1280),
  request: {instruction: "Live view", count: 1, bases: [{id, title, html: ""}], baseTitle: "A + B"}}` (bases make
  the canvas draw edges from every component, §3).
- **Main toggle** (card action, crown): visible when the card is in the slot lineage or already has `mainAt`;
  toggles `mainAt = now | unset`; tooltip "Live views show this variant. Click to unset." / "Show this variant in
  live views"; disabled unless ready.

---

## 9. In-place AI edit (`features/ai-edit`, `lib/ai-edit.ts`, `api/edit`) — M3 (busy/undo badges M2)

### State (on the design record, synced)
`editing?: {jobId, instruction, agent: codex|claude, startedAt}` while running; `editError?: string`; `versions?:
[{html, title, rationale, viewport?, viewportHeight?, replacedAt, instruction?}]` newest last, max 5. The **server**
writes the result into the card record (same id/position/edges) and pushes the previous content into `versions`.

### API
- `POST /api/edit EditRequest {designId, instruction, focus?, annotations?, agent?, podContext?, components?}` →
  `{jobId}`; 409 while already editing. Client error text = `error` or "Edit failed (<status>)".
- `DELETE /api/edit/<designId>` cancels.
- Claude agent: becomes a `mode: "edit"` queue item (scripts/agent-queue.md "Edit items").

### Behaviour / UI
- `start_ai_edit(design, instruction, {agent, podContext, components})`: open Annotate notes of the design become
  `annotations` + `focus = notes_focus(...)` ("the annotated element(s) A; B"); `rememberPrompt(instruction)`.
- `undo_ai_edit(id)`: when not editing, restore the last version's html/title/rationale/viewport/viewportHeight, pop
  it, clear `editError`.
- Card controls (`AiEditActions`): while editing a badge "Bearbeite… <instruction>" with spinner + × cancel;
  otherwise wand icon "Edit with AI (changes this card in place)" → focuses the composer in edit mode for this card
  (`focusPrompt(id, "edit")`, worker-6), undo icon when versions exist (tooltip "Undo AI edit: <instruction[..80]>"),
  "Edit failed" outline badge (tooltip error, click dismisses).
- Fullscreen side panel box **"Änderung anfragen"** (`EditRequestBox`, worker-5 places it, mounted per design id):
  textarea "z. B. Uhr größer, Softkeys höher setzen" (Enter sends, Shift+Enter newline, disabled while running) +
  send icon; running line "Bearbeite…" + cancel ("Abbrechen"); error line with dismiss; "done" line once an edit sent
  from here finished without error; "Rückgängig: <instruction>" button when a version exists. Preview refreshes in
  place when the record changes.

## 10. Annotate notes on designs (`features/annotate`, `features/notes`) — M3, needs a design decision

### State
persisted `notes/<designId> = [DesignNote {id (annotation id), designId, comment, element, elementPath,
selectedText?, timestamp, resolved?}]` sorted by timestamp.
- `open_notes_for(id)` → `[{element: "<element> (at `<path>`)", comment: "<comment> (re: "<selected>")"}]`
  (unresolved only); used as `request.annotations` by review rounds, apply-notes, AI edit.
- `notes_focus(annotations)` → "the annotated element(s) …" or none.

### Behaviour / UI
- In the web app a third-party notes toolbar (a JS library with its own external notes server) runs over
  the whole page; a note made inside a design iframe (`id="design-<designId>"`, innermost wins) is mirrored into
  `notes` (add/update upsert, delete/clear remove). Toolbar sits right above the minimap, left of the inspector.
- **NotesBadge** on cards: count of open notes.
- **NotesPanel** (inspector + fullscreen): list "element / comment" with × dismiss (removes the note), button **"Apply
  notes as a new round"** → `generate({parentId: design, label: "Apply n note(s): <title>", request: {instruction:
  "Redesign ONLY the annotated elements to address their pinned notes. Leave everything else in the design exactly
  as it is.", count: 2, focus, scope: "page", baseHtml, baseTitle, annotations}})`, then mark those notes resolved.
  Empty (non-compact): a line telling the user to open the design and pin notes on its elements with the notes
  toolbar.
- **Rune decision needed:** there is no page-wide DOM. Options: (a) inject the web app's toolbar script into the live
  WKWebView (fullscreen/review) and bridge its events through a `WKScriptMessageHandler` (proto-web), mapping
  to the shown design id; (b) native "pin a note" mode: click in the live view → JS hit-test returns element name +
  path + selected text → file the note in Prototypie's notes service ourselves. Decided: (b), with Prototypie's
  own notes service (spec in the clean-room `spec/`). Recommend (b) for M3 (no third-party UI in a native
  app), (a) as fallback. Either way proto-web must expose an eval-JS + message channel.

## 11. Projects, explorations, promote (`features/nested`, creation + promote part) — M3

Assignment (lead): navigation/space switching/URL state → worker-4; nested inspector → worker-6; **creation flows and
promote → worker-8**. The pure helpers below are shared; proposal: they live in `proto-core` queries or
`features::nested::model`, and worker-4 calls `enter_space` from the shell.

### State
- persisted `portals/<spaceId> = {x, y}`: portal card position of a nested space on its parent canvas
  (`PORTAL_W = 300`, `PORTAL_H = 330`).
- persisted `promotedFrom/<copyId> = {spaceId, designId, spaceName, at}` (`spaceName` captured so the "from" badge
  survives deleting the source space).
- persisted `seededFrom/<seedCopyId> = <originalDesignId>`.
- `spaces/<id> = {id, name, parentSpaceId?, createdAt, localPath?, kind?: "project"}`; root = `ROOT_SPACE_ID` (main
  canvas, only projects).
- design `guidelinesAt?` (at most one per space; setting one clears the others in that space).

### Placement helpers
- occupied(space) = designs (NODE_W×NODE_H) + portals of child spaces (PORTAL_W×PORTAL_H).
- default_spot(space) = `{max(r.x + r.w) + 120, min(r.y)}` over occupied, or `{0, 0}` when empty.
- find_free_spot(rects, start, w, h, mode), gap 40: `right` tries `start.x + i*(w+gap)` for i < 40; `grid` tries 4
  columns × 20 rows from start; falls back to start.

### Actions
- **create_exploration({parentSpaceId, name?, seedDesignId?}) → {spaceId, seedId?}**: name = given name, else the
  seed's title, else "Exploration <n+1>" (n = existing child spaces). Portal at find_free_spot(right) starting right
  of the seed (`seed.x + NODE_W + 80, seed.y`) or at default_spot. With a seed: copy the design (new id/createdAt)
  into the new space at {0,0}, `parentId` unset, `starred` false, and record `seededFrom[copy] = seed`.
- **create_project({name, localPath?}) → {spaceId, created}**: when a project with the same `localPath` (case-
  insensitive) exists, return it (`created: false`). Otherwise create_exploration under the root, then set
  `kind: "project"` and `localPath`.
- **promote_designs(ids) → copyIds**: per ready design whose space has a parent other than the root
  (`can_promote_to`) and that has no promoted copy yet (`find_promoted_copy`): start = below its space's portal on
  the parent canvas (`portal.y + PORTAL_H + 80`, portal or default_spot), place with find_free_spot(grid); copy into
  the parent space with `parentId = lineage_parent(src)`, `starred: false`; record `promotedFrom`.
  - find_promoted_copy(id) = a copy whose `promotedFrom.designId == id` that still lives in the parent space.
  - lineage_parent(src): if src is itself a seed whose original is on the parent canvas → that original; else walk
    src's ancestors inside the same space: the first one with a promoted copy → the copy, or a seed whose original
    is on the parent canvas → the original; else none (no edge).
  - `promote(ids)` UI wrapper: toast "Promoted to <parent>" / "Promoted N designs to <parent>" with **Show** (enter
    the parent space and select the first copy).
- **delete_space(id)** (never the root): the space and all its descendants, their portals, designs, and the
  promotedFrom/seededFrom entries of the removed designs; running jobs in them are cancelled; active space falls back
  to the parent, selection cleared if removed. (UI trigger belongs to the portal card/shell; I provide the action.)
- **new_exploration()** ("+ → Exploration", hint "New empty space inside this canvas"): `can_create_in` guard,
  create_exploration(active), enter it.
- `can_create_in(space)`: on the root, toast error "Open a project first" / "The main canvas holds only projects.
  Open a project first, or create one with + → Project." and refuse. Used by Page, Exploration, Import from app,
  Layout-Finder.

### Card actions (in the card footer, after Compare)
- **Explore** (telescope, "Explore in a nested space", needs ready): create_exploration(seed = this card), enter the
  new space with the seed selected, focus the composer on the seed (`focusPrompt`, worker-6).
- **Promote** (arrow-up-from-line, "Copy up to <parent>", needs ready) when `can_promote_to(parent)`; once promoted it
  becomes **Promoted** (primary colour, "Show the copy in <parent>") → enter the parent space and select the copy.
- **Guidelines**: unset → icon "Set as the design guidelines of <space>"; set → badge "Guidelines" ("Design
  guidelines of <space>: new designs here follow them. Click to unset.").
- **"from <space>" badge** on promoted copies (right-aligned; "(deleted)" when the source space is gone, disabled
  then) → enter the source space with the original selected.
- Implement visibility: promoted copy or has a promoted copy, and ready (§5).

### "+ → Project" dialog (`ProjectSpaceDialog`)
- Title "New project", description "Pick a project: it gets its name, folder and a project info card."
- Combobox input "Search projects, or type a name or folder path…" (autofocus, ↑/↓/Home/End move, Enter picks).
  Options: first a "new" row (sub-line = typed absolute path, or "Type a name or an absolute folder path"), then
  projects from `GET /api/projects` → `{available, file, projects: [{id, name, repo?, localPath?, keywords[],
  readme?}]}` filtered by name/keyword substring (sub-line keywords joined " · " or "No keywords"). Loading row;
  unavailable: "No project list found (private-projects.ttl). You can still start a new project.".
- New row: `from_query(q)`: an absolute path (trailing slashes trimmed) → name = last segment, localPath = path;
  else name = q or "New project". create_project; enter; if it already existed toast "„name“ is already on the canvas".
- Listed project: `GET /api/projects/<id>` (details, fallback to the list entry) → create_project({name,
  localPath}); existing → enter + same toast. New → add card `{kind: page, title: "Projekt-Info · <name>",
  rationale: "Belegte Felder aus private-projects.ttl (Name, Repository, Keywords) und, falls das Repo lokal liegt,
  der erste README-Absatz.", html: info_card_html(p), status: ready, instruction: "Project <name> from
  private-projects.ttl", position: {0,0}}`, enter with it selected, then `POST /api/projects/<id> {spaceId}` (server
  files the note "[prototypie project-info <spaceId>] …"). Toast success "Space „name“ angelegt" / "Ein Agent
  sammelt weitere Projekt-Infos." or warning "… Kein Agent erreichbar: die Projekt-Info bleibt bei der ersten
  Karte."; failure toast "Space konnte nicht angelegt werden".
- `info_card_html(p)`: German "Projekt-Info" document (copy the template from project-space.tsx): kicker, h1 name,
  README paragraph, dl rows Repository / Lokal (if ≠ repo) / Keywords chips; light + dark via prefers-color-scheme.

### Project folder lookup (shared with the inspector and portal cards)
`local_path(space)` = own `localPath`, else the nearest ancestor's, else `GET /api/projects?match=<name>` →
`{localPath}` (cached per name). Pod record per folder from `GET /api/projects` `projects[].pod = {doc, subject}`.
Display paths with `/Users/<me>` → `~`.

---

## 12. Seams with other proto-app owners

Card footer order in the reference (`design-node.tsx`): **Compare (n)** · nested actions (**Explore**, **Promote**,
**Implement** when the card is a promoted copy or has one and is ready, **AI edit** controls, **Main** toggle) ·
**Spokes/Merge** · **Poll/In poll**. Header-left of the card: **NotesBadge**. Under the preview: **PodProvenance**.
Proposal: `features` exposes `fn card_actions(design, &AppState) -> Vec<CardAction>` (id, label, icon, style:
ghost|brand|secondary, enabled, pressed, tooltip) + `fn on_card_action(id, design)`, and `card_badges(design)`;
worker-5 renders them. Likewise:
- canvas (worker-4): `extra_nodes(space)` (mix drafts), `extra_edges(space)` (mix + combine/live-view base edges),
  `on_extra_node_move`, handle drag-end hook (`on_connect`, `on_connect_end(point, hit)`), selection list for the
  selection bar, Alt+D delete.
- header (worker-4): activity dock entries **Review** (§1.6) and the **agent capacity pill** (§5); More menu items
  **Decisions** (§2) and **Import bundle** (§6); "+" create menu entries **Page** (clay, §1.3), **Import from app…**
  (§6), **Layout finden…** (§7).
- inspector (worker-6): sections Spokes/Hub + Lanes (§1.5), Beim Agenten (§5), Notes (§10); composer embeds the
  mention widgets (§7) and the agent choice.
- fullscreen (worker-5): EditRequestBox (§9), NotesPanel (§10); `open_fullscreen(id, set)` used by View (§3) and
  Compare (§4).
- every html render path: `live_view::render_html` (§8).
- core/ai: `generate(GenerateOptions)` must return the new job id synchronously (lanes register against it) and
  emit per-design completion; jobs need `cancel(jobId)`.

## 13. Priority for M2 / M3

| # | feature | M2 (read-only) | M3 (editing) |
|---|---|---|---|
| 1 | Live views `compose_live` + main variant | **required** (else slot cards render empty) | create live view, Main toggle |
| 2 | Agent capacity pill + task panel (read) | **yes** (header parity) | notes to agent, Implement send |
| 3 | Review dock count + review mode browsing | dock count, review read-only (J/K, siblings) | done/star/discard, another round |
| 4 | Compare dialog | **yes** | star toggle |
| 5 | Lanes panel (inspector) | show jobs (empty in read-only) | cancel, reveal |
| 6 | Card badges: notes count, AI-edit busy/failed, PodProvenance | **yes** | undo, cancel |
| 7 | Spokes: picker + launch, merge, clay page | – | **core M3** |
| 8 | Multi-select bar (View works read-only) | View | Revise each / Combine, drag-to-mix drafts |
| 9 | AI edit (composer edit tab + fullscreen box) | – | **core M3** |
| 10 | Polls/decisions (designer side) | Results tab read | build, publish, winners; stakeholder web page via proto-server |
| 11 | @pod / #component mentions | – | with the composer (worker-6) |
| 12 | Imports (bundles, import from app) | – | M3 late |
| 13 | Annotate notes (native pin mode) | show existing notes | pin/apply notes (needs proto-web JS bridge) |
| 14 | Layout-Finder | – | M3 late / M4 |
| 15 | Voice dictation | – | M4 |
| 16 | Projects/explorations creation, Explore, Promote, guidelines | "from" badge, Promoted state | **core M3** (Project dialog, Explore, Promote) |

Build order I propose once proto-app compiles: live_view (pure, unit-tested) → spokes query/layout helpers (pure,
unit-tested: `parse_selection`, origins, `hub_of`, `siblings_of`) → agent-tasks poller + pill/panel → compare →
review mode → lanes → selection bar → spokes dialogs/merge → polls → mentions → imports → annotate → layout finder.

## Decisions (lead, 2026-11-18)
1. `notes`, `publishedPolls`, `pollWinners` go to proto-core (worker-1).
2. `compose_live` + `main_variant` + `slot_lineage` live in proto-core as pure fns (worker-1, semantics sent);
   features builds create_live_view and the Main toggle on top.
3. worker-4 owns the header slot; features builds the capacity widget + panel.
4. Annotate pin mode vs injected toolbar: the user decides; plan only, build at M3 (§10.1).
5. proto-server (worker-2) serves `/poll/<id>` and the poll API (requirements sent).
6. `features/nested`: navigation/URL state → worker-4, nested inspector → worker-6, creation flows + promote →
   worker-8 (§11).
