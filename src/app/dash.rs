//! The dashboard: the expanded window's tiles, laid out in a grid that
//! follows the window's width. The network tiles are the meter's own
//! graphs; CPU, memory, disks, GPU, temperatures and USB are drawn here.

use super::NetMeterApp;
use crate::glass::{self, Tokens};
use crate::metrics::{self, Panel};
use crate::rates::{self, Format, Lane, Prefix, Sample, Units};
use egui::{Align2, Color32, CornerRadius, Painter, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};

/// The narrowest a column of tiles gets before the grid drops one.
const TILE_W: f32 = 340.0;
const GAP: f32 = 10.0;

/// A graph's series: how high it is in a sample (0 to 1), its colour, and
/// how strong its fill is (0 for a line alone).
struct Series<'a> {
    level: &'a dyn Fn(&Sample) -> Option<f32>,
    colour: Color32,
    fill: f32,
}

impl NetMeterApp {
    /// The tiles there's something to show for on this computer.
    fn panels(&self) -> Vec<Panel> {
        let lan = self.sampler.lan_known;
        let latest = self.history.latest().map(|s| s.sys).unwrap_or_default();
        self.s
            .panels
            .iter()
            .copied()
            .filter(|p| match p {
                Panel::Internet | Panel::Network => true,
                Panel::Lan => lan,
                Panel::Cpu => latest.cpu.is_some() || !self.snap.cores.is_empty(),
                Panel::Memory => latest.mem.is_some(),
                Panel::Disks => latest.disk.is_some() || !self.snap.volumes.is_empty(),
                Panel::Gpu => self.snap.gpu.is_some(),
                Panel::Temperatures => {
                    !self.snap.sensors.is_empty() || self.snap.sensors_note.is_some()
                }
                Panel::Usb => self.snap.usb.is_some(),
            })
            .collect()
    }

    pub(super) fn dashboard(&self, ui: &Ui, rect: Rect, t: &Tokens) {
        let p = ui.painter().with_clip_rect(rect);
        let inner = rect.shrink(14.0);
        if self.history.latest().is_none() {
            p.text(
                inner.center(),
                Align2::CENTER_CENTER,
                "Measuring…",
                glass::regular(13.0),
                t.ink2,
            );
            return;
        }
        let panels = self.panels();
        if panels.is_empty() {
            p.text(
                inner.center(),
                Align2::CENTER_CENTER,
                "No tiles chosen: pick some under Settings, Dashboard.",
                glass::regular(13.0),
                t.ink2,
            );
            return;
        }
        let n = panels.len();
        let cols = ((inner.width() / TILE_W).floor() as usize)
            .clamp(1, 4)
            .min(n);
        let rows = n.div_ceil(cols);
        let w = (inner.width() - GAP * (cols as f32 - 1.0)) / cols as f32;
        let h = (inner.height() - GAP * (rows as f32 - 1.0)) / rows as f32;
        let hover = ui.input(|i| i.pointer.hover_pos());
        for (i, panel) in panels.into_iter().enumerate() {
            let (c, r) = (i % cols, i / cols);
            // The last row's tiles share its width.
            let in_row = if r == rows - 1 { n - r * cols } else { cols };
            let w = if in_row < cols {
                (inner.width() - GAP * (in_row as f32 - 1.0)) / in_row as f32
            } else {
                w
            };
            let tile = Rect::from_min_size(
                Pos2::new(
                    inner.left() + c as f32 * (w + GAP),
                    inner.top() + r as f32 * (h + GAP),
                ),
                Vec2::new(w, h),
            );
            p.rect_filled(tile, CornerRadius::same(10), t.ink.gamma_multiply(0.035));
            p.rect_stroke(
                tile,
                CornerRadius::same(10),
                Stroke::new(1.0, t.grid),
                StrokeKind::Inside,
            );
            let body = tile.shrink2(Vec2::new(12.0, 10.0));
            let p = p.with_clip_rect(tile);
            let hover_x = hover.filter(|q| body.contains(*q)).map(|q| q.x);
            match panel {
                Panel::Internet => {
                    let lane = if self.sampler.lan_known {
                        Lane::Internet
                    } else {
                        Lane::All
                    };
                    self.net_tile(&p, body, lane, t, hover_x);
                }
                Panel::Lan => self.net_tile(&p, body, Lane::Lan, t, hover_x),
                Panel::Network => self.net_tile(&p, body, Lane::All, t, hover_x),
                Panel::Cpu => self.cpu(&p, body, t),
                Panel::Memory => self.memory(&p, body, t),
                Panel::Disks => self.disks(&p, body, t),
                Panel::Gpu => self.gpu(&p, body, t),
                Panel::Temperatures => self.temperatures(&p, body, t),
                Panel::Usb => self.usb(&p, body, t),
            }
        }
    }

