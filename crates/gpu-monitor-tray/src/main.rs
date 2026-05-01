mod client;
mod config;
mod icon;
mod tray;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use config::Config;
use icon::IconRenderer;
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;
use tray::{GpuTray, State};

const ASSET_ICON_REL: &str = "assets/tarjeta-de-video.png";

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Config::parse();
    init_tracing(&cfg.log_level);

    let icon_path = locate_icon()
        .context("could not find tarjeta-de-video.png; install via `assets/`")?;
    let renderer = IconRenderer::new(cfg.icon_height, &icon_path)?;

    if let Some(path) = cfg.dump_icon.as_ref() {
        return dump_icon_once(&renderer, &cfg.backend_url, path).await;
    }

    let cache_dir = cache_icon_dir().context("locating icon cache directory")?;
    tracing::info!(cache = %cache_dir.display(), "icon cache directory");
    let tray_state = GpuTray::new(renderer, cfg.backend_url.clone(), cache_dir)?;
    let service = ksni::TrayService::new(tray_state);
    let handle = service.handle();
    service.spawn();

    let (tx, mut rx) = mpsc::channel(8);
    client::spawn(cfg.backend_url, tx);

    while let Some(update) = rx.recv().await {
        let state = match update {
            client::Update::Connected(snap) => State::Connected(snap),
            client::Update::Disconnected(err) => State::Disconnected(err),
        };
        handle.update(|tray| tray.set_state(state));
    }

    Ok(())
}

fn init_tracing(directive: &str) {
    let filter = EnvFilter::try_new(directive).unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

async fn dump_icon_once(
    renderer: &IconRenderer,
    backend_url: &str,
    out_path: &std::path::Path,
) -> Result<()> {
    let url = format!("{}/v1/snapshot", backend_url.trim_end_matches('/'));
    let snapshot: gpu_monitor_core::Snapshot = reqwest::get(&url)
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()?
        .json()
        .await
        .context("decoding snapshot JSON")?;
    let rendered = renderer.render(&snapshot.gpus, true);

    let mut img = image::RgbaImage::new(rendered.width as u32, rendered.height as u32);
    for (chunk, pixel) in rendered.argb.chunks_exact(4).zip(img.pixels_mut()) {
        *pixel = image::Rgba([chunk[1], chunk[2], chunk[3], chunk[0]]);
    }
    img.save(out_path).with_context(|| format!("writing {}", out_path.display()))?;
    println!("wrote {} ({}x{})", out_path.display(), rendered.width, rendered.height);
    Ok(())
}

fn cache_icon_dir() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .ok_or_else(|| anyhow::anyhow!("neither XDG_CACHE_HOME nor HOME set"))?;
    Ok(base.join("gpu-monitor").join("icons"))
}

fn locate_icon() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(path) = std::env::var_os("GPU_MONITOR_TRAY_ICON") {
        candidates.push(PathBuf::from(path));
    }
    let xdg_data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    if let Some(data) = xdg_data {
        candidates.push(data.join("gpu-monitor").join("tarjeta-de-video.png"));
    }
    candidates.push(PathBuf::from("/usr/share/gpu-monitor/tarjeta-de-video.png"));
    candidates.push(PathBuf::from(ASSET_ICON_REL));
    if let Some(workspace_root) = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
    {
        candidates.push(workspace_root.join(ASSET_ICON_REL));
    }
    candidates.into_iter().find(|p| p.exists())
}
