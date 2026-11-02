//! NVIDIA's management library (NVML), which the driver installs on Linux
//! and Windows alike: load, busy, video memory, temperature and power for
//! the first NVIDIA GPU, with no special rights. Loaded when it's there;
//! without an NVIDIA driver this does nothing.

use crate::metrics::Gpu;
use libloading::{Library, Symbol};
use std::ffi::{c_char, c_int, c_uint, c_void};

type Device = *mut c_void;

#[repr(C)]
#[derive(Default)]
struct Utilization {
    gpu: c_uint,
    memory: c_uint,
}

#[repr(C)]
#[derive(Default)]
struct Memory {
    total: u64,
    free: u64,
    used: u64,
}

pub struct Nvml {
    lib: Library,
    device: Device,
    name: String,
}

#[cfg(windows)]
const NAMES: &[&str] = &["nvml.dll"];
#[cfg(not(windows))]
const NAMES: &[&str] = &["libnvidia-ml.so.1", "libnvidia-ml.so"];

impl Nvml {
    pub fn open() -> Option<Nvml> {
        // SAFETY: NVML's documented C functions, called with the types its
        // header gives.
        unsafe {
            let lib = NAMES.iter().find_map(|n| Library::new(*n).ok())?;
            let init: Symbol<unsafe extern "C" fn() -> c_int> = lib.get(b"nvmlInit_v2\0").ok()?;
            if init() != 0 {
                return None;
            }
            let by_index: Symbol<unsafe extern "C" fn(c_uint, *mut Device) -> c_int> =
                lib.get(b"nvmlDeviceGetHandleByIndex_v2\0").ok()?;
            let mut device: Device = std::ptr::null_mut();
            if by_index(0, &mut device) != 0 || device.is_null() {
                return None;
            }
            let mut name = String::from("NVIDIA GPU");
            if let Ok(get) = lib.get::<unsafe extern "C" fn(Device, *mut c_char, c_uint) -> c_int>(
                b"nvmlDeviceGetName\0",
            ) {
                let mut buf = [0 as c_char; 96];
                if get(device, buf.as_mut_ptr(), buf.len() as c_uint) == 0 {
                    name = std::ffi::CStr::from_ptr(buf.as_ptr())
                        .to_string_lossy()
                        .into_owned();
                }
            }
            Some(Nvml { lib, device, name })
        }
    }

    pub fn read(&self) -> Gpu {
        let mut g = Gpu {
            name: self.name.clone(),
            ..Default::default()
        };
        // SAFETY: as in `open`; every out-pointer is to a local of the
        // header's type.
        unsafe {
            if let Ok(f) = self
                .lib
                .get::<unsafe extern "C" fn(Device, *mut Utilization) -> c_int>(
                    b"nvmlDeviceGetUtilizationRates\0",
                )
            {
                let mut u = Utilization::default();
                if f(self.device, &mut u) == 0 {
                    g.busy = Some(u.gpu.min(100) as f32 / 100.0);
                }
            }
            if let Ok(f) = self
                .lib
                .get::<unsafe extern "C" fn(Device, *mut Memory) -> c_int>(
                    b"nvmlDeviceGetMemoryInfo\0",
                )
            {
                let mut m = Memory::default();
                if f(self.device, &mut m) == 0 && m.total > 0 {
                    g.vram = Some((m.used, m.total));
                }
            }
            if let Ok(f) = self
                .lib
                .get::<unsafe extern "C" fn(Device, c_int, *mut c_uint) -> c_int>(
                    b"nvmlDeviceGetTemperature\0",
                )
            {
                let mut t: c_uint = 0;
                // NVML_TEMPERATURE_GPU
                if f(self.device, 0, &mut t) == 0 {
                    g.temp = Some(t as f32);
                }
            }
            if let Ok(f) = self
                .lib
                .get::<unsafe extern "C" fn(Device, *mut c_uint) -> c_int>(
                    b"nvmlDeviceGetPowerUsage\0",
                )
            {
                let mut mw: c_uint = 0;
                if f(self.device, &mut mw) == 0 {
                    g.power = Some(mw as f32 / 1000.0);
                }
            }
        }
        g
    }
}