    fn net_tile(&self, p: &Painter, r: Rect, lane: Lane, t: &Tokens, hover_x: Option<f32>) {
        if let Some(e) = &self.error {
            head(p, r, lane.name(), None, t);
            p.text(
                r.center(),
                Align2::CENTER_CENTER,
                format!("Can't read the network: {e}"),
                glass::regular(12.5),
                t.ink2,
            );
            return;
        }
        self.lane(p, r, lane, t, hover_x);
    }

    /// The figure size for a tile: as the meter's, from its height.
    fn fig(r: Rect) -> f32 {
        (r.height() * 0.2).clamp(14.0, 24.0)
    }

    fn cpu(&self, p: &Painter, r: Rect, t: &Tokens) {
        let latest = self.history.latest().map(|s| s.sys).unwrap_or_default();
        let body = head(p, r, "CPU", latest.cpu.map(percent), t);
        let mut sub = Vec::new();
        if let Some(n) = &self.snap.cpu_name {
            sub.push(n.clone());
        }
        if !self.snap.cores.is_empty() {
            sub.push(format!("{} threads", self.snap.cores.len()));
        }
        if let Some(c) = latest.cpu_temp {
            sub.push(metrics::temperature(c, self.s.fahrenheit));
        }
        let body = subline(p, body, &sub.join(" · "), t);
        // Each core as a bar along the bottom, when there's room.
        let cores = &self.snap.cores;
        let bars_h = if !cores.is_empty() && body.height() > 90.0 {
            (body.height() * 0.22).clamp(16.0, 36.0)
        } else {
            0.0
        };
        let g = Rect::from_min_max(body.min, Pos2::new(body.right(), body.bottom() - bars_h));
        self.graph(
            p,
            g,
            t,
            "100%",
            &[Series {
                level: &|s| s.sys.cpu,
                colour: t.down,
                fill: 0.5,
            }],
        );
        if bars_h > 0.0 {
            let area = Rect::from_min_max(Pos2::new(body.left(), g.bottom() + 6.0), body.max);
            let n = cores.len() as f32;
            let gap = if area.width() / n > 6.0 { 2.0 } else { 1.0 };
            let bw = (area.width() - gap * (n - 1.0)) / n;
            for (i, &c) in cores.iter().enumerate() {
                let x = area.left() + i as f32 * (bw + gap);
                let cell =
                    Rect::from_min_size(Pos2::new(x, area.top()), Vec2::new(bw, area.height()));
                p.rect_filled(cell, CornerRadius::same(2), t.ink.gamma_multiply(0.06));
                let lit =
                    Rect::from_min_max(Pos2::new(x, area.bottom() - c * area.height()), cell.max);
                p.rect_filled(lit, CornerRadius::same(2), t.down.gamma_multiply(0.85));
            }
        }
    }

    fn memory(&self, p: &Painter, r: Rect, t: &Tokens) {
        let latest = self.history.latest().map(|s| s.sys).unwrap_or_default();
        let frac = |v: Option<(u64, u64)>| v.filter(|v| v.1 > 0).map(|(u, n)| u as f32 / n as f32);
        let body = head(p, r, "Memory", frac(latest.mem).map(percent), t);
        let bin = self.s.binary;
        let mut sub = Vec::new();
        if let Some((u, n)) = latest.mem {
            sub.push(format!(
                "{} of {}",
                metrics::size(u, bin),
                metrics::size(n, bin)
            ));
        }
        if let Some((u, n)) = latest.swap {
            sub.push(format!(
                "swap {} of {}",
                metrics::size(u, bin),
                metrics::size(n, bin)
            ));
        }
        let body = subline(p, body, &sub.join(" · "), t);
        let mut series = vec![Series {
            level: &|s| {
                s.sys
                    .mem
                    .filter(|v| v.1 > 0)
                    .map(|(u, n)| u as f32 / n as f32)
            },
            colour: t.down,
            fill: 0.5,
        }];
        if latest.swap.is_some() {
            series.push(Series {
                level: &|s| {
                    s.sys
                        .swap
                        .filter(|v| v.1 > 0)
                        .map(|(u, n)| u as f32 / n as f32)
                },
                colour: t.up,
                fill: 0.0,
            });
        }
        self.graph(p, body, t, "100%", &series);
    }

