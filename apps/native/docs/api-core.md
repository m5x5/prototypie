# proto-core API

Owner: proto-worker-1. Stable from here on: additions only; anything breaking is announced first.
Source: `crates/proto-core/src/` (`types.rs`, `record.rs`, `snapshot.rs`, `store.rs`, `query.rs`, `persist.rs`).

```toml
proto-core.workspace = true
```

## Model in one paragraph

The state is the reference app's `data/state.json`: `{ epoch, version, records: { "<kind>/<id>": {rev, value} | {rev, deleted: true} } }`.
Every accepted batch is one new `version`; each record remembers the `rev` that last wrote it; deletes stay as
tombstones. The canonical value of a record is its raw JSON (`Arc<serde_json::Value>`, lossless); typed views
(`Design`, `Space`, …) are parsed on first use and cached per record. Reads go through an immutable
`Arc<Snapshot>` (no lock held while you read); writes go through `Store::apply`, which swaps in a new snapshot,
broadcasts a `ChangeEvent`, and hands the disk work to the store's writer thread.

## Store

```rust
use proto_core::{Store, Change, ChangeMeta, RecordKind};

// Blocking (state.json can be 100+ MB): call off the UI thread, then share the handle.
let store = Store::open(proto_core::default_data_dir())?;   // the app's startup only
    // One writer per data dir: holds an flock on <dir>/.lock for the store's lifetime (released by the kernel
    // on exit/crash too). A second open fails: err.downcast_ref::<proto_core::StoreError>() ==
    // Some(StoreError::Locked { pid }) (pid of the holder, alive; 0 if unreadable). StoreError::Io { dir, source }
    // if the lock file can't be made. Dropping the last handle waits for the final save, then unlocks.
proto_core::lock_holder(&dir) -> Option<u32>                 // cheap: pid holding dir's lock, no state.json read
let store = Store::in_memory();                               // tests, --snap renders; never touches disk
let store = Store::from_state(state_file);                    // in-memory, preloaded

// Store: Clone + Send + Sync + 'static (Arc inside). Clone it into axum handlers, threads, app state.
store.data_dir() -> Option<&Path>          // put your own files under it (agent-queue/, polls/, …);
                                           // None (in-memory: --snap/--script/--tree) → write nothing,
                                           // never fall back to default_data_dir()
store.snapshot() -> Arc<Snapshot>          // cheap; never changes; a write makes a new one
store.cursor() -> Cursor { epoch, version }
store.read(since: u64, epoch: Option<&str>) -> StateFile
    // GET /api/state semantics: since == 0 or another epoch → everything (tombstones included),
    // else records with rev > since. Serialises exactly like the reference.
store.apply(changes: Vec<Change>, meta: ChangeMeta) -> Result<Cursor, ApplyError>
    // One version per batch; last write per path wins; an empty batch returns the cursor unchanged.
    // Rejects the whole batch (nothing applied) on an invalid path or a set without value.
    // Appends one NDJSON line per change to <data>/state-log.ndjson (ts, rev, clientId, path, op, before,
    // prevRev, title, reason, ua — same fields as the reference); saves state.json debounced (300 ms),
    // atomically (tmp + rename), on the writer thread.
store.subscribe() -> tokio::sync::broadcast::Receiver<ChangeEvent>
    // One event per accepted batch. Works without a runtime: UI polls with try_recv(); async code recv().await.
    // RecvError::Lagged(_) → resync with read(0, None) / a fresh snapshot().
store.flush() -> anyhow::Result<()>        // save now and wait; for shutdown/tests, not the UI thread
```

Writes, typed or raw:

```rust
store.apply(vec![
    Change::put(RecordKind::Designs, &design.id, &design)?,   // typed (serde)
    Change::set("portals/abc123", json!({"x": 0, "y": 0})),     // raw
    Change::delete("designs/old1"),
], ChangeMeta::client("proto-app").reason("drag"))?;
```

## Records and paths

