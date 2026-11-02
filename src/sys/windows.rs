//! Windows: what an ordinary user can read.
//!
//! - CPU (each core), disks and GPU load from performance counters (PDH).
//! - Memory from `GlobalMemoryStatusEx`; drives from the drive letters.
//! - Temperatures and fans from LibreHardwareMonitor when it's running (it
//!   publishes its sensors to WMI); Windows itself has no way to read the
//!   CPU's temperature without a driver. Failing that, the ACPI thermal
//!   zones, which some computers report.
//! - USB devices from SetupAPI. Windows doesn't say what speed each link
//!   runs at without opening the hubs, so that's left out.

use crate::metrics::{Fan, Sensor, SensorKind, Snapshot, Sys, UsbDevice, Volume};
use crate::rates::Rate;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Instant;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceInstanceIdW, SetupDiGetDevicePropertyW, SetupDiGetDeviceRegistryPropertyW,
    DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO, SPDRP_DEVICEDESC, SPDRP_FRIENDLYNAME,
    SP_DEVINFO_DATA,
};
use windows_sys::Win32::Devices::Properties::DEVPKEY_Device_BusReportedDeviceDesc;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
};
use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W,
    PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn from_wide(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: a NUL-terminated string from the system.
    unsafe {
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
    }
}

fn trim_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

// ---------------------------------------------------------------- PDH

struct Pdh {
    query: PDH_HQUERY,
    cores: Option<PDH_HCOUNTER>,
    total: Option<PDH_HCOUNTER>,
    read: Option<PDH_HCOUNTER>,
    write: Option<PDH_HCOUNTER>,
    gpu: Option<PDH_HCOUNTER>,
    zones: Option<PDH_HCOUNTER>,
}

impl Pdh {
    fn open() -> Option<Pdh> {
        let mut query: PDH_HQUERY = std::ptr::null_mut();
        // SAFETY: opens a query on the live counters; the handle is ours.
        if unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut query) } != 0 {
            return None;
        }
        let add = |path: &str| {
            let w = wide(path);
            let mut c: PDH_HCOUNTER = std::ptr::null_mut();
            // SAFETY: `w` outlives the call; the counter belongs to `query`.
            (unsafe { PdhAddEnglishCounterW(query, w.as_ptr(), 0, &mut c) } == 0).then_some(c)
        };
        let p = Pdh {
            query,
            cores: add(r"\Processor(*)\% Processor Time"),
            total: add(r"\Processor(_Total)\% Processor Time"),
            read: add(r"\PhysicalDisk(_Total)\Disk Read Bytes/sec"),
            write: add(r"\PhysicalDisk(_Total)\Disk Write Bytes/sec"),
            gpu: add(r"\GPU Engine(*engtype_3D)\Utilization Percentage"),
            zones: add(r"\Thermal Zone Information(*)\Temperature"),
        };
        // Rates need two collections: the first is the baseline.
        // SAFETY: a query we opened.
        unsafe { PdhCollectQueryData(query) };
        Some(p)
    }

    fn collect(&self) {
        // SAFETY: a query we opened.
        unsafe { PdhCollectQueryData(self.query) };
    }

    fn value(&self, c: Option<PDH_HCOUNTER>) -> Option<f64> {
        let c = c?;
        // SAFETY: a counter of our query, read into a local.
        unsafe {
            let mut v: PDH_FMT_COUNTERVALUE = std::mem::zeroed();
            (PdhGetFormattedCounterValue(c, PDH_FMT_DOUBLE, std::ptr::null_mut(), &mut v) == 0
                && v.CStatus == 0)
                .then_some(v.Anonymous.doubleValue)
        }
    }

    /// A wildcard counter's instances: (name, value).
    fn array(&self, c: Option<PDH_HCOUNTER>) -> Vec<(String, f64)> {
        let Some(c) = c else { return Vec::new() };
        // SAFETY: asks for the size, then fills a buffer of that size; the
        // names point into the same buffer.
        unsafe {
            let (mut size, mut count) = (0u32, 0u32);
            PdhGetFormattedCounterArrayW(
                c,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                std::ptr::null_mut(),
            );
            if size == 0 {
                return Vec::new();
            }
            let item = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let mut buf: Vec<u64> = vec![0; (size as usize).div_ceil(8).max(item / 8 + 1)];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(c, PDH_FMT_DOUBLE, &mut size, &mut count, items) != 0 {
                return Vec::new();
            }
            (0..count as usize)
                .map(|i| &*items.add(i))
                .filter(|it| it.FmtValue.CStatus == 0)
                .map(|it| (from_wide(it.szName), it.FmtValue.Anonymous.doubleValue))
                .collect()
        }
    }
}

