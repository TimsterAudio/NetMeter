//! Rates, their units, and the history the display scrolls through. Plain
//! Rust, no UI and no operating system: everything here is unit tested.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Download and upload, in bytes a second.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rate {
    pub down: f64,
    pub up: f64,
}

impl Rate {
    pub fn max(self) -> f64 {
        self.down.max(self.up)
    }
}

/// One reading: everything through the chosen adaptors, and how much of it
/// went to and from the local network (`None` where that can't be told).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Seconds since the meter started.
    pub at: f64,
    pub all: Rate,
    pub lan: Option<Rate>,
}

impl Sample {
    /// The Internet's share: everything that isn't local. The LAN's figure
    /// is worked out from the connections, so it can come out a little over
    /// the adaptor's; never let that go below zero.
    pub fn internet(&self) -> Option<Rate> {
        self.lan.map(|l| Rate {
            down: (self.all.down - l.down).max(0.0),
            up: (self.all.up - l.up).max(0.0),
        })
    }

    pub fn lane(&self, lane: Lane) -> Option<Rate> {
        match lane {
            Lane::All => Some(self.all),
            Lane::Internet => self.internet(),
            Lane::Lan => self.lan.map(|l| Rate {
                down: l.down.min(self.all.down),
                up: l.up.min(self.all.up),
            }),
        }
    }
}

/// One graph in the display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    Internet,
    Lan,
    All,
}

impl Lane {
    pub fn name(self) -> &'static str {
        match self {
            Lane::Internet => "Internet",
            Lane::Lan => "LAN",
            Lane::All => "All traffic",
        }
    }
}

/// What the display shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Show {
    /// Internet over LAN.
    #[default]
    Both,
    Internet,
    Lan,
    All,
}

impl Show {
    pub const ALL: [Show; 4] = [Show::Both, Show::Internet, Show::Lan, Show::All];

    pub fn name(self) -> &'static str {
        match self {
            Show::Both => "Both",
            Show::Internet => "Internet",
            Show::Lan => "LAN",
            Show::All => "All",
        }
    }

    /// The graphs, top to bottom. Without a LAN figure there's only the
    /// whole.
    pub fn lanes(self, lan_known: bool) -> Vec<Lane> {
        if !lan_known {
            return vec![Lane::All];
        }
        match self {
            Show::Both => vec![Lane::Internet, Lane::Lan],
            Show::Internet => vec![Lane::Internet],
            Show::Lan => vec![Lane::Lan],
            Show::All => vec![Lane::All],
        }
    }
}

/// Bits (Mb/s, as broadband is sold) or bytes (MB/s, as downloads count).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Units {
    #[default]
    Bits,
    Bytes,
}

/// A fixed prefix, or whichever suits the figure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Prefix {
    #[default]
    Auto,
    Kilo,
    Mega,
    Giga,
}

impl Prefix {
    pub const ALL: [Prefix; 4] = [Prefix::Auto, Prefix::Kilo, Prefix::Mega, Prefix::Giga];

    fn power(self) -> Option<i32> {
        match self {
            Prefix::Auto => None,
            Prefix::Kilo => Some(1),
            Prefix::Mega => Some(2),
            Prefix::Giga => Some(3),
        }
    }
}

/// How rates are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Format {
    pub units: Units,
    pub prefix: Prefix,
    /// KiB/s, MiB/s: powers of 1024. Bytes only; bits are always decimal.
    pub binary: bool,
}

impl Format {
    fn base(&self) -> f64 {
        if self.binary && self.units == Units::Bytes {
            1024.0
        } else {
            1000.0
        }
    }

    /// Bytes a second in this format's units (bits or bytes).
    fn scaled(&self, bytes_s: f64) -> f64 {
        match self.units {
            Units::Bits => bytes_s * 8.0,
            Units::Bytes => bytes_s,
        }
    }

    /// The unit for a power of the base: "Mb/s", "MB/s", "MiB/s".
    pub fn unit(&self, power: i32) -> String {
        let bin = self.binary && self.units == Units::Bytes;
        let p = match (power, bin) {
            (0, _) => "",
            (1, false) => "k",
            (1, true) => "Ki",
            (2, false) => "M",
            (2, true) => "Mi",
            (3, false) => "G",
            (3, true) => "Gi",
            (_, false) => "T",
            (_, true) => "Ti",
        };
        let u = match self.units {
            Units::Bits => "b",
            Units::Bytes => "B",
        };
        format!("{p}{u}/s")
    }

    /// The power of the base a figure is shown in.
    fn power_for(&self, v: f64) -> i32 {
        self.prefix.power().unwrap_or_else(|| {
            let mut p = 0;
            let mut x = v;
            while x >= self.base() && p < 4 {
                x /= self.base();
                p += 1;
            }
            p
        })
    }