```rust
pub const SHARED_KEYS: [&str; 12] = ["spaces", "designs", "runs", "portals", "promotedFrom", "seededFrom",
    "notes", "reviewedIds", "lanes", "spokeFocus", "publishedPolls", "pollWinners"];
pub enum RecordKind { Spaces, Designs, Runs, Portals, PromotedFrom, SeededFrom, Notes, ReviewedIds, Lanes,
    SpokeFocus, PublishedPolls, PollWinners }   // .as_str(), ::parse(&str), .path(id) -> "designs/<id>"
pub fn is_valid_path(path: &str) -> bool        // reference isValidPath: <shared kind>/<non-empty id>, < 512 chars
pub fn split_path(path: &str) -> Option<(RecordKind, &str)>

pub struct StateFile { pub epoch: String, pub version: u64, pub records: BTreeMap<String, StoredRecord> }
pub struct StoredRecord { pub rev: u64, pub value: Option<Arc<Value>>, pub deleted: bool }
    // serialises as {rev, value} or {rev, deleted: true}
pub struct Cursor { pub epoch: String, pub version: u64 }
pub struct Change { pub path: String, pub value: Option<Value>, pub deleted: bool }
    // Change::set(path, value) / Change::delete(path) / Change::put(kind, id, &T)
pub struct ChangeMeta { pub client_id, pub reason, pub user_agent: Option<String> }
    // ChangeMeta::client("x").reason("why")
pub struct ChangeEvent { pub epoch: String, pub version: u64, pub client_id: Option<String>, pub paths: Arc<[String]> }
    // Serialize gives the reference SSE payload {epoch, version, clientId?} (paths is skipped)
pub enum ApplyError { InvalidPath(String), MissingValue(String) }
    // Display = the reference's 400 messages ("Invalid record path (not a shared store key): …", "Missing value for …")
```

What each kind holds (record id → value):

| kind | id | value |
|---|---|---|
| `spaces` | space id | `Space` |
| `designs` | design id | `Design` |
| `runs` | run id | `Run` (in-flight generation) |
| `portals` | nested space id | `Point` (portal position on the parent canvas) |
| `promotedFrom` | promoted copy id | `PromotedRef` |
| `seededFrom` | seed copy id | string: original design id |
| `notes` | design id | `Vec<DesignNote>` (Annotate notes, oldest first) |
| `reviewedIds` | design id | `true` |
| `lanes` | job id | `Lane` |
| `spokeFocus` | design id | string: the component a spoke explores |
| `publishedPolls` | poll id | `PublishedPoll` (reference "sharedList": order by `createdAt` desc) |
| `pollWinners` | as the reference writes it | string: winning option id |

The main canvas is `ROOT_SPACE_ID = "root"`; it holds only projects (`Space.kind == Some(SpaceKind::Project)`).

## Snapshot

```rust
let s = store.snapshot();
s.epoch: Arc<str>; s.version: u64
s.read(since, epoch) -> StateFile                 // same as Store::read, on this snapshot
s.get(path) -> Option<&Arc<Entry>>                // tombstones included; Entry { kind, rev, value: Option<Arc<Value>> }
s.value(path) -> Option<&Arc<Value>>              // live raw value
s.entry(kind, id) -> Option<&Arc<Entry>>          // live only
s.live(kind) -> impl Iterator<Item = (&str /*id*/, &Arc<Entry>)>   // live records of a kind, id order
entry.typed() -> &Typed                           // Typed::Design(Arc<Design>) | Space | Run | Portal(Point) | … | Invalid

// Typed getters (None if missing, deleted or not well-formed), and iterators over all live ones:
s.space(id) / s.spaces()            -> Arc<Space>
s.design(id) / s.designs()          -> Arc<Design>
s.run(id) / s.runs()                -> Arc<Run>
s.portal(id) / s.portals()          -> Point
s.promoted_from(copy_id) / s.promoted_refs() -> Arc<PromotedRef>
s.seeded_from(seed_id) / s.seeds()  -> Arc<str>
s.notes(design_id) / s.all_notes()  -> Arc<Vec<DesignNote>>
s.lane(job_id) / s.lanes()          -> Arc<Lane>
s.spoke_focus(design_id) / s.spoke_focuses() -> Arc<str>
s.published_poll(id) / s.published_polls()   -> Arc<PublishedPoll>
s.poll_winner(id) / s.poll_winners()         -> Arc<str>
s.is_reviewed(design_id) -> bool
```

A record whose JSON doesn't fit its type is still stored and served raw (`Typed::Invalid`), never dropped.

## Queries (on `Snapshot`, ported from the reference)

