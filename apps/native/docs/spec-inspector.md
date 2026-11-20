# Inspector + composer: spec and Rune mapping

Owner: proto-worker-6 (`proto-app/src/inspector/`, `proto-app/src/composer/`). Source of truth is the reference app
(`apps/web`, dark theme), read on 2026-11-18:
`src/components/inspector.tsx`, `src/components/prompt-composer.tsx`, `src/features/nested/inspector.tsx`,
`src/features/pod-mention/{mention,components,catalog}.tsx|ts`, `src/lib/glass.ts`, `src/features/imports/import-menu.tsx`,
`src/features/poll/decisions-dock.tsx`. Rune refs are against `a checkout of anomalyco/rune` @ 4a2e437.

Desktop only: the reference's `max-md:` (phone) variants are out of scope.

---

## 0. Tokens (dark theme, resolved)

| token | reference | value | Rune |
|---|---|---|---|
| canvas bg | `--canvas-bg` | `#000` | `Color::BLACK` |
| foreground | `--foreground` oklch(.985 0 0) | ≈ `#fafafa` | `oklch(0.985,0.,0.)` |
| muted-foreground | oklch(.708 0 0) | ≈ `#a1a1a1` | `k.c.text_muted` overridden to this |
| muted / accent (hover rows) | oklch(.269 0 0) | ≈ `#262626` | `k.c.hover` |
| popover / card | oklch(.205 0 0) | ≈ `#171717` | popover fill |
| border | white 10% | `#ffffff1a` | `Color::WHITE.alpha(0.10)` |
| input border | white 15% | `#ffffff26` | `Color::WHITE.alpha(0.15)` |
| ring (focus border) | oklch(.556 0 0) | ≈ `#737373` | `k.c.border_focus` |
| primary (count pick) | oklch(.922 0 0) / fg oklch(.205) | `#e5e5e5` / `#171717` | |
| destructive | oklch(.704 .191 22.2) | ≈ `#ff6467` | |
| composer box bg | `dark:bg-[#151515]` | `#151515` | |
| brand (send) | `bg-blue-700` + `inset 4px 4px 17px rgba(255,255,255,.28)`, white | `#1447e6` | `.fill(brand).box_shadow(Shadow{x:4.,y:4.,blur:17.,color:WHITE.alpha(.28),inset:true,..})` |
| radius | `--radius` 10px; sm 6, md 8, lg 10, xl 14, 2xl 18 | | consts in `inspector/style.rs` |

Type: reference uses Google Sans Flex; the Rune app uses whatever `proto-app` shell picks (Inter default). Sizes:
text-xs 12/16, text-sm 14/20, `[13.5px]` leading-snug (1.375), `[12.5px]`, `[11.5px]`, `[11px]`, `[10.5px]`, `[10px]`.
font-medium = 500 (`TextStyle::new(sz, 500.)`), semibold = 600. Tabular numbers: `TextStyle` `features` `tnum`.
Icons: lucide (24 view, stroke 2, round caps) — paste lucide bodies as `pub static X: rune::Icon` in
`inspector/icons.rs` (no lucide set ships with Rune): Check, Info, ArrowUp, History, Layers, PenLine, Wand2, AtSign,
Hash, X, Database, FileText, Folder, Shapes, Server, Lock, Component, LayoutTemplate, FolderGit2, ArrowRight,
ArrowUpFromLine, Star, MoreHorizontal, Vote, FolderInput, Loader2 + Claude/Codex agent marks (from
`src/components/agent-icons.tsx`).

---

## 1. Panel (GLASS_PANEL)

Reference: `<aside>` absolute `top-1 right-4 bottom-4 w-[340px] rounded-2xl overflow-hidden z-20`, inside the area
below the floating header (h-12), so top ≈ 52px from the window content top. `INSPECTOR_OVERLAY = 372` (340+16+16) is
the canvas right inset used by fitView and the minimap (`mr-[388px]` = 372+16); export the same const from
`inspector::INSPECTOR_OVERLAY` for proto-worker-4's canvas.

`GLASS_PANEL` = `border border-foreground/10 bg-(--canvas-bg)/80 shadow-[0_10px_30px_-12px_rgb(0_0_0/0.6)]
backdrop-blur-[20px] backdrop-saturate-[1.3]`.

