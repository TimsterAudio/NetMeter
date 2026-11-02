//! Linux: everything from `/proc` and `/sys`, which an ordinary user can
//! read. CPU from `/proc/stat`, memory from `/proc/meminfo`, disks from
//! `/proc/diskstats` and the mounts, temperatures and fans from hwmon,
//! AMD GPUs from their DRM files, USB from `/sys/bus/usb/devices`.

use crate::metrics::{
    self, CpuTimes, Fan, Gpu, Sensor, SensorKind, Snapshot, Sys, UsbDevice, Volume,
};
use crate::rates::Rate;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

fn read(p: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn read_num<T: std::str::FromStr>(p: impl AsRef<Path>) -> Option<T> {
    read(p)?.parse().ok()
}

pub struct Platform {
    cpu: Option<(CpuTimes, Vec<CpuTimes>)>,
    disks: HashMap<String, (u64, u64)>,
    cpu_name: Option<String>,
    amd: Option<PathBuf>,
}

impl Platform {
    pub fn new() -> Platform {
        let cpu_name = read("/proc/cpuinfo").and_then(|t| {
            t.lines()
                .find(|l| l.starts_with("model name") || l.starts_with("Model"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| tidy_cpu_name(v.trim()))
        });
        Platform {
            cpu: None,
            disks: HashMap::new(),
            cpu_name,
            amd: amd_gpu(),
        }
    }

    pub fn read(&mut self, dt: f64, sys: &mut Sys, snap: &mut Snapshot) {
        snap.cpu_name = self.cpu_name.clone();

        // CPU
        if let Some(text) = read("/proc/stat") {
            let (all, cores) = metrics::parse_proc_stat(&text);
            if let (Some(all), Some((was, was_cores))) = (all, &self.cpu) {
                sys.cpu = metrics::busy_between(*was, all);
                snap.cores = cores
                    .iter()
                    .zip(was_cores)
                    .map(|(n, w)| metrics::busy_between(*w, *n).unwrap_or(0.0))
                    .collect();
            }
            if let Some(all) = all {
                self.cpu = Some((all, cores));
            }
        }

        // Memory
        if let Some(text) = read("/proc/meminfo") {
            (sys.mem, sys.swap) = metrics::parse_meminfo(&text);
        }

        // Disks: the real ones (those with a device), not partitions,
        // loop devices or the mappers on top of a disk, which would count
        // the same bytes again.
        if let Some(text) = read("/proc/diskstats") {
            let (mut r, mut w) = (0u64, 0u64);
            let mut now = HashMap::new();
            let mut known = false;
            for (name, rd, wr) in metrics::parse_diskstats(&text) {
                if !Path::new(&format!("/sys/block/{name}/device")).exists() {
                    continue;
                }
                if let Some(&(r0, w0)) = self.disks.get(&name) {
                    r += rd.saturating_sub(r0);
                    w += wr.saturating_sub(w0);
                    known = true;
                }
                now.insert(name, (rd, wr));
            }
            if known {
                sys.disk = Some(Rate {
                    down: r as f64 / dt,
                    up: w as f64 / dt,
                });
            }
            self.disks = now;
        }
        snap.volumes = volumes();

        // Temperatures and fans
        let (sensors, fans) = hwmon();
        snap.sensors = sensors;
        snap.fans = fans;

        // An AMD GPU (NVIDIA's comes from NVML, in sys/mod.rs).
        if let Some(dev) = &self.amd {
            let busy = read_num::<f32>(dev.join("gpu_busy_percent")).map(|v| v / 100.0);
            let vram = read_num::<u64>(dev.join("mem_info_vram_used"))
                .zip(read_num::<u64>(dev.join("mem_info_vram_total")));
            let temp = metrics::hottest(&snap.sensors, SensorKind::Gpu);
            snap.gpu = Some(Gpu {
                name: amd_name(dev),
                busy,
                vram,
                temp,
                power: None,
            });
        }

        snap.usb = usb();
    }
}

/// "AMD Ryzen 7 5800X 8-Core Processor" → "AMD Ryzen 7 5800X".
fn tidy_cpu_name(s: &str) -> String {
    let s = s
        .replace("(R)", "")
        .replace("(TM)", "")
        .replace("(tm)", "")
        .replace(" CPU", "");
    let s = match s.find(" @ ") {
        Some(i) => s[..i].to_string(),
        None => s,
    };
    let words: Vec<&str> = s
        .split_whitespace()
        .filter(|w| !w.ends_with("-Core") && *w != "Processor")
        .collect();
    words.join(" ")
}

const REAL_FS: [&str; 11] = [
    "ext4", "ext3", "ext2", "btrfs", "xfs", "f2fs", "vfat", "exfat", "ntfs", "ntfs3", "fuseblk",
];

fn volumes() -> Vec<Volume> {
    let Some(mounts) = read("/proc/mounts") else {
        return Vec::new();
    };
    let mut seen = Vec::new();
    let mut v = Vec::new();
    for line in mounts.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 3 || !f[0].starts_with("/dev/") || !REAL_FS.contains(&f[2]) {
            continue;
        }
        // One entry per device (btrfs mounts the same one many times).
        if seen.contains(&f[0]) {
            continue;
        }
        seen.push(f[0]);
        let path = f[1].replace("\\040", " ");
        let Ok(c) = std::ffi::CString::new(path.clone()) else {
            continue;
        };
        // SAFETY: statvfs fills the zeroed struct; the path outlives the call.
        let st = unsafe {
            let mut st: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c.as_ptr(), &mut st) != 0 {
                continue;
            }
            st
        };
        let total = st.f_blocks as u64 * st.f_frsize as u64;
        let free = st.f_bavail as u64 * st.f_frsize as u64;
        if total == 0 {
            continue;
        }
        v.push(Volume {
            name: path,
            used: total.saturating_sub(free),
            total,
        });
    }
    v
}

