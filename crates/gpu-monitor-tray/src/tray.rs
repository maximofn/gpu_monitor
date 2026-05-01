use std::path::PathBuf;

use anyhow::{Context, Result};
use gpu_monitor_core::{Gpu, ProcessKind, Snapshot};
use ksni::menu::{StandardItem, SubMenu};
use ksni::{MenuItem, ToolTip, Tray};

use crate::icon::IconRenderer;

const REPO_URL: &str = "https://github.com/maximofn/gpu_monitor";
const COFFEE_URL: &str = "https://www.buymeacoffee.com/maximofn";
const ICON_BASENAME: &str = "gpu-monitor-tray";

#[derive(Debug, Clone)]
pub enum State {
    Connecting,
    Connected(Snapshot),
    Disconnected(String),
}

pub struct GpuTray {
    renderer: IconRenderer,
    backend_url: String,
    state: State,
    icon_dir: PathBuf,
    /// Counter that increments on every redraw so the panel sees a new
    /// `IconName` and reloads the file from disk (matches what AppIndicator's
    /// `set_icon_full` does internally — GNOME-shell otherwise caches by name).
    generation: u64,
    current_icon_name: String,
}

impl GpuTray {
    pub fn new(renderer: IconRenderer, backend_url: String, icon_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&icon_dir)
            .with_context(|| format!("creating icon dir {}", icon_dir.display()))?;
        // Wipe any stale icons left by a previous run so the cache stays bounded.
        if let Ok(entries) = std::fs::read_dir(&icon_dir) {
            for entry in entries.flatten() {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(ICON_BASENAME)
                {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        let mut tray = Self {
            renderer,
            backend_url,
            state: State::Connecting,
            icon_dir,
            generation: 0,
            current_icon_name: String::new(),
        };
        tray.refresh_icon_file();
        Ok(tray)
    }

    pub fn set_state(&mut self, state: State) {
        self.state = state;
        self.refresh_icon_file();
    }

    fn refresh_icon_file(&mut self) {
        let png = match self
            .renderer
            .render_png(self.current_gpus(), self.connected())
        {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::warn!(error = %err, "failed to render icon PNG");
                return;
            }
        };
        self.generation = self.generation.wrapping_add(1);
        let new_name = format!("{ICON_BASENAME}-{}", self.generation);
        let new_path = self.icon_dir.join(format!("{new_name}.png"));
        if let Err(err) = std::fs::write(&new_path, &png) {
            tracing::warn!(error = %err, path = %new_path.display(), "failed to write icon PNG");
            return;
        }

        // Drop the previous frame so the cache directory does not grow.
        if !self.current_icon_name.is_empty() {
            let old = self
                .icon_dir
                .join(format!("{}.png", self.current_icon_name));
            let _ = std::fs::remove_file(old);
        }
        self.current_icon_name = new_name;
    }

    fn current_gpus(&self) -> &[Gpu] {
        match &self.state {
            State::Connected(snap) => snap.gpus.as_slice(),
            _ => &[],
        }
    }

    fn connected(&self) -> bool {
        matches!(self.state, State::Connected(_))
    }
}

impl Tray for GpuTray {
    fn id(&self) -> String {
        "gpu-monitor".to_string()
    }

    fn title(&self) -> String {
        "GPU Monitor".to_string()
    }

    fn icon_name(&self) -> String {
        self.current_icon_name.clone()
    }

    fn icon_theme_path(&self) -> String {
        self.icon_dir.to_string_lossy().into_owned()
    }

