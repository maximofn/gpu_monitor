use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use gpu_monitor_core::{Gpu, Process, Snapshot};
use serde::Serialize;

use super::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub uptime_s: u64,
}

pub async fn healthz(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        uptime_s: state.started_at.elapsed().as_secs(),
    })
}

#[derive(Serialize)]
pub struct InfoResponse {
    pub backend_version: &'static str,
    pub api_version: &'static str,
    pub host: String,
    pub driver_version: Option<String>,
    pub cuda_version: Option<String>,
    pub gpu_count: usize,
}

pub async fn info(State(state): State<AppState>) -> Json<InfoResponse> {
    let snap = state.snapshot_rx.borrow();
    Json(InfoResponse {
        backend_version: env!("CARGO_PKG_VERSION"),
        api_version: gpu_monitor_core::API_VERSION,
        host: snap.host.clone(),
        driver_version: snap.driver_version.clone(),
        cuda_version: snap.cuda_version.clone(),
        gpu_count: snap.gpus.len(),
    })
}

pub async fn snapshot(State(state): State<AppState>) -> Json<Snapshot> {
    Json(state.snapshot_rx.borrow().clone())
}

#[derive(Serialize)]
pub struct GpuSummary {
    pub index: u32,
    pub uuid: String,
    pub name: String,
    pub memory_total_bytes: u64,
}

pub async fn gpus(State(state): State<AppState>) -> Json<Vec<GpuSummary>> {
    let snap = state.snapshot_rx.borrow();
    Json(
        snap.gpus
            .iter()
            .map(|g| GpuSummary {
                index: g.index,
                uuid: g.uuid.clone(),
                name: g.name.clone(),
                memory_total_bytes: g.memory.total_bytes,
            })
            .collect(),
    )
}

pub async fn gpu(
    State(state): State<AppState>,
    Path(idx): Path<u32>,
) -> Result<Json<Gpu>, StatusCode> {
    let snap = state.snapshot_rx.borrow();
    snap.gpus
        .iter()
        .find(|g| g.index == idx)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

pub async fn processes(
    State(state): State<AppState>,
    Path(idx): Path<u32>,
) -> Result<Json<Vec<Process>>, StatusCode> {
    let snap = state.snapshot_rx.borrow();
    snap.gpus
        .iter()
        .find(|g| g.index == idx)
        .map(|g| Json(g.processes.clone()))
        .ok_or(StatusCode::NOT_FOUND)
}
