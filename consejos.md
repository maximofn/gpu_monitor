# Consejos para migrar `cpu_monitor`, `ram_monitor`, `disk_monitor` de Python a Rust

Trampas reales que pisé migrando `gpu_monitor` y las decisiones que funcionaron. Léelo antes de empezar para no perder horas en lo mismo.

## Arquitectura — copia tal cual

Workspace Cargo con tres crates por monitor:

```
<monitor>-core    →  tipos serde compartidos (Snapshot del recurso)
<monitor>d        →  daemon HTTP+SSE
<monitor>-tray    →  frontend Linux (system tray)
```

**Decisión ya tomada para esta familia de monitores**: tres workspaces independientes, uno por recurso (`cpu_monitor/`, `ram_monitor/`, `disk_monitor/`). NO consolides en un único `system-monitord`.

Razón: alguien sin GPU NVIDIA debe poder instalarse solo CPU + RAM + Disk sin que aparezca `gpu-monitord` como dependencia. Empaquetarlos juntos obligaría a tirar todo o nada. Cada repo es independiente y deployable por separado.

Implicaciones que asumimos:

- Tres `systemctl --user` units (`cpu-monitord.service`, `ram-monitord.service`, `disk-monitord.service`).
- Tres iconos en la barra del sistema, lado a lado.
- **Puertos distintos** para que puedan correr a la vez: gpu=9123, cpu=9124, ram=9125, disk=9126. Documenta el tuyo en `<monitor>_core::DEFAULT_PORT`.
- Duplicación de boilerplate por repo (CLAUDE.md, .gitignore, packaging/, scripts de install). Es el precio de la independencia — no intentes factorizar a un crate compartido entre repos, complica el despliegue.
- En el README de cada uno, **enlaza a los hermanos** ("Si quieres también CPU/RAM/Disk: ver…") para que el usuario los descubra.

## Toolchain y dependencias

- `rust-toolchain.toml` con `channel = "stable"`. **No pines a una versión concreta** — varias deps populares (`time`, `clap`, `tower-http`) requieren Rust ≥1.85 y van subiendo.
- `[workspace.dependencies]` en el `Cargo.toml` raíz para todo lo compartido (`tokio`, `serde`, `tracing`, `anyhow`). Los crates miembros referencian con `{ workspace = true }`.
- **`chrono`, no `time`**. El crate `time` arrastra requisitos de Rust muy recientes y rompe builds que funcionarían perfectamente.

## Backend (`<monitor>d`) — patrón obligatorio

```rust
trait CpuSource: Send + Sync {            // o RamSource / DiskSource
    fn sample(&self) -> Result<CpuSnapshot>;
}
struct ProcfsSource { ... }                // producción
struct MockSource { ... }                  // tests + CI sin hardware
```

- **Inicializa fuentes UNA VEZ al arranque**, mantenlas en `Arc`. Para GPU era NVML; para CPU/RAM/Disk son file handles abiertos de `/proc/stat`, `/proc/meminfo`, `/proc/mounts`. **No abras archivos por petición HTTP.**
- Sampler en task de tokio que publica en `tokio::sync::watch::Sender<Snapshot>`. Handlers HTTP leen `rx.borrow().clone()` (latencia µs).
- SSE con `tokio_stream::wrappers::WatchStream` (necesita feature `"sync"` en `tokio-stream` — fácil de olvidar).
- **NO uses `axum::serve(...).with_graceful_shutdown(signal)`**. Espera a que se vacíen las conexiones; SSE es infinita ⇒ `systemctl stop` queda colgado. Usa:
  ```rust
  tokio::select! {
      r = axum::serve(listener, app) => r?,
      _ = shutdown_signal() => {}
  }
  ```

### Fuentes de datos por monitor

