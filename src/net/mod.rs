//! The operating system's side: what each network adaptor has carried, and
//! how much of it went to and from the local network.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{adaptors, LanMeter};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{adaptors, LanMeter};

#[cfg(not(any(target_os = "linux", windows)))]
mod other;
#[cfg(not(any(target_os = "linux", windows)))]
pub use other::{adaptors, LanMeter};

/// A network adaptor and the bytes it has carried since it came up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adaptor {
    /// What it's remembered by.
    pub id: String,
    /// What it's called in the menu.
    pub label: String,
    /// A real adaptor (Ethernet, Wi-Fi), not a bridge, tunnel or the
    /// loopback, whose traffic is counted again on the real one.
    pub physical: bool,
    pub rx: u64,
    pub tx: u64,
}
