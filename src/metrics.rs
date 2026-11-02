//! The system's readings beside the network's: CPU, memory, disks, GPU,
//! temperatures and USB. Plain data and the arithmetic on it, no operating
//! system: each system's code in `sys/` fills these in, and everything here
//! is unit tested.

use crate::rates::Rate;
use serde::{Deserialize, Serialize};

/// What's kept for every reading, to draw the graphs from. Small and
/// `Copy`: ten minutes of these at two a second is a few hundred kilobytes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sys {
    /// All cores together, 0 to 1.
    pub cpu: Option<f32>,
    /// Bytes in use and in all.
    pub mem: Option<(u64, u64)>,
    pub swap: Option<(u64, u64)>,
    /// Bytes a second read (`down`) and written (`up`) by the real disks.
    pub disk: Option<Rate>,
    /// The first GPU: busy 0 to 1.
    pub gpu: Option<f32>,
    /// Degrees Celsius.
    pub cpu_temp: Option<f32>,
    pub gpu_temp: Option<f32>,
}

/// What's shown as it is now, without a history.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub cpu_name: Option<String>,
    /// Each core (each logical processor), 0 to 1.
    pub cores: Vec<f32>,
    pub gpu: Option<Gpu>,
    pub sensors: Vec<Sensor>,
    pub fans: Vec<Fan>,
    pub volumes: Vec<Volume>,
    /// `None` where the devices can't be listed.
    pub usb: Option<Vec<UsbDevice>>,
    /// Where the temperatures come from, when it's worth saying.
    pub sensors_note: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub busy: Option<f32>,
    /// Video memory: bytes used and in all.
    pub vram: Option<(u64, u64)>,
    pub temp: Option<f32>,
    /// Watts.
    pub power: Option<f32>,
}

/// What a temperature sensor measures, for picking the CPU's and for
/// ordering the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SensorKind {
    Cpu,
    Gpu,
    Drive,
    Board,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sensor {
    pub kind: SensorKind,
    pub label: String,
    pub celsius: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fan {
    pub label: String,
    pub rpm: u32,
}

/// A mounted file system or drive letter, and how full it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Volume {
    pub name: String,
    pub used: u64,
    pub total: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbDevice {
    /// Stable while it's plugged in: the bus path or instance id.
    pub id: String,
    pub name: String,
    /// The link's speed in Mb/s, where the system says.
    pub speed_mbps: Option<u32>,
}

impl UsbDevice {
    /// "USB 3.2 Gen 1 · 5 Gb/s", from the link speed.
    pub fn speed_label(&self) -> Option<String> {
        let s = self.speed_mbps?;
        Some(
            match s {
                0..=2 => "USB 1.1 · low speed",
                3..=12 => "USB 1.1 · 12 Mb/s",
                13..=480 => "USB 2 · 480 Mb/s",
                481..=5000 => "USB 3 · 5 Gb/s",
                5001..=10000 => "USB 3 · 10 Gb/s",
                10001..=20000 => "USB 3 · 20 Gb/s",
                _ => "USB4",
            }
            .to_string(),
        )
    }
}

/// The dashboard's tiles, in the order they're drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Panel {
    Internet,
    Lan,
    Network,
    Cpu,
    Memory,
    Disks,
    Gpu,
    Temperatures,
    Usb,
}

impl Panel {
    pub const ALL: [Panel; 9] = [
        Panel::Internet,
        Panel::Lan,
        Panel::Network,
        Panel::Cpu,
        Panel::Memory,
        Panel::Disks,
        Panel::Gpu,
        Panel::Temperatures,
        Panel::Usb,
    ];

    /// What a new dashboard shows: everything, with the network split where
    /// it can be (all traffic stands in where it can't).
    pub const DEFAULT: [Panel; 8] = [
        Panel::Internet,
        Panel::Lan,
        Panel::Cpu,
        Panel::Memory,
        Panel::Disks,
        Panel::Gpu,
        Panel::Temperatures,
        Panel::Usb,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Panel::Internet => "Internet",
            Panel::Lan => "LAN",
            Panel::Network => "All traffic",
            Panel::Cpu => "CPU",
            Panel::Memory => "Memory",
            Panel::Disks => "Disks",
            Panel::Gpu => "GPU",
            Panel::Temperatures => "Temperatures",
            Panel::Usb => "USB",
        }
    }
}