- **CPU**: `/proc/stat`. Calcula % como **delta** entre dos samples consecutivos (`(busy_now - busy_prev) / (total_now - total_prev)`). Una sola lectura no da utilización. Considera `procfs` crate o leer a mano (más rápido).
- **RAM**: `/proc/meminfo`. Reporta también `MemAvailable` (lo que el kernel considera libre, distinto de `MemFree`).
- **Disk**:
  - Espacio: `statvfs()` por mount point (crate `nix`). **Cuidado**: usar `f_frsize * f_blocks` (no `f_bsize`) para el total real.
  - I/O: `/proc/diskstats` con delta entre samples como CPU.
  - Lista de mounts: `/proc/mounts` filtrando pseudo-FS (`tmpfs`, `proc`, `sysfs`, `cgroup*`, `overlay`, etc.).
- Considera `sysinfo` si quieres atajar; cubre los tres pero impone su propio modelo y añade overhead.

### Procesos por recurso

GPU usaba `running_compute_processes()` de NVML. Para CPU/RAM no hay equivalente — itera `/proc/[0-9]+/` leyendo `stat` y `status`. Para disk I/O por proceso necesitas `/proc/<pid>/io`. **No spawnees `top`/`ps`** (regex frágil, 50–200 ms por llamada).

## Frontend Linux — el problema que más tiempo me costó

### **GNOME aplasta los `IconPixmap` anchos.** No te creas el plan inicial.

`ksni` ofrece `icon_pixmap()` para mandar el icono ARGB inline por DBus. **No funciona** en Ubuntu/GNOME con la extensión `ubuntu-appindicators`: comprime los pixmaps a proporción cuadrada y mangle iconos multi-elemento. El icono se ve squisheado/ilegible.

**Solución única que funciona** (es lo que hace `AppIndicator3.set_icon_full(path)` por debajo):

1. Renderizar a `Pixmap` con `tiny-skia`.
2. Codificar como PNG (con alpha **unpremultiplicado** — ver abajo).
3. Escribirlo en `~/.cache/<monitor>/icons/<basename>-N.png` con `N` un contador que **solo crece**.
4. Borrar el archivo del frame anterior.
5. En la impl `ksni::Tray`:
   ```rust
   fn icon_name(&self) -> String { self.current_icon_name.clone() }     // "<basename>-N"
   fn icon_theme_path(&self) -> String { self.icon_dir.to_string_lossy().into_owned() }
   // NO implementes icon_pixmap()
   ```

El contador en el nombre fuerza a GNOME a releer el archivo (cachea por nombre). Si reutilizas nombre, no se actualiza.

Si alguien "limpia" esto en el futuro reemplazándolo por `icon_pixmap()` se rompe visualmente aunque pasen los tests. Coméntalo en el código.

### Premultiplicado de alpha — dos rutas distintas, no las confundas

`tiny-skia` produce **RGBA premultiplicado**. Hay dos consumidores con expectativas distintas:

- **PNG en disco** (lo que GNOME lee): RGBA **straight**. Hay que dividir cada canal RGB por alpha:
  ```rust
  let v = (c as u32 * 255 + a as u32 / 2) / a as u32;
  ```
- **`IconPixmap` SNI** (por si vuelves a usarlo algún día): ARGB straight, network byte order. Mismo unpremul + reordenar bytes a `[A, R, G, B]`.

Si solo haces swizzle de bytes sin unpremul, los bordes anti-aliased salen oscuros/desaturados. Es muy sutil — el archivo PNG se ve "raro pero quizás bien" hasta que lo compones sobre un fondo y notas que blanco-50% sale gris-25%.

### Texto en el icono — usa `freetype-rs`, ni `fontdue` ni `ab_glyph`

Iteré en este orden y solo el tercero igualó al Python:

1. **`ab_glyph`** (sin hinting). Trazos finos a 11px quedan a alpha ~50, casi invisibles. Workaround feo: Bold + gamma `coverage.sqrt()`. Sale gordito y emborronado.
2. **`fontdue`** (con hinting básico). Mejor que `ab_glyph`, pero NO iguala al Python. fontdue hace su propio hinting simplificado, no ejecuta el TrueType bytecode interpreter — los stems no snapean al pixel grid con la fuerza de freetype. A 10–11 px todavía se ve un puntito más suave / fofo que el Python.
3. **`freetype-rs`** (wrapping libfreetype del sistema). **El único que iguala al Python**, porque PIL usa freetype por debajo. Bytecode interpreter real, stem snapping fuerte, trazos pixel-perfect a 10 px.

