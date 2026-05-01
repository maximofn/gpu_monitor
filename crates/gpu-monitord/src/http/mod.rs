pub mod routes;
pub mod sse;

use std::time::Instant;

use axum::Router;
use gpu_monitor_core::Snapshot;
use tokio::sync::watch;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub started_at: Instant,
    pub snapshot_rx: watch::Receiver<Snapshot>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", axum::routing::get(routes::healthz))
        .route("/v1/info", axum::routing::get(routes::info))
        .route("/v1/snapshot", axum::routing::get(routes::snapshot))
        .route("/v1/gpus", axum::routing::get(routes::gpus))
        .route("/v1/gpus/:idx", axum::routing::get(routes::gpu))
        .route("/v1/gpus/:idx/processes", axum::routing::get(routes::processes))
        .route("/v1/stream", axum::routing::get(sse::stream))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