// ---------------------------------------------------------------- arithmetic

/// CPU time counters: (busy, all), in any unit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub struct CpuTimes {
    pub busy: u64,
    pub total: u64,
}

/// How busy a CPU was between two readings of its counters, 0 to 1.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn busy_between(was: CpuTimes, now: CpuTimes) -> Option<f32> {
    let total = now.total.checked_sub(was.total)?;
    let busy = now.busy.checked_sub(was.busy)?;
    (total > 0).then(|| (busy as f64 / total as f64).clamp(0.0, 1.0) as f32)
}

/// A size in bytes: "7.9 GB", or "7.4 GiB" in powers of 1024.
pub fn size(bytes: u64, binary: bool) -> String {
    let base = if binary { 1024.0 } else { 1000.0 };
    let units: [&str; 5] = if binary {
        ["B", "KiB", "MiB", "GiB", "TiB"]
    } else {
        ["B", "kB", "MB", "GB", "TB"]
    };
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= base && i < units.len() - 1 {
        v /= base;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else if v < 10.0 {
        format!("{v:.1} {}", units[i])
    } else {
        format!("{v:.0} {}", units[i])
    }
}

/// A temperature in the chosen scale: "54°C" or "129°F".
pub fn temperature(celsius: f32, fahrenheit: bool) -> String {
    if fahrenheit {
        format!("{:.0}°F", celsius * 9.0 / 5.0 + 32.0)
    } else {
        format!("{celsius:.0}°C")
    }
}

/// How hot a temperature is for a computer part, 0 (cool) to 1 (too hot),
/// for colouring it: 40 °C and under is cool, 95 °C is the top.
pub fn heat(celsius: f32) -> f32 {
    ((celsius - 40.0) / 55.0).clamp(0.0, 1.0)
}

/// The CPU's temperature among the sensors: the hottest CPU sensor.
pub fn cpu_temp(sensors: &[Sensor]) -> Option<f32> {
    hottest(sensors, SensorKind::Cpu)
}

pub fn hottest(sensors: &[Sensor], kind: SensorKind) -> Option<f32> {
    sensors
        .iter()
        .filter(|s| s.kind == kind)
        .map(|s| s.celsius)
        .reduce(f32::max)
}

// ---------------------------------------------------------------- Linux's text

/// `/proc/stat`: the whole CPU's counters, then each core's.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_proc_stat(text: &str) -> (Option<CpuTimes>, Vec<CpuTimes>) {
    let mut all = None;
    let mut cores = Vec::new();
    for line in text.lines() {
        let mut f = line.split_whitespace();
        let Some(name) = f.next() else { continue };
        if !name.starts_with("cpu") {
            continue;
        }
        // user nice system idle iowait irq softirq steal (guest is in user)
        let v: Vec<u64> = f.take(8).filter_map(|x| x.parse().ok()).collect();
        if v.len() < 4 {
            continue;
        }
        let idle = v[3] + v.get(4).copied().unwrap_or(0);
        let total: u64 = v.iter().sum();
        let t = CpuTimes {
            busy: total - idle,
            total,
        };
        if name == "cpu" {
            all = Some(t);
        } else {
            cores.push(t);
        }
    }
    (all, cores)
}

/// Bytes in use and in all.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub type Used = (u64, u64);

/// `/proc/meminfo`: memory (used, total) and swap (used, total), in bytes.
/// "Used" leaves out what the system could hand back at once (caches).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_meminfo(text: &str) -> (Option<Used>, Option<Used>) {
    let get = |key: &str| {
        text.lines().find_map(|l| {
            let rest = l.strip_prefix(key)?.strip_prefix(':')?;
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            Some(kb * 1024)
        })
    };
    let mem = get("MemTotal").map(|total| {
        let avail = get("MemAvailable")
            .or_else(|| Some(get("MemFree")? + get("Cached").unwrap_or(0)))
            .unwrap_or(0);
        (total.saturating_sub(avail), total)
    });
    let swap = get("SwapTotal")
        .filter(|t| *t > 0)
        .map(|total| (total.saturating_sub(get("SwapFree").unwrap_or(0)), total));
    (mem, swap)
}