Verdict final: **`freetype-rs` + Regular weight + tamaño redondeado a entero (`(h * 0.45).round()`)** + `LoadFlag::RENDER | LoadFlag::TARGET_NORMAL`. Posiciones de glifo redondeadas al pixel antes de pintar (`pen_x.round()`, `baseline_y.round()`). Mira `crates/gpu-monitor-tray/src/icon/render.rs` para el patrón completo.

Gotchas de `freetype-rs`:

- **Versión pinneada**: en Ubuntu 20.04 la libfreetype del sistema es 23.1.17 (lo que pkg-config reporta como ABI version, no la versión "humana" 2.10.1). `freetype-rs 0.37` exige `freetype2 >= 24.3.18` (sistema con freetype 2.12+). Para 20.04 usa **`freetype-rs = "0.32"`**. Si tu distro es más nueva puedes subir.
- **No implementa `Send`**: `Library` y `Face` contienen punteros C crudos. `ksni::TrayService::spawn` exige `Send` en el state. Solución: envolver ambos en una struct con `unsafe impl Send + Sync`. Es seguro porque el acceso aquí es secuencial (solo desde la callback de `update` del thread de ksni), nunca concurrente.
  ```rust
  struct FtState { _library: Library, face: Face }
  unsafe impl Send for FtState {}
  unsafe impl Sync for FtState {}
  ```
- **API en 26.6 fixed-point**: `glyph.advance().x` y `size_metrics().ascender` vienen en 26.6, hay que hacer `>> 6` para pasarlos a píxeles enteros.
- **Pixel size entero**: `face.set_pixel_sizes(0, px_u32)` solo acepta enteros. Si tu factor da `9.9` redondea a `10`.

Empieza directamente con `freetype-rs` — si vas por `ab_glyph` o `fontdue` perderás horas iterando workarounds que no llegarán a la calidad de PIL. **Compáralo siempre contra el Python en una imagen lado-a-lado, no en aislamiento.**

Carga la fuente con un fallback de paths candidatos (`/usr/share/fonts/truetype/dejavu/...`). Si no hay, falla con un mensaje claro pidiendo `apt install fonts-dejavu-core`. También necesitas `libfreetype-dev` en build (run-time solo `libfreetype6`).

### Cuidado con caracteres "casi iguales": ºC ≠ °C

El Python original usa `ºC` con **U+00BA (Indicador Ordinal Masculino)**, no `°C` con U+00B0 (Signo de Grado). Visualmente son casi idénticos, pero copia el carácter exacto del Python para no romper la coincidencia byte-a-byte cuando alguien diff-ea o busca por texto. DejaVu Mono soporta ambos.

### Cliente SSE — backoff razonable

`reqwest-eventsource` con cap de **5s**, no 30s. Resetea el backoff al recibir `Event::Open`. 30s da una UX horrible cuando el daemon se reinicia rápido.

## `.desktop` autostart — `%h` NO es estándar

El placeholder `%h` que algunos ejemplos usan no se sustituye en GNOME Shell. Usa:

```
Exec=sh -c "exec $HOME/.local/bin/<binary>"
Icon=tarjeta-de-video        # nombre lógico, no path
```

`Icon=` con path absoluto funciona pero el nombre lógico es más portable.

### Modelo de arranque: daemon ≠ tray

Decisión deliberada que no es obvia hasta que la pisas:

- **Daemon (`<monitor>d`)**: systemd `--user` service. Se reinicia con `systemctl --user restart <monitor>d`.
- **Tray (`<monitor>-tray`)**: `.desktop` autostart en `~/.config/autostart/`, **NO es un servicio systemd**. Se lanza al login y vive hasta logout.

