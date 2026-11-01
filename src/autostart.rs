//! Starting the meter when the user logs in. The system holds the setting,
//! not the meter's saved settings, so it's read back from there:
//!
//! - Linux: `~/.config/autostart/taw-netmeter.desktop` (the XDG autostart
//!   folder every desktop reads).
//! - Windows: a "TAW Net Meter" value under the user's
//!   `Software\Microsoft\Windows\CurrentVersion\Run` key.
//!
//! Both point at the program as it was run, so turning it on from a copy
//! that later moves leaves it starting the old copy; the meter refreshes
//! the entry each time it starts.

#[cfg(target_os = "linux")]
mod imp {
    use std::path::PathBuf;

    fn entry_path() -> Option<PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(config.join("autostart").join("taw-netmeter.desktop"))
    }

    pub fn is_enabled() -> bool {
        entry_path().is_some_and(|p| p.is_file())
    }

    pub fn set(on: bool) -> Result<(), String> {
        let path = entry_path().ok_or("no home folder")?;
        if !on {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            };
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, super::desktop_entry(&exe.to_string_lossy()))
            .map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "TAW Net Meter";

    pub fn is_enabled() -> bool {
        let (key, value) = (wide(KEY), wide(VALUE));
        // SAFETY: asks only for the value's size; the strings outlive the call.
        let err = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        err == 0
    }

    pub fn set(on: bool) -> Result<(), String> {
        let (key, value) = (wide(KEY), wide(VALUE));
        let err = if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let data = wide(&format!("\"{}\"", exe.display()));
            // SAFETY: `data` is a NUL-terminated UTF-16 string of the length
            // given, and every buffer outlives the call.
            unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    value.as_ptr(),
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            }
        } else {
            // SAFETY: the strings outlive the call.
            let err =
                unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) };
            // Already gone is fine.
            if err == 2 {
                0
            } else {
                err
            }
        };
        if err == 0 {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(err as i32).to_string())
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    pub fn is_enabled() -> bool {
        false
    }

    pub fn set(_on: bool) -> Result<(), String> {
        Err("not supported on this system".into())
    }
}

pub use imp::{is_enabled, set};

/// The autostart entry for the program at `exe`, with the path quoted as
/// the desktop entry spec asks.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn desktop_entry(exe: &str) -> String {
    let mut quoted = String::from("\"");
    for c in exe.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=TAW Net Meter\n\
         Comment=Download and upload rates for the Internet and the local network\n\
         Exec={quoted}\n\
         Icon=network-transmit-receive\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_quotes_the_path() {
        let e = desktop_entry("/home/me/Apps/Net Meter $1/taw-netmeter");
        assert!(e.contains("Exec=\"/home/me/Apps/Net Meter \\$1/taw-netmeter\"\n"));
        assert!(e.starts_with("[Desktop Entry]\nType=Application\n"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn turns_on_and_off() {
        let dir =
            std::env::temp_dir().join(format!("taw-netmeter-autostart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // SAFETY: the only test that touches XDG_CONFIG_HOME.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
        assert!(!is_enabled());
        set(true).unwrap();
        assert!(is_enabled());
        let text = std::fs::read_to_string(dir.join("autostart/taw-netmeter.desktop")).unwrap();
        assert!(text.contains("Exec=\""));
        set(false).unwrap();
        assert!(!is_enabled());
        set(false).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
