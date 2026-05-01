# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Comandos

Toolchain anclado a `stable` por `rust-toolchain.toml` (rustup lo instala solo). Todo se opera desde la raíz del workspace.

```bash
cargo build --workspace                      # debug build de los 3 crates
cargo build --release --workspace            # release (lo que se distribuye)
cargo test --workspace                       # todos los tests
cargo test -p gpu-monitord nvml_source::tests::cuda_version_decodes_correctly
                                             # un test concreto
cargo clippy --workspace -- -D warnings      # CI lo exige limpio
cargo fmt --all                              # formateo

# Ejecución manual (para iterar):
./target/release/gpu-monitord --bind 127.0.0.1 --port 9123 --sample-interval-ms 1000
./target/release/gpu-monitor-tray --backend-url http://127.0.0.1:9123

# Sin GPU NVIDIA (CI, dev en otra máquina):
./target/release/gpu-monitord --mock

# Volcar el icono renderizado a un PNG y salir, sin tocar el panel.
# Imprescindible para depurar fallos visuales sin pelearte con GNOME:
./target/release/gpu-monitor-tray --backend-url http://127.0.0.1:9123 --dump-icon /tmp/icon.png
```

`gpu_monitor.py` (legacy) sigue funcional y puede correr en paralelo a la versión Rust mientras dure la migración. No los pongas a usar el mismo `--port`.

## Arquitectura

Workspace Cargo con tres crates:

```
crates/gpu-monitor-core    →  tipos compartidos (serde Snapshot/Gpu/Process)
crates/gpu-monitord        →  daemon HTTP+SSE que lee NVML
crates/gpu-monitor-tray    →  frontend Linux (system tray)
```

El protocolo de datos es REST + Server-Sent Events sobre HTTP. La razón del split no es estética: permite que un frontend remoto (Mac/Windows/web, en planificación) consuma las mismas métricas. El backend está pensado para correr 24/7 en la máquina con GPUs mientras los frontends locales o remotos van y vienen.

### Flujo del backend (`gpu-monitord`)

`main.rs` arranca y mantiene un único `GpuSource` (trait): `NvmlSource` en producción, `MockSource` cuando se pasa `--mock`. **Nunca llames `Nvml::init` por petición** — se hace una sola vez al arranque y se mantiene vivo durante toda la vida del daemon. El coste de inicialización de NVML es la mayor pérdida del Python original.

`sampler::spawn` lanza una task de tokio que muestrea cada N ms y publica en un `tokio::sync::watch::Sender<Snapshot>`. Todos los handlers HTTP leen del `Receiver` (`borrow().clone()`), latencia O(µs). El handler SSE reenvía el watch como stream con `WatchStream`. **No hagas trabajo de NVML desde un handler HTTP** — siempre desde el sampler.

Procesos por GPU: `device.running_compute_processes()` + `device.running_graphics_processes()` con merge por PID en `nvml_source::collect_processes`. El nombre del proceso viene de `/proc/<pid>/comm`. **No vuelvas a `subprocess(nvidia-smi pmon)`** — era 50–200 ms por llamada y un parser regex frágil; las APIs `_v3` de NVML lo dan en ~1 ms.

`with_graceful_shutdown` de axum **no se usa** porque espera a que se vacíen las conexiones, y los streams SSE son por naturaleza infinitos: `systemctl stop` quedaría colgado. La salida se hace con `tokio::select!` entre `axum::serve` y la señal — se aborta el server al recibir SIGTERM/SIGINT.

### Flujo del frontend (`gpu-monitor-tray`)

`client::spawn` mantiene un loop de SSE con backoff (1s → 2s → 4s → 5s tope) que se resetea al recibir `Event::Open`. Publica `Update::Connected(snapshot)` o `Update::Disconnected(error)` por mpsc.

El loop principal en `main.rs` consume el mpsc y hace `handle.update(|tray| tray.set_state(...))` sobre el `ksni::TrayService`.

`tray::GpuTray::set_state` llama a `refresh_icon_file`: rerenderiza el PNG con `IconRenderer::render_png`, lo escribe en `~/.cache/gpu-monitor/icons/gpu-monitor-tray-N.png` (donde N es un contador que solo crece), y borra el frame anterior. La impl `Tray` publica `IconName = gpu-monitor-tray-N` y `IconThemePath = ~/.cache/gpu-monitor/icons`.

#### Punto crítico: por qué un PNG en disco y no `IconPixmap`

SNI permite mandar el icono como bytes ARGB inline. **No lo hagas** — la extensión `ubuntu-appindicators` de GNOME comprime los pixmaps anchos a una proporción cuadrada y mangle el icono multi-GPU. La estrategia de archivo + `IconName` con contador incremental es exactamente lo que `AppIndicator3.set_icon_full(path, ...)` hace internamente y es la única que GNOME respeta a anchura nativa.

Si alguien "limpia" esto sustituyéndolo por `icon_pixmap()` se rompe visualmente en GNOME aunque pase los tests.

### Render del icono (`icon::render`)

`tiny-skia` produce RGBA **premultiplicado**. Hay dos rutas de salida:

- `unpremultiply_to_rgba` → para el PNG del disco (los visores PNG asumen straight RGBA).
- `rgba_premul_to_argb_straight` → para `IconPixmap` por si en algún momento se vuelve a usar.

Las dos están testeadas. Si añades una nueva ruta de salida, comprueba qué espacio de alpha espera el consumidor antes de mandarle bytes.

Texto: `ab_glyph` con DejaVu Sans Mono **Bold** (Regular se ve borroso a 12 px sin hinting). El coverage del rasterizado se eleva con `coverage.sqrt()` (gamma 2.0) antes de mezclar — sin esto los strokes finos quedan en alpha ~50 y el texto es ilegible. La fuente se busca en una lista de paths candidatos en `DEFAULT_FONT_PATHS`; si falta, requiere `fonts-dejavu-core`.

## Convenciones del repo

- **API versioning** por prefijo de path (`/v1/...`). Romper compat = subir a `/v2/`. `gpu_monitor_core::API_VERSION` es la fuente de verdad.
- **Tipos serializados** viven en `gpu-monitor-core`. Si añades un campo a `Snapshot` / `Gpu` / `Process`, tanto backend como tray lo ven sin drift, pero **es un cambio de schema** — clientes externos pueden romperse.
- **Defaults seguros**: el daemon bindea `127.0.0.1` sin auth. Cuando se añada bind LAN (roadmap v2.1), `--auth-token` debe ser obligatorio si `--bind != 127.0.0.1`.
- **Dependencias compartidas** declaradas en `[workspace.dependencies]` del `Cargo.toml` raíz; los crates las referencian con `{ workspace = true }`. Solo añade deps específicas al `Cargo.toml` del crate cuando solo lo use ese crate.
- **Logging** vía `tracing` + `tracing-subscriber` en ambos binarios; controlable con `RUST_LOG` o `--log-level`.
- **Tests del frontend** evitan dependencias de runtime gráfico: render del icono se testa generando pixmaps y comparando bytes, no abriendo ventanas. NVML se testea contra `MockSource`, nunca contra hardware real.
