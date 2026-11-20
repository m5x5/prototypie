//! The app's one state. Each feature owns its state struct in its own module and plugs in here
//! with one field (Rune's "a feature owns its state" rule). Modules talk through this struct:
//! read other features' fields, but only mutate your own (or call the owner's methods).

use proto_core::{ApplyError, Change, ChangeMeta, Cursor, Snapshot, Store};
use rune::{Tx, Waker};

use crate::app::data::{Data, Source};
use crate::app::prefs::Prefs;
use crate::app::server::Server;
use crate::canvas::CanvasState;
use crate::cards::CardsState;
use crate::composer::ComposerState;
use crate::features::FeaturesState;
use crate::fullscreen::FullscreenState;
use crate::inspector::InspectorState;
use crate::shell::ShellState;

/// The main canvas' space id, as in the reference app (`ROOT_SPACE_ID`).
pub const ROOT_SPACE: &str = "root";

/// Where the user is: the space shown on the canvas (main canvas = projects only, a project →
/// its explorations, an exploration → its designs) and the selection inside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    /// The active space's id (`ROOT_SPACE` for the main canvas).
    pub space: String,
    /// Path from the main canvas to the active space, root excluded: `(id, name)` pairs, the
    /// active space last. The header's breadcrumb shows it. Filled from proto-core by `enter`.
    pub path: Vec<(String, String)>,
    /// Selected design or card ids in the active space.
    pub selected: Vec<String>,
}

impl Default for Route {
    fn default() -> Self {
        Route { space: ROOT_SPACE.into(), path: Vec::new(), selected: Vec::new() }
    }
}

impl Route {
    pub fn is_root(&self) -> bool {
        self.space == ROOT_SPACE
    }
}

/// Theme choice; `None` follows the system (the reference app is dark-first).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ThemePref {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Default)]
pub struct AppState {
    /// proto-core's store and the snapshot the view reads (`store()`, `snap()`).
    pub data: Data,
    /// The embedded HTTP API on :3124, sharing `data.store` (`app::server`).
    pub server: Server,
    /// The waker from `setup`, for threads that finish later (the server's bind).
    pub wake: Option<Waker>,
    /// The first window's first frame is on screen (set once from `on_ready` in main.rs; read-only for
    /// everyone else). Work that would compete with launch (thumbnail webviews) waits for it.
    pub first_frame_shown: bool,
    pub route: Route,
    pub theme: ThemePref,
    /// Per-device state that survives restarts: active space, drafts, prompt history, agent
    /// choice (`app::prefs`). Mutate through `prefs_mut` so the shell knows to save.
    pub prefs: Prefs,
    /// Bumped by `prefs_mut`; the shell saves when it differs from what it last wrote.
    pub prefs_rev: u64,
    /// Header, breadcrumb, menus (proto-worker-4).
    pub shell: ShellState,
    /// Camera, node layer, selection, dragging (proto-worker-4).
    pub canvas: CanvasState,
    /// Design and project cards (proto-worker-5).
    pub cards: CardsState,
    /// Fullscreen dialog + live webview (proto-worker-5).
    pub fullscreen: FullscreenState,
    /// Floating inspector panel (proto-worker-6).
    pub inspector: InspectorState,
    /// Prompt composer (proto-worker-6).
    pub composer: ComposerState,
    /// Spokes, reviews, polls, multi-select, imports, agent tasks, pod, live views (proto-worker-8).
    pub features: FeaturesState,
    /// Generation runner: composer outbox → proto-ai jobs → store (proto-worker-7).
    pub runner: crate::runner::RunnerState,
}

impl AppState {
    pub fn new() -> Self {
        crate::perf::mark("init");
        AppState::default()
    }

    /// A state on `store` (scenes, tests): the real store isn't loaded.
    pub fn use_store(&mut self, store: Store) {
        self.data = Data::fixture(store);
        self.canvas.fit_pending = true;
    }

    /// The store, for writes: `s.store().apply(changes, ChangeMeta::client("proto-app"))`.
    pub fn store(&self) -> &Store {
        &self.data.store
    }

    /// The current snapshot (refreshed in `poll` when the store changes).
    pub fn snap(&self) -> &Snapshot {
        &self.data.snap
    }