    fn disks(&self, p: &Painter, r: Rect, t: &Tokens) {
        let latest = self.history.latest().map(|s| s.sys).unwrap_or_default();
        let fmt = Format {
            units: Units::Bytes,
            prefix: Prefix::Auto,
            binary: self.s.binary,
        };
        let body = head(p, r, "Disks", None, t);
        // Read and write as the network's down and up.
        if let Some(d) = latest.disk {
            figures(
                p,
                Pos2::new(r.right(), r.top() + head_h(r) / 2.0 - 2.0),
                Self::fig(r),
                &[
                    (fmt.rate(d.down), false, t.down),
                    (fmt.rate(d.up), true, t.up),
                ],
                t,
            );
        }
        let vols = &self.snap.volumes;
        let row_h = 24.0;
        let list_h = if body.height() > 80.0 {
            let rows = ((body.height() * 0.5) / row_h)
                .floor()
                .min(vols.len() as f32);
            rows * row_h
        } else {
            0.0
        };
        let g = Rect::from_min_max(
            body.min,
            Pos2::new(
                body.right(),
                body.bottom() - list_h - if list_h > 0.0 { 8.0 } else { 0.0 },
            ),
        );
        let span = self.s.span();
        let top = fmt.nice_ceiling(
            self.history
                .recent(span)
                .filter_map(|s| s.sys.disk)
                .map(|d| d.max())
                .fold(0.0, f64::max),
        );
        let label = fmt.mark(top);
        self.graph(
            p,
            g,
            t,
            &label,
            &[
                Series {
                    level: &|s| s.sys.disk.map(|d| rates::level(d.down, top, false)),
                    colour: t.down,
                    fill: 0.5,
                },
                Series {
                    level: &|s| s.sys.disk.map(|d| rates::level(d.up, top, false)),
                    colour: t.up,
                    fill: 0.0,
                },
            ],
        );
        if list_h > 0.0 {
            let mut y = body.bottom() - list_h;
            for v in vols {
                if y + row_h > body.bottom() + 0.5 {
                    break;
                }
                let full = v.used as f32 / v.total.max(1) as f32;
                let row =
                    Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), row_h));
                meter_row(
                    p,
                    row,
                    &v.name,
                    &format!(
                        "{} of {}",
                        metrics::size(v.used, self.s.binary),
                        metrics::size(v.total, self.s.binary)
                    ),
                    full,
                    t.heat(((full - 0.6) / 0.35).max(0.0)),
                    t,
                );
                y += row_h;
            }
        }
    }

    fn gpu(&self, p: &Painter, r: Rect, t: &Tokens) {
        let Some(g) = &self.snap.gpu else { return };
        let latest = self.history.latest().map(|s| s.sys).unwrap_or_default();
        let body = head(p, r, "GPU", latest.gpu.map(percent), t);
        let mut sub = vec![g.name.clone()];
        if let Some(c) = latest.gpu_temp {
            sub.push(metrics::temperature(c, self.s.fahrenheit));
        }
        if let Some((u, n)) = g.vram {
            sub.push(format!(
                "{} of {} video memory",
                metrics::size(u, self.s.binary),
                metrics::size(n, self.s.binary)
            ));
        }
        if let Some(w) = g.power {
            sub.push(format!("{w:.0} W"));
        }
        let body = subline(p, body, &sub.join(" · "), t);
        if latest.gpu.is_none()
            && self
                .history
                .recent(self.s.span())
                .all(|s| s.sys.gpu.is_none())
        {
            p.text(
                body.center(),
                Align2::CENTER_CENTER,
                "This GPU doesn't report how busy it is.",
                glass::regular(12.5),
                t.ink2,
            );
            return;
        }
        self.graph(
            p,
            body,
            t,
            "100%",
            &[Series {
                level: &|s| s.sys.gpu,
                colour: t.down,
                fill: 0.5,
            }],
        );
    }

    fn temperatures(&self, p: &Painter, r: Rect, t: &Tokens) {
        let latest = self.history.latest().map(|s| s.sys).unwrap_or_default();
        let f = self.s.fahrenheit;
        let hottest = self.snap.sensors.iter().map(|s| s.celsius).reduce(f32::max);
        let body = head(
            p,
            r,
            "Temperatures",
            latest
                .cpu_temp
                .or(hottest)
                .map(|c| metrics::temperature(c, f)),
            t,
        );
        let sensors = &self.snap.sensors;
        if sensors.is_empty() {
            let note = self
                .snap
                .sensors_note
                .as_deref()
                .unwrap_or("No temperature sensors found.");
            let g = p.layout(
                note.to_string(),
                glass::regular(12.5),
                t.ink2,
                body.width().max(40.0),
            );
            p.galley(body.left_top() + Vec2::new(0.0, 4.0), g, t.ink2);
            return;
        }
        // The CPU and GPU over time, 20 °C to 100 °C, then every sensor.
        let has_history = latest.cpu_temp.is_some() || latest.gpu_temp.is_some();
        let row_h = 17.0;
        let cols = if body.width() > 420.0 { 2 } else { 1 };
        let rows_fit =
            ((body.height() * if has_history { 0.55 } else { 1.0 }) / row_h).floor() as usize;
        let items: Vec<(String, String, f32)> = sensors
            .iter()
            .map(|s| {
                (
                    s.label.clone(),
                    metrics::temperature(s.celsius, f),
                    metrics::heat(s.celsius),
                )
            })
            .chain(
                self.snap
                    .fans
                    .iter()
                    .map(|fan| (fan.label.clone(), format!("{} rpm", fan.rpm), -1.0)),
            )
            .collect();
        let shown = items.len().min(rows_fit * cols);
        let rows = shown.div_ceil(cols);
        let list_h = rows as f32 * row_h;
        if has_history {
            let g = Rect::from_min_max(
                body.min,
                Pos2::new(body.right(), body.bottom() - list_h - 8.0),
            );
            let lvl = |c: Option<f32>| c.map(|c| ((c - 20.0) / 80.0).clamp(0.0, 1.0));
            self.graph(
                p,
                g,
                t,
                &metrics::temperature(100.0, f),
                &[
                    Series {
                        level: &move |s| lvl(s.sys.cpu_temp),
                        colour: t.down,
                        fill: 0.35,
                    },
                    Series {
                        level: &move |s| lvl(s.sys.gpu_temp),
                        colour: t.up,
                        fill: 0.0,
                    },
                ],
            );
        }
        let col_w = (body.width() - 16.0 * (cols as f32 - 1.0)) / cols as f32;
        for (i, (label, value, heat)) in items.iter().take(shown).enumerate() {
            let (c, r) = (i / rows.max(1), i % rows.max(1));
            let row = Rect::from_min_size(
                Pos2::new(
                    body.left() + c as f32 * (col_w + 16.0),
                    body.bottom() - list_h + r as f32 * row_h,
                ),
                Vec2::new(col_w, row_h),
            );
            let colour = if *heat < 0.0 { t.ink2 } else { t.heat(*heat) };
            p.circle_filled(Pos2::new(row.left() + 3.5, row.center().y), 3.5, colour);
            let vr = p.text(
                Pos2::new(row.right(), row.center().y),
                Align2::RIGHT_CENTER,
                value,
                glass::regular(12.5),
                t.ink,
            );
            fit_text(
                p,
                Pos2::new(row.left() + 12.0, row.center().y),
                label,
                vr.left() - row.left() - 20.0,
                t.ink2,
            );
        }
    }

    fn usb(&self, p: &Painter, r: Rect, t: &Tokens) {
        let devices = self.snap.usb.as_deref().unwrap_or(&[]);
        let body = head(p, r, "USB", Some(devices.len().to_string()), t);
        let row_h = 18.0;
        let mut y = body.top();
        if devices.is_empty() {
            p.text(
                Pos2::new(body.left(), y + row_h / 2.0),
                Align2::LEFT_CENTER,
                "Nothing plugged in.",
                glass::regular(12.5),
                t.ink2,
            );
            y += row_h;
        }
        // The events take the bottom: the last few plugged in and out.
        let events: Vec<_> = self.usb_events.iter().rev().take(4).collect();
        let events_h = if events.is_empty() {
            0.0
        } else {
            8.0 + row_h * events.len() as f32
        };
        for d in devices {
            if y + row_h > body.bottom() - events_h + 0.5 {
                break;
            }
            let row =
                Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), row_h));
            let speed = d.speed_label();
            let right = match &speed {
                Some(sp) => {
                    // A USB 2 link for something that could go faster is
                    // worth a look; say it in the warm colour.
                    let slow = d.speed_mbps.is_some_and(|s| s <= 480);
                    p.text(
                        Pos2::new(row.right(), row.center().y),
                        Align2::RIGHT_CENTER,
                        sp,
                        glass::regular(12.0),
                        if slow { t.ink2 } else { t.down },
                    )
                    .left()
                }
                None => row.right(),
            };
            fit_text(
                p,
                Pos2::new(row.left(), row.center().y),
                &d.name,
                right - row.left() - 12.0,
                t.ink,
            );
            y += row_h;
        }
        if !events.is_empty() {
            let mut y = body.bottom() - events_h + 8.0;
            p.line_segment(
                [
                    Pos2::new(body.left(), y - 4.0),
                    Pos2::new(body.right(), y - 4.0),
                ],
                Stroke::new(1.0, t.grid),
            );
            for e in events {
                let ago = ago(self.now - e.at);
                let row =
                    Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), row_h));
                let ar = p.text(
                    Pos2::new(row.right(), row.center().y),
                    Align2::RIGHT_CENTER,
                    ago,
                    glass::regular(11.5),
                    t.ink2,
                );
                fit_text(
                    p,
                    Pos2::new(row.left(), row.center().y),
                    &e.text,
                    ar.left() - row.left() - 12.0,
                    t.ink2,
                );
                y += row_h;
            }
        }
    }

    /// A graph over the history's span, in the tile's area, with its top's
    /// label.
    fn graph(&self, p: &Painter, g: Rect, t: &Tokens, top: &str, series: &[Series]) {
        if g.height() < 12.0 || g.width() < 20.0 {
            return;
        }
        let span = self.s.span();
        let Some(newest) = self.history.latest().map(|s| s.at) else {
            return;
        };
        let x_of = |at: f64| g.right() - ((newest - at) / span) as f32 * g.width();
        p.line_segment(
            [
                Pos2::new(g.left(), g.bottom() - 0.5),
                Pos2::new(g.right(), g.bottom() - 0.5),
            ],
            Stroke::new(1.0, t.grid),
        );
        if g.height() > 30.0 {
            p.text(
                Pos2::new(g.left(), g.top() + 1.0),
                Align2::LEFT_TOP,
                top,
                glass::regular(10.5),
                t.ink2,
            );
        }
        for s in series {
            let pts: Vec<Pos2> = self
                .history
                .recent(span)
                .filter_map(|x| {
                    (s.level)(x)
                        .map(|l| Pos2::new(x_of(x.at), g.bottom() - l.clamp(0.0, 1.0) * g.height()))
                })
                .collect();
            glass::trace(
                p,
                &pts,
                g,
                g.bottom(),
                s.colour,
                s.fill,
                if s.fill > 0.0 { 1.6 } else { 1.2 },
            );
        }
    }

    /// A passing message along the bottom of the meter (USB devices).
    pub(super) fn show_notice(&self, ui: &Ui, screen: Rect, t: &Tokens) {
        let Some((msg, at)) = &self.notice else {
            return;
        };
        let age = self.now - at;
        if age > 6.0 {
            return;
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
        let p = ui.painter();
        let font = glass::regular(12.0);
        let g = p.layout_no_wrap(msg.clone(), font, t.ink);
        let w = (g.size().x + 24.0).min(screen.width() - 20.0);
        let r = Rect::from_min_size(
            Pos2::new(screen.left() + 10.0, screen.bottom() - 10.0 - 26.0),
            Vec2::new(w, 26.0),
        );
        glass::strip(p, r, t);
        fit_text(
            &p.with_clip_rect(r),
            Pos2::new(r.left() + 12.0, r.center().y),
            msg,
            r.width() - 24.0,
            t.ink,
        );
    }
}

