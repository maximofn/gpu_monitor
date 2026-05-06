# front-mac — frontend macOS para gpu-monitord

Status bar app nativo (Swift + AppKit) que consume el HTTP+SSE de `gpu-monitord`
y muestra un icono dinámico por GPU en la barra superior del Mac, con menú
desplegable de detalle por GPU y procesos.

Réplica funcional del tray Linux (`crates/gpu-monitor-tray`). Mismo schema
(`/v1/snapshot`, `/v1/stream`), mismos colores, mismos umbrales. Sin Dock icon
ni ventanas — `LSUIElement = true`.

## Requisitos

- macOS 13 (Ventura) o superior.
- Swift 5.9+ (Xcode 15+ o `swift` en línea de comandos via toolchain).
- Un `gpu-monitord` corriendo y accesible por red.

## Build

```bash
cd front-mac
swift build -c release
```

El binario sale en `.build/release/GPUMonitorTray`. Puede ejecutarse tal cual
para iterar:

```bash
.build/release/GPUMonitorTray --backend-url http://127.0.0.1:9123
```

Para uso real, empaquetar en `.app` (sin Dock icon):

```bash
./scripts/build-app.sh
open "build/GPU Monitor.app" --args --backend-url http://192.168.1.50:9123
```

## CLI

| Flag                     | Default                       | Descripción                                |
| ------------------------ | ----------------------------- | ------------------------------------------ |
| `--backend-url <URL>`    | `http://127.0.0.1:9123`       | Base del API (env: `GPU_MONITOR_TRAY_URL`) |
| `--icon-height <PT>`     | `22`                          | Altura lógica del icono en la barra        |
| `--log-level <LEVEL>`    | `info`                        | trace/debug/info/warn/error (OSLog)        |
| `--dump-icon <PATH>`     | —                             | Pinta el snapshot actual a PNG y sale      |
| `--version`              | —                             | Versión                                    |
| `-h`, `--help`           | —                             | Ayuda                                      |

## Autostart en login

Hay dos rutas; la de Settings es la limpia:

1. **System Settings → General → Login Items → Open at Login** → `+` → seleccionar
   `GPU Monitor.app`.
2. Vía script (idempotente):
   ```bash
   osascript -e 'tell application "System Events" to make login item at end with properties {path:"/Applications/GPU Monitor.app", hidden:false}'
   ```

## Network: HTTP plano

El backend está pensado para LAN privada con HTTP plano. El bundle declara
`NSAppTransportSecurity → NSAllowsArbitraryLoads = true` para permitir
`http://192.168.x.x` sin certificados. **No expongas el backend a Internet con
esta config**: el roadmap del backend prevé `--auth-token` cuando se haga bind
fuera de loopback.

## Schema y compatibilidad

`Models.swift` replica `crates/gpu-monitor-core/src/model.rs`. Si añades campos
al `Snapshot`/`Gpu`/`Process` en Rust, **replica aquí** o el JSON decode
ignorará los nuevos campos en silencio. La API está versionada por path
(`/v1/...`) — un cambio incompatible se hace subiendo a `/v2/`.

## Diferencias con el tray Linux

- El renderer es Core Graphics + Core Text en vez de tiny-skia + freetype. La
  geometría (donut, gaps, layout) y los colores están portados 1:1.
- La fuente es **SF Mono** (system) en vez de DejaVu Sans Mono. Hinting nativo
  de macOS, sin TTC manual ni búsqueda de paths.
- No hay archivos PNG en `~/.cache` — `NSStatusItem` acepta `NSImage` en
  memoria sin los problemas de cacheo que tiene GNOME-shell.
- El menubar app no necesita autorizaciones especiales: arranca con `.accessory`
  policy y aparece sin más.

## Verificación rápida

```bash
# 1. Backend mock en otra máquina (o local):
cargo run -p gpu-monitord --release -- --mock --bind 0.0.0.0 --port 9123

# 2. Frontend Mac apuntando al mock:
swift run --package-path front-mac GPUMonitorTray --backend-url http://<host>:9123

# 3. Volcar el icono a un PNG sin tocar la status bar (debug):
swift run --package-path front-mac GPUMonitorTray \
    --backend-url http://<host>:9123 --dump-icon /tmp/mac-icon.png
```

Compara `/tmp/mac-icon.png` (Mac) contra el equivalente Linux:
```bash
./target/release/gpu-monitor-tray --backend-url http://<host>:9123 \
    --dump-icon /tmp/linux-icon.png
```
Los dos PNG deben verse equivalentes.