    /// Write `changes` to the store as one batch (client "proto-app", `reason` for the log) and
    /// refresh `snap` at once, so the next lines (e.g. `enter_space` on a space just created) see it.
    pub fn apply(&mut self, changes: Vec<Change>, reason: &str) -> Result<Cursor, ApplyError> {
        let cursor = self.data.store.apply(changes, ChangeMeta::client("proto-app").reason(reason))?;
        self.data.snap = self.data.store.snapshot();
        Ok(cursor)
    }

    /// Go to space `id` (its breadcrumb path comes from the store's current snapshot), optionally selecting a
    /// design there. Unknown ids go to the main canvas.
    pub fn enter_space(&mut self, id: &str, select: Option<String>) {
        let path = breadcrumb_path(&self.data.store.snapshot(), id);
        let id = if id != ROOT_SPACE && path.is_empty() { ROOT_SPACE } else { id };
        self.enter(id.to_string(), path);
        if let Some(d) = select {
            self.route.selected = vec![d];
        }
    }

    /// The app's one generation service, the runner's (worker-7), for the server to share:
    /// agent-queue deliveries to :3124 must reach the runner's waiters.
    fn shared_ai(&mut self) -> Option<proto_ai::Ai> {
        crate::runner::ensure_host(self).map(|h| h.ai().clone())
    }

    /// Whether the embedded server has had its chance to start: running, failed, or never going to
    /// (fixture store, headless run). Create `proto_web::Snapshots` only after this, so thumbnails
    /// load with the server's `/vendor` assets.
    pub fn server_settled(&self) -> bool {
        use crate::app::server::ServerState;
        match self.data.source {
            Source::Fixture | Source::Failed(_) | Source::Locked { .. } => true,
            Source::Idle | Source::Loading => false,
            Source::Ready => rune::is_headless() || !matches!(self.server.state, ServerState::Off | ServerState::Starting),
        }
    }

    /// The banner's Retry: start the embedded server again after it failed (port was taken).
    pub fn retry_server(&mut self) {
        if self.data.source == Source::Ready && !rune::is_headless() {
            let ai = self.shared_ai();
            self.server.retry(self.data.store.clone(), ai);
        }
    }

    /// On window close (the app quits): save the store, then stop the server.
    pub fn shutdown(&mut self) {
        if let Err(e) = self.data.store.flush() {
            eprintln!("prototypie: saving the store failed: {e:#}");
        }
        self.server.stop();
    }

    /// From `AppSpec::setup`: start loading the store and prefs, then the features' setup.
    pub fn setup(&mut self, wake: Waker) {
        crate::perf::mark("setup");
        self.wake = Some(wake.clone());
        // Queue items started from this app are announced to the notes service like the server's own
        // (same pair as `ServeOpts::default()`), so worker agents see them.
        self.runner.config.notifier = Some(proto_server::notifier(proto_server::DEFAULT_NOTES_URL, proto_server::DEFAULT_APP_URL));
        self.data.start_loading(wake.clone());
        crate::cards::setup(self, wake.clone());
        crate::runner::setup(self, wake.clone());
        crate::features::setup(self, wake);
    }