Rune:
```rust
col().key("inspector")
    .pin(None, Some(HEADER_H + 4.), Some(16.), Some(16.)).w(340.)
    .radius(18.).clip_rounded(18.)
    .backdrop_blur(20.)                                   // σ in points == CSS blur(); budget: MAX_BACKDROP_BLURS = 4/frame
    .fill(Color::BLACK.alpha(0.80))                       // light theme: page bg (#fff) .alpha(0.80)
    .border(1., Color::WHITE.alpha(0.10))                 // light: BLACK.alpha(0.10)
    .box_shadow(Shadow { y: 10., blur: 30., spread: -12., color: Color::BLACK.alpha(0.6), ..Default::default() })
    .child(scroll body)
```
- `backdrop-saturate(1.3)` has no Rune equivalent (only rune-fx lens). Drop it; at 80% black fill the difference is
  invisible. Note in the parity checklist.
- Blur budget: panel (1) + minimap GLASS (1) + header fades are gradients, not blurs. Mention popovers/menus stay
  opaque (popover fill) so they don't spend blurs.
- Body: `col().key("inspector-scroll").h_full().min_h(0.).scroll(ScrollSpec{..})` with kit `scroll_bar`; content
  `padding(16.)`, `gap(16.)` (`space-y-4`).

## 2. Body order (top → bottom)

1. **Design header** (only with a selected design), `gap 8`:
   - Row `gap 6`, items-center: **CopyTitle** + **MadeFrom** (only if `design.instruction`).
   - **Rationale**.
2. If at main canvas (root space): dashed note box instead of the composer — `rounded-lg(10) border dashed px-12 py-10
   text-xs muted`: "The main canvas holds only projects. Open a project first, or create one with + → Project."
   (`.border_dash(4., 3.)`.) Else **Composer** (§3).
3. Edit error (Edit tab only): `text-xs destructive`.
4. **Flows into generation** (pod context panel, §4.4) when ≥1 @mention.
5. Slots owned by proto-worker-8 (`src/features/`), each a fn `(&State, &Kit) -> Option<El>` I call in order:
   NotesPanel(compact) if design ready, SpokesInspector, then **NestedInspector** (§5: AgentProgress slot,
   Exploration section, child spaces list), PollInspector.
6. Separator (1px `border`), **Agent runs** (§6).

### CopyTitle
`h2` text-sm(14) semibold, truncate, button: click/Enter/Space copies title to clipboard
(`tx.clipboard`/rune clipboard API), hover underline (muted/50, offset 2). Tooltip "Titel kopieren" → "Kopiert" for
1.5s; Check icon 14px muted shown while copied. Timer = `Tx::wake_at(now+1.5)` event, not a thread.
Rune: `text(title).font(ts(14,600)).truncate()` in an `el()` with `.on_press`, kit `Tooltip`.

### MadeFrom
Info icon 14px muted (hover fg) → Popover side bottom, align start, offset 6, `w 288 (w-72)`, `rounded-lg p-12
text-xs`, popover fill, `shadow-md`, ring 1px fg/10. Content: "Made from" (500), prompt `max-h 240` scroll,
pre-wrap, muted; "Reuse prompt" text button (500, muted→fg) sets the composer draft to the prompt and closes.
Rune: kit `Popover` (overlay.rs:84) `.placement(Placement::BottomStart).gutter(6.)`.

### Rationale
text-xs muted. Collapsed: single line, ellipsis (`.truncate()`); expanded: `.wrap()`. Button "Read more"/"Show less"
(500, hover fg) shown only when clamped or open. Clamp detection: measure the single-line text width vs. available
(`Cx::layout_size`) — show the toggle when `natural_width > 306` (340−2·16−border). Reset to collapsed when the
selected design changes.

## 3. Composer

### Modes
`mode = "new"` (no ready selected design) · `"self"` (vary selected) · `"kids"` (revise its ready variations) ·
`"edit"` (in-place AI edit). Target state lives in the inspector: `Target::{Self_, Kids, Edit}`; derived mode:
`Edit if target==Edit && ready`, `Kids if target==Kids && child_count>0`, `Self_ if ready`, else `New`.
"Edit with AI" on a card (proto-worker-5) bumps `prompt_focus_tick` with `prompt_target=edit` → set Edit + focus.

