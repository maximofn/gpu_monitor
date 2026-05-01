use std::collections::HashMap;

use anyhow::Result;
use gpu_monitor_core::{Gpu, Memory, Process, ProcessKind, Utilization};
use nvml_wrapper::{
    enum_wrappers::device::TemperatureSensor, enums::device::UsedGpuMemory, Device, Nvml,
};

use crate::proc_name;

pub trait GpuSource: Send + Sync {
    fn driver_version(&self) -> Option<String>;
    fn cuda_version(&self) -> Option<String>;
    fn sample(&self) -> Result<Vec<Gpu>>;
}

pub struct NvmlSource {
    nvml: Nvml,
    driver_version: Option<String>,
    cuda_version: Option<String>,
}

impl NvmlSource {
    pub fn init() -> Result<Self> {
        let nvml = Nvml::init()?;
        let driver_version = nvml.sys_driver_version().ok();
        let cuda_version = nvml.sys_cuda_driver_version().ok().map(format_cuda_version);
        Ok(Self {
            nvml,
            driver_version,
            cuda_version,
        })
    }

    fn sample_device(&self, index: u32, device: &Device<'_>) -> Result<Gpu> {
        let uuid = device.uuid().unwrap_or_default();
        let name = device.name().unwrap_or_else(|_| format!("GPU {index}"));

        let temperature_c = device.temperature(TemperatureSensor::Gpu).ok();
        let fan_speed_percent = device.fan_speed(0).ok();
        let power_draw_w = device.power_usage().ok().map(|mw| mw as f32 / 1000.0);
        let power_limit_w = device
            .enforced_power_limit()
            .ok()
            .map(|mw| mw as f32 / 1000.0);

        let utilization = device
            .utilization_rates()
            .map(|u| Utilization {
                gpu_percent: u.gpu,
                memory_percent: u.memory,
            })
            .unwrap_or_default();

        let memory_info = device.memory_info()?;
        let memory = Memory {
            used_bytes: memory_info.used,
            free_bytes: memory_info.free,
            total_bytes: memory_info.total,
        };

        let processes = collect_processes(device);

        Ok(Gpu {
            index,
            uuid,
            name,
            temperature_c,
            fan_speed_percent,
            power_draw_w,
            power_limit_w,
            utilization,
            memory,
            processes,
        })
    }
}

impl GpuSource for NvmlSource {
    fn driver_version(&self) -> Option<String> {
        self.driver_version.clone()
    }

    fn cuda_version(&self) -> Option<String> {
        self.cuda_version.clone()
    }

    fn sample(&self) -> Result<Vec<Gpu>> {
        let count = self.nvml.device_count()?;
        let mut gpus = Vec::with_capacity(count as usize);
        for i in 0..count {
            let device = self.nvml.device_by_index(i)?;
            gpus.push(self.sample_device(i, &device)?);
        }
        Ok(gpus)
    }
}

fn format_cuda_version(encoded: i32) -> String {
    let major = encoded / 1000;
    let minor = (encoded % 1000) / 10;
    format!("{major}.{minor}")
}

fn collect_processes(device: &Device<'_>) -> Vec<Process> {
    let compute = device.running_compute_processes().unwrap_or_default();
    let graphics = device.running_graphics_processes().unwrap_or_default();

    let mut by_pid: HashMap<u32, Process> = HashMap::new();

    for p in compute {
        let bytes = used_bytes(&p.used_gpu_memory);
        by_pid
            .entry(p.pid)
            .and_modify(|existing| {
                existing.kind = ProcessKind::Mixed;
                if bytes > existing.used_memory_bytes {
                    existing.used_memory_bytes = bytes;
                }
            })
            .or_insert_with(|| Process {
                pid: p.pid,
                name: proc_name::for_pid(p.pid),
                used_memory_bytes: bytes,
                kind: ProcessKind::Compute,
            });
    }

    for p in graphics {
        let bytes = used_bytes(&p.used_gpu_memory);
        by_pid
            .entry(p.pid)
            .and_modify(|existing| {
                if existing.kind == ProcessKind::Compute {
                    existing.kind = ProcessKind::Mixed;
                }
                if bytes > existing.used_memory_bytes {
                    existing.used_memory_bytes = bytes;
                }
            })
            .or_insert_with(|| Process {
                pid: p.pid,
                name: proc_name::for_pid(p.pid),
                used_memory_bytes: bytes,
                kind: ProcessKind::Graphics,
            });
    }

    let mut out: Vec<Process> = by_pid.into_values().collect();
    out.sort_by(|a, b| {
        b.used_memory_bytes
            .cmp(&a.used_memory_bytes)
            .then(a.pid.cmp(&b.pid))
    });
    out
}

fn used_bytes(used: &UsedGpuMemory) -> u64 {
    match used {
        UsedGpuMemory::Used(bytes) => *bytes,
        UsedGpuMemory::Unavailable => 0,
    }
}

pub struct MockSource {
    gpus: Vec<Gpu>,
    driver: Option<String>,
    cuda: Option<String>,
}

impl MockSource {
    pub fn new(gpus: Vec<Gpu>) -> Self {
        Self {
            gpus,
            driver: Some("mock-driver".to_string()),
            cuda: Some("0.0".to_string()),
        }
    }
}

impl GpuSource for MockSource {
    fn driver_version(&self) -> Option<String> {
        self.driver.clone()
    }

    fn cuda_version(&self) -> Option<String> {
        self.cuda.clone()
    }

    fn sample(&self) -> Result<Vec<Gpu>> {
        Ok(self.gpus.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuda_version_decodes_correctly() {
        assert_eq!(format_cuda_version(12050), "12.5");
        assert_eq!(format_cuda_version(11080), "11.8");
        assert_eq!(format_cuda_version(12000), "12.0");
    }

    #[test]
    fn mock_source_returns_seeded_gpus() {
        let gpu = Gpu {
            index: 0,
            uuid: "GPU-mock".into(),
            name: "Mock GPU".into(),
            temperature_c: Some(50),
            fan_speed_percent: None,
            power_draw_w: None,
            power_limit_w: None,
            utilization: Utilization::default(),
            memory: Memory {
                used_bytes: 0,
                free_bytes: 100,
                total_bytes: 100,
            },
            processes: vec![],
        };
        let mock = MockSource::new(vec![gpu.clone()]);
        let sample = mock.sample().unwrap();
        assert_eq!(sample.len(), 1);
        assert_eq!(sample[0], gpu);
        assert_eq!(mock.driver_version().as_deref(), Some("mock-driver"));
    }
}
