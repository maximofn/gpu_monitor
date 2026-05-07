# GPU Monitor

Real-time NVIDIA GPU monitor for Linux. Split into a small backend daemon that reads NVML and exposes an HTTP/SSE API, plus a system-tray frontend that renders an icon and menu in the Ubuntu/GNOME panel.

![gpu monitor](assets/gpu_monitor.gif)

## Architecture

```
+-------------------+       HTTP/SSE        +----------------------+
|   gpu-monitord    | <-------------------- |   gpu-monitor-tray   |
|  (NVML sampler)   |   /v1/stream JSON     |  (ksni + tiny-skia)  |
+-------------------+                       +----------------------+
        ^                                            ^
        | NVML                                       | DBus (StatusNotifierItem)
        v                                            v
   NVIDIA driver                              GNOME / KDE panel
```

Both Rust binaries live in a single Cargo workspace under `crates/`:

- `gpu-monitor-core` — shared `Snapshot` / `Gpu` / `Process` types serialised with `serde`.
- `gpu-monitord` — backend daemon. Initialises NVML once at start, samples every second on a tokio task, holds the latest snapshot in a `watch` channel, serves it over REST + Server-Sent Events. Defaults to `127.0.0.1:9123`.
- `gpu-monitor-tray` — Linux system-tray frontend. Subscribes to `/v1/stream`, composes a per-GPU icon (base PNG + temperature label + donut chart) with `tiny-skia`, writes it to `~/.cache/gpu-monitor/icons/` and publishes it as a StatusNotifierItem via `ksni`. Per-GPU submenu with temp / utilisation / memory / power / processes.

A native macOS frontend lives in `front-mac/` as an independent Swift Package (Swift + AppKit + CoreGraphics, zero third-party deps). It consumes the same `/v1/stream` endpoint and renders into the macOS menubar via `NSStatusItem`. See [`front-mac/README.md`](front-mac/README.md).

Splitting the daemon from the UI lets another machine on the LAN consume the same metrics — a remote frontend just hits the API. The Mac frontend can connect directly when the daemon binds LAN, or through SSH port forwarding while the daemon stays on `127.0.0.1`.

### Why a PNG file instead of an SNI in-memory pixmap?

The tray could send the icon directly over DBus as ARGB bytes (`IconPixmap`), but the GNOME `ubuntu-appindicators` extension squishes wide pixmaps to a square aspect ratio, mangling a multi-GPU icon. The same trick the Python version uses works here: write the icon to disk and publish `IconThemePath` + `IconName` so GNOME loads the file and respects its native dimensions. The frame counter in the filename forces a re-read on every update.

## Performance

Measured against the original Python script monitoring the same two RTX 3090s:

| | RSS | CPU |
|---|---|---|
| `gpu_monitor.py` | 181 MB | 176% (2 cores) |
| `gpu-monitord` + `gpu-monitor-tray` | ~24 MB | ~1.6% |

The Python's CPU cost was dominated by re-rendering the icon with matplotlib + PIL and writing PNGs each second. Going through `nvml-wrapper` directly (no `nvidia-smi pmon` subprocess) and keeping NVML initialised across samples accounts for most of the daemon's win. Replacing matplotlib with `tiny-skia` is the bulk of the tray's win.

## Requirements

- NVIDIA driver with NVML (the `nvidia-smi` package). Tested with driver 555.42.06 / CUDA 12.5.
- DejaVu Sans Mono font (`apt install fonts-dejavu-core`).
- A desktop with StatusNotifierItem support. On Ubuntu/GNOME this means the **AppIndicator** extension (`gnome-shell-extension-appindicator`) must be enabled. KDE works out of the box.
- Rust toolchain (`stable`, ≥ 1.85). `rustup` will pick it up automatically from `rust-toolchain.toml`.

## Build

```bash
cargo build --release --workspace
```

Produces two binaries:

- `target/release/gpu-monitord`
- `target/release/gpu-monitor-tray`

## Run

In two terminals (or as services, see below):

