# Consejos para migrar `cpu_monitor`, `ram_monitor`, `disk_monitor` de Python a Rust

Trampas reales que pisé migrando `gpu_monitor` y las decisiones que funcionaron. Léelo antes de empezar para no perder horas en lo mismo.

## Arquitectura — copia tal cual

Workspace Cargo con tres crates por monitor:

```
<monitor>-core    →  tipos serde compartidos (Snapshot del recurso)
<monitor>d        →  daemon HTTP+SSE
<monitor>-tray    →  frontend Linux (system tray)
```

**Decisión ya tomada para esta familia de monitores**: workspaces independientes, uno por recurso (`cpu_monitor/`, `ram_monitor/`, `disk_monitor/`, `input_audio_device/`, `output_audio_device/`). NO consolides en un único `system-monitord`.

Razón: alguien sin GPU NVIDIA debe poder instalarse solo CPU + RAM + Disk sin que aparezca `gpu-monitord` como dependencia. Empaquetarlos juntos obligaría a tirar todo o nada. Cada repo es independiente y deployable por separado.

Implicaciones que asumimos:

- Una `systemctl --user` unit por monitor (`cpu-monitord.service`, ...).
- Un icono por monitor en la barra del sistema, lado a lado.
- **Puertos distintos** para que puedan correr a la vez: gpu=9123, cpu=9124, ram=9125, disk=9126, input-audio=9127, output-audio=9128. Documenta el tuyo en `<monitor>_core::DEFAULT_PORT`.
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

### `IconThemePath` debe ser absoluta o GNOME enseña tres puntos

Síntoma: el tray arranca limpio, los logs dicen "SSE stream open", el icono **no aparece** y en su lugar el panel muestra tres puntos (placeholder genérico de GNOME-shell cuando el SNI publica un `IconName` que su resolver no encuentra). El bug es siempre el mismo: alguien pasa una ruta relativa a `icon_theme_path()`.

GNOME-shell **no resuelve rutas relativas al cwd del proceso del tray**. Tampoco te lo dice — falla silenciosamente y pinta el placeholder. Asegúrate de canonicalizar antes de mandárselo a ksni:

```rust
fn icon_theme_path(&self) -> String {
    self.icon_dir.canonicalize()                  // o canonicalize() en el setup,
        .unwrap_or_else(|_| self.icon_dir.clone()) // y guardar el resultado
        .to_string_lossy()
        .into_owned()
}
```

Lo pisé con `output_audio_device`: el binario corría desde la raíz del workspace y publicaba `IconThemePath = "assets"`. Funciona en KDE (que sí walkea relativo en algunos paths), no en GNOME. Soluciona en el `prepare_icon_dir` con `dir.canonicalize()?` antes de devolverlo.

### Monitores sin overlay numérico: salta `tiny-skia` + `freetype-rs` enteros

Si tu monitor solo necesita un **icono identificable** y no pinta una métrica numérica encima (CPU%, temperatura, MB libres, ...) no metas el stack de rendering. El tray de `output_audio_device` es solo "muestra un altavoz, abre menú con sinks": apunta `IconThemePath` al directorio donde vive `speaker.png` instalado, `IconName = "speaker"`, y listo. Nada de `tiny-skia`, nada de `freetype-rs`, nada de `~/.cache/<monitor>/icons/`. El binario adelgaza ~2 MB y arranca al instante.

Aplica si:
- El monitor es de control (cambiar default sink, mute/unmute, conmutar perfil) más que de telemetría.
- La métrica no cabe en el icono a tamaño tray (ej. lista de N elementos donde N varía).
- Vas a iterar el menú, no el icono.

Si más tarde quieres **feedback visual de disconnected** (icono gris cuando el daemon no responde), solo entonces metes `tiny-skia` para componer una versión gris al vuelo. Sigue sin necesitar `freetype-rs` mientras no haya texto.

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

### Layout y código de colores del icono