// ---------------------------------------------------------------- LibreHardwareMonitor

#[derive(Deserialize)]
#[serde(rename = "Sensor")]
#[serde(rename_all = "PascalCase")]
struct LhmSensor {
    name: String,
    sensor_type: String,
    value: f32,
    identifier: String,
}

fn lhm_kind(identifier: &str) -> SensorKind {
    let id = identifier.to_ascii_lowercase();
    if id.contains("cpu/") {
        SensorKind::Cpu
    } else if id.starts_with("/gpu") {
        SensorKind::Gpu
    } else if id.starts_with("/nvme") || id.starts_with("/hdd") || id.starts_with("/ssd") {
        SensorKind::Drive
    } else if id.starts_with("/lpc") || id.starts_with("/mainboard") {
        SensorKind::Board
    } else {
        SensorKind::Other
    }
}

fn kind_word(k: SensorKind) -> &'static str {
    match k {
        SensorKind::Cpu => "CPU",
        SensorKind::Gpu => "GPU",
        SensorKind::Drive => "Drive",
        SensorKind::Board => "Board",
        SensorKind::Other => "Sensor",
    }
}

// ---------------------------------------------------------------- the reader

pub struct Platform {
    pdh: Option<Pdh>,
    cpu_name: Option<String>,
    lhm: Option<wmi::WMIConnection>,
    lhm_tried: Option<Instant>,
    usb: Option<Vec<UsbDevice>>,
    usb_read: Option<Instant>,
}

impl Platform {
    pub fn new() -> Platform {
        Platform {
            pdh: Pdh::open(),
            cpu_name: cpu_name(),
            lhm: None,
            lhm_tried: None,
            usb: None,
            usb_read: None,
        }
    }

    pub fn read(&mut self, _dt: f64, sys: &mut Sys, snap: &mut Snapshot) {
        snap.cpu_name = self.cpu_name.clone();
        if let Some(p) = &self.pdh {
            p.collect();
            sys.cpu = p.value(p.total).map(|v| (v / 100.0).clamp(0.0, 1.0) as f32);
            let mut cores: Vec<(u32, f32)> = p
                .array(p.cores)
                .into_iter()
                .filter_map(|(n, v)| {
                    // "0", "1", … and "0,0" on machines with processor
                    // groups; "_Total" is left out.
                    let k: u32 = n.replace(',', "").parse().ok()?;
                    Some((k, (v / 100.0).clamp(0.0, 1.0) as f32))
                })
                .collect();
            cores.sort_by_key(|c| c.0);
            snap.cores = cores.into_iter().map(|c| c.1).collect();
            if let (Some(r), Some(w)) = (p.value(p.read), p.value(p.write)) {
                sys.disk = Some(Rate { down: r, up: w });
            }
            // The 3D engine's load, summed over the programs using it, for
            // the busiest adapter.
            let mut by_adapter: HashMap<String, f64> = HashMap::new();
            for (n, v) in p.array(p.gpu) {
                let key = n
                    .find("luid_")
                    .map(|i| {
                        let rest = &n[i..];
                        rest[..rest.find("_eng_").unwrap_or(rest.len())].to_string()
                    })
                    .unwrap_or(n);
                *by_adapter.entry(key).or_default() += v;
            }
            if let Some(max) = by_adapter.values().copied().reduce(f64::max) {
                sys.gpu = Some((max / 100.0).clamp(0.0, 1.0) as f32);
            }
            // ACPI zones, in kelvin, for when there's nothing better.
            for (n, k) in p.array(p.zones) {
                let c = (k - 273.15) as f32;
                if (1.0..150.0).contains(&c) {
                    snap.sensors.push(Sensor {
                        kind: SensorKind::Board,
                        label: format!("ACPI zone {}", n.trim_start_matches(r"\_TZ.")),
                        celsius: c,
                    });
                }
            }
        }

        // Memory
        // SAFETY: the struct's size is set as the call asks.
        unsafe {
            let mut m: MEMORYSTATUSEX = std::mem::zeroed();
            m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut m) != 0 && m.ullTotalPhys > 0 {
                sys.mem = Some((m.ullTotalPhys - m.ullAvailPhys, m.ullTotalPhys));
            }
        }
        snap.volumes = volumes();

