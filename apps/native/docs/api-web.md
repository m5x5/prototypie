# proto-web API (owner: proto-worker-3)

HTML rendering on macOS through WKWebView: offscreen **snapshots** of a design's html into `rune::Pixels`
(card thumbnails, morph images), and a live interactive **LiveView** (a WKWebView child NSView placed over a
rect of the Rune window: fullscreen preview, later live views and compare).

Everything here is **UI-thread only** (`!Send`): WebKit must be driven from the main run loop, which Rune's
event loop runs. WebKit renders in its own processes; the CGImage → RGBA conversion runs on a proto-web worker
thread. No file I/O anywhere in this crate.

Status: **implemented** (`cargo test -p proto-web`: 16 unit tests; clippy clean). Verified headlessly with real
designs from the reference state (`examples/snap.rs`, `examples/live_probe.rs`): web fonts, backdrop-filter,
content crop (an 851 px page with a 350 px sidebar → the sidebar), phone and fixed-size screens. Timing: the
first page of a process ~0.7-1 s (WebKit process spawn), then ~420 ms per thumbnail per web view (400 ms is the
settle), RGBA conversion ~10 ms on the worker. LiveView: `prepare` → loaded ~0.45 s, `show` 0.8 ms (no reload
for the same key/rev), `hide` 0.07 ms; the page lays out at `viewport_w` under the zoom (checked via `eval`).
Not verifiable headlessly: the key monitor (needs a focused window) — its decision logic is unit tested.

## Snapshots

```rust
use proto_web::{Snapshots, SnapshotLimits, ThumbSpec, Thumb, Snapshot};

// once, e.g. in on_ready (after the first frame: the first render spawns WebKit's processes)
let snaps = Snapshots::new(Arc::new(move || app_handle.wake()), SnapshotLimits::default());

// in a view build / poll, whenever a card wants its picture
match snaps.request(&design.id, design_rev, &design.html, ThumbSpec { viewport_w, viewport_h, out_w }) {
    Thumb::Pending => /* placeholder: fill muted */,
    Thumb::Ready(s) => /* draw s.pixels */,
    Thumb::Failed(why) => /* placeholder; `why` for a tooltip/log */,
}

// in AppSpec::poll: drains finished renders, true when some Thumb changed (then rebuild)
if snaps.poll() { changed = true; }
```