    fn tool_tip(&self) -> ToolTip {
        let title = "GPU Monitor".to_string();
        let description = match &self.state {
            State::Connecting => format!("Connecting to {}", self.backend_url),
            State::Connected(snap) => {
                let header = format!(
                    "{} GPU(s) — {}",
                    snap.gpus.len(),
                    snap.driver_version.as_deref().unwrap_or("driver: unknown"),
                );
                let body: Vec<String> = snap
                    .gpus
                    .iter()
                    .map(|g| {
                        format!(
                            "GPU {} {} — {}°C, {}/{} GiB ({:.0}%)",
                            g.index,
                            g.name,
                            g.temperature_c.unwrap_or(0),
                            bytes_to_gib(g.memory.used_bytes),
                            bytes_to_gib(g.memory.total_bytes),
                            g.memory.used_percent(),
                        )
                    })
                    .collect();
                format!("{}\n{}", header, body.join("\n"))
            }
            State::Disconnected(err) => format!("Backend offline: {err}"),
        };
        ToolTip {
            icon_name: String::new(),
            icon_pixmap: Vec::new(),
            title,
            description,
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Vec::new();

        match &self.state {
            State::Connecting => {
                items.push(disabled_item(format!(
                    "Connecting to {}…",
                    self.backend_url
                )));
                items.push(MenuItem::Separator);
            }
            State::Disconnected(err) => {
                items.push(disabled_item(format!("Backend offline: {err}")));
                items.push(disabled_item(format!("Backend: {}", self.backend_url)));
                items.push(MenuItem::Separator);
            }
            State::Connected(snap) => {
                for gpu in &snap.gpus {
                    items.push(MenuItem::SubMenu(gpu_submenu(gpu)));
                }
                items.push(MenuItem::Separator);
                items.push(disabled_item(format!(
                    "Backend: {}{}",
                    self.backend_url,
                    snap.driver_version
                        .as_ref()
                        .map(|d| format!(" — driver {d}"))
                        .unwrap_or_default()
                )));
                items.push(disabled_item(format!(
                    "Updated: {}",
                    short_time(&snap.timestamp)
                )));
                items.push(MenuItem::Separator);
            }
        }

        items.push(MenuItem::Standard(StandardItem {
            label: "Repository".into(),
            activate: Box::new(|_| open_url(REPO_URL)),
            ..Default::default()
        }));
        items.push(MenuItem::Standard(StandardItem {
            label: "Buy me a coffee".into(),
            activate: Box::new(|_| open_url(COFFEE_URL)),
            ..Default::default()
        }));
        items.push(MenuItem::Separator);
        items.push(MenuItem::Standard(StandardItem {
            label: "Quit".into(),
            activate: Box::new(|_| std::process::exit(0)),
            ..Default::default()
        }));

        items
    }
}

fn gpu_submenu(gpu: &Gpu) -> SubMenu<GpuTray> {
    let header = format!("GPU {} — {}", gpu.index, gpu.name);
    let mut entries: Vec<MenuItem<GpuTray>> = Vec::new();

    entries.push(disabled_item(format!(
        "Temperature: {}°C",
        gpu.temperature_c.unwrap_or(0)
    )));
    entries.push(disabled_item(format!(
        "Utilization: GPU {}% / Mem {}%",
        gpu.utilization.gpu_percent, gpu.utilization.memory_percent
    )));
    entries.push(disabled_item(format!(
        "Memory used: {} GiB",
        format_bytes(gpu.memory.used_bytes)
    )));
    entries.push(disabled_item(format!(
        "Memory free: {} GiB",
        format_bytes(gpu.memory.free_bytes)
    )));
    entries.push(disabled_item(format!(
        "Memory total: {} GiB ({:.0}% used)",
        format_bytes(gpu.memory.total_bytes),
        gpu.memory.used_percent()
    )));
    if let (Some(draw), Some(limit)) = (gpu.power_draw_w, gpu.power_limit_w) {
        entries.push(disabled_item(format!(
            "Power: {:.0}W / {:.0}W",
            draw, limit
        )));
    }

    if gpu.processes.is_empty() {
        entries.push(MenuItem::Separator);
        entries.push(disabled_item("No GPU processes".into()));
    } else {
        entries.push(MenuItem::Separator);
        entries.push(disabled_item(format!(
            "Processes ({})",
            gpu.processes.len()
        )));
        for proc in &gpu.processes {
            entries.push(disabled_item(format!(
                "  {:>6} {:<7} {} ({})",
                proc.pid,
                kind_label(proc.kind),
                proc.name,
                format_bytes(proc.used_memory_bytes)
            )));
        }
    }

    SubMenu {
        label: header,
        submenu: entries,
        ..Default::default()
    }
}

fn disabled_item(label: String) -> MenuItem<GpuTray> {
    MenuItem::Standard(StandardItem {
        label,
        enabled: false,
        ..Default::default()
    })
}

fn kind_label(kind: ProcessKind) -> &'static str {
    match kind {
        ProcessKind::Compute => "compute",
        ProcessKind::Graphics => "graphic",
        ProcessKind::Mixed => "mixed",
    }
}

fn open_url(url: &str) {
    if let Err(err) = open::that(url) {
        tracing::warn!(%url, error = %err, "could not open url");
    }
}

fn bytes_to_gib(bytes: u64) -> u64 {
    bytes / (1024 * 1024 * 1024)
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.0} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} B", bytes)
    }
}

fn short_time(rfc3339: &str) -> &str {
    rfc3339
        .split('T')
        .nth(1)
        .and_then(|s| s.split('.').next())
        .unwrap_or(rfc3339)
}
