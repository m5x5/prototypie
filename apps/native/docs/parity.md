# Parity checklist: reference app (Next.js, :3123) vs Rune app (proto-app)

Written by proto-worker-8 on 2026-11-18. Sources: the reference UI (`src/components/**`, `src/features/**`,
`src/app/poll/**`), `docs/spec-*.md` and the Rune code as it stands.

How each status was checked: **S** = seen in a headless `--scene` snap, **T** = a unit or integration test,
**C** = read in the code only (no snap or test), **R** = needs a look in the running app (native web views
never appear in `--snap`). All 29 scenes render without a panic (checked 2026-11-18). Statuses are
done / partial / missing / blocked-on-user. Owners: w4 shell+canvas, w5 cards+fullscreen, w6 inspector+composer,
w7 runner/ai, w8 features, w2 server, w3 web.

## Two checks only the user can do in the running app

1. **Fullscreen live page (w5):** open a design in fullscreen. The interactive page should land on the preview
   after the open animation, follow window resizes, and step with ←/→ through a selection. Esc closes it and the
   page disappears in the same moment (no web view left floating over the canvas).
2. **Review stage live page (w8):** open Review; once the design on stage has loaded, the stage should show the interactive page; ←/→ inside it move through the round and Esc closes review.
3. **Compare live column (w8):** open Compare (n) on a card with variations. The focused column should show the
   interactive page and the others pictures. Click another column and the page moves there; scroll the row
   sideways and the page should move with its column, not stay put or cover the header. Esc closes.

## Header (shell)

| feature | status | how | owner |
|---|---|---|---|
| Logo → main canvas, breadcrumb with › separators, "up" button | done | S (nested) | w4 |
| Breadcrumb collapses beyond 4 crumbs ("…" menu of hidden spaces) | missing | C (no collapse in header.rs) | w4 |
| Rename the current space inline (click name, Enter/Esc) | missing | C (no rename UI) | w4 |
| Header backdrops fade over the canvas; titlebar drag | done | S | w4 |
| Agent capacity pill + task panel (groups, detail, steps, thread, Zur Karte, note composer) | done | S, T (poller, parse) | w8 |
| Review dock entry (count, generating dot, queue panel, Start reviewing) | done | S (review-dock) | w8 |
| "+" New menu: Page, Exploration, Project, Layout finden…, Import from app… (only Project on the main canvas) | done | S (new-menu) | w4 + w8 |
| Fullscreen window toggle | done | C | w4 |
| ⋯ menu: Decisions (status), Import bundle list | done | S (more-menu), C | w6 + w8 |
| ⋯ button spinner while a bundle imports | missing | C (`features.imports.busy` not read) | w4 |
| Toasts for shell/card actions (delete with Undo, etc.) | partial: shared toast API exists (`features::notify[_with]`, `undo_for` + `NoticeAction::Undo`, tested). Cards (w5): done, card Delete shows "Deleted “X”" + Undo (undo captured before the write). Shell actions (w4): open | T; w5: C (cards/ops.rs) | w4 + w5 |

## Canvas

| feature | status | how | owner |
|---|---|---|---|
| Dot grid, pan (two-finger/wheel), pinch and ⌘-wheel zoom about the pointer | done | C, S (zoomed-out) | w4 |
| Fit view on entering a space, with insets | done | C (poll_fit) | w4 |
| Zoom controls (bottom-left: +, −, fit, lock) | missing | C (no controls in canvas/) | w4 |
| Glass minimap (bottom-right) | missing | C (only constants in insets/style) | w4 |
| Click selects, ⇧-click toggles, drag moves (selection moves together) | done | C (nodes.rs), T (canvas tests) | w4 |
| Marquee selection (⇧/drag on empty canvas) | missing | C | w4 |
| Click on empty canvas clears the selection | missing | C (no background press handler) | w4 |
| Parent edges, combined-base edges, draft edges | done | T (canvas test), C | w4 + w8 |
| Edge "animated" dash for generating designs | partial | w4: dash drawn but doesn't move yet | w4 |
| Card handle drag → draft card / mix (drag-to-mix) | done | T (canvas + mix tests) | w4 + w8 |
| Alt/Option+D deletes the selection | done | C | w8 |
| Main canvas = projects only (create guard + notice) | done | T, S | w8 |