```bash
./target/release/gpu-monitord --bind 127.0.0.1 --port 9123
./target/release/gpu-monitor-tray --backend-url http://127.0.0.1:9123
```

The daemon flags (`--bind`, `--port`, `--sample-interval-ms`, `--mock`) are documented in [`docs/api.md`](docs/api.md). The tray accepts `--backend-url`, `--icon-height` and `--dump-icon <path>` (write the next rendered icon to a PNG and exit; useful to inspect what the panel receives).

### Quick API smoke test

```bash
curl -s http://127.0.0.1:9123/v1/snapshot | jq
curl -N http://127.0.0.1:9123/v1/stream      # SSE: one event per second
```

## Install (optional)

Daemon as a `systemd --user` service:

```bash
install -Dm755 target/release/gpu-monitord ~/.local/bin/gpu-monitord
install -Dm644 packaging/systemd/gpu-monitord.service \
    ~/.config/systemd/user/gpu-monitord.service
systemctl --user daemon-reload
systemctl --user enable --now gpu-monitord
journalctl --user -u gpu-monitord -f
```

Tray as a session autostart:

```bash
install -Dm755 target/release/gpu-monitor-tray ~/.local/bin/gpu-monitor-tray
install -Dm644 assets/tarjeta-de-video.png \
    ~/.local/share/gpu-monitor/tarjeta-de-video.png
install -Dm644 packaging/autostart/gpu-monitor-tray.desktop \
    ~/.config/autostart/gpu-monitor-tray.desktop
```

## API

See [`docs/api.md`](docs/api.md) for the full schema and endpoint reference. Quick reference:

| Endpoint | Purpose |
|---|---|
| `GET /healthz` | liveness |
| `GET /v1/info` | backend / driver metadata |
| `GET /v1/snapshot` | full latest snapshot |
| `GET /v1/gpus` | per-GPU metadata only |
| `GET /v1/gpus/{idx}` | one GPU |
| `GET /v1/gpus/{idx}/processes` | process list |
| `GET /v1/stream` | SSE — one snapshot per event |

## macOS frontend

```bash
cd front-mac
./scripts/build-app.sh
open "build/GPU Monitor.app" --args --backend-url http://127.0.0.1:9123
```

The daemon defaults to binding `127.0.0.1` (no auth). To consume metrics from a remote Linux box without exposing the API on the LAN, forward the port over SSH:

```bash
ssh -fN -L 9123:127.0.0.1:9123 <ubuntu-host>
open "build/GPU Monitor.app" --args --backend-url http://127.0.0.1:9123
```

Requires macOS 13 or later. The app is menubar-only (`LSUIElement=true`): no Dock icon, no window. Click the icon for a per-GPU submenu with the same data the Linux tray exposes.

To auto-start the tray on login, install the bundled LaunchAgent:

```bash
cd front-mac
./scripts/install-launchagent.sh             # install + load now
./scripts/install-launchagent.sh uninstall   # remove
```

Logs land in `~/Library/Logs/gpu-monitor-tray.{out,err}.log`. The agent expects the backend reachable at `http://127.0.0.1:9123` — pair it with the SSH-tunnel LaunchAgent (`com.maximofn.gpu-monitor-tunnel`) when the daemon runs on a remote host.

## Roadmap

- v2.0: Linux tray frontend (released)
- v2.1: macOS menubar frontend (`front-mac/`, released)
- v2.2: Auth token + LAN bind for remote consumption
- v2.3: Windows tray frontend

## Legacy Python script

The original `gpu_monitor.py` and its `add_to_startup.sh` / `gpu_monitor.sh` helpers live in `legacy/` for reference. They still work standalone (`python3 legacy/gpu_monitor.py`) but are no longer wired into autostart. They will be removed entirely after a soak period on the Rust release.

## Support

If this is useful to you, consider giving a **☆ Star** to the repository or buying a coffee:

[![BuyMeACoffee](https://img.shields.io/badge/Buy_Me_A_Coffee-support_my_work-FFDD00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=white&labelColor=101010)](https://www.buymeacoffee.com/maximofn)
