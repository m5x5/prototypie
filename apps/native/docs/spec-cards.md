# proto-app cards + fullscreen: spec

Owner: proto-worker-5 (`crates/proto-app/src/cards/`, `src/fullscreen/`). Source of truth: the reference app
`apps/web` as of 2026-11-18: `src/components/canvas/design-node.tsx`,
`src/features/nested/{design-actions,portal-node}.tsx`, `src/components/fullscreen-dialog.tsx`,
`src/store/ui.ts` (morph), `src/components/design-preview.tsx` (thumbnail rules), `src/components/ui/{button,badge,dialog}.tsx`
and `src/app/globals.css` (tokens). Desktop only: the reference's `max-md:` (phone) variants are left out.

All sizes are **world units** (= CSS px at zoom 1); the canvas camera scales them. Colours are the **dark** theme
(the app is dark-first); light values are in §1 where they differ. `≈` marks an oklch → sRGB conversion.

Rune: checkout `a checkout of anomalyco/rune` at the pinned rev 4a2e437; API names below were checked against
it (file refs are `crates/rune/src/…` unless noted). §7 maps every piece to Rune builders.

---

## 1. Tokens

| token | dark | light | used for |
|---|---|---|---|
| `background` | `#0a0a0a` (oklch .145) | `#ffffff` | fullscreen page |
| `card` / `popover` | `#171717` (oklch .205) | `#ffffff` | card surfaces |
| `fg` | `#fafafa` | `#0a0a0a` | text |
| `muted` / `secondary` / `accent` | `#262626` (oklch .269) | `#f5f5f5` | Vary button, pills, preview backdrop |
| `muted_fg` | `#a1a1a1` (oklch .708) | `#737373` | labels, counts, icons |
| `primary` | `#e5e5e5` (oklch .922; **near-white in dark**) | `#171717` | selection outline/ring, "Promoted", "Main" on, current step pill |
| `primary_fg` | `#171717` | `#fafafa` | text on primary |
| `border` | white 10% | `#e5e5e5` | hairlines |
| `input` | white 15% | `#e5e5e5` | outline-button border |
| `ring` | `#737373` (oklch .556) | `#a1a1a1` | focus ring (3px at 50%) |
| `destructive` | `≈#ff6467` (oklch .704 .191 22.2) | `≈#e7000b` | error state |
| brand | `#1447e6` + white text + inset `4 4 17 0 white/.28` | same | Implement (idle) |
| blue-300/400/500/600 | `≈#8ec5ff` / `≈#51a2ff` / `≈#2b7fff` / `≈#155dfc` | | portal "new"/"generating" |
| amber-400 | `≈#ffb900` | | star fill |
| emerald-400 / 700 | `≈#00d492` / `≈#007a55` | | "In poll" text (dark / light) |

Radii: `--radius` 10. `rounded-md` 8, `rounded-lg` 10, `rounded-xl` 14, `rounded-4xl` 26 (badges → pill),
buttons `sm`/`icon-xs`/`icon-sm` = 8, `xs` = 8.
Font: Google Sans Flex (sans), Geist Mono (mono). `text-xs` 12/16, `text-sm` 14/20, `text-base` 16,
`text-[0.8rem]` 12.8, `text-[11px]`, `text-[11.5px]`, `text-[10px]`. Weights: normal 400, medium 500, semibold 600.
Default transition (`transition-*` utilities): 150 ms `cubic-bezier(.4,0,.2,1)`.

Shadows:
- design card: `0 8 30 0 black/.40` (light: `/.10`); selected `0 12 36 0 black/.50` (light `/.16`).
- `shadow-sm`: `0 1 3 0 black/.1` + `0 1 2 -1 black/.1`. `shadow-lg`: `0 10 15 -3 black/.1` + `0 4 6 -4 black/.1`.

### Shared controls (from `ui/button.tsx`, `ui/badge.tsx`)

Button base: inline row, centred, 1px transparent border, `text-sm` medium, no wrap; focus-visible: border `ring`
+ 3px ring `ring/50`; active (not a menu trigger): translate y +1; disabled: opacity .5, no pointer events.
Icons default 16, `sm` 14, `xs`/`icon-xs` 12 unless the call site sets a size.

| size | box | padding x | gap | text | radius |
|---|---|---|---|---|---|
| `sm` | h 28 | 10 | 4 | 12.8 medium | 8 |
| `icon-xs` | 24×24 | – | – | – | 8 |
| `icon-sm` | 28×28 | – | – | – | 8 |
| `default` | h 32 | 10 | 6 | 14 | 10 |

| variant | rest | hover |
|---|---|---|
| `ghost` | transparent | bg `muted/50` (dark), `muted` (light); text `fg` |
| `secondary` | bg `secondary`, text `fg` | secondary mixed 5% toward `fg` |
| `outline` | border `input`, bg `input/30` (dark) | bg `input/50` |
| `brand` | bg `#1447e6`, white, inset highlight | bg 90%; active 75% |