## Design cards

| feature | status | how | owner |
|---|---|---|---|
| Title row, kind badge, fullscreen / star / delete icons, notes indicator | done | S (cards) | w5 |
| Thumbnail from proto-web snapshots, card sizing rules | done in app, empty in --snap | R (headless snaps show empty frames) | w5 + w3 |
| Generating / error / interrupted states, Retry | done | C, T (runner) | w5 + w7 |
| Action row: Vary, Compare (n), Explore, Promote/Promoted, Implement (phases), AI edit (wand, undo, busy, error), Main, Guidelines, "from" badge, Spokes/Merge, Poll/In poll | done | S (cards), C | w5 + w8 |
| Implement "Erneut senden …" menu after Erledigt | done: split button (chevron) → confirm inline on the card ("Erneut an den Agenten senden?", Abbrechen / Erneut senden) → `implement::send(s, id, true)`; inline instead of a modal (no shared dialog yet) | C (cards/actions.rs, ops.rs); build + tests green, not snapped (needs a Done task) | w5 |
| Notes badge (open Annotate notes count) | done | C | w5 + w8 |
| Pod provenance strip on cards made with @-mentions | done: `features::pod_provenance` mounted under the card preview (inside the card memo; reads only the record) | C (cards/design.rs, mod.rs); not snapped (fixture has no podContext) | w5 + w8 |
| Project / exploration (portal) cards: path, hero, counts, generating dot, "new" badge, delete space | done | S (projects, explorations) | w5 |
| Light mode + every state (ready, generating, failed + Retry, AI edit running / failed + undo, Implement split, kind/notes badges, selection outline; portal with hero, empty, designing shimmer, "N new", "N generating", selected ring) | done (2026-11-18): compared with the reference at :3123 in light (headless, POSTs blocked) by computed styles + zoomed screenshots: card #fff, border ≈#e5e5e5, radius 12, shadow 0 8 30 /.10 (selected 0 12 36 /.16 + 2px outline #171717/.6 offset 2), title #737373 12px, action buttons h28 12.8px r8; portal r14, ring 2px #171717 + shadow-lg, chip primary/10, 300×268.5. One difference fixed: secondary button/badge text now `secondary-foreground` (#171717 light) instead of foreground | S (cards-light, projects-light, explorations-light; zoomed script), reference computed styles | w5 |

## Inspector and composer

| feature | status | how | owner |
|---|---|---|---|
| Design details (title, made from, rationale with Read more) | done | S (inspector-design, inspector-made-from) | w6 |
| Composer tabs (this design / its variations / edit), count 1–6, agent picker, history, send | done | S (composer-*) | w6 |
| @ pod / # component mentions (list, chips, Flows into generation) | done | S (mention), T | w8 + w6 |
| Feature sections: notes list + Apply notes, Hub/Spoke box + Merge, Beim Agenten, Exploration lanes | done | T, C (no dedicated scene with running lanes) | w8 |
| Exploration stats, promote starred, explorations/projects list | done | S (inspector-exploration) | w6 |
| Pod link on projects in the list | partial | C (`pod_for` returns None) | w6 |

## Fullscreen

| feature | status | how | owner |
|---|---|---|---|
| Page, set stepping (←/→, stepper, strip), Esc; card → fullscreen morph | done | S (fullscreen, fullscreen-set); script (→ → from 2/3 lands on 1/3; morph frames show the preview flying from the card) | w5 |
| Live interactive page | done, unverified | R, check 1 above | w5 + w3 |
| Designer's own note box (free text, autosave) | done: "Notiz" first in the side panel; saves `design.notes` 600 ms after the last keystroke, flushes on ←/→ step and close, blank removes; remote changes replace the text only while nothing is pending; raw record patch | S (fullscreen), script (tap → type → "Speichert…" at 1.2 s → "Gespeichert" + stored at 2.2 s) | w5 |
| Fullscreen in light (page bg, preview box r8 + border, note box, side panel) | done | S (fullscreen-light) | w5 |
| "Änderung anfragen" AI edit box, notes list | done | C, T (ai_edit) | w8 + w5 |

