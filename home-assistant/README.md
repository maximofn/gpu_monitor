# Home Assistant integration

Expone el estado de las GPUs (`gpu-monitord`) en Home Assistant como sensores
nativos. Sin custom component: solo configuración YAML usando la integración
`rest` que viene con `default_config`.

## Arquitectura

```
[ Ubuntu (wallabot) ]                 [ Raspberry (raspihome) ]
  gpu-monitord                            Home Assistant (Docker, host net)
  127.0.0.1:9123  ◄──── ssh -L ─────  127.0.0.1:9123
                                            │
                                            └─► sensor.rest (scan_interval=15s)
```

El daemon sigue bindeado a `127.0.0.1` (default seguro). Un **túnel SSH
forward** desde raspihome abre `127.0.0.1:9123` en la pi y reenvía cada
conexión al loopback de wallabot. Toda la persistencia vive en raspihome
(systemd user unit con linger); en wallabot solo hay una clave pública
restringida en `~/.ssh/authorized_keys` (cero servicios nuevos).

HA corre en Docker con `--network host`, así que `127.0.0.1` desde dentro del
contenedor es exactamente el loopback de raspihome.

## Instalación

### 1) Túnel SSH desde raspihome

En raspihome:

```bash
# Pre-req: linger habilitado para tu usuario:
#   sudo loginctl enable-linger "$USER"
cd /ruta/al/repo/gpu_monitor/home-assistant/tunnel
./install.sh
```

`install.sh`:
1. Genera `~/.ssh/id_ed25519_gpu_tunnel` (sin passphrase, dedicada al túnel).
2. Te imprime una línea para añadir a `~/.ssh/authorized_keys` en wallabot:
   `restrict,port-forwarding,permitopen="127.0.0.1:9123" ssh-ed25519 AAA...`
   — esa clave solo puede port-forwardear al puerto 9123, nada más.
3. Verifica que `ssh wallabot@wallabot` autentica.
4. Instala `gpu-monitor-ha-tunnel.service` como user systemd unit y lo arranca.

Verifica:

```bash
systemctl --user status gpu-monitor-ha-tunnel.service
curl -fsS http://127.0.0.1:9123/healthz       # raíz, no bajo /v1
curl -fsS http://127.0.0.1:9123/v1/info | jq
```

### 2) Paquete de Home Assistant

Habilita packages en `configuration.yaml` de raspihome (una sola vez):

```yaml
homeassistant:
  packages: !include_dir_named packages
```

Copia el paquete:

```bash
ssh raspihome 'mkdir -p /home/raspihome/docker/homeassistant/packages'
scp packages/gpu_monitor.yaml raspihome:/home/raspihome/docker/homeassistant/packages/
```

Comprueba la config y recarga:

```bash
# Comprueba YAML
ssh raspihome 'docker exec homeassistant python -m homeassistant --script check_config -c /config'
# Recarga sin reiniciar (Developer Tools → YAML → "All YAML configuration"),
# o reinicia el contenedor:
ssh raspihome 'docker restart homeassistant'
```

Tras recargar, en HA aparecen las entidades:

```
sensor.gpu_monitor_host        sensor.gpu_0_temperature   sensor.gpu_1_temperature
sensor.gpu_monitor_driver      sensor.gpu_0_utilization   sensor.gpu_1_utilization
sensor.gpu_monitor_cuda        sensor.gpu_0_memory_*      sensor.gpu_1_memory_*
sensor.gpu_monitor_count       sensor.gpu_0_power_*       sensor.gpu_1_power_*
                               sensor.gpu_0_fan_speed     sensor.gpu_1_fan_speed
                               sensor.gpu_0_top_process   sensor.gpu_1_top_process
                               sensor.gpu_0_process_count sensor.gpu_1_process_count
```

`sensor.gpu_N_top_process` lleva la lista completa de procesos en
`attributes.processes` para usar desde plantillas o tarjetas.

### 3) Dashboard (opcional)

En `lovelace/gpu_dashboard.yaml` hay una vista lista para pegar (Settings →
Dashboards → Edit → Raw configuration editor → añadir como `views: -`).

## Cambiar nº de GPUs

`packages/gpu_monitor.yaml` tiene un bloque por GPU (índices 0 y 1, las
físicas de wallabot). Para una máquina con más GPUs, copia el bloque "GPU 1"
entero, cambia los índices de `gpus[1]` a `gpus[2]` y los `unique_id` de
`gpu1_*` a `gpu2_*`.

## Por qué REST y no SSE

`gpu-monitord` también expone Server-Sent Events. HA tiene integración
`rest` nativa (probada, declarativa, multi-sensor compartiendo una request)
pero su soporte para SSE requeriría un custom component. A 15 s de poll, el
overhead es mínimo (~600 B/req) y no se pierde nada útil — el daemon
internamente muestrea a 1 Hz; HA recoge el último snapshot completo cada
ciclo.

## Troubleshooting

- **Sensores `unavailable`**: el túnel SSH se cayó o el daemon no responde.
  Desde raspihome: `curl http://127.0.0.1:9123/healthz` (raíz, no bajo /v1).
  Si timeout: `systemctl --user status gpu-monitor-ha-tunnel.service` en
  raspihome y `journalctl --user -u gpu-monitor-ha-tunnel.service -n 50`.
- **YAML check OK pero las entidades no aparecen**: `default_config` debe
  estar presente (lo trae `rest`); si lo quitaste, añade `rest:` como
  integración top-level — el paquete ya define la sección.
- **Recarga sin reiniciar**: Developer Tools → Actions →
  `homeassistant.reload_config_entry` o el botón "Reload REST entities" en
  YAML config tools.