```rust
s.child_spaces(space_id) -> Vec<Arc<Space>>        // direct children, oldest first (portal order)
s.projects() -> Vec<Arc<Space>>                    // project spaces on the main canvas
s.designs_in(space_id) -> impl Iterator<Arc<Design>>
s.descendant_space_ids(space_id) -> Vec<String>    // descendantSpaceIds
s.subtree_designs(space_id) -> Vec<Arc<Design>>    // selectSubtreeDesigns
s.space_path(space_id) -> Vec<Arc<Space>>          // spacePath: root first (breadcrumb)
s.project_of(space_id) -> Option<Arc<Space>>       // self or nearest ancestor with kind project
s.local_path_of(space_id) -> Option<String>        // own localPath, else nearest ancestor's (linkedPath).
                                                   // The name-match fallback (private-projects.ttl) is not in core.
s.hub_of(design_id) -> Option<Arc<Design>>         // hubOf
s.children_of(design_id) -> Vec<Arc<Design>>       // parentId children, oldest first
s.main_variant(design_id) -> Option<Arc<Design>>   // mainVariant (live views)
s.promoted_copy_of(design_id) -> Option<Arc<Design>> // findPromotedCopy
s.promoted_design_ids() -> HashSet<String>         // originals that were promoted
s.reviewed_ids() -> HashSet<String>
s.guidelines_for(space_id) -> Option<Arc<Design>>  // guidelinesFor: own card, else nearest ancestor's
proto_core::has_slots(html) -> bool
```

All are scans over the snapshot (≈ 1–2k records): fine per event; cache per `version` if you need one every frame.

## Types (`proto_core::types`, re-exported at the crate root)

`Design`, `DesignKind {Page, Spoke, Variation}`, `DesignStatus {Generating, Ready, Error}`, `DesignVersion`,
`Space`, `SpaceKind {Project}`, `Point {x, y}`, `VariationRequest`, `Direction`, `Run`, `Lane`,
`LaneType {Clay, Spoke, Round, Merge}`, `PromotedRef`, `DesignNote`, `PublishedPoll`, `PollDecision`, `PollOption`.
Field names follow the reference JSON (camelCase on the wire, snake_case in Rust; times are ms as `f64`). Every
struct has `extra: Map<String, Value>` holding fields the type doesn't name, written back unchanged.
Helpers: `Design::is_ready / is_starred / is_component`, `Space::is_project`.

## Data dir, import

```rust
proto_core::default_data_dir() -> PathBuf      // $PROTOTYPIE_DATA, else ~/Library/Application Support/Prototypie
                                               // (where the app opens its store; not a fallback for writes)
proto_core::reference_state_path() -> PathBuf  // apps/web/data/state.json (read only)
proto_core::import_reference(source, data_dir) -> anyhow::Result<ImportOutcome>
    // Before Store::open: copies a reference state.json into data_dir once (validated, atomic).
    // Never writes the source; never overwrites an existing state → ImportOutcome::AlreadyPresent.
    // Takes the dir's lock while writing; dir in use by another store → AlreadyPresent if a state exists,
    // else Err(StoreError::Locked).
proto_core::STATE_FILE = "state.json"; LOG_FILE = "state-log.ndjson"

proto_core::public_dir(data_dir) -> PathBuf     // <data>/public: the app's static files
proto_core::import_reference_assets(reference_public, data_dir) -> anyhow::Result<AssetImport { copied, kept }>
    // copies <repo>/public/{vendor,imports} into <data>/public (REFERENCE_ASSET_DIRS): missing files only, each atomic,
    // never overwrites, never writes the source. import_reference calls it too (repo = source/../..), on every run.
```

