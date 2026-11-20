use std::time::Instant;
use sysinfo::{Components, DiskRefreshKind, Disks, Gpus, Networks, System};

use crate::model::{DiskUsage, Gpu, Metrics};

pub struct Monitor {
    system: System,
    components: Option<Components>,
    gpus: Option<Gpus>,
    networks: Option<Networks>,
    disks: Option<Disks>,
    last_sample: Option<Instant>,
    host: Option<String>,
}

impl Monitor {
    pub fn new() -> Result<Self, sysinfo::Error> {
        Ok(Self {
            system: System::new()?,
            components: Components::new().ok(),
            gpus: Gpus::new().ok(),
            networks: Networks::new().ok(),
            disks: Disks::new().ok(),
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
        if let Some(components) = &mut self.components {
            components.refresh(true);
        }
        if let Some(networks) = &mut self.networks {
            networks.refresh(true);
        }
        if let Some(gpus) = self.gpus.as_mut() {
            gpus.refresh(true);
        } else {
            self.gpus = Gpus::new_with_refreshed_list().ok();
        }

        let gpu = self.gpus.as_ref().and_then(|gpus| {
            let gpu = gpus
                .iter()
                .max_by_key(|gpu| (gpu.usage().is_some(), gpu.total_memory().unwrap_or(0)))?;
            let name = gpu
                .model()
                .or_else(|| gpu.vendor())
                .unwrap_or("Graphics adapter");
            Some(Gpu {
                name: name.to_owned(),
                key: gpu.pci().to_string(),
                load: gpu.usage().filter(|load| load.is_finite()),
                vram: gpu
                    .used_memory()
                    .zip(gpu.total_memory())
                    .filter(|(_, total)| *total > 0),
            })
        });

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

        Metrics {
            host: self
                .host
                .get_or_insert_with(|| {
                    System::host_name().unwrap_or_else(|_| "Local system".into())
                })
                .clone(),
            uptime: System::uptime().ok(),
            cpu: elapsed
                .filter(|_| !self.system.cpus().is_empty())
                .map(|_| self.system.global_cpu_usage()),
            cpu_name: self
                .system
                .cpus()
                .first()
                .map(|cpu| cpu.brand().to_owned())
                .unwrap_or_default(),
            threads: self.system.cpus().len(),
            temperature: self.cpu_temperature(),
            gpu,
            memory_used: self.system.used_memory(),
            memory_total: self.system.total_memory(),
            download: elapsed.map_or(0.0, |seconds| received as f32 / seconds),
            upload: elapsed.map_or(0.0, |seconds| sent as f32 / seconds),
            network_available,
            storage: self.storage(),
        }
    }

    fn storage(&mut self) -> Vec<DiskUsage> {
        let mut volumes = Vec::new();
        if let Some(disks) = &mut self.disks {
            disks.refresh_specifics(true, DiskRefreshKind::nothing().with_storage());
            volumes.extend(disks.iter().filter_map(|disk| {
                let total = disk.total_space();
                let mount = disk.mount_point();
                (total > 0 && mount.is_dir()).then(|| DiskUsage {
                    mount: mount.to_string_lossy().into_owned(),
                    used: total.saturating_sub(disk.available_space()),
                    total,
                })
            }));
        }
        if !volumes.iter().any(|disk| disk.mount == "/")
            && let Ok(root) = rustix::fs::statvfs("/")
        {
            let total = root.f_blocks.saturating_mul(root.f_frsize);
            if total > 0 {
                let available = root.f_bavail.saturating_mul(root.f_frsize);
                volumes.push(DiskUsage {
                    mount: "/".into(),
                    used: total.saturating_sub(available),
                    total,
                });
            }
        }
        volumes.sort_by(|a, b| a.mount.cmp(&b.mount));
        volumes.dedup_by(|a, b| a.mount == b.mount);
        volumes
    }

    fn cpu_temperature(&self) -> Option<f32> {
        self.components
            .as_ref()?
            .iter()
            .filter_map(|sensor| {
                let label = sensor.label().to_ascii_lowercase();
                let priority = if ["package", "tctl", "tdie", "cpu"]
                    .iter()
                    .any(|name| label.contains(name))
                {
                    2
                } else if label.contains("coretemp") || label.contains("core ") {
                    1
                } else {
                    return None;
                };
                sensor
                    .temperature()
                    .filter(|value| value.is_finite())
                    .map(|value| (priority, value))
            })
            .max_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)))
            .map(|(_, value)| value)
    }
}