Badge: h 20, px 8 (py 2), pill radius, `text-xs` medium, gap 4, icons 12. Variants: `secondary` (bg `secondary`),
`outline` (border `border`, text `fg`).
Tooltips: every tooltip in this spec uses the app tooltip (600 ms initial delay, neighbours instant).

---

## 2. Design card (`design-node.tsx`, node type `design`)

Width `NODE_W = 380`; height is content-driven (layout uses `NODE_H = 420` as an estimate). Two parts stacked:
a **title label** above, on the canvas (no surface), and the **card surface**.

### 2.1 Title label (h 28: 24 content + 4 bottom padding; px 2; row, items centred, gap 6)

1. Title: `text-xs`, `muted_fg`, single line, truncate with ellipsis; tooltip = full title. **Selected**: medium
   weight, `fg` (150 ms colour transition).
2. Kind badge, only when `kind != "variation"`: `secondary` badge, h 16, px 6, `text-[10px]`, capitalised
   (`Page`, `Spoke`, `Component` …).
3. Open-notes badge (Annotate), only when the design has unresolved notes: `outline` badge,
   `MessageSquareText` 12 + count; tooltip "N Annotate note(s)".
4. Notes icon, only when `design.notes` is non-blank: `NotebookPen` 14, `muted_fg`; tooltip = the note text.
5. Right-aligned (margin-left auto), `muted_fg`, no gap, three `ghost icon-xs` buttons with 14 icons:
   - `Maximize2` "Open fullscreen": disabled unless `status == ready`; opens fullscreen (§4.1).
   - `Star` / "Unstar": toggles `starred`; starred = icon filled and stroked amber-400.
   - `Trash2` "Delete": `removeDesign(id)` immediately (no confirm).

### 2.2 Surface

- Radius 12, 1px `border`, fill `card`, shadow per §1; outline 2px at offset 2, transparent at rest.
- **Selected**: shadow → selected shadow, outline → `primary/60` (`#e5e5e5` at 60%). Both transition 150 ms.
- Connection handles: target dot top centre, source dot bottom centre, fill `muted_fg` (React Flow default size
  6×6 with a 1px border; the canvas engine owns edges, the card only reports its anchor points).
- The preview area clips to radius 11 on the top corners only (the surface itself isn't clipped, so the handles
  on its edges stay whole).

### 2.3 Preview area (by `status`)

`vw = viewport ?? 1280`, `vh = viewportHeight` (optional). Inner width = 378 (380 − 2 border).

**ready**: a button-like region; **double-click opens fullscreen** (§4.1). It shows the thumbnail snapshot
(proto-web) sized by:

| case | box |
|---|---|
| `vh` set, `vh > vw` (portrait screen) | height 420, width `420·vw/vh`, centred horizontally |
| `vh` set, landscape | full width, aspect `vw:vh` |
| no `vh`, `vw < 600` (phone) | height 420, aspect 9:17 (width ≈ 222), centred |
| otherwise | full width, aspect 4:3 (378 × 283.5) |
| … but "tall content" (see below) | full width, aspect `fit.w : min(fit.h, 3·fit.w)` |

Backdrop behind the image: `muted` for non-phone; transparent for phone (phone pages also drop their own page
background in thumbnails). The image scale = box width / `vw` (fixed screens: `min(w/vw, h/vh)`), anchored top-left.

Thumbnail rules the snapshot must follow (proto-web renders them; listed so the card can ask for them):
- Animations frozen at their **end** state, transitions off, scripts stopped ~1.2 s after load; DialKit chrome hidden.
- **Content crop** (not for fixed `vh` screens, not for phones): measure the bounding box of all elements narrower
  than 90% of `vw` (plus any img/svg/canvas/video); if its width ≤ 60% of `vw`, crop to it with 24 px padding
  (`fit = {x, y, w, h}`), drop the page's background-image, and scale so the whole crop fits when that keeps it at
  least half the fill-width scale, else fill the width from the top. Never scale above 1:1. Content that is
  tall (`fit.h / fit.w > 1.5`) makes the box taller (the "tall content" row above).
- Snapshots load lazily: only after the card has been in view for 180 ms (100 px margin), then kept.
- Live-view cards (html with `<prototypie-slot>`) are composed with each slot's main variant before rendering.

**generating**: aspect 4:3, fill `muted/40`, column centred, gap 8: spinner (`Loader2` 20, rotating 1 s linear),
"Designing…" `text-xs` `muted_fg`.

**error**: aspect 4:3, fill `destructive/5`, padding 16, centred column, gap 8, colour `destructive`:
`AlertTriangle` 20, `design.error ?? "Generation failed"` `text-xs` (wrapping, centred), then an `outline sm`
button "Retry" (`RotateCcw`) with 4 extra top margin → `retryDesign(id)`. The button must not start a drag.