Valores destilados tras varias iteraciones con el usuario mirando capturas reales del panel. No los elijas de cero — copia y ajusta solo si el monitor tuyo tiene métricas distintas.

**Layout** (con `icon_height = 22` px):

```
[icono recurso ~22x22] 2px [label texto] 2px [donut 18x18]   <gap 4px>   [siguiente bloque...]
```

- **Tamaño de texto del label**: `(h * 0.45).round()` = 10 px. Probé 0.50 (sale grande), 0.40 (sale ilegible).
- **Tamaño del donut**: `h - DONUT_PADDING*2` = 18 px. `r_inner = r_outer * 0.78` ⇒ anillo del 22% de grosor (probé 0.55 grueso, 0.72 medio, 0.80 demasiado fino).
- **Texto del número dentro del donut**: 8 px hardcoded (depende del inner diameter, no de `h`). Encaja "100" justo justo en el inner diameter de 14 px sin tocar el anillo.

**Ancho del label: per-bloque vs. máximo global.** Decisión que depende de si el contenido del label cambia frame a frame:

- **GPU/CPU** (label incluye temperatura, ej. `0(45ºC)`): calcular el ancho como `measure_text("0(00ºC)")` — el **máximo posible** entre todos los GPUs, NO el de cada uno por separado. Si dejas que cada GPU tenga su propio ancho, los donuts saltan ±1–2 px frame a frame cuando la temperatura cruza de 2 a 3 dígitos (45 → 100). Visualmente espantoso a 1 Hz.
- **Disk** (label es el mountpoint corto, estático en la sesión): cada bloque mide su propio label. `/` ocupa 6 px, `seagat` ocupa 32 px — desperdiciar el ancho de `seagat` para `/` añadiría ~26 px de barra que el usuario no recupera. Como el label NO cambia dentro de una sesión, los donuts no saltan.

Regla: si la métrica que va dentro del label puede cambiar de ancho dentro de una sesión, máximo global; si es estática, per-bloque.

**Paleta** (constantes `[u8; 4]` RGBA en `render.rs`):

| const            | hex       | uso                                         |
|---               |---        |---                                          |
| `COLOR_TEXT`     | `#ffffff` | label normal, número dentro del donut       |
| `COLOR_FREE`     | `#66b3ff` | anillo: porción libre                       |
| `COLOR_OK`       | `#99ff99` | anillo lleno <70% del recurso               |
| `COLOR_WARN1`    | `#ffdb4d` | anillo 70–80%, **label cuando temp 60–80**  |
| `COLOR_WARN2`    | `#ffcc99` | anillo 80–90%                               |
| `COLOR_HIGH`     | `#ff6666` | anillo ≥90%, **label cuando temp ≥80**      |

Para CPU/RAM/disk reusa la misma paleta; cambia solo qué umbrales activan qué color para tu recurso.

**Estado disconnected**: todo el bloque pasa a gris `#aaaaaa` (texto), `#808080` (anillo libre), `#606060` (anillo usado). Sigue mostrando el último snapshot, pero la atenuación de color avisa de que los datos pueden estar viejos.

**Umbrales** (los que el usuario aceptó tras iterar):

- **Memoria** (color del wedge usado en el donut): 0–69% verde, 70–79% amarillo, 80–89% naranja, ≥90% rojo.
- **Temperatura** (color del label de texto **entero**): <60 ºC blanco, 60–79 ºC amarillo, ≥80 ºC rojo.

Un solo umbral combinado para texto + ºC suena obvio pero no lo era al principio: probé "solo los dígitos en amarillo", luego "dígitos + ºC", y al final "todo el label". Lo más legible a 22 px de alto resulta ser el label completo cambiando de color — el ojo capta el cambio cromático antes que un detalle dentro del label.