fn percent(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

fn ago(secs: f64) -> String {
    if secs < 60.0 {
        "just now".into()
    } else if secs < 3600.0 {
        format!("{:.0} min ago", secs / 60.0)
    } else {
        format!("{:.0} h ago", secs / 3600.0)
    }
}

fn head_h(r: Rect) -> f32 {
    NetMeterApp::fig(r) + 8.0
}

/// A tile's heading: its name in spaced capitals and, on the right, its
/// figure in light type. Returns the room left under it.
fn head(p: &Painter, r: Rect, name: &str, figure: Option<String>, t: &Tokens) -> Rect {
    let fig = NetMeterApp::fig(r);
    let h = head_h(r);
    let y = r.top() + h / 2.0 - 2.0;
    let mut room = r.width();
    if let Some(f) = figure {
        let fr = p.text(
            Pos2::new(r.right(), y),
            Align2::RIGHT_CENTER,
            f,
            glass::light(fig),
            t.ink,
        );
        room = fr.left() - r.left() - 12.0;
    }
    glass::caps(
        p,
        Pos2::new(r.left(), y),
        name,
        (fig * 0.48).clamp(9.5, 11.0),
        t.ink2,
        room.max(10.0),
    );
    Rect::from_min_max(Pos2::new(r.left(), r.top() + h), r.max)
}

/// A line of detail under the heading; returns the room left under it.
fn subline(p: &Painter, r: Rect, s: &str, t: &Tokens) -> Rect {
    if s.is_empty() || r.height() < 60.0 {
        return r;
    }
    fit_text(p, Pos2::new(r.left(), r.top() + 7.0), s, r.width(), t.ink2);
    Rect::from_min_max(Pos2::new(r.left(), r.top() + 20.0), r.max)
}

/// ▼ read ▲ write, or download and upload, right-aligned at `at`.
fn figures(p: &Painter, at: Pos2, fig: f32, items: &[(String, bool, Color32)], t: &Tokens) {
    let mut x = at.x;
    for (text, up, c) in items.iter().rev() {
        let tr = p.text(
            Pos2::new(x, at.y),
            Align2::RIGHT_CENTER,
            text,
            glass::light(fig),
            t.ink,
        );
        let a = fig * 0.5;
        glass::arrow(
            p,
            Pos2::new(tr.left() - a * 0.5 - 5.0, at.y + 1.0),
            *up,
            a,
            *c,
        );
        x = tr.left() - a - fig * 0.7;
    }
}

/// A name, a figure and a bar under them: a drive and how full it is.
fn meter_row(p: &Painter, r: Rect, name: &str, value: &str, full: f32, c: Color32, t: &Tokens) {
    let vr = p.text(
        Pos2::new(r.right(), r.center().y - 3.0),
        Align2::RIGHT_CENTER,
        value,
        glass::regular(11.5),
        t.ink2,
    );
    fit_text(
        p,
        Pos2::new(r.left(), r.center().y - 3.0),
        name,
        vr.left() - r.left() - 10.0,
        t.ink,
    );
    let bar = Rect::from_min_size(
        Pos2::new(r.left(), r.bottom() - 4.0),
        Vec2::new(r.width(), 2.5),
    );
    p.rect_filled(bar, CornerRadius::same(1), t.ink.gamma_multiply(0.08));
    p.rect_filled(
        Rect::from_min_size(
            bar.min,
            Vec2::new(bar.width() * full.clamp(0.0, 1.0), bar.height()),
        ),
        CornerRadius::same(1),
        c,
    );
}

/// Plain text cut to `max_w` with "…".
fn fit_text(p: &Painter, pos: Pos2, s: &str, max_w: f32, c: Color32) {
    if max_w < 8.0 {
        return;
    }
    let mut job = egui::text::LayoutJob::single_section(
        s.to_string(),
        egui::TextFormat {
            font_id: glass::regular(12.5),
            color: c,
            ..Default::default()
        },
    );
    job.wrap.max_width = max_w;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let g = p.layout_job(job);
    let r = Align2::LEFT_CENTER.anchor_size(pos, g.size());
    p.galley(r.min, g, c);
}
