//! Each system's way of reading the CPU, memory, disks, GPU, temperatures
//! and USB devices. Made on the meter's thread and used only there (some
//! of it, such as WMI on Windows, belongs to the thread that opened it).

use crate::metrics::{self, SensorKind, Snapshot, Sys};

mod nvml;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::Platform;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::Platform;

#[cfg(not(any(target_os = "linux", windows)))]
struct Platform;
#[cfg(not(any(target_os = "linux", windows)))]
impl Platform {
    fn new() -> Platform {
        Platform
    }
    fn read(&mut self, _dt: f64, _sys: &mut Sys, _snap: &mut Snapshot) {}
}

pub struct SysMeter {
    platform: Platform,
    nvml: Option<nvml::Nvml>,
}

impl SysMeter {
    pub fn new() -> SysMeter {
        SysMeter {
            platform: Platform::new(),
            nvml: nvml::Nvml::open(),
        }
    }

    /// One reading; `dt` is the seconds since the last.
    pub fn read(&mut self, dt: f64) -> (Sys, Snapshot) {
        let mut sys = Sys::default();
        let mut snap = Snapshot::default();
        self.platform.read(dt, &mut sys, &mut snap);
        // NVIDIA's own figures beat the system's guesses.
        if let Some(n) = &self.nvml {
            let g = n.read();
            sys.gpu = g.busy.or(sys.gpu);
            if let Some(t) = g.temp {
                snap.sensors
                    .retain(|s| s.kind != SensorKind::Gpu || !s.label.starts_with("GPU"));
                snap.sensors.push(metrics::Sensor {
                    kind: SensorKind::Gpu,
                    label: format!("GPU · {}", g.name),
                    celsius: t,
                });
                snap.sensors
                    .sort_by(|a, b| a.kind.cmp(&b.kind).then(a.label.cmp(&b.label)));
            }
            snap.gpu = Some(g);
        }
        if let Some(g) = &snap.gpu {
            sys.gpu = sys.gpu.or(g.busy);
        }
        sys.cpu_temp = metrics::cpu_temp(&snap.sensors);
        sys.gpu_temp = snap
            .gpu
            .as_ref()
            .and_then(|g| g.temp)
            .or_else(|| metrics::hottest(&snap.sensors, SensorKind::Gpu));
        (sys, snap)
    }
}