The rationale is **not** on the card (inspector and fullscreen show it).

### 2.4 Pod provenance (only when `request.podContext` is non-empty)

Top border, px 12, py 8, gap 6 rows: line `Database` 12 + "N field(s) from the pod" (`text-[11.5px]`,
`muted_fg`, N = total fields); then a wrapping row (gap 4) of pod chips (one per context entry, tooltip
"source · access · stand"). The chip component belongs to features (pod mentions, proto-worker-8).

### 2.5 Action row

Top border, padding 8, wrapping row, gap 4, items centred. Clicks here never start a canvas drag. In order:

| # | control | shown when | look | action |
|---|---|---|---|---|
| 1 | **Vary** (`Sparkles`) | always | `secondary sm` | focus the composer on this design (`focusPrompt(id)`); disabled unless ready |
| 2 | **Compare (N)** (`Columns3`) | N = ready children (`parentId == id`) > 0 | `ghost sm` | open compare dialog (features) |
| 3 | **Explore** (`Telescope`) | always | `ghost sm`, tooltip "Explore in a nested space" | create exploration seeded with this design, enter it with the seed selected, focus composer on the seed; disabled unless ready |
| 4a | **Promote** (`ArrowUpFromLine`) | parent space exists and isn't root, no promoted copy yet | `ghost sm`, tooltip "Copy up to {parent}" | `promote([id])` (+ toast); disabled unless ready |
| 4b | **Promoted** | same, a promoted copy exists | `ghost sm`, text `primary`, tooltip "Show the copy in {parent}" | jump to the copy in the parent space |
| 5 | **Implement** | ready and (has a promoted copy or is a promoted copy) | idle: `brand sm` `Code2` "Implement"; sent: ghost `Send` "Gesendet"; working: ghost, Claude minion icon "In Arbeit"; done: ghost `Check` "Erledigt" + split chevron menu "Erneut senden …" (confirm dialog) | idle → POST implement; otherwise open the task panel (features) |
| 6 | AI edit | ready: `ghost icon-sm` `Wand2` (tooltip "Edit with AI (changes this card in place)") → composer in edit mode; + `Undo2` icon-sm when `versions` non-empty; + outline destructive badge "Edit failed" (click dismisses) when `editError`. While `editing`: replaced by an outline `primary/40` badge with spinner "Bearbeite…" and an ✕ cancel | | |
| 7 | **Main** (`Crown`) | the card is in a live view's lineage, or `mainAt` set | `ghost sm`; on: text `primary`, crown filled | toggle `mainAt` |
| 8 | Guidelines | always | set: outline badge, border `primary/40`, text `primary`, `BookMarked` + "Guidelines" (click unsets); unset: `ghost icon-sm` `BookMarked` (disabled unless ready) | `setGuidelines(id, bool)` |
| 9 | "from {space}" badge (`CornerLeftDown`) | this is a promoted copy | outline badge, `muted_fg` → `fg` on hover, margin-left auto, truncates; "(deleted)" + disabled when the source space is gone | enter the source space with the source design selected |
| 10 | **Spokes** (`Boxes`) / **Merge** (`Merge`) | Merge when `kind == spoke` and its hub exists, else Spokes | `ghost sm` | Spokes: open the spokes picker; Merge: merge lane into hub (disabled unless both ready) |
| 11 | **Poll** (`Vote`) / **In poll** (`Check`) | always | `ghost sm`; in poll: `secondary`, emerald text, `aria-pressed` | toggle poll candidate; disabled unless ready (or already in) |

Rows 2, 5–11 are rendered by other features in the reference; the card owns the row's layout and asks each
feature for its controls (§5).

### 2.6 Card interactions

- Click selects (canvas engine); shift/marquee multi-select (canvas engine).
- Fullscreen from a card that is part of a multi-selection (≥ 2 designs selected) steps through the selection in
  **canvas order**: only ready designs, rows top → bottom (tops within 160 count as one row), then left → right.
- Wheel/drag over the preview pans the canvas (thumbnails are never interactive).

---

## 3. Project / exploration card (`portal-node.tsx`, node type `portal`)

Width `PORTAL_W = 300`, layout height estimate `PORTAL_H = 330`. A project is a space with `kind == "project"`
(the main canvas holds only projects); anything else is an exploration. **Double-click anywhere enters the
space.** Accessible name: "Project {name}, double-click to open" / "Exploration …".

Surface: radius 14, 1px `border`, fill `card`, `shadow-sm`. Selected: 2px ring `primary` (no offset) +
`shadow-lg`; shadow transitions 150 ms. Three blocks:

### 3.1 Header (py 10, pl 12, pr 10, row, gap 8, centred)

