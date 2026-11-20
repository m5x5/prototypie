//! The router. One module per reference route group.

mod agent;
mod assets;
mod edit;
mod implement;
mod misc;
pub mod polls;
mod state;
mod variations;

use axum::Router;
use axum::routing::{get, post};

use crate::AppState;

pub fn router(app: AppState) -> Router {
    Router::new()
        .route("/api/state", get(state::get).put(state::put))
        .route("/api/state/events", get(state::events))
        .route("/api/agent-queue", get(agent::queue_list))
        .route("/api/agent-queue/{job_id}/{index}", post(agent::queue_deliver))
        .route("/api/agent-queue/{job_id}/{index}/claim", post(agent::queue_claim))
        .route("/api/agent/canvas", get(agent::canvas))
        .route("/api/agent/cards", post(agent::cards))
        .route("/api/agent/explorations", post(agent::explorations))
        .route("/api/variations", post(variations::start))
        .route("/api/variations/{id}", get(variations::follow).head(variations::exists).delete(variations::cancel))
        .route("/api/edit", post(edit::start))
        .route("/api/edit/{design_id}", axum::routing::delete(edit::cancel))
        .route("/api/implement", post(implement::implement))
        .route("/api/agent-tasks", get(misc::agent_tasks))
        .route("/api/agent-tasks/{id}/notes", post(misc::agent_task_note))
        .route("/api/projects", get(misc::projects_list))
        .route("/api/projects/{id}", get(misc::project_get).post(misc::project_enrich))
        .route("/api/pod-catalog", get(misc::pod_catalog))
        .route("/api/bundles", get(misc::bundles))
        .route("/api/spokes", post(misc::spokes))
        .route("/api/decisions", post(misc::decisions))
        .route("/api/polls", post(polls::create))
        .route("/api/polls/share", get(polls::share_get).post(polls::share_start).delete(polls::share_stop))
        .route("/api/polls/{id}", get(polls::get_poll))
        .route("/api/polls/{id}/votes", get(polls::get_votes).post(polls::post_vote))
        .route("/poll/{id}", get(polls::page))
        .merge(assets::routes())
        // The notes service (own 1 MiB limit, JSON only, local only).
        .merge(crate::notes::routes::routes())
        // Large designs: the reference accepts big JSON bodies (polls hold full HTML snapshots).
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024))
        .with_state(app)
}
