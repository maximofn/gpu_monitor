use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(name = "gpu-monitor-tray", about = "Linux system-tray frontend for gpu-monitord", version)]
pub struct Config {
    /// Base URL of the gpu-monitord HTTP API.
    #[arg(long, env = "GPU_MONITOR_TRAY_URL", default_value = "http://127.0.0.1:9123")]
    pub backend_url: String,

    /// tracing-subscriber EnvFilter directive.
    #[arg(long, env = "RUST_LOG", default_value = "info")]
    pub log_level: String,

    /// Tray icon height in pixels.
    #[arg(long, env = "GPU_MONITOR_TRAY_ICON_HEIGHT", default_value_t = 22)]
    pub icon_height: u32,

    /// Render one snapshot to a PNG and exit (for debugging the icon).
    #[arg(long, value_name = "PATH")]
    pub dump_icon: Option<std::path::PathBuf>,
}