1. Icon chip 24×24, radius 8, fill `primary/10`, icon 14 `primary`: `FolderGit2` (project) / `Layers` (exploration).
2. Column (min-width 0): name `text-sm` medium, truncate, tooltip = name; under it, when a local folder is known,
   the path in mono `11/16` `muted_fg`, truncated, with `/Users/<name>` → `~`. Folder lookup: the space's own
   `localPath`, else the nearest ancestor's, else a name match (`GET /api/projects?match=<name>`, cached per name).
3. Generating dot, when any subtree design is generating: 6×6 blue-500 circle with a ping ring (inset −4, 1px
   blue-500 border, opacity .6, scale 1 → 2 and fade out, 1 s `cubic-bezier(0,0,.2,1)`, infinite; static with
   reduced motion). Tooltip "Generating".
4. "N new" pill, when N > 0: radius full, px 6, py 2, `text-[11px]` medium tabular; dark: bg blue-400/15, text
   blue-300; light: bg blue-600/10, text blue-700. N = the space's **own** designs that aren't generating and
   were created after this device last **left** the space (fallback when never left: created within 24 h).
   "Left" = timestamp written whenever the active space changes away from it (per device, not synced).
5. Delete: `ghost icon-sm` `Trash2` 16, margin-left auto, `muted_fg`, opacity .6 → 1 on card hover or focus;
   asks for confirmation (the nested "delete space" dialog), label "Delete project" / "Delete exploration".

No pod link on the card (parity with the reference; decided 2026-11-18): the pod link lives in the inspector's
project list (proto-worker-6).

### 3.2 Hero (px 10)

Pool: project → all subtree designs; exploration → the space's own designs.
- Empty pool: margin-x 10, aspect 16:10, radius 9, 1px `border`, fill `muted/30`, centred column gap 6,
  `FolderTree` 20 + "Empty project" / "Empty exploration" `text-xs` `muted_fg`.
- Hero choice: first **ready** design among the promotion refs (newest promotion first), else the newest ready
  design of the pool by `createdAt`. Promotion refs: project → promoted copies that live in this space;
  exploration → the source designs promoted *out of* this space.
- Hero: full width, aspect 16:10, radius 9, 1px `border`, fill `muted/30`, clipped; snapshot laid out at the
  hero's `viewport` and scaled to width (same thumbnail rules as §2.3, incl. content crop). Starred hero: an
  18×18 `card` circle with `shadow-sm` at top 8 / right 8 holding a filled amber `Star` 10.
- Pool not empty but nothing ready yet: aspect 16:10, radius 9, 1px **dashed** blue-500/50 border, fill `muted`,
  pulsing (opacity 1 → .5 → 1, 2 s `cubic-bezier(.4,0,.6,1)`, infinite; static with reduced motion).
  Tooltip "Designing…".

### 3.3 Footer (py 10, pl 14, pr 10, row, space-between, gap 8)

Left: counts row, gap 10, `text-xs` `muted_fg`, tabular. Each entry = 12 icon at opacity .8 + gap 4 + the number
in medium `fg`, with a tooltip:
- `LayoutGrid` designs (subtree count, always shown),
- `Star` starred (subtree, if > 0),
- `ArrowUpFromLine` promoted (subtree designs that are a promotion source, if > 0),
- `Layers` nested explorations (descendant spaces, if > 0).
Accessible text: "N designs, S starred, P promoted, K nested".
Right: when generating > 0, a pill (radius full, px 8, py 2, fill `muted`, `text-[11px]` medium tabular, blue-600 /
dark blue-400) "N generating".

---

## 4. Fullscreen (`fullscreen-dialog.tsx`, `store/ui.ts`)

### 4.1 State

`FullscreenState { id: Option<DesignId>, set: Option<Vec<DesignId>> }`. `open(id, set?)`: `set` is reduced to
canvas order (§2.6) and kept only when it has ≥ 2 ids and contains `id`. Steps skip designs deleted meanwhile.
Opening and closing animate (§4.4); stepping within a set doesn't.

### 4.2 Layout (fills the window; fill `background`; padding 16; column, gap 12)

- Close button: `ghost icon-sm` ✕ (16) at top 8 / right 8.
- Header (column, gap 8):
  - Title row, `text-base` medium, leading 1. With a set: first a group (gap 2) of `ghost icon-sm`
    `ChevronLeft` 16 "Previous design (←)", the position "i / n" (`text-xs` normal `muted_fg`, tabular,
    min-width 40, centred, announced politely), `ChevronRight` "Next design (→)"; title truncates; right
    padding 40 keeps clear of the close button.
  - Description: `rationale`, `text-sm` `muted_fg`, wrapping.
  - With a set: a strip of step pills, gap 4, horizontally scrollable, no scrollbar. Pill: h 28, px 10, radius
    full, 1px border, `text-xs`, gap 6, max-width 224, index (tabular, opacity .7) + truncated title, tooltip = title.
    Current: border + fill `primary`, text `primary_fg`. Others: `muted_fg`, hover fill `muted`, text `fg`.
    Focus ring 2px `ring`. Click → that design.