/// `/proc/diskstats`: (device, bytes read, bytes written) for each line.
/// Its sectors are always 512 bytes, whatever the disk's own.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn parse_diskstats(text: &str) -> Vec<(String, u64, u64)> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 10 {
                return None;
            }
            let read: u64 = f[5].parse().ok()?;
            let written: u64 = f[9].parse().ok()?;
            Some((f[2].to_string(), read * 512, written * 512))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_from_proc_stat() {
        let a = "cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 50 0 25 400 25 0 0 0 0 0\ncpu1 50 0 25 400 25 0 0 0 0 0\nintr 1 2 3\n";
        let b = "cpu  200 0 100 900 100 0 0 0 0 0\ncpu0 150 0 75 400 25 0 0 0 0 0\ncpu1 50 0 25 500 75 0 0 0 0 0\n";
        let (wa, ca) = parse_proc_stat(a);
        let (wb, cb) = parse_proc_stat(b);
        assert_eq!(ca.len(), 2);
        // 150 busy of 300
        assert_eq!(busy_between(wa.unwrap(), wb.unwrap()), Some(0.5));
        assert_eq!(busy_between(ca[0], cb[0]), Some(1.0));
        assert_eq!(busy_between(ca[1], cb[1]), Some(0.0));
        assert_eq!(busy_between(cb[1], ca[1]), None);
    }

    #[test]
    fn memory_from_meminfo() {
        let t = "MemTotal:       16000000 kB\nMemFree:         1000000 kB\nMemAvailable:    6000000 kB\nCached: 3000000 kB\nSwapTotal:       2000000 kB\nSwapFree:        1500000 kB\n";
        let (m, s) = parse_meminfo(t);
        assert_eq!(m, Some((10_000_000 * 1024, 16_000_000 * 1024)));
        assert_eq!(s, Some((500_000 * 1024, 2_000_000 * 1024)));
        let (_, s) = parse_meminfo("MemTotal: 10 kB\nSwapTotal: 0 kB\n");
        assert_eq!(s, None);
    }

    #[test]
    fn disks_from_diskstats() {
        let t = " 259       0 nvme0n1 1000 0 2048 300 500 0 4096 200 0 400 500 0 0 0 0\n   7       0 loop0 1 0 2 0 0 0 0 0 0 0 0\n";
        let v = parse_diskstats(t);
        assert_eq!(v[0], ("nvme0n1".to_string(), 2048 * 512, 4096 * 512));
        assert_eq!(v[1].0, "loop0");
    }

    #[test]
    fn sizes_and_temperatures() {
        assert_eq!(size(7_900_000_000, false), "7.9 GB");
        assert_eq!(size(512, false), "512 B");
        assert_eq!(size(32 * 1024 * 1024 * 1024, true), "32 GiB");
        assert_eq!(temperature(54.4, false), "54°C");
        assert_eq!(temperature(100.0, true), "212°F");
        assert_eq!(heat(30.0), 0.0);
        assert_eq!(heat(95.0), 1.0);
    }

    #[test]
    fn usb_speeds() {
        let d = |s| UsbDevice {
            id: "1-2".into(),
            name: "Drive".into(),
            speed_mbps: s,
        };
        assert_eq!(d(Some(480)).speed_label().unwrap(), "USB 2 · 480 Mb/s");
        assert_eq!(d(Some(5000)).speed_label().unwrap(), "USB 3 · 5 Gb/s");
        assert_eq!(d(None).speed_label(), None);
    }

    #[test]
    fn the_cpus_temperature_is_its_hottest_sensor() {
        let s = |kind, c| Sensor {
            kind,
            label: String::new(),
            celsius: c,
        };
        let v = [
            s(SensorKind::Cpu, 50.0),
            s(SensorKind::Drive, 70.0),
            s(SensorKind::Cpu, 61.0),
        ];
        assert_eq!(cpu_temp(&v), Some(61.0));
        assert_eq!(hottest(&v, SensorKind::Gpu), None);
    }
}
