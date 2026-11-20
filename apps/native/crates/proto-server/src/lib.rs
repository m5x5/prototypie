//! proto-server: the embedded HTTP API on 127.0.0.1:3124 with route and JSON parity to the reference app's
//! `src/app/api/**`. Route table: docs/api-server.md.

mod canvas;
mod context;
mod delivery;
mod notes;
mod pod;
mod polls;
mod projects;
mod routes;
mod store;
mod tasks;
mod util;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

pub use notes::NotesService;
pub use routes::polls::ShareState;
pub use store::SharedStore;

pub const DEFAULT_ADDR: &str = "127.0.0.1:3124";
/// The notes service of the embedded server (its own address).
pub const DEFAULT_NOTES_URL: &str = "http://127.0.0.1:3124";
/// The location the server files its own notes under.
pub const DEFAULT_APP_URL: &str = "http://localhost:3124/";

pub struct ServerConfig {
    pub addr: SocketAddr,
    pub data_dir: PathBuf,
    /// The notes service, default [`DEFAULT_NOTES_URL`].
    pub notes_url: String,
    /// The app URL the server's notes are filed under in the notes service.
    pub app_url: String,
    /// Use the Codex LLM provider ($CODEX_HOME login). Off: only the Claude agent-queue mode generates.
    pub codex: bool,
}

impl ServerConfig {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            addr: DEFAULT_ADDR.parse().expect("addr"),
            data_dir,
            notes_url: DEFAULT_NOTES_URL.into(),
            app_url: DEFAULT_APP_URL.into(),
            codex: true,
        }
    }
}

/// Shared by every handler.
#[derive(Clone)]
pub struct AppState {
    pub store: SharedStore,
    pub data_dir: Arc<PathBuf>,
    pub ai: proto_ai::Ai,
    notes: NotesService,
    /// The notes service this server answers under `/api/notes` (kept in `<data dir>/notes/`).
    notes_store: notes::NotesStore,
    polls: polls::Polls,
    pods: pod::PodCatalog,
    tasks: tasks::TaskCache,
    share: routes::polls::Share,
}

impl AppState {
    /// `ai`: the app's generation service, or None to build one here (Codex per `cfg.codex`).
    pub fn new(cfg: &ServerConfig, store: SharedStore, ai: Option<proto_ai::Ai>) -> Self {
        let notes = NotesService::new(cfg.notes_url.clone(), cfg.app_url.clone());
        let ai = ai.unwrap_or_else(|| {
            let notifier: Arc<dyn proto_ai::Notifier> = Arc::new(notes.clone());
            if cfg.codex {
                proto_ai::Ai::with_codex(cfg.data_dir.clone(), notifier)
            } else {
                proto_ai::Ai::new(cfg.data_dir.clone(), None, notifier)
            }
        });
        let polls = polls::Polls::new(&cfg.data_dir);
        let notes_store = notes::NotesStore::open(&cfg.data_dir);
        Self {
            store,
            data_dir: Arc::new(cfg.data_dir.clone()),
            ai,
            notes,
            notes_store,
            polls,
            pods: pod::PodCatalog::default(),
            tasks: tasks::TaskCache::default(),
            share: routes::polls::Share::default(),
        }
    }

    /// A deleted placeholder card cancels the queue item that would have filled it.
    pub async fn cancel_for_deleted(&self, design_ids: &[String]) {
        delivery::cancel_for_deleted(&self.store, self.ai.queue(), design_ids).await;
    }
}

/// The notes service as a proto-ai notifier, for an app that builds its own `Ai` (pass the same
/// `notes_url`/`app_url` as in [`ServeOpts`]).
pub fn notifier(notes_url: &str, app_url: &str) -> Arc<dyn proto_ai::Notifier> {
    Arc::new(NotesService::new(notes_url, app_url))
}

/// Options for the embedded server ([`start`]). `Default`: 127.0.0.1:3124, its own notes service, the poll LAN
/// listener off.
#[derive(Clone, Debug)]
pub struct ServeOpts {
    pub addr: SocketAddr,
    /// The notes service, default [`DEFAULT_NOTES_URL`].
    pub notes_url: String,
    pub app_url: String,
    /// The runtime to serve on; None = the current one (`start` must then run inside a tokio runtime).
    pub runtime: Option<tokio::runtime::Handle>,
}

