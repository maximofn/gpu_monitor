mod config;
mod http;
mod nvml_source;
mod proc_name;
mod sampler;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;
use config::Config;
use gpu_monitor_core::Snapshot;
use nvml_source::{GpuSource, MockSource, NvmlSource};
use sampler::{build_snapshot, empty_snapshot};
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::watch;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Config::parse();
    init_tracing(&cfg.log_level);

    let host = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "localhost".to_string());

    let source: Arc<dyn GpuSource> = if cfg.mock {
        tracing::warn!("running with MOCK GPU source");
        Arc::new(MockSource::new(mock_gpus()))
    } else {
        Arc::new(
            NvmlSource::init()
                .context("failed to initialise NVML; is the NVIDIA driver loaded?")?,
        )
    };

    let initial: Snapshot = match source.sample() {
        Ok(_) => build_snapshot(&host, source.as_ref()),
        Err(err) => {
            tracing::warn!(error = %err, "initial sample failed; serving empty snapshot");
            empty_snapshot(&host, source.driver_version(), source.cuda_version())
        }
    };
    let (tx, rx) = watch::channel(initial);

    sampler::spawn(source, host, cfg.sample_interval_ms, tx);

    let state = http::AppState {
        started_at: Instant::now(),
        snapshot_rx: rx,
    };
    let app = http::build_router(state);

    let addr = SocketAddr::new(cfg.bind, cfg.port);
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    tracing::info!(%addr, "gpu-monitord listening");

    tokio::select! {
        result = axum::serve(listener, app) => {
            result.context("HTTP server error")?;
        }
        _ = shutdown_signal() => {
            tracing::info!("shutdown requested; aborting in-flight SSE streams");
        }
    }

    tracing::info!("shutdown complete");
    Ok(())
}

fn init_tracing(directive: &str) {
    let filter = EnvFilter::try_new(directive).unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.ok();
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(err) => {
                tracing::warn!(error = %err, "could not install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("ctrl-c received"),
        _ = terminate => tracing::info!("SIGTERM received"),
    }
}

fn mock_gpus() -> Vec<gpu_monitor_core::Gpu> {
    use gpu_monitor_core::{Gpu, Memory, Process, ProcessKind, Utilization};
    vec![
        Gpu {
            index: 0,
            uuid: "GPU-mock-0".into(),
            name: "Mock RTX 4090".into(),
            temperature_c: Some(45),
            fan_speed_percent: Some(30),
            power_draw_w: Some(120.0),
            power_limit_w: Some(450.0),
            utilization: Utilization { gpu_percent: 25, memory_percent: 10 },
            memory: Memory {
                used_bytes: 4 * 1024 * 1024 * 1024,
                free_bytes: 20 * 1024 * 1024 * 1024,
                total_bytes: 24 * 1024 * 1024 * 1024,
            },
            processes: vec![Process {
                pid: 1234,
                name: "ollama".into(),
                used_memory_bytes: 3 * 1024 * 1024 * 1024,
                kind: ProcessKind::Compute,
            }],
        },
        Gpu {
            index: 1,
            uuid: "GPU-mock-1".into(),
            name: "Mock RTX 3090".into(),
            temperature_c: Some(38),
            fan_speed_percent: Some(20),
            power_draw_w: Some(50.0),
            power_limit_w: Some(350.0),
            utilization: Utilization { gpu_percent: 5, memory_percent: 1 },
            memory: Memory {
                used_bytes: 1024 * 1024 * 1024,
                free_bytes: 23 * 1024 * 1024 * 1024,
                total_bytes: 24 * 1024 * 1024 * 1024,
            },
            processes: vec![],
        },
    ]
}
