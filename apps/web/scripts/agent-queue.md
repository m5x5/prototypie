# Claude agent queue

When the inspector's agent toggle is set to **Claude**, `/api/variations` doesn't call an LLM. Each direction
becomes a work item in `data/agent-queue/<jobId>-<index>.json` (also mirrored as an Agentation annotation
starting with `[prototypie job <jobId>#<index>]`), and the job waits until a Claude Code session delivers it.

## Watching the queue

1. List pending items: `curl -s localhost:3123/api/agent-queue`
   Each item has `jobId`, `index`, `instruction`, `direction {title, brief}`, and optionally `baseHtml`/`baseTitle`
   (the design to vary), `focus` + `scope`, and `annotations` (Agentation notes: `[element] comment`).
   `podContext` lists @-mentioned Solid pod data (`label`, `kind`, `source`, `access` live/401/ref/cache, `stand`,
   the selected `fields` `{p, v?, type?}`, `relations`, `note`): use exactly these values, invent none, show gaps
   honestly. The full catalog is `GET /api/pod-catalog`.
   `components` (`[{id, title, html}]`) are cards the designer #-mentioned in the prompt: existing components
   (from this space or its parent spaces) to build with.
   `guidelines` (`{cardId, title, text}`) are the design guidelines of the space the result lands in: the card marked
   as that space's guidelines (bookmark button on a card), or the nearest parent space's. `text` is the card's
   content as plain text; the full HTML is the card `cardId` (`GET /api/state`, record `designs/<cardId>`).
   `context` says where the result lands: `spaceId`, `spaceName`, `spacePath` (exploration path from the main
   canvas), `baseDesignId`, `placeholderId` (the card this item fills), the space's other `cards` (`id`, `title`) and,
   for an exploration started with "Explore", its `seedDesign` (`id`, `title`, `originalId` in the parent space).
   Use these as `spaceId` / `nearId` / `parentId` with the canvas skills below when one card isn't enough.
   **Claim the item before working on it**, so the task panel shows who works on it (re-claiming overwrites;
   the name is stored in the item and returned as `agent` by `/api/agent-queue` and `/api/agent-tasks`):
   ```sh
   curl -s -X POST localhost:3124/api/agent-queue/<jobId>/<index>/claim -H 'content-type: application/json' \
     -d '{"agent":"proto-worker-6"}'   # or your session name; 404 unknown item, 409 no longer pending
   ```
   (Rune app on :3124 only; the Next dev server on :3123 has no claim route.)
2. Design it as one self-contained HTML document: inline `<style>`, optional inline `<script>`, Google Fonts allowed,
   no external images or JS, real copy. With a base design, keep its product, content and visual language.
   **Hard rules (from the user):**
   - **Follow `guidelines` when present.** They set the frame for every design in that space (device, colours,
     type, spacing, what not to do) and win over the base design and your own taste; only an explicit
     instruction in the prompt overrides them.
   - **Only the component being talked about.** Design that component and put all the work there. Don't add,
     redesign or decorate anything else.
   - **No explanatory text at all.** No headings, captions, labels, spec lists, legends, annotations or "why"
     notes around or inside the design. The prototype is the product UI only; reasoning goes in `rationale`.
   - **Titles are short and space-scoped:** never include the project/space name or words like Komponente/
     Component; 1-4 words describing the design (e.g. "Werkzeugleiste", "Kompakte Sidebar"). The card already
     lives in the project's space. Same for `POST /api/agent/cards` and `/api/agent/explorations`.
   - **One variant per item.** Deliver a single design, not several options, states or versions side by side.
     If the prompt wants more, create more cards with the skills below, one design each.
   - `focus` with `scope: "page"`, or any `annotations`: change only those elements; everything else stays
     identical to `baseHtml`.
   - `scope: "component"`: return only the focused component, centred on a neutral backdrop.
   - Several items of the same job (`Direction 1..N`): make them clearly distinct from each other.
   - `bases` (`[{id, title, html}]`, from a multi-selection "Combine"): **combine these designs** into ONE new
     design that merges the strongest ideas of all of them (layout, components, interactions, visual language),
     steered by `instruction`. Don't place them side by side. `baseHtml` is absent then.
   - `mode: "import"` (from "+ → Import from app…"): see **Import items** below. Recreate, don't design.
   - `components`: **reuse each mentioned component faithfully** wherever the design needs it: same markup,
     styles, tokens, icons, copy and sizes, adapting only what placement requires (width, outer spacing). Don't
     redesign, restyle or re-theme them, and don't lay them out as a reference sheet. If the instruction asks to
     change a mentioned component, change only what it asks.
