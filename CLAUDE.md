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

### Frontend macOS (`front-mac/`)

Swift Package independiente. **No vive en el workspace Cargo** — es Swift + AppKit, sin deps externas.

```bash
cd front-mac
swift build -c release                       # binario en .build/release/GPUMonitorTray
./scripts/build-app.sh                       # empaqueta "GPU Monitor.app" en build/
open "build/GPU Monitor.app" --args --backend-url http://127.0.0.1:9123

# Volcar el icono renderizado a un PNG (mismo flag que el tray Linux):
./.build/release/GPUMonitorTray --backend-url http://127.0.0.1:9123 --dump-icon /tmp/icon.png
```

El backend bindea `127.0.0.1` por defecto (ver "Defaults seguros"). Para acceder desde el Mac al daemon en otra máquina sin abrir el bind LAN, **SSH port forward**:

```bash
ssh -fN -L 9123:127.0.0.1:9123 <ubuntu-host>
open "build/GPU Monitor.app" --args --backend-url http://127.0.0.1:9123
```

Tras tocar código Swift hay que **re-empaquetar el `.app`** (`./scripts/build-app.sh`) — `swift build` solo regenera `.build/release/GPUMonitorTray`, no copia el binario al bundle. Lanzar `open GPU Monitor.app` con el bundle viejo es la confusión #1 cuando algo "no se actualiza".

## Arquitectura

Workspace Cargo con tres crates Rust + un Swift Package separado:

```
crates/gpu-monitor-core    →  tipos compartidos (serde Snapshot/Gpu/Process)
crates/gpu-monitord        →  daemon HTTP+SSE que lee NVML
crates/gpu-monitor-tray    →  frontend Linux (system tray)
front-mac/                 →  frontend macOS (Swift + AppKit, NSStatusItem)
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

Texto: `fontdue` con DejaVu Sans Mono **Regular** y tamaño `0.50 * h` (mismo perfil que el tray Python con PIL/freetype). `fontdue` aplica hinting TrueType, que es lo que mantiene los strokes finos pixel-aligned y nítidos a 11 px. Versiones previas usaban `ab_glyph` (sin hinting) con Bold + gamma `coverage.sqrt()` para compensar; el resultado era texto gordo y borroso comparado con el Python original. Si vuelves a `ab_glyph` o desactivas hinting, el texto pierde definición — verifícalo con `--dump-icon` antes de mergear. La fuente se busca en una lista de paths candidatos en `DEFAULT_FONT_PATHS`; si falta, requiere `fonts-dejavu-core`.

### Frontend macOS (`front-mac/`)

Swift Package con un único target ejecutable. Sin deps externas — todo lo que se usa viene del SDK (Foundation, AppKit, CoreGraphics, CoreText, ImageIO).

`Sources/GPUMonitorTray/`:
- `main.swift` — entry point. `NSApp.run()` en modo normal; `--dump-icon` corre síncrono y sale.
- `AppDelegate.swift` — instala `StatusBarController` + bridge AsyncStream entre el callback Sendable del cliente SSE y el consumidor `@MainActor`.
- `Config.swift` — parser CLI hecho a mano (sin `swift-argument-parser`). Flags: `--backend-url`, `--icon-height`, `--log-level`, `--dump-icon`, `--version`.
- `Models.swift` — `Codable` que reflejan `gpu-monitor-core::model`. Si tocas el schema en Rust, replica aquí.
- `Client.swift` — `actor SSEClient` con `URLSession.bytes(for:)` + backoff 1→2→4→5s.
- `IconRenderer.swift` — CoreGraphics + CoreText. Render a 2× para Retina, devuelve `NSImage` con `size` lógico.
- `StatusBarController.swift` — `@MainActor` envoltorio de `NSStatusItem` + `NSMenu`.

#### Punto crítico #1: `Foundation.AsyncBytes.lines` colapsa los `\n\n`

El protocolo SSE separa eventos con una línea en blanco. **`bytes.lines` no la entrega nunca** — colapsa cualquier secuencia de saltos consecutivos, así que el "flush trigger" canónico jamás dispara. La consecuencia es que el cliente parece conectado (recibe líneas `data:`) pero nunca decodifica un `Snapshot`.

Solución: **decodificar después de cada línea `data:`**. `gpu-monitord` envía un `Snapshot` completo por línea, así que el JSON siempre es self-contained. Si en el futuro pasamos a multi-line `data:` habrá que volver al flush por blank-line con un parser de `bytes` crudos (no `.lines`).

#### Punto crítico #2: NO hagas KVO sobre `effectiveAppearance`

Un commit anterior observaba `statusItem.button.effectiveAppearance` para repintar al cambiar light↔dark. Resultado: **CPU sostenida al 85–95%**. AppKit re-evalúa `effectiveAppearance` durante repaints normales, y cualquier reacción ahí entra en bucle: `set image → repaint → KVO → re-render → set image`.

Solución: suscribirse a `DistributedNotificationCenter` con `AppleInterfaceThemeChangedNotification`. Es el evento del sistema, no se dispara en repaints. Adicionalmente hay dedupe de renders: si los inputs visibles (idx, temp, %mem redondeado, appearance) no cambian, se salta tanto el `CGContext` como el `setImage`. A 1 Hz la mayoría de ticks tienen estado idéntico.

#### Punto crítico #3: bundle `.app` ≠ binario de `swift build`

`swift build -c release` actualiza `.build/release/GPUMonitorTray`. **No toca** el binario dentro de `build/GPU Monitor.app/Contents/MacOS/`. Si lanzas el `.app` después de cambiar código, sigue corriendo la versión vieja y nada parece tener efecto. Siempre `./scripts/build-app.sh` antes de `open` cuando hayas editado fuentes.

#### Otras decisiones

- **`NSStatusItem.variableLength`** para acomodar iconos multi-GPU. Sin esto, macOS recorta a un cuadrado.
- **`NSImage.isTemplate = false`**: el icono lleva colores propios (donut + base PNG). Si lo pones a `true`, macOS lo tiñe con el accent del sistema y pierdes el código de colores.
- **Texto blanco hardcoded**: `NSStatusBarButton.effectiveAppearance` discrepa del color visible de la barra (hereda de `NSApp`). Detectar dynamically devolvía `.light` incluso en barra oscura. La solución probada: blanco siempre, que combina con cualquier wallpaper razonable y mantiene legibilidad.
- **`monospacedDigitSystemFont`** para el label numérico — coincide con el formato del reloj y la batería del sistema. Se nota.
- **`--dump-icon` síncrono**: usar `URLSession.shared.dataTask` con `semaphore.wait()` en main thread bloquea el `MainActor` y la task del renderer nunca corre. Solución: `Data(contentsOf:)` síncrono + `CGImageDestination` para escribir el PNG.
- **`LSUIElement = true`** en `Info.plist`: app menubar-only, sin Dock icon, sin menú "Aplicación".

## Convenciones del repo

- **API versioning** por prefijo de path (`/v1/...`). Romper compat = subir a `/v2/`. `gpu_monitor_core::API_VERSION` es la fuente de verdad.
- **Tipos serializados** viven en `gpu-monitor-core`. Si añades un campo a `Snapshot` / `Gpu` / `Process`, tanto backend como tray lo ven sin drift, pero **es un cambio de schema** — clientes externos pueden romperse. **Acuérdate de replicarlo también en `front-mac/Sources/GPUMonitorTray/Models.swift`** — Swift no comparte el crate `gpu-monitor-core`, así que cualquier campo nuevo es un cambio paralelo manual.
- **Defaults seguros**: el daemon bindea `127.0.0.1` sin auth. Cuando se añada bind LAN (roadmap v2.1), `--auth-token` debe ser obligatorio si `--bind != 127.0.0.1`.
- **Dependencias compartidas** declaradas en `[workspace.dependencies]` del `Cargo.toml` raíz; los crates las referencian con `{ workspace = true }`. Solo añade deps específicas al `Cargo.toml` del crate cuando solo lo use ese crate.
- **Logging** vía `tracing` + `tracing-subscriber` en ambos binarios; controlable con `RUST_LOG` o `--log-level`.
- **Tests del frontend** evitan dependencias de runtime gráfico: render del icono se testa generando pixmaps y comparando bytes, no abriendo ventanas. NVML se testea contra `MockSource`, nunca contra hardware real.