    /// The number and the unit apart: ("12.4", "Mb/s").
    pub fn parts(&self, bytes_s: f64) -> (String, String) {
        let v = self.scaled(bytes_s.max(0.0));
        let p = self.power_for(v);
        let x = v / self.base().powi(p);
        let n = if p == 0 && self.prefix == Prefix::Auto {
            format!("{x:.0}")
        } else if x < 10.0 {
            format!("{x:.2}")
        } else if x < 100.0 {
            format!("{x:.1}")
        } else {
            format!("{x:.0}")
        };
        (n, self.unit(p))
    }

    pub fn rate(&self, bytes_s: f64) -> String {
        let (n, u) = self.parts(bytes_s);
        format!("{n} {u}")
    }

    /// A round figure for a scale mark: "10 Mb/s", "2.5 GB/s".
    pub fn mark(&self, bytes_s: f64) -> String {
        let v = self.scaled(bytes_s.max(0.0));
        let p = Format {
            prefix: Prefix::Auto,
            ..*self
        }
        .power_for(v * 1.000_001);
        let x = v / self.base().powi(p);
        let n = format!("{:.2}", x);
        let n = n.trim_end_matches('0').trim_end_matches('.');
        format!("{n} {}", self.unit(p))
    }

    /// The smallest round figure (1, 2, 5 × 10ⁿ of a unit) at or above
    /// `bytes_s`, for the top of an automatic scale. Never below 10 kb/s (or
    /// 10 kB/s), so an idle line doesn't fill the graph with noise.
    pub fn nice_ceiling(&self, bytes_s: f64) -> f64 {
        let floor = 10.0 * self.base();
        let v = self.scaled(bytes_s).max(floor);
        let p = Format {
            prefix: Prefix::Auto,
            ..*self
        }
        .power_for(v);
        let unit = self.base().powi(p);
        let x = v / unit;
        let mag = 10f64.powf(x.log10().floor());
        let m = x / mag;
        let step = [1.0, 2.0, 5.0, 10.0]
            .into_iter()
            .find(|s| m <= s * (1.0 + 1e-9))
            .unwrap_or(10.0);
        let top = step * mag * unit;
        match self.units {
            Units::Bits => top / 8.0,
            Units::Bytes => top,
        }
    }

    /// The fixed scales to choose from, in bytes a second.
    pub fn fixed_scales(&self) -> Vec<f64> {
        match self.units {
            Units::Bits => [1e6, 10e6, 50e6, 100e6, 250e6, 500e6, 1e9, 2.5e9, 10e9]
                .iter()
                .map(|b| b / 8.0)
                .collect(),
            Units::Bytes => {
                let k = self.base();
                [
                    100.0 * k,
                    k * k,
                    5.0 * k * k,
                    10.0 * k * k,
                    25.0 * k * k,
                    50.0 * k * k,
                    100.0 * k * k,
                    250.0 * k * k,
                    k * k * k,
                ]
                .to_vec()
            }
        }
    }
}

/// Where a rate sits on a scale topped by `top`, from 0 at the bottom to 1.
/// The log scale runs from 1 kb/s up, so a trickle still shows next to a
/// download at full speed.
pub fn level(bytes_s: f64, top: f64, log: bool) -> f32 {
    if top <= 0.0 {
        return 0.0;
    }
    let l = if log {
        let floor = 125.0f64.min(top / 10.0);
        if bytes_s <= floor {
            0.0
        } else {
            (bytes_s / floor).ln() / (top / floor).ln()
        }
    } else {
        bytes_s / top
    };
    l.clamp(0.0, 1.0) as f32
}

/// The readings, oldest first, up to the longest history the display offers.
#[derive(Default)]
pub struct History {
    samples: VecDeque<Sample>,
}

/// The longest history the display can show, in seconds.
pub const HISTORY_MAX_S: f64 = 600.0;