¿Por qué? El tray necesita la sesión gráfica (DBus user bus, panel SNI). systemd-user arranca antes que la sesión gráfica completa en algunos compositores; con un `.desktop` te aseguras que el tray solo arranca cuando hay panel donde plantar el icono.

Consecuencia: tras `cargo build` + `install` del binario nuevo del tray, **`systemctl --user restart <monitor>-tray` falla con "Unit not found"**. El flujo correcto:

```bash
install -m 0755 target/release/<monitor>-tray ~/.local/bin/
pkill -x <monitor>-tray                    # mata el viejo
nohup ~/.local/bin/<monitor>-tray >/dev/null 2>&1 & disown
```

O simplemente logout/login si no tienes prisa. Documenta esto en el README de cada monitor — es la pregunta #1 que se hace cualquiera tras hacer `cargo build`.

## Localización del icono base

Para que el binario instalado funcione independiente de dónde se compiló, busca en este orden:

1. `$GPU_MONITOR_TRAY_ICON` (override env var)
2. `$XDG_DATA_HOME/<monitor>/<icon>.png` (típicamente `~/.local/share/<monitor>/`)
3. `/usr/share/<monitor>/<icon>.png`
4. `assets/<icon>.png` relativo al cwd (dev)
5. `<workspace>/assets/<icon>.png` baked-in via `env!("CARGO_MANIFEST_DIR")` (último recurso, solo funciona en la máquina de build)

## systemd `--user` service

```ini
[Unit]
After=network.target
[Service]
Type=simple
ExecStart=%h/.local/bin/<monitord>
Restart=on-failure
RestartSec=2s
MemoryMax=128M
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=read-only       # NO uses 'strict' — el tray necesita escribir en ~/.cache
PrivateTmp=true
Environment=RUST_LOG=info
[Install]
WantedBy=default.target
```

`%h` SÍ funciona en unit files de systemd (al revés que en `.desktop`).

## Test sin hardware

`MockSource` que sirve datos sintéticos coherentes (CPU al 25%, RAM medio llena, disco con dos mounts). Permite:
- CI sin GPU/permisos especiales
- Iterar el frontend sin `/proc` real
- Snapshots de regresión del PNG renderizado

`cargo test --workspace` debe pasar en cualquier máquina, no solo donde corra producción.

## CLI flags y env vars

`clap` con `#[arg(long, env = "...")]` para que cada flag tenga su variable equivalente. Mínimo:
- `--bind` (default `127.0.0.1`, NO LAN sin auth)
- `--port` (cada monitor el suyo)
- `--sample-interval-ms` (default 1000, mínimo 50)
- `--log-level` (= `RUST_LOG`)
- `--mock` (forza `MockSource`)
- `--dump-icon <path>` solo en el tray, vital para depurar visual sin pelearte con GNOME

## Fases de migración (replica el plan probado)

1. **Fase 0**: workspace + 3 crates vacíos + `.gitignore` con `target/`, `*.rs.bk`, `.claude/`. **NO muevas el script Python a `legacy/` todavía** (rompes su autostart). Cópialo cuando hagas cutover.
2. **Fase 1**: backend con REST. Validar con `curl | jq` contra el script Python en debug mode.
3. **Fase 2**: SSE.
4. **Fase 3**: tray Linux. Iterar el icono con `--dump-icon /tmp/x.png` + visor de imágenes (componer sobre fondo gris/oscuro/claro al 5–8× para ver lo que el panel verá realmente).
5. **Fase 4 (cutover, requiere OK explícito del usuario)**: instalar binarios a `~/.local/bin/`, instalar systemd unit, swap del autostart `.desktop`, mover `.py` a `legacy/`, limpiar PNGs basura del repo, tag `v2.0.0-rc1`. Tras una semana sin issues, retag a `v2.0.0` y borrar `legacy/`.

## Errores que cometí y no debes repetir

