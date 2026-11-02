//! Reads the counters on a thread of its own, at the chosen interval, and
//! hands each reading to the window, so the graph keeps its history while
//! the window is minimised.

use crate::metrics::Snapshot;
use crate::net::{self, Adaptor, LanMeter};
use crate::rates::{Rate, Sample};
use crate::sys::SysMeter;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What the thread reads, set from the window.
pub struct Config {
    pub interval_ms: AtomicU32,
    /// One adaptor, or `None` for every real one.
    pub adaptor: Mutex<Option<String>>,
    pub stop: AtomicBool,
}

/// One reading: the graphs' sample, the system as it is now, and why the
/// network couldn't be read, if it couldn't.
pub struct Reading {
    pub sample: Sample,
    pub snap: Snapshot,
    pub net_error: Option<String>,
}

pub struct Sampler {
    pub config: Arc<Config>,
    pub rx: Receiver<Reading>,
    /// Whether this computer can tell the LAN from the Internet.
    pub lan_known: bool,
}

impl Sampler {
    pub fn start(interval_ms: u32, adaptor: Option<String>, ctx: egui::Context) -> Sampler {
        let config = Arc::new(Config {
            interval_ms: AtomicU32::new(interval_ms),
            adaptor: Mutex::new(adaptor),
            stop: AtomicBool::new(false),
        });
        let lan = LanMeter::new();
        let lan_known = lan.is_some();
        let (tx, rx) = mpsc::channel();
        let c = config.clone();
        std::thread::Builder::new()
            .name("net-meter".into())
            .spawn(move || run(c, lan, tx, ctx))
            .expect("start the meter's thread");
        Sampler {
            config,
            rx,
            lan_known,
        }
    }

    pub fn set_interval(&self, ms: u32) {
        self.config.interval_ms.store(ms, Ordering::Relaxed);
    }

    pub fn set_adaptor(&self, a: Option<String>) {
        *self.config.adaptor.lock().unwrap() = a;
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.config.stop.store(true, Ordering::Relaxed);
    }
}

/// The adaptors a choice covers: the one named, or every real one.
pub fn chosen<'a>(
    all: &'a [Adaptor],
    choice: Option<&'a str>,
) -> impl Iterator<Item = &'a Adaptor> {
    all.iter().filter(move |a| match choice {
        Some(id) => a.id == id,
        None => a.physical,
    })
}

/// Bytes received and sent by the chosen adaptors since `last`, which is
/// brought up to date. An adaptor that only just appeared, or whose count
/// went back (it was reset), adds nothing this time.
pub fn moved(
    last: &mut HashMap<String, (u64, u64)>,
    all: &[Adaptor],
    choice: Option<&str>,
) -> (u64, u64) {
    let (mut rx, mut tx) = (0, 0);
    for a in chosen(all, choice) {
        if let Some(&(r0, t0)) = last.get(&a.id) {
            rx += a.rx.saturating_sub(r0);
            tx += a.tx.saturating_sub(t0);
        }
    }
    last.clear();
    last.extend(all.iter().map(|a| (a.id.clone(), (a.rx, a.tx))));
    (rx, tx)
}

fn run(config: Arc<Config>, mut lan: Option<LanMeter>, out: Sender<Reading>, ctx: egui::Context) {
    let start = Instant::now();
    // Made here: some of it belongs to the thread that opens it.
    let mut sys = SysMeter::new();
    sys.read(1.0);
    let mut last = HashMap::new();
    let mut then = Instant::now();
    if let Ok(a) = net::adaptors() {
        moved(&mut last, &a, None);
    }
    loop {
        let ms = config.interval_ms.load(Ordering::Relaxed).clamp(250, 5000);
        std::thread::sleep(Duration::from_millis(ms as u64));
        if config.stop.load(Ordering::Relaxed) {
            return;
        }
        let now = Instant::now();
        let dt = now.duration_since(then).as_secs_f64().max(1e-3);
        then = now;
        let choice = config.adaptor.lock().unwrap().clone();
        let (sys_now, snap) = sys.read(dt);
        let mut sample = Sample {
            at: now.duration_since(start).as_secs_f64(),
            all: Rate::default(),
            lan: None,
            sys: sys_now,
        };
        let net_error = match net::adaptors() {
            Ok(a) => {
                let (rx, tx) = moved(&mut last, &a, choice.as_deref());
                sample.lan = lan.as_mut().and_then(LanMeter::read).map(|(r, t)| Rate {
                    down: r as f64 / dt,
                    up: t as f64 / dt,
                });
                sample.all = Rate {
                    down: rx as f64 / dt,
                    up: tx as f64 / dt,
                };
                None
            }
            Err(e) => Some(e.to_string()),
        };
        let reading = Reading {
            sample,
            snap,
            net_error,
        };
        if out.send(reading).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(id: &str, physical: bool, rx: u64, tx: u64) -> Adaptor {
        Adaptor {
            id: id.into(),
            label: id.into(),
            physical,
            rx,
            tx,
        }
    }

    #[test]
    fn counts_real_adaptors_or_the_one_chosen() {
        let mut last = HashMap::new();
        moved(
            &mut last,
            &[a("eth0", true, 100, 10), a("tun0", false, 50, 5)],
            None,
        );
        let now = [
            a("eth0", true, 1100, 210),
            a("tun0", false, 550, 105),
            a("wlan0", true, 999, 999),
        ];
        // The tunnel's traffic is already on eth0; wlan0 is new.
        assert_eq!(moved(&mut last.clone(), &now, None), (1000, 200));
        assert_eq!(moved(&mut last, &now, Some("tun0")), (500, 100));
        // A reset counter adds nothing.
        assert_eq!(moved(&mut last, &[a("eth0", true, 5, 5)], None), (0, 0));
    }
}