## Compare / Review / Spokes / Lanes

| feature | status | how | owner |
|---|---|---|---|
| Compare: columns, widths, star, open fullscreen, Esc | done | S (compare), T | w8 |
| Compare: live page in the focused column | done, unverified | R, check 2 above | w8 |
| Review mode: queue, stage, siblings, Star/Discard/Done, Undo, Mark all, Show on canvas, keys J/K/←→/S/X/↵/1–9/F/⌘↵ | done | S (review), script, T | w8 |
| Review: Another round (lanes, picks, annotations) | done | T | w8 |
| Review stage live page (LiveCtl over the stage; Esc closes, ←/→ through the round) | done, unverified | R (snapshot until loaded / in --snap) | w8 |
| Spokes picker (analysis, retry, custom rows, direction, count), block-of-clay page, merge with note | done | S (spokes, clay), T | w8 |
| Lanes panel (running + recent, cancel, review, reveal, archived) | done | C (needs a running generation to snap) | w8 |
| Voice dictation (feedback, clay brief) | blocked-on-user (M4) | – | w8 |

## Multi-select, drag-to-mix

| feature | status | how | owner |
|---|---|---|---|
| Selection bar: count/clear, View, Live, Revise each / Combine, counts, Enter/Esc | done | S (multi, multi-open), T | w8 |
| Draft / mix cards (sources, text, count, send, Esc) | done | T | w8 |

## Polls and decisions

| feature | status | how | owner |
|---|---|---|---|
| Poll tray, Decisions popover, Build tab (agent, editors, validation, publish with LAN share) | done | S (decisions, decisions-dock), T | w8 |
| Results tab (votes, tallies, Leading, Mark winner, Forget, Copy) | done | T (winners), C (no published poll in a scene) | w8 |
| "Geteilt im LAN" / "nur lokal" status in the popover | done | C (GET /api/polls/share on open and after publish) | w8 |
| Stakeholder page /poll/<id> (intro, steps, done, 404) and LAN listener | done (per w2) | not checked by me | w2 |

## Imports, Implement, tasks, pod

| feature | status | how | owner |
|---|---|---|---|
| Import bundle (list, create project/exploration, rows, parent links) | done | T | w8 |
| Import from app dialog (Claude only, Source prefill) | done | C | w8 |
| Screenshot attachment in Import from app (file picker or paste; ≤ 8 MB; data URL) | done (paste); picker needs Rune's `native-file-dialog` feature in proto-app's Cargo.toml | T (base64, types), C | w8 + w4 |
| Implement request + phases + task panel jump | done | C | w8 + w5 |
| Layout-Finder (4 layouts, A/B, relations, Übernehmen) | done | S (layouts), T | w8 |
| Annotate: pin notes from inside the app | blocked-on-user | – | w8 + w3 |

## Cross-cutting

| feature | status | how | owner |
|---|---|---|---|
| Light mode | done | S (light, cards-light, review/decisions in light). Fixed today: 13 feature text fields had no text colour and were invisible in light mode | all |
| Keyboard: Esc closes every feature overlay; Enter/⌘↵ submit per reference | done | C, script | w8 |
| Empty states (inbox zero, no decisions, no polls, no bundles, no tasks, nothing matches) | done | C, S (some) | w8 |
| Error states (spokes analysis, find decisions, publish, implement, notes, catalog) | done | C | w8 |
| Mobile layouts (pane switch, max-md variants) | out of scope (lead decision) | – | – |
| Local-time formatting of task and poll times | done | T | w8 |

## Keyboard shortcuts

