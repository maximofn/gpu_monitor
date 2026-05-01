# gpu-monitord HTTP API

Default bind: `127.0.0.1:9123`. All responses are JSON unless noted.

## `GET /healthz`

Liveness probe.

```json
{"status":"ok","uptime_s":42}
```

## `GET /v1/info`

Backend metadata. Cheap, no NVML cost.

```json
{
  "backend_version": "2.0.0-alpha.1",
  "api_version": "v1",
  "host": "wallabot",
  "driver_version": "555.42.06",
  "cuda_version": "12.5",
  "gpu_count": 2
}
```

## `GET /v1/snapshot`

Most recent cached snapshot (refreshed by the sampler at `--sample-interval-ms`).

```json
{
  "timestamp": "2026-05-01T16:16:29.188+00:00",
  "host": "wallabot",
  "driver_version": "555.42.06",
  "cuda_version": "12.5",
  "gpus": [
    {
      "index": 0,
      "uuid": "GPU-d8ff25c1-...",
      "name": "NVIDIA GeForce RTX 3090",
      "temperature_c": 26,
      "fan_speed_percent": 0,
      "power_draw_w": 15.2,
      "power_limit_w": 350.0,
      "utilization": { "gpu_percent": 0, "memory_percent": 0 },
      "memory": {
        "used_bytes": 453246976,
        "free_bytes": 25316556800,
        "total_bytes": 25769803776
      },
      "processes": [
        {
          "pid": 1788,
          "name": "Xorg",
          "used_memory_bytes": 4677632,
          "type": "graphics"
        }
      ]
    }
  ]
}
```

`type` is one of `compute`, `graphics`, `mixed`. Process `name` is read from `/proc/<pid>/comm`; fallback `pid:<n>` if `/proc` is unreadable.

## `GET /v1/gpus`

Lightweight metadata for every GPU.

```json
[
  {
    "index": 0,
    "uuid": "GPU-d8ff25c1-...",
    "name": "NVIDIA GeForce RTX 3090",
    "memory_total_bytes": 25769803776
  }
]
```

## `GET /v1/gpus/{idx}`

Full `Gpu` object as it appears in `/v1/snapshot` (including `processes`). Returns `404` if `idx` is unknown.

## `GET /v1/gpus/{idx}/processes`

Just the process list for one GPU. Returns `404` if `idx` is unknown.

## `GET /v1/stream`

Server-Sent Events. Each event payload is a full `Snapshot` JSON, emitted every `--sample-interval-ms`. Ping comments every 15s keep the connection alive through proxies.

Example consumption:

```bash
curl -N http://127.0.0.1:9123/v1/stream
```

The frontend in this repo subscribes to this stream and falls back to `/v1/snapshot` polling on disconnect.

## CLI flags

| Flag | Env var | Default | Notes |
|---|---|---|---|
| `--bind` | `GPU_MONITORD_BIND` | `127.0.0.1` | Use `0.0.0.0` to expose on LAN. |
| `--port` | `GPU_MONITORD_PORT` | `9123` | |
| `--sample-interval-ms` | `GPU_MONITORD_SAMPLE_INTERVAL_MS` | `1000` | Floor of 50 ms is enforced. |
| `--log-level` | `RUST_LOG` | `info` | Standard tracing-subscriber EnvFilter syntax. |
| `--mock` | `GPU_MONITORD_MOCK` | `false` | Synthetic data for development on machines without NVIDIA. |

## Authentication

The current release is unauthenticated and bound to `127.0.0.1` by default. When the LAN-facing release (Phase 5) lands, a `--auth-token <hex>` flag will require `Authorization: Bearer <hex>` on every request.