    /// From `AppSpec::poll`: take the loaded store and prefs, follow store changes (renamed or
    /// deleted spaces), save prefs, then the features' poll. Returns whether the view must be rebuilt.
    pub fn poll(&mut self, clock: f64, tx: &mut Tx) -> bool {
        crate::perf::mark("first-poll");
        // Headless runs never get on_ready: count the first second as shown, as Rune does for a
        // window that isn't on screen, so thumbnails still start there.
        if !self.first_frame_shown && clock >= 1.0 {
            self.first_frame_shown = true;
        }
        let was_loading = self.data.source == Source::Loading;
        let (loaded_prefs, mut changed) = self.data.poll();
        if was_loading && self.data.source != Source::Loading {
            self.canvas.fit_pending = true;
            if self.data.source == Source::Ready && !rune::is_headless() {
                let ai = self.shared_ai();
                self.server.start(self.data.store.clone(), ai);
                changed = true;
            }
            // A read-only copy (another Prototypie holds the data): lock the canvas, as its
            // zoom-controls lock does, so nodes don't move or connect into changes nobody saves.
            if matches!(self.data.source, Source::Locked { .. }) && self.canvas.controls.interactive() {
                self.canvas.controls.toggle_lock();
            }
        }
        if let Some(prefs) = loaded_prefs {
            let space = prefs.active_space.clone();
            self.prefs = prefs;
            if let Some(space) = space {
                self.enter_space(&space, None);
            }
        } else if changed && !self.route.is_root() {
            let path = breadcrumb_path(self.snap(), &self.route.space);
            if path.is_empty() {
                self.go_to_main_canvas();
            } else {
                self.route.path = path;
            }
        }
        let loading = matches!(self.data.source, Source::Idle | Source::Loading);
        let fit = self.canvas.poll_fit(&self.data.snap, &self.route.space, loading);
        let (version, space) = (self.data.snap.version, self.route.space.clone());
        // Which spaces have animated edges is cached per snapshot; only a stale cache asks.
        let has = (!self.canvas.dash_fresh(version, &space)).then(|| crate::canvas::edges::has_animated(self));
        let dash = self.canvas.poll_dash(version, &space, has, clock, tx);
        let inertia = self.canvas.poll_inertia(clock, tx);
        self.data.save_prefs(&self.prefs, self.prefs_rev);
        // Each owner module's poll, every one run. A true re-runs the whole view (~20 ms with a
        // canvas of cards), so a module returns true only when something it shows changed;
        // `PROTOTYPIE_TRACE_POLL=1` prints who did.
        let polls = [
            ("data", changed),
            ("fit", fit),
            ("dash", dash),
            ("inertia", inertia),
            ("cards", crate::cards::poll(self, clock, tx)),
            ("fullscreen", crate::fullscreen::poll(self, clock, tx)),
            ("immersive", crate::shell::immersive::poll(self)),
            ("inspector", crate::inspector::poll(self, clock, tx)),
            ("composer", crate::composer::poll(self, clock, tx)),
            ("runner", crate::runner::poll(self, clock, tx)),
            ("crumbs", crate::shell::crumbs::poll(self)),
            ("features", crate::features::poll(self, clock, tx)),
            ("minimap", crate::canvas::minimap::poll(self, clock, tx)),
        ];
        trace_polls(&polls, clock);
        polls.iter().any(|(_, c)| *c)
    }

    /// Switch the canvas to `space` with its breadcrumb `path` (root excluded); clears the
    /// selection. The one place that changes `route.space`.
    pub fn enter(&mut self, space: impl Into<String>, path: Vec<(String, String)>) {
        let space = space.into();
        if self.route.space == space {
            return;
        }
        let left = std::mem::take(&mut self.route.space);
        self.cards.on_leave(&left);
        self.prefs_mut().active_space = Some(space.clone());
        self.route = Route { space, path, selected: Vec::new() };
        self.canvas.on_enter_space();
    }

    /// The prefs for changing; marks them for saving.
    pub fn prefs_mut(&mut self) -> &mut Prefs {
        self.prefs_rev += 1;
        &mut self.prefs
    }

    /// Back to the main canvas (the header logo); a no-op when already there.
    pub fn go_to_main_canvas(&mut self) {
        self.enter(ROOT_SPACE, Vec::new());
    }
}

/// A space's breadcrumb: `(id, name)` from below the main canvas to the space itself. Empty for
/// the main canvas and for unknown spaces.
pub fn breadcrumb_path(snap: &Snapshot, id: &str) -> Vec<(String, String)> {
    if id == ROOT_SPACE {
        return Vec::new();
    }
    snap.space_path(id).iter().filter(|sp| sp.id != ROOT_SPACE).map(|sp| (sp.id.clone(), sp.name.clone())).collect()
}

/// Which module's poll asked for a view re-run (`PROTOTYPIE_TRACE_POLL=1`; counted in tests).
fn trace_polls(polls: &[(&'static str, bool)], clock: f64) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    #[cfg(test)]
    POLL_TRUES.with(|m| {
        let mut m = m.borrow_mut();
        for (name, c) in polls.iter().filter(|(_, c)| *c) {
            let _ = c;
            *m.entry(*name).or_insert(0) += 1;
        }
    });
    if *ON.get_or_init(|| std::env::var_os("PROTOTYPIE_TRACE_POLL").is_some()) {
        let who: Vec<&str> = polls.iter().filter(|(_, c)| *c).map(|(n, _)| *n).collect();
        if !who.is_empty() {
            eprintln!("poll {clock:9.3}s: {}", who.join(", "));
        }
    }
}

#[cfg(test)]
thread_local! {
    /// Per-module count of polls that returned true (benches).
    pub static POLL_TRUES: std::cell::RefCell<std::collections::BTreeMap<&'static str, u64>> = Default::default();
}