- `request(design_id: &str, rev: u64, html: &str, spec: ThumbSpec) -> Thumb`: take `rev` and `html` from
  **`proto_core::PreviewInputs`** (docs/api-core.md §Preview inputs): `p.key` is a content key (html hash, or
  `live_view::fingerprint` for live views), `p.html` is already composed. So a card re-renders when it or a
  component it shows changes, and not on drag/star/rename. Build it with
  `Wrap = Arc::new(|h| proto_web::spec::for_preview(h, false))` for the components. A cheap lookup; starts a render
  only when the cache has nothing usable. **Latest wins** per design id: a newer `rev` (or different spec)
  replaces the queued job; while it renders, the previous `Ready` snapshot keeps being returned (no flashing).
  Call it only for cards that should load (the 180 ms in-view rule is the caller's).
- `poll() -> bool`: call from `AppSpec::poll`. The `wake` passed to `new` fires from the worker thread whenever
  a result is waiting.
- `invalidate(design_id)`: drop a design's entry (deleted design). `clear()`: everything.
- Live-view cards (`<prototypie-slot>`): `PreviewInputs` composes them (slots → main variants); pass its `html`
  as is. `request` adds the page-level preview CSS itself (a second copy on composed parts is harmless).
- `set_base_url(Option<String>)`: where relative URLs in designs resolve (`/vendor/...`, `/imports/...`, like
  the reference's same-origin iframe). Default `http://127.0.0.1:3124/` (proto-server), overridable with
  `PROTOTYPIE_ASSET_BASE`. **proto-server must serve the reference's `public/` assets** (`/vendor`, `/imports`)
  there (proto-core copies them into `<data>/public`; proto-server serves that dir at `/`), or designs that use
  them render without those scripts/styles. Start proto-server **before** creating `Snapshots`: a thumbnail
  rendered while :3124 is down is cached without its assets until its key changes.
- Robustness: per-phase timeouts. A load stalled by a subresource is stopped after 5 s and captured as it is;
  a measure script that doesn't answer in 3 s captures with defaults; a snapshot WebKit doesn't deliver in 4 s
  fails the job (`Thumb::Failed`, not retried at that rev). A web content process crash fails the job.
- `Snapshots::new` must run on the main thread. It is cheap (a worker thread, no WebKit): the renderer, its
  offscreen window and each web view are made on the first `request` that renders, one view per slot as
  slots are first used.
- **`proto_web::prewarm()`** (main thread, once, after the first frame): starts WebKit ahead of the first
  thumbnail. The offscreen window and configuration are made in the call (~40-60 ms debug), the web view
  and its empty-page load (the content process launch) in the next two main-queue turns, so frames draw in
  between. The renderer's first slot adopts the warm view. Measured (debug): first thumbnail 0.69-0.79 s
  after prewarm vs 0.9-1.36 s cold, and `Renderer` creation drops from 30-40 ms to ~10 µs. A render
  started before prewarm finishes takes over what is ready.

```rust
pub struct ThumbSpec {
    pub viewport_w: u32,         // design.viewport.unwrap_or(1280): layout width in CSS px
    pub viewport_h: Option<u32>, // design.viewport_height: a fixed-size screen
    pub out_w: u32,              // device px of the box width (box w × camera zoom × backing scale)
}
pub enum Thumb { Pending, Ready(Snapshot), Failed(String) }
pub struct Snapshot {
    pub pixels: rune::Pixels,    // straight-alpha sRGB, already cropped to `rect`
    pub rect: [f32; 4],          // the captured area, viewport CSS px: x, y, w, h
    pub crop: Option<[f32; 4]>,  // Some when the content crop applied (then rect == crop, height ≤ 3·w)
    pub doc_h: f32,              // document scrollHeight in CSS px
    pub rev: u64,                // the `rev` requested for this picture
    pub serial: u64,             // unique per finished render: swap your drawn picture when it changes
}
```

### What gets captured (the reference's thumbnail rules, applied inside proto-web)

| case | layout viewport (100vh) | captured `rect` | crop |
|---|---|---|---|
| `viewport_h = Some(vh)` (fixed screen) | `vw × vh` | `0,0,vw,vh` | never |
| `vw < 600` (phone) | `vw × vw·17/9` | `0,0,vw,vw·17/9`, page background transparent | never |
| content narrower than 60% of `vw` | `vw × vw·3/4` | the padded crop box (x, y, w, h; h capped at 3·w) | Some |
| otherwise | `vw × vw·3/4` | `0,0,vw,vw·3/4` | None |

- The crop rule is the reference's `contentCrop` verbatim: bounding box of all elements narrower than 90% of
  `vw` (img/svg/canvas/video always count), clipped to `[0, vw]`, `y1` clamped to the document's scrollHeight;
  crop when its width ≤ 60% of `vw`, padded by 24 px; then html/body `background-image: none`.
- Page prep (reference `previewCss` + `quiesce`): animations frozen at their **end**
  (`animation-delay:-60s; animation-play-state:paused`), transitions and smooth scrolling off, `.dialkit-root`,
  `.dialkit-panel` hidden; after load, web fonts and a short settle (400 ms instead of the reference's 1.2 s, so
  a pool of web views keeps up with a canvas full of cards), timers/intervals/rAF/media are stopped and the
  picture is taken.
- Card side: draw `pixels` with `pixels_fit(Fill)` anchored top-left in a box of aspect `rect.w : rect.h`
  scaled per spec-cards §2.3 (never above 1:1; a crop fits whole when that keeps ≥ half the fill scale).

### Sizes and memory

- Output width = `min(out_w, rect.w × 2)` device px (never more than 2 px per CSS px), height by aspect.
- **Zoom:** a cached snapshot serves any request up to **1.25×** its pixel width. A larger `out_w` (zooming in on
  a card) re-renders at the larger size, bucketed to 256 px steps, while the old snapshot keeps showing. Smaller
  requests reuse the larger one; the cache never re-renders to go smaller.
- `SnapshotLimits { max_entries: 400, max_bytes: 384 MiB (decoded RGBA), web_views: 3 }` by default; least
  recently requested entries go first. Pixels a view still draws stay alive through its `Pixels` clone.

## LiveView

A real, interactive WKWebView as a child of the window's content NSView.

```rust
// once per native window (App::on_window runs again after a recreate)
let live = LiveView::attach(&window_handle, Arc::new(move || app_handle.wake()))?;

live.prepare(&page, &opts);                // start loading hidden (call when the open flight starts)
live.show([x, y, w, h], &page, &opts);     // window logical px, top-left origin; loads only if key/rev changed
live.move_to([x, y, w, h]);                // on resize / layout change
live.hide();                               // keeps the page loaded: reopening the same design is instant
live.is_loaded() -> bool;                  // navigation finished for the current page
live.take_keys() -> Vec<LiveKey>;          // drain in poll (see Keys)
live.set_base_url(Option<String>);         // as for Snapshots; default http://127.0.0.1:3124/
```

```rust
pub struct LivePage<'a> { pub key: &'a str, pub rev: u64, pub html: &'a str, pub viewport_w: u32, pub viewport_h: Option<u32> }
pub struct LiveOptions { /* builder */ }   // LiveOptions::default().user_script(js) — WKUserScript at document end
pub enum LiveKey { Escape, Left, Right }
```

- `show`, `move_to`, `hide` are synchronous AppKit calls (`setFrame`, `setHidden`, `pageZoom`) on the calling
  turn: no async step, no layout pass of their own. They are safe to call in the same commit that starts or
  ends a flight.
- Scale: the page lays out at `viewport_w` CSS px and fills the rect (fixed screen: `min(w / vw, h / vh)`),
  like the reference's scaled iframe. At a zoom ≥ 0.9 that's WebKit's `pageZoom`. Below 0.9 the web view
  lays out at full size with pageZoom 1 inside a container NSView whose bounds are larger than its frame,
  so AppKit scales the whole view. This matters because WebKit floors text at 9pt on screen after pageZoom:
  15px text at pageZoom 0.5 laid out at 18px. Scaled, text keeps its designed size. Hit-testing and
  `convertPoint` go through AppKit's bounds scaling; the probe checked window point → page px exactly in both
  modes. Small views cost more GPU memory (WebKit renders the full-size page and the compositor downsamples
  it). The html is the design's own (no thumbnail prep, no crop, scripts run).
- **Drawing order caveat:** the web view is a native view, so it sits **on top of every Rune element**, flights,
  popovers, menus and tooltips included. Nothing Rune-drawn may overlap its rect while it is shown: open
  side-panel menus beside it, and show it only after a flight into its rect has landed (hide it in the same
  commit that starts a flight out). It never appears in `--snap` output: scenes show the snapshot instead.
- **Keys:** while the web view (or anything inside it) is first responder, a local key monitor consumes
  **Esc** always, and **← / →** unless a modifier is held or an editable element inside the page has focus
  (tracked by an injected focus script). Consumed keys are queued for `take_keys()` and the `wake` callback
  fires. Other keys go to the page. `hide()` hands first responder back to the Rune view if the web view held it.

### Scripting (for proto-worker-8 / Annotate; implemented, `eval` verified)

- `live.eval(js, |Result<serde_json::Value, String>| ..)`: runs `js` as an async function body in the page
  (`return` a value), result JSON-serialised.
- `live.on_message(name, |serde_json::Value| ..)`: a `WKScriptMessageHandler` named `name`; the page posts
  with `window.webkit.messageHandlers[name].postMessage(JSON.stringify(payload))` (plain strings, numbers and
  booleans also arrive). `protoFocus` is reserved.
- User scripts (`LiveOptions::user_script`) take effect at the next load; `show`/`prepare` with changed
  `LiveOptions` reload.

### Annotate (to be re-implemented; contract in the clean-room spec, `AN-IF-2xx`)

Picking HTML elements in a live design page. The page renders this UI itself: the native web view covers
every Rune element, so nothing drawn by Rune could show on top of it.

```rust
live.set_annotate(on: bool, dark: bool);   // annotate mode on/off and its look; kept across loads
live.set_pins(Vec<WebPin>);                 // numbered markers for existing notes, also while off
live.take_annotate_events() -> Vec<AnnotateEvent>; // drain in poll; `wake` fires
live.missing_pins() -> Vec<String>;         // pin ids whose element isn't on this page (after each load)

pub enum AnnotateEvent { Save { comment, targets: Vec<WebPick> }, Resolve { id }, Dismiss { id }, Reply { id, text }, Cancel }
pub struct WebPick { element, element_path, full_path, css_classes, nearby_text, nearby_elements, selected_text,
                     computed_styles, accessibility, attributes: BTreeMap, rect /* page CSS px */, window_rect /* window pt */ }
pub struct WebPin { id, number, element_path, comment, status /* pending|acknowledged|resolved */, thread: Vec<(author, text)> }
```

`protoAnnotate` is a reserved message handler name. Behaviour: see the clean-room specification.

## Files

`src/spec.rs` (rules, pure + tests) · `src/mac/render.rs` (WKWebView pool, per-slot state machine, timeouts) ·
`src/mac/js.rs` (measure/crop/quiesce + focus scripts) · `src/mac/image.rs` (CGImage → RGBA) ·
`src/mac/snapshots.rs` (cache, LRU, worker) · `src/mac/live.rs` (LiveView, key monitor) · `src/mac/annotate.rs` +
`annotate.js` (annotate picker, stub) · `src/mac/nav.rs`,
`src/mac/messages.rs` (delegates). Examples: `snap` (html → PNG through the thumbnail pipeline; `REPEAT=n`,
`PROTO_WEB_TRACE=1`), `live_probe`, `scale_probe`.