Written by proto-worker-6 on 2026-11-18. Every key handler in the reference (`grep` over `src/**` for
`e.key`/`keydown`/`onKeyDown`/`metaKey`, plus React Flow and base-ui defaults the app relies on), checked in the
Rune app. **K** = scripted check: `--scene X --script keys.json --tree out.json`, asserting on the element tree
(harness: 26 checks, all on in-memory fixture scenes; none send a prompt or delete anything). Destructive or
sending keys are checked in process instead (**T**: `Runtime` tests where the runner never runs).

| where | shortcut (reference) | Rune | how | owner |
|---|---|---|---|---|
| Composer | Enter sends, Shift+Enter newline (not while composing IME) | done | T (`enter_sends_and_shift_enter_breaks_the_line`), K (Shift+Enter) | w6 |
| Composer | `@` / `#` open the mention list; ↑/↓ move, Enter/Tab pick, Esc closes; Backspace at 0 drops the last chip | done | K (`#` opens, Esc closes), T (w8 mention tests) | w8/w6 |
| Composer | count picker ←/↓ −1, →/↑ +1, Home 1, End 6 | done | K (→), T (`keys_step_and_clamp`) | w6 |
| Composer | recent-prompts and agent menus: ↑/↓ rove, Enter picks, Esc / outside click close | done | K (Esc ×2, ↓ roves) | w6 |
| Composer | target tabs ←/→ (tab group) | done (focus group) | C | w6 |
| Inspector | title: Enter/Space copies; "Made from" popover Esc / outside click closes | done | K (Esc) | w6 |
| Header ⋯ menu | Esc / outside click close, ↑/↓ rove | done | K (Esc) | w6 |
| Header | New menu Esc / outside click | done | K | w4 |
| Header | rename the space: Enter / blur saves, Esc cancels (Rune adds F2) | done | K (Esc), T (crumbs tests) | w4 |
| Canvas | Shift / ⌘ / Ctrl + click adds to the selection | done | C (`canvas/nodes.rs`) | w4 |
| Canvas | Shift + drag draws a marquee; ⌘/Ctrl + wheel and pinch zoom | done | C, T (canvas tests) | w4 |
| Canvas | ⌥D deletes the selected design(s) (not while typing); Delete/Backspace do nothing (`deleteKeyCode={null}`) | done | C (`features/mod.rs` `on_key`); not scripted (destructive), a `Runtime` test on a fixture would cover it | w8 |
| Canvas | React Flow default: ↑↓←→ move a keyboard-focused selected node | missing | – (low priority; incidental in the reference) | w4 |
| Fullscreen | Esc closes (first Esc only leaves browser fullscreen); ←/→ step through the set (not while typing) | done | K (Esc, →, ←) | w5 |
| Review | j/↓ next, k/↑ previous, →/← within the strip, s star, x discard, 1–9 toggle siblings, f or / focus feedback, Enter done, ⌘Enter another round | done | K (Esc, j, ↓, s), C (rest) | w8 |
| Review | `m` hotkey: dictate into the feedback box (voice) | missing | C (no voice input in the Rune app) | w8 |
| Review dock, Decisions popover | Esc / outside click close | done | K | w8 |
| Agent tasks panel (capacity pill) | Esc / outside click close, focus back to the pill; ⌘Enter sends the note | **Esc missing** (only × closes it); ⌘Enter done | K (Esc fails), C | w8 |
| Project dialog | ↑/↓/Home/End move, Enter opens/creates, Esc closes | done | K (Esc), C | w8 |
| Layout finder | arrow keys rove the radio groups; Esc closes | done | K (→ roves, Esc) | w8 |
| Clay brief | ⌘Enter submits | done | C | w8 |
| Spokes merge note | ⌘Enter merges | missing (merge only by button) | C (merge-note field has no key handler) | w8 |
| Multi-select bar, mix card, AI edit box | Enter sends (Shift+Enter newline), Esc collapses | done | K (multi Esc), C | w8 |

## Accessibility

Checked with `--tree` over all 29 scenes (accessibility tree: role, name, focusable, focused) and a script that
flags every button / link / menu item / tab / radio without a name.