- Programar `ScheduleWakeup` con sentinel `<<autonomous-loop-dynamic>>` cuando solo necesitaba esperar 60s. Para esperar usa `until`-loop dentro de `Bash` o `run_in_background: true`.
- Confiar en el preview de imagen interno del agente para validar el icono. Es engañoso con fondos transparentes — siempre componer sobre fondo opaco antes de juzgar. El comando que sí funciona:
  ```bash
  convert /tmp/before.png -background "#222" -flatten -scale 600% /tmp/before.png
  convert /tmp/after.png  -background "#222" -flatten -scale 600% /tmp/after.png
  convert /tmp/before.png /tmp/after.png -append -bordercolor "#444" -border 4 /tmp/cmp.png
  ```
  Componer y escalar 6× revela diferencias de stroke-weight invisibles a tamaño nativo.
- Asumir que `pgrep -f` con un patrón coge solo lo que quieres. Mátalos por **PID concreto** (`kill <pid>`), no por patrón en sandbox compartido.
- Pushear directo a `main` sin avisar. La sandbox lo bloquea (correcto). Aunque el repo sea solo, di explícitamente "voy a pushear a main" y deja que el usuario lo haga si la política lo bloquea.
- Justificar una decisión visualmente cuestionable con un comentario en `CLAUDE.md` ("Bold + gamma porque sin ello el texto es ilegible"). Si la doc explica un workaround feo, **el workaround probablemente está mal** — busca la causa raíz (en este caso: `ab_glyph` no hace hinting, la solución era cambiar de rasterizador). No "documentes" un mal sabor: arréglalo.
- Confundir `cargo fmt --all` con un cambio sin efecto. Reformatea archivos que no tocaste y ensucia el commit. Si tu cambio es localizado, usa `cargo fmt -p <crate>` o aplica fmt **antes** de empezar (en un commit separado) para que tu PR contenga solo lógica.
- Asumir que un reemplazo pure-Rust iguala a la lib C de referencia. `fontdue` "hace hinting", sí — pero no ejecuta el TrueType bytecode interpreter como freetype. A pequeño tamaño la diferencia es visible. **Si Python usa libX por debajo, usa libX, no su clon pure-Rust.** El trabajo de igualar visualmente a freetype con un rasterizador alternativo no se cierra.
- Usar `pkill -x <binary>` para matar el tray. **El kernel trunca `comm` (la columna de `ps`) a 15 caracteres**; un nombre como `gpu-monitor-tray` (16 chars) NO matchea exact con `-x`. Mata por PID concreto tras `pgrep -f "<full-path>$"`, o usa `pkill -f "<full-path>$"` con el path completo (que no se trunca porque vive en cmdline, no en comm).
- Pretender más precisión de la que da el sensor. NVML expone temperatura como `u32` Celsius — no hay `.5` ni `.7`. Mostrar `45.0ºC` solo añade ruido visual fingiendo precisión que el hardware no entrega. Refleja la precisión real de la fuente.

## Comparativa de recursos a la que aspirar

Medido en `gpu_monitor` (RTX 3090 ×2, samples cada segundo):

| | Python original | Rust back+front |
|---|---|---|
| RSS | 147 MB | ~24 MB |
| CPU | 160% (1.6 cores) | 1.5% |

Para CPU/RAM/Disk la mejora será aún mayor proporcionalmente — el Python original probablemente arrastra menos peso (sin matplotlib si no pinta gráficos), pero el coste relativo de Rust es mínimo.

## Lecturas obligatorias antes de empezar

- `CLAUDE.md` de este repo — covers el patrón completo del backend y los gotchas del icono.
- `crates/gpu-monitor-tray/src/icon/render.rs` — implementación completa de iconos con `tiny-skia` + `freetype-rs` + ambas conversiones de alpha.
- `crates/gpu-monitor-tray/src/tray.rs` — patrón `set_state` + `refresh_icon_file` + `IconName`/`IconThemePath` por SNI.
- `crates/gpu-monitord/src/sampler.rs` — patrón sampler + watch channel.