3. Deliver it (one item at a time), **the moment that item is finished**. Never hold finished items back to post
   a job's items together (e.g. at the end of a multi-agent workflow): the canvas shows each card as soon as its POST
   lands, so batching makes every card sit on "Designing…" until the slowest one is done. In a workflow, POST from
   inside each item's own step, not after collecting all results.
   ```sh
   curl -s -X POST localhost:3123/api/agent-queue/<jobId>/<index> \
     -H 'content-type: application/json' \
     -d @result.json   # { "title": "...", "rationale": "1-2 sentences for a stakeholder", "html": "<!doctype html>...", "viewport": 1240 }
   ```
   `viewport` (optional, 200–4000) is the width the design is laid out at: set it when the target device differs
   from the placeholder's frame (e.g. `1240` for a portrait tablet, `1654` landscape, `~400` for a phone). Without it
   the width comes from a `<meta name="viewport" content="width=N">` in the html, else the card keeps its frame
   (a variation inherits its base's, so a phone base stays phone-sized).
   `viewportHeight` (optional, 200–4000) fixes a screen's height so the card keeps its ratio (e.g. `640`×`960` for a 2:3 phone; also read from `height=M` in that meta; inherited like `viewport`).
   `200 {ok:true}` shows it on the canvas immediately and resolves the mirrored annotation. `404` means the job
   is gone (cancelled, or the dev server restarted). `409` means the user deleted the card this item
   fills, so the item is cancelled; add a card via `/api/agent/cards` only if the prompt still wants it. `400` lists schema errors.

Cancelling the run in the app marks its items `cancelled` and dismisses their annotations.

## Live views (components by reference)

A card can place other cards by reference instead of copying their html:
`<prototypie-slot data-design="<design id>" class="…" style="…"></prototypie-slot>`. When rendered, each slot
shows the current html of that card's **main variant** (the newest card marked "Main" among it and its parentId
variations, else the card itself) in an iframe that grows to the content's height (`data-fit="fill"`: fill the
slot's own height; `data-height="N"`: fixed N px). So the designer iterates on components separately and the live
view always shows the latest main one. Created with "Live" in the multi-selection pill, or by any card whose html
uses slots. When asked for a live/combined view of components, lay out the page shell yourself and put a slot
where each component goes (ids from `context.cards`). When editing a card that contains slots, keep them as they
are unless the instruction is about them; `baseHtml` holds the slots, not the components' html.

## Import items

`mode: "import"` items carry `element` (what the designer named, e.g. "Toolbar"), `importSource` (a local repo
path or URL, usually the space's project repo) and optionally `importScreenshot` (absolute path of a PNG/JPEG the
designer attached, saved under `data/agent-queue/`). `scope` is `"component"` and there is no `baseHtml`. The card is flagged `component: true`; title it with
the element's plain name (e.g. "Werkzeugleiste"), no "Import" prefix.

- Find the element in the source and recreate it **faithfully as it exists today**: same structure, design
  tokens (colors, type, radii, spacing, elevation), icons, real copy and real sizes. Set `viewport` to the
  device the app runs on (phone ≈ 400, tablet 1240, desktop 1280+). The screenshot, if given, wins over guesses.
- Don't redesign, improve, restyle or add anything: the card is the starting point the designer improves later.
  Where the source leaves a value open (dynamic data, theme-dependent colour), use the app's default and say so
  in `rationale`, not in the design.
- Component only, centred on the app's own background; no labels or captions (the usual hard rules apply).
- `guidelines` (the space's design guidelines) come along as context. For an import, the app's source still
  wins: use the guidelines only for values the source leaves open, and note real conflicts in `rationale`.
- **Read-only:** read the other repo, never edit, build, install or commit in it, and never start its dev
  server. Don't touch connected devices (adb, simulators, the user's phone) unless the brief explicitly allows it.