**Principio clave: cada métrica colorea solo SU elemento.** No mezcles. La temperatura **no** afecta al donut (que es de memoria) ni al número del centro. El número del centro va siempre en color neutro (`COLOR_TEXT` / gris) porque el wedge del anillo ya hace el código de colores para memoria. Si pintas el número en rojo cuando la memoria está al 95%, encima de un anillo ya rojo, queda redundante y ruidoso.

**Format del label**: `"{idx}({temp:>2}ºC)"`. Sin prefijo "GPU "/"CPU "/"RAM " — gana espacio del panel y el icono del recurso a la izquierda ya identifica qué es. El `:>2` reserva 2 chars de ancho para que `5ºC` y `45ºC` ocupen lo mismo y el donut no salte de posición al variar la temperatura.

**Coloreado del label en segmentos**: si en algún momento quieres colorear solo parte del label (ej. solo los dígitos de temperatura), parte el `format!` en 3 strings y haz 3 llamadas a `draw_text` avanzando con `measure_text` entre cada una. Funciona bien con monospace porque el ancho total no cambia. Pero el verdict tras probarlo fue: colorear el label entero es más legible a tamaño tray.

### Callbacks de menú con trabajo async — funelízalos por mpsc

Si tu tray va a hacer trabajo asíncrono cuando el usuario clica un `MenuItem` (HTTP POST al daemon, lectura de un fichero grande, lo que sea), **no llames `.await` directamente desde el `activate` callback**. Ese callback corre en el thread del servicio ksni, que no tiene runtime de tokio asociado: `reqwest::Client::post(...).send().await` falla con "no reactor running".

La solución limpia es un `tokio::sync::mpsc::Sender` que captura cada callback y una task async dedicada que consume el receptor:

```rust
let (action_tx, action_rx) = mpsc::channel::<Action>(8);
client::spawn_action_worker(backend_url, action_rx); // task async con reqwest
let tray_state = MyTray::new(action_tx, ...);

// dentro del menu():
MenuItem::Standard(StandardItem {
    activate: Box::new(move |_tray| {
        // try_send es safe desde cualquier thread; si el buffer está
        // lleno (8) descartamos el click — el usuario lo verá como
        // "no respondió" y volverá a clicar.
        let _ = action_tx.try_send(Action::SwitchSink(name.clone()));
    }),
    ..Default::default()
})
```

`try_send` no bloquea, no necesita runtime, y no entra en pánico si el canal está cerrado. **No** envuelvas el callback en `tokio::runtime::Handle::current().block_on(...)` — funciona hoy por accidente y se rompe en cuanto la task del servicio ksni cambia de thread.

Patrón implementado en `output_audio_device/crates/audio-monitor-tray/src/client.rs::spawn_switcher`.

### Endpoints mutadores — empuja el snapshot fresco al `watch::Sender`

Si tu daemon expone un POST que cambia el estado del recurso (en `output_audio_device` es `POST /v1/sinks/default` que llama a `pactl set-default-sink`), no esperes al próximo tick del sampler para que los clientes SSE vean el cambio. Tras la mutación:

```rust
state.source.set_default_sink(&req.name).await?;
let fresh = sampler::build_snapshot(&state.host, state.source.as_ref()).await;
let _ = state.snapshot_tx.send(fresh.clone());   // mismo Sender que tiene el sampler
Ok(Json(fresh))
```

El handler comparte el `tokio::sync::watch::Sender` con el sampler — `watch` coalesce internamente, da igual quién empuje el último snapshot. Latencia percibida del click → confirmación visual: ~50 ms en vez de hasta 1 s.

CPU/RAM/disk probablemente no necesitan endpoints mutadores (son monitores read-only), pero si añades uno en el futuro este es el patrón.

**Excepción real ya pisada en `disk_monitor`**: el daemon expone `POST /v1/rescan/{mount}` para forzar un walk de los archivos más grandes en un mount. Es un mutador "lateral" — no cambia métricas instantáneas, pero arranca trabajo en background cuyo resultado se publica en el siguiente snapshot. El daemon ya hace coalesce internamente (varios POSTs seguidos = 1 walk por mount), así que el cliente puede mandar sin pensar.