Designs load assets root-relative (`/vendor/usage-direct/styles.css`, …): proto-server serves `public_dir(data)` at
`/` on :3124, and web views load design html with `http://127.0.0.1:3124/` as base URL. `public/imports/` holds the
bundles for "Import" (the reference's `/api/bundles` lists them).

Typical startup (off the UI thread):

```rust
let dir = proto_core::default_data_dir();
proto_core::import_reference(&proto_core::reference_state_path(), &dir)?;  // no-op after the first run
let store = proto_core::Store::open(&dir)?;
```

## Live views (`proto_core::live_view`)

Port of the reference's `src/features/live-view/compose.ts`; pure functions, used by every render path.

```rust
use proto_core::live_view::*;
has_slots(html) -> bool                         // case-insensitive <prototypie-slot\b (also proto_core::has_slots)
slot_ids(html) -> Vec<String>                   // slot targets in document order
s.main_variant(id) -> Option<Arc<Design>>       // (on Snapshot) newest ready mainAt among id + variations, else id
slot_lineage(&Snapshot) -> HashSet<String>      // slot targets + their variations ("Main" toggle); memoise per version
compose_live(html, &Snapshot, wrap: Option<&dyn Fn(&str) -> String>) -> String
    // each slot → <div class="pt-slot[ cls]"…attrs><iframe title srcdoc [style=height:Npx]></iframe></div>,
    // recursive up to depth 3, SIZER before the last </body> (else appended); html without slots unchanged
fingerprint(html, &Snapshot) -> u64             // html + every (nested) slot's main-variant html: key snapshot caches on this
ComposeCache::new(200).compose(card_id, html, &Snapshot, wrap) -> Arc<str>   // memoised by fingerprint, oldest evicted
default_live_html(&[&str], viewport: u32) -> String
SIZER: &str                                     // verbatim from the reference
```

## Preview inputs (`proto_core::preview`): what a card's picture renders

The record's `rev` is the wrong snapshot key (it moves on every drag/star/rename). Use this instead:

```rust
use proto_core::{PreviewInputs, preview::Wrap};
// one per render path; thumbnails wrap each slotted component like the reference (forPreview(h, false))
let wrap: Wrap = Arc::new(|h| proto_web::spec::for_preview(h, false));
let mut inputs = PreviewInputs::new(Some(wrap), 1000);
if let Some(p) = inputs.get(&store.snapshot(), &design.id) {
    snaps.request(&design.id, p.key, &p.html, spec);   // p.key as proto-web's `rev`
}
inputs.invalidate(id);   // deleted design
```

- Plain card: `html` = its own html, `key` = hash of it; recomputed only when the record's rev moves, and unchanged
  when the html is the same (a drag keeps key and `Arc`).
- Live view: `html` = `compose_live` output with `wrap` applied to every component, `key` = `live_view::fingerprint`;
  rechecked once per snapshot version, recomposed only when a slotted component's main variant changed.
- Cheap per frame (a hash lookup when nothing changed). Bounded; not thread-safe by itself (UI thread).

## Write helpers (`proto_core::ops`): each returns the `Change`s for one `apply`

Ported from the reference's `nested/slice.ts` and `store/core.ts`, same placement rules and sizes
(`NODE_W 380 × NODE_H 420`, `PORTAL_W 300 × PORTAL_H 330`, 40 px gap). Pure over a snapshot; apply the result:
`store.apply(changes, ChangeMeta::client("proto-app").reason("promote"))`.

```rust
use proto_core::ops::*;
move_design(&snap, id, Point) -> Option<Change>        // drag end; patches raw JSON (keeps every other field)
move_portal(space_id, Point) -> Change
create_exploration(&snap, parent_id, name: Option<&str>, seed_design_id: Option<&str>) -> Created { space_id, seed_id, changes }
    // "Explore": seed copy at the origin + seededFrom; portal right of the seed, else right of everything;
    // name = name, else the seed's title, else "Exploration N"
create_project(&snap, name, local_path: Option<&str>) -> (Created, bool /*created*/)
    // one project per folder: an existing project with that path (case-insensitive) is returned, no changes
rename_space(&snap, id, name) -> Option<Change>
delete_space(&snap, id) -> Vec<Change>   // subtree spaces, portals, designs, their promotedFrom/seededFrom; never root
promote(&snap, &[ids]) -> (Vec<Change>, Vec<String> /*copy ids*/)
    // ready + not yet promoted + parent isn't the main canvas; grid below the space's portal; edge to the nearest
    // relative in the parent (lineageParent); promotedFrom entry
can_promote_to(parent_space_id: Option<&str>) -> bool   // false for the main canvas
set_starred(&snap, id, on) -> Option<Change>
set_guidelines(&snap, id, on) -> Vec<Change>             // unmarks the space's other guidelines card
set_main(&snap, id, on) -> Option<Change>                // live views' main variant (newest mark wins)
set_reviewed(&[ids], on) -> Vec<Change>
set_spoke_focus(&[ids], focus) -> Vec<Change>            // spokeFocus/<id>
put_lane(&snap, &Lane) -> Vec<Change>                    // + deletes for lanes beyond the newest MAX_LANES (60) by startedAt
set_poll_winner(poll_id, decision_id, Option<option_id>) -> Change   // pollWinners/<pollId>/<decisionId>; None deletes
free_card_spot(&snap, space_id, near: Option<Point>) -> Point; free_portal_spot(&snap, space_id) -> Point
new_id(len) -> String   // nanoid alphabet: 8 for spaces/runs, 10 for designs
now_ms() -> f64
```

Generation (placeholders, runs, delivery) stays with proto-ai / proto-server; they can build on `new_id`,
`free_card_spot` and `Change::put`.

## Fidelity

- Raw values are byte-faithful to the reference: with the workspace's serde_json `float_roundtrip` + `preserve_order`,
  numbers and key order survive load → save. `state.json` is written with records in path order (the reference keeps
  insertion order); record contents are unchanged.
- Typed writes (`Change::put`) serialise numbers like `JSON.stringify` (integral values without `.0`). Explicit nulls on
  optional fields (e.g. `"error": null`) come back as absent from a typed view; the raw value keeps them. Verified on
  the full reference state: all 1751 records round-trip (`cargo test -p proto-core -- --ignored`).