- Deliver with the usual POST. A URL source you can't read: report it to the lead instead of guessing.

With the Codex/LLM agent, imports aren't possible: the entry is disabled in the dialog and the server answers
with an error card.

## Edit items

`mode: "edit"` items are **in-place AI edits** of an existing card (the "Edit" tab in the prompt composer, or the
wand button on a card). They carry `targetDesignId` (the card), its current html as `baseHtml`, `baseTitle`, the
edit `instruction`, and, when the card has open Agentation notes, `annotations` + `focus` (`scope` is `"page"`).
`context.placeholderId` is the target card itself.

- Return **the same document with only the requested change**: every other element, style, script, copy and
  order stays byte-identical. No redesign, no new directions, no extra cards. `guidelines` still apply.
- Deliver with the usual POST. It **overwrites the target card** (same id, position and edges) instead of filling
  a placeholder; the card's `title` is kept, `rationale` should say what changed in one sentence, and
  `viewport` is only needed if the edit changes the device.
- The previous content is kept in the card's `versions` (newest last, at most 5) so the designer can undo; while
  the item is pending the card shows "Bearbeite…" with a cancel button. Cancelling marks the item `cancelled`.

The same edit can be started directly: `POST /api/edit { designId, instruction, focus?, annotations?, agent?:
"claude" | "codex", podContext?, components? }` -> `{ jobId }` (409 while the card is already being edited);
`DELETE /api/edit/<designId>` cancels it. With `agent: "codex"` the LLM edits the html itself.

## Skills: creating cards and explorations directly

A queue item answers with exactly one card. When the prompt asks for more ("make 3 cards", "start an exploration
for this", "put these in their own space"), create them yourself with these endpoints. They write to the shared
state, so every open browser shows the result live without a reload. Deliver or cancel the queue item as well,
so the run doesn't keep waiting. Its own design still goes to `/api/agent-queue/<jobId>/<index>`.

Find where things go first:
```sh
curl -s localhost:3123/api/agent/canvas   # { spaces: [{ id, name, parentSpaceId, kind?, localPath?, cards: [{ id, title, status, kind, position }] }] }
```
The root canvas is `root`; it holds only projects (spaces with `kind: "project"`, one per local folder, never nested), so
cards and explorations can't go there: a request that would land on `root` is rejected with 400. The space the prompt
came from is the base design's `spaceId` (look it up by title here).

**Add cards** to a space, in a row right of `nearId` when given (otherwise below the space's content):
```sh
curl -s -X POST localhost:3123/api/agent/cards -H 'content-type: application/json' -d '{
  "spaceId": "<space id, optional: defaults to nearId'"'"'s space; never root>",
  "nearId": "<design id to sit next to, optional>",
  "cards": [{ "title": "...", "rationale": "1-2 sentences", "html": "<!doctype html>...",
              "viewport": 1280, "kind": "variation", "parentId": "<design id, optional: draws an edge>" }]
}'   # -> { spaceId, ids }
```
`kind` is one of `page`, `variation` or `spoke`. Set `"component": true` on a card that is a reusable
component (not a page), so it's listed first when the designer #-mentions components. Set `parentId` to the design a card derives from so the
canvas draws the edge. Same-space parents only.

**Start an exploration** (a nested space plus its portal card on the parent canvas), like the app's Explore button:
```sh
curl -s -X POST localhost:3123/api/agent/explorations -H 'content-type: application/json' -d '{
  "name": "Header: stacked layouts",
  "seedId": "<design id, optional: copied in as the starting card>",
  "parentSpaceId": "<optional: defaults to the seed'"'"'s space; never root>",
  "cards": [ { "title": "...", "html": "..." } ]
}'   # -> { spaceId, portal, seedId, cardIds }
```
Open it at `localhost:3123/?space=<spaceId>`. Cards follow the same HTML rules as queue results (step 2 above).

Inputs are validated (400 with `issues`): title 1–200 chars, non-empty html, `kind` page|variation|spoke,
`viewport` 200–4000, ids `[\w-]{1,64}`, at most 50 cards. An exploration with its cards is written in one batch,
so a rejected request leaves nothing behind. Test data goes under a `zz-bench` name and gets deleted afterwards
(PUT `/api/state` with `{ changes: [{ path, deleted: true }] }`).