        self.temperatures(snap);
        if snap.gpu.is_none() && sys.gpu.is_some() {
            snap.gpu = Some(crate::metrics::Gpu {
                name: "Graphics".into(),
                busy: sys.gpu,
                temp: crate::metrics::hottest(&snap.sensors, SensorKind::Gpu),
                ..Default::default()
            });
        }

        // USB: every two seconds is plenty.
        if self
            .usb_read
            .is_none_or(|t| t.elapsed().as_secs_f32() >= 2.0)
        {
            self.usb = usb();
            self.usb_read = Some(Instant::now());
        }
        snap.usb = self.usb.clone();
    }

    fn temperatures(&mut self, snap: &mut Snapshot) {
        if self.lhm.is_none() && self.lhm_tried.is_none_or(|t| t.elapsed().as_secs() >= 30) {
            self.lhm_tried = Some(Instant::now());
            self.lhm = wmi::WMIConnection::with_namespace_path(r"ROOT\LibreHardwareMonitor").ok();
        }
        let Some(c) = &self.lhm else {
            snap.sensors_note = Some(
                "Run LibreHardwareMonitor for CPU, GPU and drive temperatures and fans: Windows can't read them on its own."
                    .into(),
            );
            return;
        };
        let rows: Result<Vec<LhmSensor>, _> = c.raw_query(
            "SELECT Name, SensorType, Value, Identifier FROM Sensor WHERE SensorType='Temperature' OR SensorType='Fan'",
        );
        match rows {
            Ok(rows) => {
                // LibreHardwareMonitor's readings replace the ACPI zones.
                snap.sensors.clear();
                for r in rows {
                    if r.sensor_type == "Fan" {
                        if r.value > 0.0 {
                            snap.fans.push(Fan {
                                label: r.name,
                                rpm: r.value.round() as u32,
                            });
                        }
                        continue;
                    }
                    let kind = lhm_kind(&r.identifier);
                    snap.sensors.push(Sensor {
                        kind,
                        label: format!("{} · {}", kind_word(kind), r.name),
                        celsius: r.value,
                    });
                }
                snap.sensors
                    .sort_by(|a, b| a.kind.cmp(&b.kind).then(a.label.cmp(&b.label)));
                snap.sensors_note = Some("From LibreHardwareMonitor".into());
            }
            // It was closed: try again later.
            Err(_) => self.lhm = None,
        }
    }
}