fn hwmon() -> (Vec<Sensor>, Vec<Fan>) {
    let mut sensors = Vec::new();
    let mut fans = Vec::new();
    let Ok(dirs) = fs::read_dir("/sys/class/hwmon") else {
        return (sensors, fans);
    };
    for d in dirs.flatten() {
        let dir = d.path();
        let chip = read(dir.join("name")).unwrap_or_default();
        let kind = match chip.as_str() {
            "k10temp" | "coretemp" | "zenpower" | "cpu_thermal" | "cpu" => SensorKind::Cpu,
            "amdgpu" | "nouveau" | "radeon" | "i915" | "xe" => SensorKind::Gpu,
            "nvme" | "drivetemp" => SensorKind::Drive,
            "acpitz" | "nct6775" | "nct6683" | "it87" | "asus_ec_sensors" => SensorKind::Board,
            _ => SensorKind::Other,
        };
        let Ok(files) = fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<String> = files
            .flatten()
            .map(|f| f.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for n in &names {
            if let Some(i) = n
                .strip_prefix("temp")
                .and_then(|r| r.strip_suffix("_input"))
            {
                let Some(milli) = read_num::<i64>(dir.join(n)) else {
                    continue;
                };
                let c = milli as f32 / 1000.0;
                // Unplugged inputs read as nonsense.
                if !(-20.0..=150.0).contains(&c) || c == 0.0 {
                    continue;
                }
                let label = read(dir.join(format!("temp{i}_label")));
                sensors.push(Sensor {
                    kind,
                    label: sensor_label(&chip, label.as_deref(), &dir),
                    celsius: c,
                });
            } else if let Some(i) = n.strip_prefix("fan").and_then(|r| r.strip_suffix("_input")) {
                let Some(rpm) = read_num::<u32>(dir.join(n)) else {
                    continue;
                };
                let label =
                    read(dir.join(format!("fan{i}_label"))).unwrap_or_else(|| format!("Fan {i}"));
                fans.push(Fan { label, rpm });
            }
        }
    }
    sensors.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.label.cmp(&b.label)));
    (sensors, fans)
}

