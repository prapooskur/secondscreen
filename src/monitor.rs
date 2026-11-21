use std::time::Instant;
use sysinfo::{Gpus, Networks, System};

use crate::model::{Metrics, Sensor};

mod sensors;
mod storage;

pub struct Monitor {
    system: System,
    gpus: Option<Gpus>,
    networks: Option<Networks>,
    last_sample: Option<Instant>,
    host: Option<String>,
}

impl Monitor {
    pub fn new() -> Result<Self, sysinfo::Error> {
        Ok(Self {
            system: System::new()?,
            gpus: Gpus::new().ok(),
            networks: Networks::new().ok(),
            last_sample: None,
            host: None,
        })
    }

    pub fn sample(&mut self) -> Metrics {
        let now = Instant::now();
        let elapsed = self
            .last_sample
            .replace(now)
            .map(|then| (now - then).as_secs_f32());
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        if let Some(networks) = &mut self.networks {
            networks.refresh(true);
        }
        if let Some(gpus) = self.gpus.as_mut() {
            gpus.refresh(true);
        } else {
            self.gpus = Gpus::new_with_refreshed_list().ok();
        }

        let interfaces = self
            .networks
            .iter()
            .flat_map(|networks| networks.iter())
            .filter(|(name, _)| *name != "lo" && !name.to_string_lossy().starts_with("Loopback"));
        let mut network_available = false;
        let (mut received, mut sent) = (0u64, 0u64);
        for (_, network) in interfaces {
            network_available = true;
            received = received.saturating_add(network.received());
            sent = sent.saturating_add(network.transmitted());
        }

        let memory_used = self.system.used_memory();
        let memory_total = self.system.total_memory();
        let gib = |n: u64| n as f32 / 1_073_741_824.;
        let mut sensors = vec![
            Sensor::new(
                "cpu",
                "CPU load",
                "%",
                elapsed
                    .filter(|_| !self.system.cpus().is_empty())
                    .map(|_| self.system.global_cpu_usage()),
            ),
            Sensor::new(
                "memory",
                "Memory load",
                "%",
                (memory_total > 0).then(|| memory_used as f32 / memory_total as f32 * 100.),
            ),
            Sensor::new("ram-used", "Memory used", "GiB", Some(gib(memory_used))),
        ];
        if let Some(gpus) = &self.gpus {
            for gpu in gpus.iter() {
                let name = gpu.model().or_else(|| gpu.vendor()).unwrap_or("GPU");
                let id = gpu.pci();
                sensors.push(Sensor::new(
                    format!("gpu:{id}:load"),
                    format!("{name} · GPU load"),
                    "%",
                    gpu.usage(),
                ));
                if let Some(used) = gpu.used_memory() {
                    sensors.push(Sensor::new(
                        format!("gpu:{id}:vram"),
                        format!("{name} · VRAM used"),
                        "GiB",
                        Some(gib(used)),
                    ));
                }
            }
        }
        sensors::hardware(std::path::Path::new("/sys/class/hwmon"), &mut sensors);
        for (index, cpu) in self.system.cpus().iter().enumerate() {
            sensors.push(Sensor::new(
                format!("cpu:{index}"),
                format!("CPU {index} load"),
                "%",
                elapsed.map(|_| cpu.usage()),
            ));
        }
        Metrics {
            host: self
                .host
                .get_or_insert_with(|| {
                    System::host_name().unwrap_or_else(|_| "Local system".into())
                })
                .clone(),
            uptime: System::uptime().ok(),
            sensors,
            cpu_name: self
                .system
                .cpus()
                .first()
                .map(|cpu| cpu.brand().to_owned())
                .unwrap_or_default(),
            memory_used,
            memory_total,
            download: elapsed.map_or(0.0, |seconds| received as f32 / seconds),
            upload: elapsed.map_or(0.0, |seconds| sent as f32 / seconds),
            network_available,
            storage: storage::sample(),
        }
    }
}