fn cpu_name() -> Option<String> {
    let key = wide(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0");
    let value = wide("ProcessorNameString");
    let mut buf = [0u16; 128];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: reads a string into our buffer, of the size given.
    let err = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    (err == 0).then(|| {
        let s = trim_wide(&buf).replace("(R)", "").replace("(TM)", "");
        let s = s.split(" @ ").next().unwrap_or("").to_string();
        s.split_whitespace()
            .filter(|w| !w.ends_with("-Core") && *w != "Processor" && *w != "CPU")
            .collect::<Vec<_>>()
            .join(" ")
    })
}

fn volumes() -> Vec<Volume> {
    let mut buf = [0u16; 512];
    // SAFETY: fills our buffer with "C:\␀D:\␀…␀".
    let n = unsafe { GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr()) } as usize;
    let mut v = Vec::new();
    for root in buf[..n.min(buf.len())]
        .split(|&c| c == 0)
        .filter(|s| !s.is_empty())
    {
        let mut path = root.to_vec();
        path.push(0);
        // SAFETY: `path` is NUL-terminated and outlives each call.
        unsafe {
            // Removable (2) and fixed (3) drives only: not CD drives, which
            // spin up, or network drives, which can hang.
            let kind = GetDriveTypeW(path.as_ptr());
            if kind != 2 && kind != 3 {
                continue;
            }
            let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
            if GetDiskFreeSpaceExW(path.as_ptr(), &mut avail, &mut total, &mut free) == 0
                || total == 0
            {
                continue;
            }
            let mut label = [0u16; 128];
            GetVolumeInformationW(
                path.as_ptr(),
                label.as_mut_ptr(),
                label.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            );
            let letter = String::from_utf16_lossy(root)
                .trim_end_matches('\\')
                .to_string();
            let label = trim_wide(&label);
            v.push(Volume {
                name: if label.is_empty() {
                    letter
                } else {
                    format!("{letter} {label}")
                },
                used: total.saturating_sub(free),
                total,
            });
        }
    }
    v
}

fn usb() -> Option<Vec<UsbDevice>> {
    let enumerator = wide("USB");
    // SAFETY: SetupAPI's device list, walked and destroyed here.
    unsafe {
        let set = SetupDiGetClassDevsW(
            std::ptr::null(),
            enumerator.as_ptr(),
            std::ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_ALLCLASSES,
        );
        if set == INVALID_HANDLE_VALUE as HDEVINFO {
            return None;
        }
        let mut v = Vec::new();
        let mut i = 0;
        loop {
            let mut data: SP_DEVINFO_DATA = std::mem::zeroed();
            data.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiEnumDeviceInfo(set, i, &mut data) == 0 {
                break;
            }
            i += 1;
            let mut id = [0u16; 256];
            if SetupDiGetDeviceInstanceIdW(
                set,
                &data,
                id.as_mut_ptr(),
                id.len() as u32,
                std::ptr::null_mut(),
            ) == 0
            {
                continue;
            }
            let id = trim_wide(&id);
            // Interfaces of a composite device ("&MI_") and hubs are not
            // devices you plugged in.
            if id.contains("&MI_") || id.contains("ROOT_HUB") {
                continue;
            }
            let mut name = [0u16; 256];
            let mut ty = 0u32;
            let mut got = SetupDiGetDevicePropertyW(
                set,
                &data,
                &DEVPKEY_Device_BusReportedDeviceDesc,
                &mut ty,
                name.as_mut_ptr().cast(),
                (name.len() * 2) as u32,
                std::ptr::null_mut(),
                0,
            ) != 0;
            for prop in [SPDRP_FRIENDLYNAME, SPDRP_DEVICEDESC] {
                if got {
                    break;
                }
                got = SetupDiGetDeviceRegistryPropertyW(
                    set,
                    &data,
                    prop,
                    std::ptr::null_mut(),
                    name.as_mut_ptr().cast(),
                    (name.len() * 2) as u32,
                    std::ptr::null_mut(),
                ) != 0;
            }
            let name = trim_wide(&name);
            if name.is_empty() || name.contains("Hub") {
                continue;
            }
            v.push(UsbDevice {
                id,
                name,
                speed_mbps: None,
            });
        }
        SetupDiDestroyDeviceInfoList(set);
        v.sort_by(|a, b| a.name.cmp(&b.name));
        Some(v)
    }
}
