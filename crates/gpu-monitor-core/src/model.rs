use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub timestamp: String,
    pub host: String,
    pub driver_version: Option<String>,
    pub cuda_version: Option<String>,
    pub gpus: Vec<Gpu>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Gpu {
    pub index: u32,
    pub uuid: String,
    pub name: String,
    pub temperature_c: Option<u32>,
    pub fan_speed_percent: Option<u32>,
    pub power_draw_w: Option<f32>,
    pub power_limit_w: Option<f32>,
    pub utilization: Utilization,
    pub memory: Memory,
    pub processes: Vec<Process>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Utilization {
    pub gpu_percent: u32,
    pub memory_percent: u32,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Memory {
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub total_bytes: u64,
}

impl Memory {
    pub fn used_percent(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.used_bytes as f32 / self.total_bytes as f32) * 100.0
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub name: String,
    pub used_memory_bytes: u64,
    #[serde(rename = "type")]
    pub kind: ProcessKind,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProcessKind {
    Compute,
    Graphics,
    Mixed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_roundtrips_through_json() {
        let snapshot = Snapshot {
            timestamp: "2026-05-01T18:00:00Z".to_string(),
            host: "carbon".to_string(),
            driver_version: Some("555.42.06".to_string()),
            cuda_version: Some("12.5".to_string()),
            gpus: vec![Gpu {
                index: 0,
                uuid: "GPU-abc".to_string(),
                name: "NVIDIA RTX 4090".to_string(),
                temperature_c: Some(45),
                fan_speed_percent: Some(30),
                power_draw_w: Some(120.5),
                power_limit_w: Some(450.0),
                utilization: Utilization { gpu_percent: 25, memory_percent: 10 },
                memory: Memory {
                    used_bytes: 2 * 1024 * 1024 * 1024,
                    free_bytes: 22 * 1024 * 1024 * 1024,
                    total_bytes: 24 * 1024 * 1024 * 1024,
                },
                processes: vec![Process {
                    pid: 1234,
                    name: "ollama".to_string(),
                    used_memory_bytes: 1_500_000_000,
                    kind: ProcessKind::Compute,
                }],
            }],
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let back: Snapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snapshot, back);
        assert!(json.contains("\"type\":\"compute\""));
    }

    #[test]
    fn used_percent_handles_zero_total() {
        let m = Memory::default();
        assert_eq!(m.used_percent(), 0.0);
    }

    #[test]
    fn used_percent_computes_correctly() {
        let m = Memory { used_bytes: 50, free_bytes: 50, total_bytes: 100 };
        assert!((m.used_percent() - 50.0).abs() < f32::EPSILON);
    }
}