| check | status | owner |
|---|---|---|
| Inspector, composer, ⋯ menu: every control named (icon-only ones by `label`); Tab stops with the reference's ring (3pt, ring/50, keyboard only); menus are modals (focus trap, Esc, outside click, opener restored) with ↑/↓ roving | done (K, tree) | w6 |
| Card controls (vary, star, open…) focusable with rings | done | w5 |
| **Unnamed**: the canvas pane (`canvas`, role button) and every node frame (`design:<id>`, `portal:<id>`); VoiceOver reads only "button". Name the frames by card title, the pane "Canvas" | missing | w4 |
| **Unnamed**: compare columns (`compare-col-<id>`) | missing | w8 |
| **Not keyboard-reachable**: header logo, breadcrumbs, up, New, fullscreen, ⋯, agent capacity pill, review dock button. App-wide fix: `cx.web_focus()` in `shell::view` (every pressable button/link/tab/radio becomes a Tab stop, as HTML buttons are) plus `cx.focus_ring(3.0, 0.0, ring.alpha(0.5))` for one consistent ring | missing | w4 (w8 for their header buttons) |
| **Focus order**: Rune uses document order, which is canvas → header → inspector (the canvas is painted first); the reference is header → canvas → inspector. Fix: header controls `.tab_index(1)` (positive indices precede document order and keep their own order) | missing | w4 |
| Focus rings visible | partial: inspector, composer, cards yes; header and feature buttons none until the two fixes above | w4/w8 |

## Visual parity pass (2026-11-18, w8)

Method: headless Chromium screenshots of the reference at :3123 (1280×800 @2×, dark and light, every non-GET request
blocked, page closed after each run) next to Rune `--snap`s of the same views on the real store. Thumbnails are empty
in Rune headless snaps (no WebKit), so card pictures were not compared.

| view | difference | status | owner |
|---|---|---|---|
| Header | Right group order: reference capacity · **+ New** · Review · fullscreen · ⋯; Rune puts Review before +. | open | w4 |
| Header | "+" opens as a blue pill "+ New ⌄" while the menu is open (reference); Rune keeps the round + | open | w4 |
| + menu | Reference: 2-column tile grid, each tile a bordered icon square, title, 2-line hint; Rune: a plain list without icons. `CreateItem.icon` now carries each lucide icon. | open | w4 |
| Canvas | Light mode: Rune's dot grid is far more visible than the reference at low zoom (reference dots fade out when zoomed out) | open | w4 |
| Canvas | Glass minimap missing (bottom right); Annotate toolbar button (blocked on the user) | open / blocked | w4 / user |
| Fullscreen | "Notiz" box shows a caret on open in the snap (looks autofocused again) | check | w5 |
| ⋯ menu | Matches; Rune rows ~2pt taller and the menu ~10pt lower | minor | w6 |
| Capacity pill | Spark icon → the reference's pixel "minion" | fixed | w8 |
| Task panel | Anchored to the window edge → under the pill (right-aligned); group headings now "WARTET 9" (caps + count); minion in the heading; Esc/outside click close it | fixed | w8 |
| Review dock | Compact badge → the reference's two-line entry (Review / "677 waiting · n generating"); panel anchored under the button | fixed | w8 |
| Review | Icons on Star/Discard/Done, Another round, Show on canvas, Mark all reviewed; kind badge capitalised ("Variation"); queue order with equal createdAt now left-to-right (matches the reference) | fixed | w8 |
| Compare | No frame/fill around designs (reference); smaller header; column order for equal createdAt matches the reference | fixed | w8 |
| Änderung anfragen | Send moved into the box as an icon button (reference), no separate "Senden" button | fixed | w8 |
| Inspector sections | No separator before the Hub card; box icon in its title; "Break into spokes" a filled muted button with icon; +1 padding for the 1px border; @/# lucide icons (from w6's review) | fixed | w8 |
| Lanes panel | Showed every stored lane, also on the main canvas; the reference lists only this tab's jobs → now only runs started this session | fixed | w8 |
