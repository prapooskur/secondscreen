use std::time::Instant;
use sysinfo::{Gpus, InterfaceOperationalState, Networks, System};

use crate::model::{Metrics, NetworkRates, Sensor};

mod sensors;
pub(crate) mod storage;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sources {
    pub per_core: bool,
    pub gpu: bool,
    pub hardware: bool,
    pub network: bool,
    pub storage: bool,
}

impl Sources {
    pub const ALL: Self = Self {
        per_core: true,
        gpu: true,
        hardware: true,
        network: true,
        storage: true,
    };
}

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
            gpus: None,
            networks: None,
            last_sample: None,
            host: None,
        })
    }

    pub fn sample(&mut self, sources: Sources) -> Metrics {
        let now = Instant::now();
        let elapsed = self
            .last_sample
            .replace(now)
            .map(|then| (now - then).as_secs_f32());
        // A newly enabled counter needs two samples before it can report a rate.
        let network_elapsed = elapsed.filter(|_| self.networks.is_some());
        if !sources.network {
            self.networks = None;
        } else if self.networks.is_none() {
            self.networks = Networks::new().ok();
        }
        let network = self.networks.as_mut().and_then(|networks| {
            networks.refresh(true);
            let seconds = network_elapsed.filter(|&seconds| seconds > 0.)?;
            let (received, sent) = networks
                .iter()
                .filter(|(name, network)| {
                    *name != "lo"
                        && !matches!(
                            network.operational_state(),
                            InterfaceOperationalState::Down
                                | InterfaceOperationalState::LowerLayerDown
                                | InterfaceOperationalState::NotPresent
                                | InterfaceOperationalState::Testing
                        )
                })
                .map(|(_, network)| (network.received(), network.transmitted()))
                .reduce(|(received, sent), (rx, tx)| {
                    (received.saturating_add(rx), sent.saturating_add(tx))
                })?;
            Some(NetworkRates {
                download: received as f32 / seconds,
                upload: sent as f32 / seconds,
            })
        });
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        if !sources.gpu {
            self.gpus = None;
        } else if let Some(gpus) = self.gpus.as_mut() {
            gpus.refresh(true);
        } else {
            self.gpus = Gpus::new_with_refreshed_list().ok();
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
        if sources.hardware {
            sensors::hardware(std::path::Path::new("/sys/class/hwmon"), &mut sensors);
        }
        if sources.per_core {
            for (index, cpu) in self.system.cpus().iter().enumerate() {
                sensors.push(Sensor::new(
                    format!("cpu:{index}"),
                    format!("CPU {index} load"),
                    "%",
                    elapsed.map(|_| cpu.usage()),
                ));
            }
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
            network,
        }
    }
}