/// "CPU (Tctl)", "GPU (edge)", "NVMe drive 0", …
fn sensor_label(chip: &str, label: Option<&str>, dir: &Path) -> String {
    let what = match chip {
        "k10temp" | "coretemp" | "zenpower" | "cpu_thermal" | "cpu" => "CPU".to_string(),
        "amdgpu" | "radeon" | "nouveau" => "GPU".to_string(),
        "i915" | "xe" => "Intel graphics".to_string(),
        "nvme" => {
            // hwmonN/device is the controller, nvme0.
            let ctrl = fs::canonicalize(dir.join("device"))
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "nvme".into());
            format!("Drive {ctrl}")
        }
        "drivetemp" => "Drive".to_string(),
        "acpitz" => "Board (ACPI)".to_string(),
        "" => "Sensor".to_string(),
        other => other.to_string(),
    };
    match label {
        Some(l) if !l.is_empty() && l != "Composite" => format!("{what} · {l}"),
        _ => what,
    }
}

/// The first AMD graphics card's device folder.
fn amd_gpu() -> Option<PathBuf> {
    let mut cards: Vec<PathBuf> = fs::read_dir("/sys/class/drm")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().into_owned());
            n.is_some_and(|n| n.starts_with("card") && !n.contains('-'))
        })
        .map(|p| p.join("device"))
        .filter(|d| {
            read(d.join("vendor")).as_deref() == Some("0x1002")
                && d.join("gpu_busy_percent").exists()
        })
        .collect();
    cards.sort();
    cards.into_iter().next()
}

fn amd_name(dev: &Path) -> String {
    read(dev.join("product_name"))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "AMD graphics".into())
}

/// The USB devices plugged in, without the hubs; `None` without USB (or
/// where it can't be seen, such as in a container).
fn usb() -> Option<Vec<UsbDevice>> {
    let mut v = Vec::new();
    let dirs = fs::read_dir("/sys/bus/usb/devices").ok()?;
    for d in dirs.flatten() {
        let name = d.file_name().to_string_lossy().into_owned();
        // Devices are "1-2", "1-2.4"; "usb1" is a root hub, "1-2:1.0" an
        // interface.
        if name.starts_with("usb") || name.contains(':') {
            continue;
        }
        let dir = d.path();
        if read(dir.join("bDeviceClass")).as_deref() == Some("09") {
            continue;
        }
        let product = read(dir.join("product"));
        let maker = read(dir.join("manufacturer"));
        let label = match (maker, product) {
            (Some(m), Some(p)) if !p.contains(&m) => format!("{m} {p}"),
            (_, Some(p)) => p,
            (Some(m), None) => format!("{m} device"),
            (None, None) => {
                let id = read(dir.join("idVendor")).unwrap_or_default();
                let p = read(dir.join("idProduct")).unwrap_or_default();
                format!("USB device {id}:{p}")
            }
        };
        let speed = read(dir.join("speed"))
            .and_then(|s| s.parse::<f32>().ok())
            .map(|s| s.round() as u32);
        v.push(UsbDevice {
            id: name,
            name: label,
            speed_mbps: speed,
        });
    }
    v.sort_by(|a, b| a.name.cmp(&b.name));
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_names_are_tidied() {
        assert_eq!(
            tidy_cpu_name("AMD Ryzen 7 5800X 8-Core Processor"),
            "AMD Ryzen 7 5800X"
        );
        assert_eq!(
            tidy_cpu_name("Intel(R) Core(TM) i7-8700 CPU @ 3.20GHz"),
            "Intel Core i7-8700"
        );
    }

    #[test]
    fn reads_this_computer() {
        let mut p = Platform::new();
        let (mut sys, mut snap) = (Sys::default(), Snapshot::default());
        p.read(1.0, &mut sys, &mut snap);
        std::thread::sleep(std::time::Duration::from_millis(50));
        p.read(0.05, &mut sys, &mut snap);
        assert!(sys.cpu.is_some_and(|c| (0.0..=1.0).contains(&c)));
        let (used, total) = sys.mem.expect("memory");
        assert!(used > 0 && used <= total);
        assert!(!snap.cores.is_empty());
    }
}