Desde el menú del frontend macOS lo dispara así, sin bloquear el cierre del menú:

```swift
@objc private func rescanMount(_ sender: NSMenuItem) {
    guard let mountPoint = sender.representedObject as? String else { return }
    let url = SSEClient.rescanURL(from: backendURL, mountPoint: mountPoint)
    Task.detached {
        var req = URLRequest(url: url); req.httpMethod = "POST"; req.timeoutInterval = 5
        _ = try? await URLSession.shared.data(for: req)
    }
}
```

`Task.detached` para que el handler del menú devuelva inmediatamente. Si haces `await` en el `@objc` directamente, el menú se queda colgado renderizando el "click feedback" hasta que vuelve la respuesta. Equivalente al patrón mpsc del tray Linux pero usando el runtime de Swift Concurrency en vez de tokio. **URL-encode el mount point** (`/media/wallabot/seagate2T` → percent-escape los `/`s después de stripping del leading) — si no, axum no matchea la ruta.

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
- Devolver una ruta relativa desde `icon_theme_path()`. El daemon arrancaba bien, ksni publicaba el SNI, los logs estaban limpios — y el panel mostraba tres puntos. GNOME-shell ignora paths relativos sin avisar. Canonicaliza siempre antes de devolver. Pisado en `output_audio_device`; tardé ~10 min en darme cuenta porque el log decía `dir=assets` y eso "parecía OK".
- Asumir que todo monitor de la familia necesita el stack `tiny-skia` + `freetype-rs`. Para `output_audio_device` (icono estático, métrica = lista de devices, no número) sobraba todo el rendering: 2 MB menos de binario, 0 dependencias nativas (sin `libfreetype-dev` en build, sin `fonts-dejavu-core` en runtime), iteración mucho más rápida. Pregúntate primero **qué pintas** antes de copiar el render del `gpu_monitor`.

## Comparativa de recursos a la que aspirar

Medido en `gpu_monitor` (RTX 3090 ×2, samples cada segundo):

| | Python original | Rust back+front |
|---|---|---|
| RSS | 147 MB | ~24 MB |
| CPU | 160% (1.6 cores) | 1.5% |

Medido en `output_audio_device` (icono estático, sin overlay numérico):

| | Python original (`output_audio_device.py`) | Rust back+front |
|---|---|---|
| RSS | ~42 MB | ~8 MB (4.4 MB daemon + 3.7 MB tray) |
| CPU | 0.8% | 0.1% |

Sin texto/donut por pintar el tray cae a ~4 MB; el daemon es minúsculo porque `pactl` ya hace el trabajo y nosotros solo parseamos su stdout.

Para CPU/RAM/Disk la mejora será aún mayor proporcionalmente — el Python original probablemente arrastra menos peso (sin matplotlib si no pinta gráficos), pero el coste relativo de Rust es mínimo.

## Frontend macOS — Swift, no Rust

Si vas a portar un tray de estos a macOS, **no lo hagas en Rust**. La cadena `tray-icon → muda → objc2 → AppKit` tiene ~3 años, depende de mantenedores externos y arrastra `tao` + tokio + rustls aunque no los necesites para nada. Idle ~20–35 MB RSS solo para tener un icono en la barra. Y cualquier bump de macOS interno puede romper la cadena sin que Apple te avise.

**Swift + AppKit gana en estabilidad y en consumo**: idle 10–18 MB RSS, deps externas = 0 (todo del SDK). `NSStatusItem` es API pública desde 2001, mantenida por Apple. El coste real es duplicar ~250 líneas (tipos `Codable`, cliente SSE, renderer de icono con CoreGraphics+CoreText). Vale la pena.

Layout aplicado en `front-mac/`:

```
Sources/GPUMonitorTray/
├── main.swift              # NSApp.run() | dump-icon síncrono
├── AppDelegate.swift       # bridge AsyncStream entre SSE Sendable y MainActor
├── Config.swift            # parser CLI a mano (sin swift-argument-parser)
├── Models.swift            # Codable que reflejan gpu-monitor-core
├── Client.swift            # actor SSEClient con URLSession.bytes(for:)
├── IconRenderer.swift      # CoreGraphics + CoreText (render @ 2× para Retina)
└── StatusBarController.swift  # @MainActor sobre NSStatusItem + NSMenu
```

### Trampas que pisé en el port a macOS

#### `Foundation.AsyncBytes.lines` colapsa los `\n\n` del SSE

SSE separa eventos con una línea en blanco. La doc oficial dice que esa blank line es el flush trigger. **`URLSession.bytes(for:).lines` no la entrega nunca** — colapsa secuencias de saltos consecutivos. Pasas líneas `data:` a un acumulador, esperas la blank line para decodificar… y nunca llega. El cliente queda eternamente en "connecting" aunque el byte stream esté vivo.

Solución: **decodificar después de cada `data:`**. `gpu-monitord` envía un `Snapshot` completo por línea, así que el JSON es self-contained. Si más adelante el backend pasa a multi-line `data:`, hay que parsear `bytes` crudos en vez de `.lines`.

#### KVO sobre `effectiveAppearance` mete a la app al 90% de CPU

Plan inicial: observar `statusItem.button.effectiveAppearance` para repintar al cambiar light↔dark. Resultado: bucle apocalíptico — AppKit re-evalúa `effectiveAppearance` durante repaints normales del icono, así que `set image → repaint → KVO dispara → re-render → set image → repaint…`. La barra del Mac se calienta físicamente.

Solución: **`DistributedNotificationCenter` con `AppleInterfaceThemeChangedNotification`**. Es el evento del sistema, solo dispara cuando el usuario realmente toggle-ea el tema. Y dedupe de renders por inputs visibles (idx, temp, %mem redondeado, appearance) — a 1 Hz la mayoría de ticks tienen estado idéntico.

Regla general: **nunca observes propiedades que AppKit re-evalúa durante el ciclo de pintado.** Si la propiedad cambia "porque sí" en un repaint y tu callback dispara un repaint, has hecho una bomba.

#### El bundle `.app` no se actualiza con `swift build`

`swift build -c release` actualiza `.build/release/GPUMonitorTray`. **No copia** el binario al `.app`. Si lanzas el bundle viejo después de cambiar código, sigues corriendo la versión anterior y nada parece responder. Pisado dos veces en una sesión.

Solución: el script de empaquetado (`./scripts/build-app.sh`) compila Y copia. Úsalo siempre antes de `open` cuando hayas tocado fuentes. La regla mental: "si edité Swift, re-empaqueto antes de abrir".

#### Menubar-only requiere `LSUIElement=true` en el `Info.plist`

Sin esa clave, el ejecutable se comporta como app normal: aparece en el Dock con icono genérico, tiene menú de aplicación, ocupa espacio. El bundle `.app` debe llevar `Info.plist` con `LSUIElement=true` y `LSMinimumSystemVersion=13.0` (mínimo para `URLSession.bytes(for:)` con cancelación limpia).

#### `NSImage.isTemplate` por defecto tiñe tu icono

Si publicas un `NSImage` y no tocas `isTemplate`, depende del SDK target qué pasa. Si lo dejas en `true`, macOS aplica el accent color del sistema y mata tu paleta (donut verde/amarillo/rojo se vuelven monocromos). Para un icono con colores propios: `image.isTemplate = false`. Explícito.

#### `monospacedDigitSystemFont` ≠ `monospacedSystemFont`

