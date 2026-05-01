use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use gpu_monitor_core::Snapshot;
use tokio::sync::watch;
use tokio::time::{interval, MissedTickBehavior};

use crate::nvml_source::GpuSource;

pub fn empty_snapshot(host: &str, driver: Option<String>, cuda: Option<String>) -> Snapshot {
    Snapshot {
        timestamp: Utc::now().to_rfc3339(),
        host: host.to_string(),
        driver_version: driver,
        cuda_version: cuda,
        gpus: Vec::new(),
    }
}

pub fn build_snapshot(host: &str, source: &dyn GpuSource) -> Snapshot {
    let gpus = source.sample().unwrap_or_else(|err| {
        tracing::warn!(error = %err, "GPU sample failed; emitting empty list");
        Vec::new()
    });
    Snapshot {
        timestamp: Utc::now().to_rfc3339(),
        host: host.to_string(),
        driver_version: source.driver_version(),
        cuda_version: source.cuda_version(),
        gpus,
    }
}

pub fn spawn(
    source: Arc<dyn GpuSource>,
    host: String,
    interval_ms: u64,
    tx: watch::Sender<Snapshot>,
) {
    tokio::spawn(async move {
        let period = Duration::from_millis(interval_ms.max(50));
        let mut ticker = interval(period);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            ticker.tick().await;
            let snapshot = build_snapshot(&host, source.as_ref());
            if tx.send(snapshot).is_err() {
                tracing::info!("snapshot channel closed; sampler exiting");
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nvml_source::MockSource;
    use gpu_monitor_core::{Gpu, Memory, Utilization};

    fn fake_gpu(idx: u32) -> Gpu {
        Gpu {
            index: idx,
            uuid: format!("GPU-{idx}"),
            name: format!("Fake {idx}"),
            temperature_c: Some(40 + idx),
            fan_speed_percent: None,
            power_draw_w: None,
            power_limit_w: None,
            utilization: Utilization::default(),
            memory: Memory { used_bytes: 0, free_bytes: 1024, total_bytes: 1024 },
            processes: vec![],
        }
    }

    #[test]
    fn build_snapshot_uses_source_metadata() {
        let source = MockSource::new(vec![fake_gpu(0), fake_gpu(1)]);
        let snap = build_snapshot("host-x", &source);
        assert_eq!(snap.host, "host-x");
        assert_eq!(snap.gpus.len(), 2);
        assert_eq!(snap.driver_version.as_deref(), Some("mock-driver"));
        assert!(!snap.timestamp.is_empty());
    }

    #[test]
    fn empty_snapshot_has_no_gpus() {
        let snap = empty_snapshot("h", None, None);
        assert!(snap.gpus.is_empty());
    }
}