- Body (row, gap 16, fills the rest):
  - **Live preview** (proto-web `LiveView`, interactive, the design's real html, no animation freeze, no crop),
    1px `border`, radius 8, backdrop `muted`, laid out at width `vw`, scaled by box width / `vw` (fixed screen:
    `min(w/vw, h/vh)`):

    | case | box |
    |---|---|
    | `vh > vw` (portrait screen) | height = body height, max `vh`; width by ratio; centred both ways |
    | `vh`, landscape | fills the row width (flex 1), aligned top, max-width `vw`, max-height body, ratio `vw:vh` |
    | no `vh`, `vw < 600` | width exactly `vw` (native size), full height, centred |
    | otherwise | fills the remaining width and the full height |

  - **Side panel**, width 288, scrolls vertically, gap 12, in order:
    1. Designer note ("Notiz"): label row (`text-xs` medium) with a save status at the right
       (`text-[11px]` `muted_fg`: spinner + "Speichert…" while dirty, else `Check` + "Gespeichert" if a note
       exists). Textarea, 4 rows, min-height 96, max-height 288, vertically resizable, `text-sm`, placeholder
       "Gedanken zu diesem Design…". Autosave 600 ms after the last keystroke; also saves immediately on blur,
       on stepping to another design and on close. Blank → the note is removed. A remote change replaces the
       text only while nothing is waiting to be saved.
    2. AI edit request box (ai-edit feature; proto-worker-8 / composer, embedded here).
    3. *(Reference only, left out for now)* the notes hint: `MessageSquareText` 14 + a line telling the user to
       use the web app's notes toolbar to click elements in the design and leave notes, then apply them as a new
       round (`text-xs` `muted_fg`). Annotate in the native app (native pin mode vs. browser-only) is a pending
       user decision.
       **Seam:** the side panel takes an optional `annotations: Option<fn(&Design, &AppState) -> El>` slot
       (default `None`) rendered at this position, and `LiveView` keeps a hook for injecting a page script, so
       either choice plugs in without touching the layout.
    4. Annotate notes panel, compact (features); shown only when the annotations seam is filled.

### 4.3 Keys and focus

- On open, focus sits on the dialog itself, **not** inside the web view, so Esc and ←/→ reach the dialog.
  In Rune: intercept these keys before the WKWebView gets them (window-level key handling), or Esc stops working
  once the user clicks into the design.
- **Esc** closes. (The reference skips Esc while the browser is in fullscreen, because there Esc only exits
  browser fullscreen. Native: Esc always closes the dialog, unless a text field in the side panel has focus,
  where it first blurs it.)
- **← / →** step through the set (wrapping) when there is a set, unless a modifier is held or focus is in a text
  field (note textarea, edit box, a field inside the web view).
- Closing saves a pending note, hides the live view and returns focus to the canvas.

### 4.4 Card ↔ fullscreen morph

On **open** and **close** (not when stepping), the card's preview and the fullscreen preview are one shared
element:

- Geometry: the card preview's on-screen rect (world rect × camera) ⇄ the fullscreen preview rect (§4.2),
  animated as **translate + scale from the top-left corner** (no relayout per frame), 320 ms,
  `cubic-bezier(.2,.8,.2,1)`.
- Content: the old and new images crossfade inside the moving rect over the same 320 ms, each drawn
  `object-fit: cover` anchored at the **top**.
- Everything else (canvas ↔ fullscreen page) crossfades over 320 ms.
- During the morph the dialog's own fade/zoom and any backdrop blur are off, so nothing replays at the end.
- Open: before animating, wait up to **250 ms** for the fullscreen preview to finish loading (screen holds the
  old frame), so the animation runs on finished pixels.
- Close: the fullscreen page is hidden for the end state; the morph lands on the card.
- Skipped (instant switch) when the card isn't on screen / not found, or with reduced motion.
- Without the morph the dialog uses its default entrance: fade in + zoom 95% → 100%, 100 ms (exit mirrored).

---

## 5. Module boundaries in proto-app

- `cards/` renders both node types for the canvas engine (proto-worker-4): the engine owns camera, positions,
  selection, drag and edges; it asks `cards` for a node's element at world size and for its preview rect and
  handle anchors. Selection comes in as a flag.
- Action-row controls owned by other modules (Compare, Implement, AI edit, Main, Spokes/Merge, Poll, pod chips,
  notes badge) are provided by those modules as small builder functions `fn(&Design, &AppState) -> Option<El>`
  in the order of §2.5; `cards` owns the row's layout, wrapping and the shared button styles.
- `fullscreen/` owns `FullscreenState`, the page layout, keys, the note autosave and the morph; it uses
  proto-web's `LiveView` for the interactive preview and `Snapshots` for the morph images, and embeds the AI edit
  box and notes panel from features.

## 6. Decisions and open items

Decided by the lead (2026-11-18):
1. No pod link on the project card (parity); PLAN.md updated.
2. Annotate: pending user decision; hint left out, seam kept (§4.2.3).
3. Mobile (`max-md`) variants are out of scope (desktop macOS).

Being agreed directly with the owners:
4. proto-web (worker-3): **agreed** — pre-cropped snapshots with `rect`/`crop`, `LiveView::take_keys`, user-script
   seam (§7.2, §7.5).
5. Shell (worker-4): an `on_leave(space)` hook or route-change signal for the per-device "N new" counter (§7.4),
   and how the canvas engine asks `cards` for node elements, preview rects and handle anchors (§5).

---

## 7. Rune builder equivalents

### 7.1 General mapping (Tailwind → Rune `El`)

| reference | Rune |
|---|---|
| `flex` row / column, `gap-*`, `p*-*`, `items-center`, `justify-between`, `flex-wrap`, `ml-auto` | `row()` / `col()`, `.gap(g)`, `.px/.py/.pl/.pr/.pt/.pb`, `.center_y()` / `.items_center()`, `.justify_between()`, `.flex_wrap()`, `spacer()` or `.ml(..)` (el/layout.rs) |
| `w-*`, `h-*`, `aspect-[a/b]`, `min-w-0`, `flex-1` | `.w/.h/.size`, `.aspect_ratio(a/b)` (layout.rs:44), `.min_w(0.0)`, `.grow(1.0)`. Nested scroll columns need `.min_h(0.0)` on each column (README gotcha) |
| `absolute top-2 right-2`, `inset-0` | `.absolute(l, t)` or `.pin(Some(l), Some(t), Some(r), Some(b))` |
| `bg-*`, `rounded-*`, `border`, `border-dashed` | `.fill(c)`, `.radius(r)` / `.corners([tl,tr,br,bl])`, `.border(1.0, c)`, `.border_dash(dash, gap)` (visuals.rs:118) |
| `outline-2 outline-offset-2` (card selection) | `.outline(2.0, 2.0, c)` (visuals.rs:231) |
| `ring-2` (portal selection) | `.outline(2.0, 0.0, c)`, or a zero-blur outer `Shadow { spread: 2.0, .. }` |
| `shadow-[…]`, multi-layer | `.box_shadow(Shadow { x, y, blur, spread, color, inset })` once per layer (blur = CSS blur); the first four layers spring between states |
| brand inset highlight | `style::brand_highlight()` (already in `app/style.rs`) |
| `overflow-hidden rounded-t-[11px]` | `.corners([11,11,0,0]).clip_corners([11,11,0,0])`; whole-radius clip: `.clip_rounded(r)` (visuals.rs:453) |
| `opacity-*`, `group-hover:opacity-100` | `.opacity(o)`; parent `.hover_group()` (visuals.rs:497) + child `.group_opacity(0.6, 1.0)` (visuals.rs:537) |
| `hover:bg-*`, `active:translate-y-px` | `.hover(color)`, `.press(color)` overlays (no view re-run); `.press_scale(..)` or `.offset(0,1)` while pressed |
| `focus-visible:ring-3 ring/50` | `.focusable(true).focus_ring(3.0, 0.0, ring.alpha(0.5))` (focus.rs:221) |
| `disabled:opacity-50 pointer-events-none` | `.dim(0.5)` + no `on_press` (or `.inert()`) |
| `truncate`, `title=` tooltip | `text(s).single_line().truncate()` (text.rs:306) + `.help("full title")` (help.rs:22, native tooltip) |
| `tabular-nums` | `TextStyle::new(size, weight).features("'tnum' 1")` (tok.rs:114) |
| `font-mono` | `TextStyle::new(..).mono()` |
| lucide icons | `rune::Icon { name, view: 24.0, body: "<path …/>" }` with lucide's path bodies (stroke 2, `currentColor`), drawn with `icon(&I, 14.0).color(c)`. Keep them in one `cards/icons.rs` (or a shared `app/icons.rs` if worker-4 starts one) |
| `animate-spin` / `animate-ping` / `animate-pulse` | `.looping(LoopSpec)` on rotation / scale+opacity / opacity (motion.rs:281); off under reduced motion |
| `transition-[box-shadow,outline-color]` 150 ms | implicit: changed fills/shadows/outline animate on the element's `.motion(Spring)`; use `tok::motion::SNAP` (.25) as the closest token, or `Spring::new(0.15, 0.0)` |
| `onClick`, `onDoubleClick` | `.on_press(|s| ..)` / `.on_press_tx`, `.on_double_press(|s| ..)` (handlers.rs:40). Presses go to the topmost hit with a handler, so a button inside the card never also fires the card: no `stopPropagation`, no `nodrag` needed beyond giving controls their own handlers |
| cursor | `.cursor(Cursor::Pointer)` |

### 7.2 Images (thumbnails, hero)

- proto-web hands back `rune::Pixels` (`docs/api-web.md`, `Snapshots::request(design_id, rev, html, viewport)`).
  Draw: `el().w_full().aspect_ratio(4.0/3.0).fill(muted).pixels_fit(&p, SurfaceFit::Fill).surface_anchor(Align::Start, Align::Start)`
  (visuals.rs:223, 680). Top-left anchoring + Fill = the reference's "scale by width, anchored top".
- **Agreed with proto-worker-3 (2026-11-18; authoritative shapes in `docs/api-web.md`):**
  `Snapshots::new(wake, limits)`; `request(design_id, rev, html, ThumbSpec { viewport_w, viewport_h: Option, out_w })
  -> Thumb { Pending | Ready(Snapshot) | Failed(String) }` (latest-wins per id, rev-invalidated, repeat calls are cache
  lookups); `poll() -> bool` from `AppSpec::poll`. `Snapshot { pixels, rect, crop: Option<[f32;4]>, doc_h }`.
  proto-web applies the §2.3 box rules and page prep itself and captures exactly the area the card shows
  (fixed screen vw×vh; phone vw × vw·17/9, transparent; crop box incl. tall content up to 3·w; else vw × vw·3/4).
  `rect` = what the pixels cover (viewport CSS px). Cards draw them `Fill` anchored top-left and apply only the
  scale rule (never above 1:1; whole crop if that keeps ≥ half the fill scale). The 180 ms lazy rule stays in cards.
  `out_w` = box width × camera zoom × backing scale (proto-web caps at 2 px per CSS px): a snapshot serves requests
  up to 1.25× its width; larger ones re-render in 256 px buckets while the old picture keeps showing, so re-request
  on zoom changes freely.
- Rounded clipping of the image: put `.radius(r).clip_rounded(r)` on the parent (the `matched` example does this).
- Lazy loading: request the snapshot only once the card has been visible for 180 ms: `.on_visible(|s, on, tx| ..)`
  starts a timer (`tx.wake_after(180ms)`), `poll` issues the request. Model it as an explicit state machine
  (`Thumb { Idle, Waiting(deadline), Requested, Ready }`, AGENTS.md: no bool clusters).

### 7.3 Design card skeleton

```rust
// cards/design.rs  (sketch; real code < 800 lines per file)
pub fn design_card(d: &Design, sel: bool, s: &AppState, k: &Kit, p: &Palette) -> El {
    col().key(("design", &d.id)).w(NODE_W)
        .child(title_label(d, sel, p))                       // h 28, row, gap 6
        .child(
            col().radius(12.0).border(1.0, p.border).fill(p.card)
                .box_shadow(if sel { CARD_SHADOW_SEL } else { CARD_SHADOW })
                .outline(2.0, 2.0, if sel { p.primary.alpha(0.6) } else { Color::TRANSPARENT })
                .child(preview(d, s, p))                      // §2.3, clipped top corners 11
                .children(pod_provenance(d, s))               // §2.4
                .child(action_row(d, s, k, p)),               // §2.5, .flex_wrap().gap(4).padding(8), top rule
        )
}
```

- The **preview element** carries `.matched(format!("fs:{id}"))` (§7.5) and `.on_double_press(open_fullscreen)`.
- Handles: the card reports anchor points (top centre / bottom centre of the surface) to the canvas engine; it
  doesn't draw edges. If the engine wants the dots drawn by the card: 6×6 `muted_fg` circles, `.absolute(..)`.
- Buttons: hand-built to match §1 exactly (rune-kit `Button`/`Badge` use opencode metrics: badge h 16, radius 2),
  so `cards/controls.rs` holds `btn_sm(label, icon, variant)`, `icon_btn(icon, size)`, `badge(..)` with the
  §1 tables. Other modules' action-row controls use the same helpers (export them).

### 7.4 Project card skeleton

- Surface: `col().w(PORTAL_W).radius(14.0).border(1.0, p.border).fill(p.card).box_shadow(SHADOW_SM…)`; selected:
  `.outline(2.0, 0.0, p.primary)` + `SHADOW_LG` layers. Card `.hover_group()` so the delete button uses
  `.group_opacity(0.6, 1.0)`; the whole card `.on_double_press(enter_space)`.
- Hero: `.aspect_ratio(16.0/10.0).radius(9.0).clip_rounded(9.0).border(1.0, p.border)`; shimmer:
  `.border_dash(..)` with blue-500/50 + opacity `looping` 1 → .5 (2 s), static under reduced motion.
- Generating dot: 6×6 circle + a looping ring (scale 1 → 2, opacity .6 → 0, 1 s).
- "N new": needs per-device "last left" timestamps. Keep them in app-local prefs (not the synced store);
  write when `route.space` changes away (hook in `AppState::enter`, owned by worker-4: ask for an `on_leave`
  callback or let cards observe route changes in `poll`).
- Local path: `proto-core` query (`localPath` + ancestor inheritance, per PLAN) + name match through
  `/api/projects?match=` equivalent (proto-server/core function), cached per name, fetched off the UI thread.

### 7.5 Fullscreen + morph

- Page: a full-window layer `col().pin(Some(0.),Some(0.),Some(0.),Some(0.)).fill(p.background).padding(16).gap(12)`
  mounted at the root above the canvas, with `.modal(false, |s| s.fullscreen.close())` (focus.rs:263: traps Tab,
  Esc dismisses, background inert, focus returns to the opener). App-level `AppSpec::on_key` handles ←/→ (and
  Esc as a backstop) so they work even when the web view holds focus (see below).
- **Morph = `El::matched`** (motion.rs:231; README ~1099; `examples/matched`): the card's preview and the fullscreen
  preview box carry the same id `fs:{design_id}`. In the commit that opens fullscreen, the card **stops rendering
  its matched preview** (renders a plain placeholder of the same size, e.g. `fill(muted)` without `.matched`) and
  the fullscreen box mounts with the id; on close the reverse. Rune then flies the box from the card's on-screen
  rect (camera scale included, since it starts "where the old one was seen") to the fullscreen rect, children
  scaled to cover, drawn above the window. Children = the snapshot `pixels_fit(Fill)` anchored top, which is the
  reference's `object-fit: cover; object-position: top` crossfade target.
  - Timing: the reference uses 320 ms `cubic-bezier(.2,.8,.2,1)`; matched uses a spring: commit with
    `tx.motion(Spring::new(0.32, 0.0))` (≈ `tok::motion::GLIDE` .32).
  - The rest of the page fades: fullscreen layer `.transition(Transition::opacity())` (entrances play only in
    input-caused commits; call `tx.animated()` when opening from code).
  - Reduced motion: Rune skips matching and crossfades in place (0.15 s), the reference's instant switch.
  - Stepping through a set: change only the content, no matched id swap (key the box by the *set*, not the design,
    e.g. `fs:set`, or drop `.matched` while stepping).
  - The 250 ms "wait for the page to load" step is unnecessary: the flight shows the already-loaded snapshot.
- **Live web view** (proto-web `LiveView`, a WKWebView child NSView; agreed API: `LiveView::attach(&WindowHandle)`
  in `on_window`, `prepare(&LivePage, LiveOptions)`, `show(rect, &LivePage, LiveOptions)`, `move_to(rect)`, `hide()`
  (page stays loaded), `is_loaded()`, `take_keys() -> Vec<LiveKey::{Escape, Left, Right}>`; `LivePage { key, rev,
  html, viewport_w, viewport_h }`; `LiveOptions::default().user_script(js)` = the Annotate seam; pageZoom =
  rect.w / viewport_w). Call `prepare` when the open flight starts so `show` at landing finds it loaded; drain
  `take_keys` in poll. Native subviews draw **above** all Rune content, flights and popovers included. So:
  - Show it only after the open flight has landed: track the box with `El::drawn_rect_into(&Live<[f32;4]>)`
    (layout_watch.rs:132) and call `LiveView::show(rect, html)` once the rect is stable (or after the motion
    settles, via `on_drawn_layout_tx` + `tx.live_only()`), `move_to(rect)` on resize.
  - Until then (and while it loads) the box shows the snapshot, so there's no blank frame.
  - On close: `LiveView::hide()` in the same commit that starts the reverse flight.
  - Nothing Rune-drawn may overlap the web view rect while it's shown (menus or tooltips from the side panel
    must open beside it, never over it).
  - Keys: while the WKWebView is first responder, proto-web's key monitor consumes Esc (always) and ←/→ (no
    modifier, no editable element focused in the page) and queues them for `take_keys()`, so §4.3 holds.
- Note autosave: an explicit state machine `Note { Clean, Dirty { text, due }, Saving }` with
  `tx.wake_at(due)` (600 ms), saved off the UI thread through the store; flush on blur/step/close.

### 7.6 Headless checks

- `--snap` scenes (`AppSpec::scenes`): `card-ready`, `card-generating`, `card-error`, `card-selected`,
  `card-phone`, `card-fixed-portrait`, `portal-project`, `portal-empty`, `portal-designing`, `fullscreen`,
  `fullscreen-set`; `--theme dark|light`.
- `--script` + `--strip` for the morph (double-click a card preview by key, then `{"type":"key","name":"Escape"}`).
- `--tree` to compare rects against §2–§4 numbers. The web view isn't in snaps (native): scenes show the snapshot.