Para que el label de temperatura encaje con el formato del reloj y la batería del sistema (que es lo que el usuario espera ver al lado), usa `NSFont.monospacedDigitSystemFont(ofSize:weight:)`. Es SF Pro con dígitos de ancho fijo, NO SF Mono. Visualmente es un mundo: SF Mono parece "código", SF Pro con dígitos mono parece "métrica del sistema".

#### `effectiveAppearance` del status button miente

Pensé que detectaría si la barra está en modo claro u oscuro. Devolvía `.light` incluso con barra oscura. Razón: hereda de `NSApp`, y `NSApp.effectiveAppearance` no refleja el color real de la barra del Mac (que depende del wallpaper, no del tema del sistema). Tras una hora intentando detectar correctamente → **texto blanco hardcoded**. Combina con cualquier wallpaper razonable y mantiene legibilidad. Cuando la detección no funciona, no la fuerces — elige un default que funcione siempre.

#### `--dump-icon` síncrono o se cuelga el `MainActor`

Implementación naive: `URLSession.shared.dataTask` con un `DispatchSemaphore.wait()` en el main thread para esperar el snapshot. Bloquea el `MainActor`, la task del renderer nunca corre, deadlock. Solución: para el path de dump usar `Data(contentsOf:)` síncrono + `CGImageDestination` para escribir el PNG. La regla: **el path "render once and exit" no debe compartir async runtime con la app interactiva.**

#### `pkill -f GPUMonitorTray` no mata nada

Cargo target = `GPUMonitorTray`, pero el script de empaquetado renombra el binario a `gpu-monitor-tray-mac` al meterlo en el bundle (`Contents/MacOS/gpu-monitor-tray-mac`). `pkill -f GPUMonitorTray` no encuentra nada y crees que ya está parada cuando sigue corriendo. Mata por el nombre real: `pkill -f gpu-monitor-tray-mac` (o por PID concreto, mejor).

#### Lanzar el `.app` con `&` desde bash → muere por SIGHUP

`./.build/.../gpu-monitor-tray-mac --backend-url ... &` aparenta funcionar — el binario se queda en background, el icono aparece — pero al cerrar la sesión bash el shell manda SIGHUP a sus hijos y la app se va. Ni `disown` ni `nohup` se llevan bien con un Swift NSStatusItem en este flujo. **Lánzala con `open -n`**, que la registra en launchd como app GUI proper:

```bash
open -n "build/GPU Monitor.app" --args --backend-url http://127.0.0.1:9123
```

#### `open` reactiva la instancia existente y descarta `--args`

Si ya hay una `GPU Monitor.app` corriendo, `open ... --args --backend-url X` **no relanza nada**: macOS solo activa la instancia existente y los args se ignoran silenciosamente. Crees que cambiaste la URL de backend y sigues hablando con la vieja. Dos formas correctas:

- `open -n` para forzar una instancia nueva (puedes acabar con dos icons en la barra — mata la vieja antes).
- Matar la vieja por nombre real (`pkill -f gpu-monitor-tray-mac`) y luego `open` normal.

### Túnel SSH persistente vía LaunchAgent — alternativa al `ssh -fN` manual

`ssh -fN -L 9123:127.0.0.1:9123 <host>` lanzado a mano funciona pero muere si reinicias el Mac, si SSH se cae por inactividad o si la VPN renegocia. Para uso real, mete un LaunchAgent que lo gobierne:

```xml
<!-- ~/Library/LaunchAgents/com.maximofn.gpu-monitor-tunnel.plist -->
<key>ProgramArguments</key><array>
  <string>/usr/bin/ssh</string>
  <string>-N</string>
  <string>-o</string><string>ExitOnForwardFailure=yes</string>
  <string>-o</string><string>ServerAliveInterval=30</string>
  <string>-o</string><string>ServerAliveCountMax=3</string>
  <string>-L</string><string>9123:127.0.0.1:9123</string>
  <string>wallabot</string>
</array>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><true/>
<key>ThrottleInterval</key><integer>10</integer>
```

Tres claves importantes:

- **`ExitOnForwardFailure=yes`**: si el puerto local 9123 ya está ocupado o el host no responde, ssh sale en vez de quedarse en estado "conectado pero sin forward". Combinado con `KeepAlive=true` + `ThrottleInterval=10`, launchd reintenta cada 10 s.
- **`ServerAliveInterval=30`**: el cliente SSH manda keepalives. Sin esto, la NAT del router puede tirar la conexión a los 5–10 minutos de idle (los handlers HTTP tienen tráfico, pero si la app del tray no está corriendo, el túnel queda silencioso).
- **`autossh` no hace falta**. Antes era estándar para esto, hoy `KeepAlive` de launchd + `ServerAliveInterval` de ssh cubren el mismo caso sin dependencias extra (Homebrew, etc.).

`ssh-agent` no se invoca explícitamente — macOS expone el agente del Keychain en `SSH_AUTH_SOCK` para LaunchAgents de la sesión GUI, así que las claves `~/.ssh/id_*` desbloqueadas en login se usan transparentes. Si tu clave tiene passphrase y no está en el Keychain, el agent fallará silencioso al login y verás `Permission denied (publickey)` en `~/Library/Logs/gpu-monitor-tunnel.err.log`.

**Añade `StrictHostKeyChecking=accept-new`** al plist. Sin esto, si el host nunca apareció en `~/.ssh/known_hosts` (Mac recién migrado, primera vez que el túnel sube), `ssh` se queda esperando un "yes" interactivo que nadie va a teclear y el túnel queda colgado para siempre. `accept-new` añade la huella la primera vez sin prompt, pero rechaza si la huella *cambia* (que es lo que quieres detectar — un MITM, no un host nuevo conocido).

### **Empaqueta el plist + install script del túnel junto al del tray**

Cuando `gpu_monitor` documentaba el túnel como bloque XML inline en el README, terminé re-derivando el plist a mano para `cpu_monitor` y escribiendo un `install-tunnel.sh` paralelo a `install-launchagent.sh`. Tres minutos de pegar XML, sí, pero tres minutos por monitor que se multiplican y un sitio más donde tener el plist mal copiado.

**Patrón ya validado para esta familia**: cada `front-mac/scripts/` lleva DOS pares plist+script:

```
scripts/
├── com.maximofn.<monitor>-tray.plist        + install-launchagent.sh
└── com.maximofn.<monitor>-tunnel.plist      + install-tunnel.sh
```

Ambos scripts son idempotentes (bootout antes de bootstrap si ya existe), aceptan `uninstall`, y usan el mismo flujo `bootstrap` + `enable` + `kickstart -k` (no `launchctl load -w`, que está deprecado). El plist del túnel viene pre-rellenado con el host SSH del autor; el README explica el `sed` de una línea para reapuntarlo.

Hay que mantener los **puertos sincronizados** entre el plist del túnel (`-L PUERTO:127.0.0.1:PUERTO`) y `<monitor>_core::DEFAULT_PORT`. Es un sitio fácil de olvidar al subir el bind LAN o al cambiar de puerto — si el daemon mueve a 9130 y el plist sigue forwardeando 9124, el tray se queda en "connecting" eternamente sin error claro porque el puerto local existe pero no llega al daemon.

### Autostart del tray macOS — LaunchAgent al binario del bundle, no a `open`

Para que el tray arranque al login en el Mac hay un LaunchAgent paralelo al del túnel: `front-mac/scripts/com.maximofn.gpu-monitor-tray.plist`. Cosas no obvias del patrón:

- **Lanzar el binario del bundle directamente**, no `open "GPU Monitor.app"`. launchd y `open` se llevan mal: `open` retorna inmediatamente y launchd cree que el proceso ha muerto, lo que combinado con `KeepAlive` da reinicios en bucle. La ruta correcta es `Contents/MacOS/gpu-monitor-tray-mac` como `ProgramArguments[0]`. `Bundle.main` resuelve el `Info.plist` del `.app` padre igual, así que `LSUIElement = true` se aplica y no aparece icono en el Dock.
- **`KeepAlive = false`** explícito. Con `true`, si el usuario cierra la app desde el menú (acción legítima), launchd la relanza al instante — UX rota. `RunAtLoad = true` + `KeepAlive = false` da el comportamiento correcto: arranca al login, se queda fuera si el usuario lo decide.
- **`ProcessType = Interactive`** para que macOS no le aplique throttling de tarea de fondo. Sin esto, el sampler SSE sufre pausas raras cuando el sistema está bajo carga.
- **Flujo `bootstrap` + `enable` + `kickstart -k`**, no el viejo `launchctl load -w`. `load` está deprecado desde Catalina y silencia errores que `bootstrap` reporta.
- **Ruta absoluta hardcoded**: si mueves el repo de directorio, el plist apunta al bundle en la ubicación vieja y la app no arranca. El script de instalación debe reinstalarse (`./scripts/install-launchagent.sh`) tras mover.
- **Re-empaquetar el `.app` no requiere reinstalar el agent** (la ruta no cambia), pero sí `launchctl kickstart -k gui/$(id -u)/<label>` para que el tray corra el binario nuevo. Sin kickstart sigues con el binario que estaba en memoria al arrancar la sesión.

Para CPU/RAM/disk en macOS aplica el mismo patrón cuando llegues a portar — un LaunchAgent por monitor lanzando el binario del bundle correspondiente.

#### El donut gris al 0% no significa "sin datos"

Primer intento del estado disconnected en macOS: pintar el donut con `usedPercent=0` y palette atenuada (`#808080` libre, `#606060` usado). Visualmente: un anillo gris cerrado. **Se lee como "0% de uso real" en gris**, no como "sin datos". El usuario lo dijo en cuanto lo vio.

Solución que sí comunica ausencia: **icono GPU atenuado + un guion `-`, sin donut**. El guion no tiene una métrica plausible que se le pueda atribuir, así que se lee inequívocamente como "no hay valor". Además ahorra ancho de barra (más útil con multi-GPU).

Aplica la misma lógica a CPU/RAM/disk cuando los portes: si la métrica tiene un valor "0" semánticamente válido, no uses "0" o un anillo a 0% para representar disconnected. Usa un símbolo no-numérico (`-`, `?`, atenuación total).

### Defaults seguros y SSH port forward

El daemon Linux bindea `127.0.0.1` por convención del repo (sin auth). Para que el Mac consuma el backend remoto **sin abrir LAN**, lo limpio es:

```bash
ssh -fN -L 9123:127.0.0.1:9123 <ubuntu-host>
open "GPU Monitor.app" --args --backend-url http://127.0.0.1:9123
```

No edites el systemd unit del daemon para bindearlo a `0.0.0.0` "rápidamente" — viola los defaults del repo y acaba siendo difícil de revertir. Cuando llegue v2.2 con `--auth-token`, ya se abrirá el bind LAN como ciudadano de primera clase.

## Lecturas obligatorias antes de empezar

- `CLAUDE.md` de este repo — covers el patrón completo del backend y los gotchas del icono.
- `crates/gpu-monitor-tray/src/icon/render.rs` — implementación completa de iconos con `tiny-skia` + `freetype-rs` + ambas conversiones de alpha.
- `crates/gpu-monitor-tray/src/tray.rs` — patrón `set_state` + `refresh_icon_file` + `IconName`/`IconThemePath` por SNI.
- `crates/gpu-monitord/src/sampler.rs` — patrón sampler + watch channel.
- `front-mac/Sources/GPUMonitorTray/Client.swift` — cliente SSE en Swift con el workaround del flush trigger colapsado de `bytes.lines`.
- `front-mac/Sources/GPUMonitorTray/StatusBarController.swift` — patrón anti-feedback-loop para repintar al cambiar tema.