### Layout
- **Tabs** (not in `new`): row of 3 equal buttons (`flex-1`), h 32, `rounded-t-[10px]`, border 1 without bottom,
  12.5px/500, icon 14 + gap 6:
  - "This design" (PenLine) · "Its variations" (Layers) + count pill (`rounded-full bg-muted px-6 10.5px tnum`),
    disabled (opacity .5) when 0, title "No variations of this design yet" / "Revise each variation, one revision each" ·
    "Edit" (Wand2), title "Change this design in place with AI (undo on the card)".
  - Active: border `input` + bg `#151515`, fg; inactive: transparent border, muted → fg on hover.
  - The box below overlaps by 1px (`-mt-px`) and squares the corner under the active tab: self → top-left 0,
    edit → top-right 0, kids → both rounded. Rune: `.corners([tl,tr,br,bl])`.
- **Box**: `rounded-xl(14)`, border 1 `input` (focus-within → `ring` #737373), bg `#151515`, relative.
  1. Chips rows (if any): pod chips then component chips, each `flex-wrap gap 4 px 8 pt 8`.
  2. Textarea: 3 rows, min-h 60, no resize, transparent, `px 10 pt 8 pb 4`, 13.5px leading 1.375, placeholder muted:
     - new: "Describe what to build, e.g. landing page for a climbing gym…"
     - kids: "What should change in all of them? e.g. use Lucide icons…"
     - edit: "What should change? e.g. make the save button pill-shaped…"
     - self: "How should it vary? e.g. bolder hierarchy, try a dark theme…"
  3. Bottom bar: `border-t p-6 gap 6` items-center:
     - kids/edit: label text-xs muted px 4: "One revision each" / "Replaces this card"; else **CountPicker**.
     - **History** button (only if history non-empty): 26×26 rounded-md, History 14px, muted, hover bg muted.
       Menu align end, w 288: label "Recent prompts", up to 10 items (History icon + truncated text, title = full);
       pick → replaces draft.
     - **@** button (AtSign) and **#** button (Hash), 26×26 same style, `aria-expanded` → bg muted + fg.
       Mouse-down must not steal the field's focus/caret.
     - **Send** (ml-auto): 32×32 circle, brand fill + inset highlight, ArrowUp 16 white, active scale .95,
       disabled opacity .5. Tooltip "`{sendLabel}` · ↵". sendLabel: new "Generate N direction(s)", kids "Revise N
       variation(s)", edit "Edit this design in place", self "Propose N variation(s)".
     - **Agent badge**: 18×18 circle at the send button's bottom-right (−6,−6), border 2 `#151515`, bg muted,
       agent icon 10px. Menu side top, align end, w 176: label "Who works on it", radio items
       Claude ("Queued for Claude Code via the watcher") / Codex ("Codex CLI generates the designs directly").
  4. Mention listboxes (§4) anchored under the box.

### Behaviour
- Draft per key (`design.id` or `"new"`), persisted with app state (`drafts`), restored on selection change.
  History: `promptHistory`, newest first, dedup, max 30 (`rememberPrompt` on send).
- Enter sends (when `canSend`), Shift+Enter newline, not during IME composition. Mention list open → its keys win.
- canSend: edit → text && !design.editing; kids → text; else text || ready.
- Send:
  - edit → `start_ai_edit(design, text, agent, podContext, components)`; error string → edit error line.
  - kids → one `generate` per ready child: `{spaceId: child.space, parentId: child.id, kind: variation,
    label: "<text[..40]>: <child.title>", request {instruction, count 1, baseHtml/baseTitle of child, agent, podContext, components}}`.
  - else → `generate {spaceId: active, parentId: ready? design.id, kind: ready? variation : page, request
    {instruction, count, baseHtml/baseTitle if ready, agent, podContext, components}}`.
  - Then clear draft, mentions, fields-off, component ids. Not on the main canvas (`canCreateIn` false → no-op).
  - Generation calls go through proto-ai / app state (proto-worker-7/4 API); composer only builds the request.
- Agent choice persisted (`agentChoice`, default `claude`). Count (1–6, default 3) is local, not persisted.
- Focus: `prompt_focus_tick` change → focus the field (`TextEdit.focused = true`).

### CountPicker
Group `inline-flex gap 1 rounded-md(8) bg fg/5 p 2`, six buttons 20×22, radius 5, 11.5px tnum; selected:
bg primary `#e5e5e5`, fg `#171717`, 600; others muted → fg hover. Cursor ew-resize. Press-and-drag scrubs: pick
the button whose centre is closest to pointer x (pointer capture). Keys ←/↓ −1, →/↑ +1, Home 1, End 6 (roving focus).
Rune: own widget (`composer/count.rs`), drag via `.on_drag`/pointer events, `Cursor::EwResize`.

### Rune mapping
- Field: kit `TextArea::new(&s.composer.edit).width(None).rows(3)` (or `.auto_grow(3, 10)` — reference is fixed
  3 rows without growth; keep `rows(3)` + internal scroll), `.placeholder(..)`, `.on_edit(|s| &mut s.composer.edit)`,
  `.on_submit(send)` (plain Return submits, Shift/Option+Return newline; text.rs:611), `.on_key(mention keys)`.
  Restyle to the reference: no own bg/border (the box draws them) — if TextArea's shell can't be made transparent,
  draw the box as the TextArea itself and put chips/bottom bar outside → check in the kit-gallery first.
- Key the field `"composer"`; popups anchor to `"composer-box"`.
- Tabs/menus: kit `Menu` (+ `MenuItem.checked`) for history and agent; `Tooltip` for tooltips.
- Send: `IconButton::new(&ARROW_UP)` won't take the brand inset highlight directly → hand-built `el()` circle with
  `.fill(brand).box_shadow(inset).press(..)`.

## 4. Mentions (@ pod data, # components)

Shared state machine (`composer/mention.rs`), one instance per trigger:
```rust
enum Mention { Closed, Open { start: usize /*byte of trigger*/, query: String, active: usize } }
enum MentionEv { Edited { text, caret }, Clicked { caret }, Blur, Key(KeyEvent), Pick(id), Button }
```
- Detect on edit/click: regex on `text[..caret]`: `(^|\s)@([^\s@]*)$` (resp. `#`, `[^\s#]`). Match → Open with
  `start = caret - query.len() - 1`; query change resets active to 0. No match / blur → Closed.
- Keys while open: ↓/↑ cycle (wrap), Enter/Tab pick active (only if options), Esc close. Consumed keys don't send.
- Backspace with caret at 0 and no selection, ≥1 chip → remove last chip (pod first? no: each machine checks its own
  list; @ handler runs first, so the last *pod* chip goes first; keep that order).
- Pick: remove `trigger+query` from the text (caret → start), append id to mentions (dedup), close, refocus field.
- Button: if open → close; else insert trigger at caret (prefix a space if the char before isn't whitespace), caret
  after it, open with empty query.
- `@` and `#` never open together.

Chips: **chips sit above the text**, not inline (reference). Rune has inline pills (`edit.insert_pill`, `Mark::Pill`),
but keep reference behaviour: separate `row().flex_wrap().gap(4.)` above the field. Chip: h 24, radius 8, pl 6 pr 2,
12px/500, kind icon 12, truncated label, remove button 18×18 (X 12) radius 4, opacity .8 → 1, hover bg fg/10.

### 4.1 @ listbox (pod data)
Anchored below the box (`top: 100% + 6`, full box width), max-h 380 scroll, rounded-lg(10), border, popover fill,
p 4, shadow-lg. Header row: AtSign 12 + query (bold fg) or "Pod data" muted, border-b, `px 8 pt 4 pb 8 mb 4`.
States: no catalog → "Pod catalog unavailable (<err>)" or "Loading pod data…"; empty → "No pod data found".
Groups in order Resources, Containers, "Shapes · types", Pods (11.5px/500 muted label, `px 8 pt 6 pb 2`).
Row: grid `[22px | 1fr | auto]`, gap-x 8, `px 8 py 6`, radius 8; active → bg accent `#262626`; used → opacity .55;
icon tile 22×22 radius 8 kind tone; label 13px/500 truncate; AccessBadge; second line col-span 2: path or source,
mono 11px muted truncate. Hover sets active, click picks.
- Kind tones (dark): resource sky-500/12 bg + sky-300 fg; container amber-500/14 + amber-300; shape emerald-500/12 +
  emerald-300; pod violet-500/12 + violet-300. Icons FileText / Folder / Shapes / Server.
- AccessBadge: radius 4 px 6 10.5px/600 lh 16: live (emerald-500/14, emerald-300), 401 (amber-500/16, amber-300,
  Lock 10), reference / cache (muted bg, muted fg). Labels: live, 401, reference, cache.
- Catalog: `GET /api/pod-catalog` once per app run (proto-server :3124 or proto-core directly), retried on next open
  after failure; off the UI thread. Match: query in label, path or group label (case-insensitive), ordered by kind.
  `sourceOf`: pod display root + "/" + path (shape: `root · path`).

### 4.2 # listbox (components)
Same shell; header Hash + query or "Components". Empty: "No matching cards in this space". Groups "Components"
(component=true) then "Other cards". Row: icon tile (Component icon, fuchsia-500/12 + fuchsia-300 for components;
LayoutTemplate on muted for others), title 13px/500 truncate, `from` (parent space name) 11px muted max-w 112 truncate.
Options: designs in the space chain (current space up to, excluding, root; just root when at root), status ready,
non-empty html, title contains query; sort component first, then nearer space, then title. component =
`design.component || request.scope=="component" || kind=="spoke"`. Chips use fuchsia tone + Hash icon. Chips of
deleted cards drop out automatically. Request carries `components: [{id, title, html}]` (current html).

### 4.3 Rune mapping
Kit `PopupList::new("mention-pop", "composer-box").anchored("composer-box", Side::Bottom, Align::Start, 6.)
.width(box_w).max_height(380.).items(..).active(Some(i)).on_active(..).on_choose(..).on_close(..)`
(popup_list.rs:90; takes no focus — drive `active` from the TextArea `on_key`, as kit-gallery stories.rs:227).
PopupList rows are 28pt single-line with `description`; the reference's two-line rows + group labels + header row
need either `PopupItem.leading/trailing/description` (check fit) or an own list in `kit::popover::anchored(..)` +
`raised(..)`. Start with PopupList; fall back to custom if two-line rows don't fit.

### 4.4 Flows into generation (pod context panel)
Below the composer, `gap 8`. Head: Database 14 muted, "Flows into generation" 12px/500 muted (grow), summary 11.5px
muted tnum: "N field(s) · M resource(s)[ · K shape(s)]" (fields = enabled fields; resources = mentioned resources +
resource children of mentioned containers; shapes = mentioned shapes).
Per entry card: radius 10 border, overflow hidden.
- Head: bg muted/45, `px 10 py 8`, grid [22 | 1fr | auto]: kind tile, label 12.5px/600 wrap, AccessBadge; line 2:
  "<Kind one> · <source mono>[ · stand]" 11px muted wrap. Kind one: Resource / Container / "Shape · type" / Pod.
- Fields list `py 4`: per field grid [16 | ≤108 | 1fr] gap 6 `px 10 py 4` 12px: Checkbox (kit `Checkbox`), predicate
  mono 11px muted, value (`v ?? type`). Unchecked → row opacity .45. Key `"<entryId>:<index>"`; unchecked keys go to
  `fields_off`; unmentioned entries' keys are pruned.
- Children (≠ self): read-only PodChips (h 22, 11.5px/500, kind tone) `flex-wrap gap 4 px 10 py 4 pl 32`.
- Note: Lock 12 + text 11.5px amber-300 `px 10 pb 8`.
- podContext sent = mentioned entries with enabled fields only (+ relations, note, stand).

## 5. Nested inspector (explorations / projects)

Order: AgentProgress slot (worker-8), Exploration section, child spaces list.

**Exploration section** — only when the active space has a parent and `canPromoteTo(parent)` (i.e. inside an
exploration, not a project on the main canvas). Separator, then `gap 12`:
- Label row: Layers 14 (primary) + "Exploration" (Label: 14/500), Info 14 muted at ml-auto with tooltip
  "Make a mess here. Promote the strands worth keeping up to <parent name>." (max-w 240).
- Stats grid 4 cols gap 6: Designs, Starred, Promoted, Nested. Tile: radius 10 border bg muted/30 `px 8 py 6`;
  value 14/500 tnum; label 10px muted. Promoted = designs here whose copies live in the parent (via `promotedFrom`);
  Nested = descendant space count.
- Selected design in this space: already promoted → outline button full-width "Promoted · show in <parent>"
  (ArrowUpFromLine primary) → show copy; else default (primary) button "Promote “<title>”", disabled unless ready.
- Secondary button full-width "Promote starred" (Star) + count at right (12px tnum .7), disabled when none
  (starred, ready, not yet promoted). Hint when 0 starred: "Star designs on the canvas to promote them in bulk." 12px muted.

**Child spaces** — children of the active space by createdAt. Separator, `gap 6`, Label "Projects" (root) or
"Explorations here".
- Grouping (not at root, and >1 distinct project folder among children): per folder (first-appearance order) a group
  header `px 8 pt 4` 11px muted: FolderGit2 12, folder name (500), `~`-path mono truncate (title = full), PodLink
  ml-auto; then rows; then "Other" group for spaces without a folder. Otherwise a flat list.
- Row: hover bg muted radius 8; button `px 8 py 6 gap 8` 12px: icon 14 muted (FolderGit2 at root, else Layers),
  name truncate (+ at root: path line mono 11px/16 muted truncate), count (subtree designs) tnum muted ml-auto,
  ArrowRight 12 visible on hover only. Click → enter space. At root: PodLink at the row's right (mr 6).
- PodLink: Database 12, p 2 radius 4, muted → fg, hover bg muted, opens `pod.doc` in the browser
  (`tx.open_url`), tooltip "<subject> in <doc>".
- Data: project path = space `localPath` with ancestor inheritance (proto-core query); pod record per path
  (reference `usePodRecords`; source to confirm with proto-worker-2: pod-catalog / projects API). `tildePath`:
  `^/(Users|home)/<name>` → `~`.

## 6. Agent runs
Label "Agent runs" (14/500), then up to 8 jobs newest first, `gap 6`, row `gap 8` 12px: status dot 8px
(running: blue-500 pulsing; done: emerald-500; error: destructive) — or, for a running Claude-queue run, the
ClaudeMinion "working" animation (16×12) in place of the dot. Label truncate (`label || "Variations"`), tooltip error
or label. Pulse: a rune-motion loop that only polls while a running job exists.

## 7. ⋯ More menu (header) with Decisions
Trigger: ghost icon button (icon-sm ≈ 28) MoreHorizontal, "More actions"; spinner (Loader2) while an import runs.
Menu align end, w 256:
- Group of feature items: **Decisions** (Vote icon) with status at right (12px muted truncate):
  "<n> in poll · <m> shared" or "Share a poll". Click → opens the Decisions popover anchored to the ⋯ button
  (popover content owned by proto-worker-8's polls UI; I expose the anchor key `"more-button"`).
- Separator, label "Import bundle" (12px muted), then on open fetch `/api/bundles`: Loading… (disabled) / "No bundles
  yet" (disabled) / per bundle FolderInput + name + count (tnum muted) → import, toast "Imported N designs from <name>".
Rune: kit `Menu::new("more").trigger(..).item(MenuItem::new("Decisions").icon(..).badge/trailing(status))
.separator().group_label("Import bundle")..build(&k, &mut layer)`; the trigger sits in proto-worker-4's header
→ I provide `more_menu(s, k, layer) -> El` and worker-4 places it.

## 8. State + files (proto-app)
- `inspector/mod.rs` — `InspectorState { target, edit_error, title_copied_until, rationale_open: Option<DesignId>,
  made_from_open, fields_off, pod: PodCatalogState(Loading|Ready|Failed) }`, `view()`, `INSPECTOR_OVERLAY`.
- `inspector/details.rs` (title, made-from, rationale), `inspector/nested.rs` (§5), `inspector/runs.rs`,
  `inspector/more_menu.rs`, `inspector/style.rs` (tokens, GLASS_PANEL builder), `inspector/icons.rs`.
- `composer/mod.rs` — `ComposerState { edit: TextEdit, count, pod_mention: Mention, comp_mention: Mention,
  pod_ids, component_ids, history_open, agent_open }`, view + send; `composer/mention.rs` (machine + tests),
  `composer/listbox.rs`, `composer/count.rs`, `composer/pod_context.rs`.
- Persisted via app state (worker-4): `drafts`, `prompt_history`, `agent_choice`.
- Each file < 800 lines; machines (`Mention`, count drag, catalog load) as `enum State` + `step`.

## 9. Verification
- Scenes (`AppSpec::scenes`): `inspector-empty`, `inspector-design` (title, made-from, long rationale),
  `composer-tabs-kids`, `composer-mention-pod` (open @ list with query), `composer-mention-comp`,
  `composer-chips` (2 pod + 1 component chips, context panel), `inspector-exploration`, `inspector-root-projects`.
- `--snap` each at 1440×900 dark, compare side-by-side with the reference at :3123 (screenshots of the same state).
- Unit tests: mention detect/pick/button insertion/backspace; count scrub mapping; podContext/fields_off pruning;
  component option ordering; send request shapes per mode.

## 10. Parity gaps (known)
- backdrop-saturate(1.3): not available → dropped.
- Dashed border (root note): available (`El::border_dash`), used.
- Lucide icons: hand-imported statics.
- Chips above text (reference) vs. Rune inline pills: reference kept; inline pills an optional later improvement.
