//! Timster's Audio Works Net Meter (NM-001): download and upload rates,
//! for the Internet and the local network, as scrolling graphs in a small
//! window to keep on the desktop. It only reads the computer's own
//! counters and never sends anything.

// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod autostart;
mod glass;
mod icon;
// Windows has no LAN figure yet (see net/windows.rs).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod lan;
mod net;
mod rates;
mod sampler;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TAW Net Meter")
            .with_app_id("taw-netmeter")
            .with_inner_size(app::DEFAULT_SIZE)
            .with_min_inner_size(app::MIN_SIZE)
            // The meter draws its own controls; Settings
            // can bring back the system's title bar.
            .with_decorations(false)
            // Glass: the desktop shows through.
            .with_transparent(true)
            .with_icon(icon::icon()),
        ..Default::default()
    };
    eframe::run_native(
        "TAW Net Meter",
        options,
        Box::new(|cc| Ok(Box::new(app::NetMeterApp::new(cc)))),
    )
}