impl Default for ServeOpts {
    fn default() -> Self {
        Self {
            addr: DEFAULT_ADDR.parse().expect("addr"),
            notes_url: DEFAULT_NOTES_URL.into(),
            app_url: DEFAULT_APP_URL.into(),
            runtime: None,
        }
    }
}

/// Embed the server in the app: shares the app's `Store` (one per data dir) and `Ai`. Synchronous: binds the
/// port now (errors such as "address in use" come back here), then serves on the runtime. Stop with
/// [`ServerHandle::shutdown`] or by dropping the handle.
pub fn start(store: proto_core::Store, ai: proto_ai::Ai, opts: ServeOpts) -> anyhow::Result<ServerHandle> {
    let data_dir = store.data_dir().map(PathBuf::from).ok_or_else(|| anyhow::anyhow!("the store has no data dir (in-memory)"))?;
    let cfg = ServerConfig { addr: opts.addr, data_dir, notes_url: opts.notes_url, app_url: opts.app_url, codex: false };
    let rt = match opts.runtime {
        Some(h) => h,
        None => tokio::runtime::Handle::try_current().map_err(|_| anyhow::anyhow!("proto_server::start needs a tokio runtime (ServeOpts::runtime)"))?,
    };
    let std_listener = std::net::TcpListener::bind(cfg.addr)?;
    std_listener.set_nonblocking(true)?;
    let state = AppState::new(&cfg, store, Some(ai));
    let _guard = rt.enter();
    let listener = tokio::net::TcpListener::from_std(std_listener)?;
    Ok(spawn_server(listener, state, &rt))
}

fn spawn_server(listener: tokio::net::TcpListener, state: AppState, rt: &tokio::runtime::Handle) -> ServerHandle {
    let addr = listener.local_addr().expect("bound");
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
    let app = router(state.clone());
    rt.spawn(async move {
        // With the peer address: the notes service answers loopback peers only.
        let _ = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
        let _ = done_tx.send(());
    });
    ServerHandle { addr, ai: state.ai.clone(), state, shutdown: Some(tx), done: Some(done_rx) }
}

/// The full router (also used by tests with `tower::ServiceExt::oneshot`).
pub fn router(state: AppState) -> axum::Router {
    routes::router(state)
}

/// Stops the server when dropped.
pub struct ServerHandle {
    pub addr: SocketAddr,
    /// The generation service the server uses (share it with the app).
    pub ai: proto_ai::Ai,
    state: AppState,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    done: Option<tokio::sync::oneshot::Receiver<()>>,
}

impl ServerHandle {
    /// Graceful shutdown: stop accepting, let open requests finish (SSE streams end), stop poll sharing.
    /// Waiting notes watchers end first (503), so they never hold up the 5 s grace period.
    pub async fn shutdown(mut self) {
        self.state.notes_store.shut_down();
        self.state.share.stop();
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(done) = self.done.take() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), done).await;
        }
    }

    /// Start the LAN stakeholder listener for shared polls (default port 3125); returns its state with origins.
    pub async fn share_polls(&self, port: u16) -> anyhow::Result<ShareState> {
        self.state.share.start(self.state.clone(), port).await
    }

    pub fn stop_sharing(&self) {
        self.state.share.stop();
    }

    /// Sharing on/off and its origins, as a watch channel (for "Geteilt im LAN" / "nur lokal").
    pub fn sharing(&self) -> tokio::sync::watch::Receiver<ShareState> {
        self.state.share.subscribe()
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.state.notes_store.shut_down();
        self.state.share.stop();
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

/// Bind and serve on the current tokio runtime.
/// Standalone/test variant: builds its own `Ai` when `ai` is None (Codex per `cfg.codex`).
pub async fn serve(cfg: ServerConfig, store: SharedStore, ai: Option<proto_ai::Ai>) -> anyhow::Result<ServerHandle> {
    let state = AppState::new(&cfg, store, ai);
    let listener = tokio::net::TcpListener::bind(cfg.addr).await?;
    Ok(spawn_server(listener, state, &tokio::runtime::Handle::current()))
}