impl History {
    pub fn push(&mut self, s: Sample) {
        self.samples.push_back(s);
        while let Some(first) = self.samples.front() {
            if s.at - first.at > HISTORY_MAX_S + 5.0 {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    pub fn latest(&self) -> Option<&Sample> {
        self.samples.back()
    }

    /// The readings in the last `span` seconds (counting back from the
    /// newest), with the one just before, so a line starts at the edge.
    pub fn recent(&self, span: f64) -> impl Iterator<Item = &Sample> {
        let newest = self.latest().map_or(0.0, |s| s.at);
        let first = self
            .samples
            .iter()
            .rposition(|s| newest - s.at > span)
            .unwrap_or(0);
        self.samples.range(first..)
    }

    /// The highest rate on a lane in the last `span` seconds.
    pub fn peak(&self, lane: Lane, span: f64) -> f64 {
        self.recent(span)
            .filter_map(|s| s.lane(lane))
            .map(Rate::max)
            .fold(0.0, f64::max)
    }

    /// The reading nearest `at`.
    pub fn nearest(&self, at: f64) -> Option<&Sample> {
        self.samples
            .iter()
            .min_by(|a, b| (a.at - at).abs().total_cmp(&(b.at - at).abs()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(units: Units, prefix: Prefix, binary: bool) -> Format {
        Format {
            units,
            prefix,
            binary,
        }
    }

    #[test]
    fn writes_rates() {
        let bits = f(Units::Bits, Prefix::Auto, false);
        assert_eq!(bits.rate(0.0), "0 b/s");
        assert_eq!(bits.rate(100.0), "800 b/s");
        assert_eq!(bits.rate(1_550_000.0), "12.4 Mb/s");
        assert_eq!(bits.rate(125_000_000.0), "1.00 Gb/s");
        let bytes = f(Units::Bytes, Prefix::Auto, false);
        assert_eq!(bytes.rate(1_550_000.0), "1.55 MB/s");
        assert_eq!(bytes.rate(999.0), "999 B/s");
        let bin = f(Units::Bytes, Prefix::Auto, true);
        assert_eq!(bin.rate(1024.0 * 1024.0 * 3.0), "3.00 MiB/s");
        assert_eq!(bin.rate(2048.0), "2.00 KiB/s");
        // Bits are always decimal, whatever the binary switch says.
        assert_eq!(f(Units::Bits, Prefix::Auto, true).rate(125.0), "1.00 kb/s");
    }

    #[test]
    fn fixed_prefixes() {
        let kb = f(Units::Bits, Prefix::Kilo, false);
        assert_eq!(kb.rate(1_550_000.0), "12400 kb/s");
        assert_eq!(kb.rate(10.0), "0.08 kb/s");
        let mb = f(Units::Bytes, Prefix::Mega, false);
        assert_eq!(mb.rate(25_000.0), "0.03 MB/s");
        assert_eq!(f(Units::Bits, Prefix::Giga, false).unit(3), "Gb/s");
    }

    #[test]
    fn scale_tops_are_round() {
        let bits = f(Units::Bits, Prefix::Auto, false);
        // 12.4 Mb/s → 20 Mb/s
        assert!((bits.nice_ceiling(1_550_000.0) * 8.0 - 20e6).abs() < 1.0);
        // 60 Mb/s → 100 Mb/s
        assert!((bits.nice_ceiling(7_500_000.0) * 8.0 - 100e6).abs() < 1.0);
        // Exactly 50 Mb/s stays 50.
        assert!((bits.nice_ceiling(6_250_000.0) * 8.0 - 50e6).abs() < 1.0);
        // Idle: the floor, 10 kb/s.
        assert!((bits.nice_ceiling(0.0) * 8.0 - 10e3).abs() < 1e-6);
        let bin = f(Units::Bytes, Prefix::Auto, true);
        // 3 MiB/s → 5 MiB/s
        assert!((bin.nice_ceiling(3.0 * 1024.0 * 1024.0) - 5.0 * 1024.0 * 1024.0).abs() < 1.0);
        assert_eq!(bits.mark(bits.nice_ceiling(1_550_000.0)), "20 Mb/s");
        assert_eq!(bits.mark(2.5e9 / 8.0), "2.5 Gb/s");
        assert_eq!(bin.mark(1024.0 * 1024.0), "1 MiB/s");
    }

    #[test]
    fn levels() {
        assert_eq!(level(50.0, 100.0, false), 0.5);
        assert_eq!(level(500.0, 100.0, false), 1.0);
        assert_eq!(level(0.0, 1e6, true), 0.0);
        assert!((level(1e6, 1e6, true) - 1.0).abs() < 1e-6);
        let mid = level(125.0 * 1000.0, 125.0 * 1e6, true);
        assert!((mid - 0.5).abs() < 1e-3, "{mid}");
    }

    #[test]
    fn internet_is_the_rest() {
        let s = Sample {
            at: 0.0,
            all: Rate {
                down: 100.0,
                up: 10.0,
            },
            lan: Some(Rate {
                down: 30.0,
                up: 12.0,
            }),
        };
        assert_eq!(
            s.lane(Lane::Internet),
            Some(Rate {
                down: 70.0,
                up: 0.0
            })
        );
        assert_eq!(
            s.lane(Lane::Lan),
            Some(Rate {
                down: 30.0,
                up: 10.0
            })
        );
        let unknown = Sample { lan: None, ..s };
        assert_eq!(unknown.lane(Lane::Internet), None);
        assert_eq!(Show::Both.lanes(false), vec![Lane::All]);
        assert_eq!(Show::Both.lanes(true), vec![Lane::Internet, Lane::Lan]);
    }

    #[test]
    fn history_keeps_ten_minutes() {
        let mut h = History::default();
        for i in 0..2000 {
            h.push(Sample {
                at: i as f64 * 0.5,
                all: Rate {
                    down: i as f64,
                    up: 0.0,
                },
                lan: None,
            });
        }
        let first = h.samples.front().unwrap().at;
        let last = h.latest().unwrap().at;
        assert!(last - first <= HISTORY_MAX_S + 5.0);
        assert_eq!(h.peak(Lane::All, 10.0), 1999.0);
        // The last 10 s, plus the one before so the line meets the edge.
        assert_eq!(h.recent(10.0).count(), 22);
        assert_eq!(h.nearest(last - 3.1).unwrap().at, last - 3.0);
    }
}
