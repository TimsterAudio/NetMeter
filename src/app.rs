//! The Net Meter (NM-001): a small window of scrolling graphs, download and
//! upload, for the Internet and the local network, to keep on the desktop.
//!
//! The look is Glass (see `glass.rs`): a see-through rounded pane with no
//! faceplate. The controls (settings, stay on top, minimise, close) sit in
//! a pill at the bottom right that shows only while the mouse is over the
//! window. Drag anywhere to move it; right-click for the everyday choices.
//! Settings open in a window of their own.

mod dash;

use crate::autostart;
use crate::glass::{self, Build, Icon, Tokens};
use crate::metrics::{Panel, Snapshot, UsbDevice};
use crate::net;
use crate::rates::{self, Format, History, Lane, Prefix, Rate, Sample, Show, Units};
use crate::sampler::Sampler;
use egui::{
    Align2, Id, Key, Pos2, Rect, Sense, Stroke, Ui, Vec2, ViewportBuilder, ViewportClass,
    ViewportCommand, ViewportId, WindowLevel,
};
use serde::{Deserialize, Serialize};
use taw_panel::hifi;

const STORAGE_KEY: &str = "taw-netmeter";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_SIZE: Vec2 = Vec2::new(420.0, 180.0);
/// The dashboard's size the first time it opens.
pub const EXPANDED_SIZE: Vec2 = Vec2::new(860.0, 560.0);
pub const MIN_SIZE: Vec2 = Vec2::new(200.0, 90.0);
/// The settings window's size when it opens.
const SETTINGS_SIZE: Vec2 = Vec2::new(380.0, 600.0);
/// The room between the meter and the settings beside it.
const SETTINGS_GAP: f32 = 12.0;
const HISTORIES: [u32; 4] = [60, 120, 300, 600];
const INTERVALS: [u32; 3] = [500, 1000, 2000];

/// Everything remembered between runs.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    /// The hi-fi look 0.1.0 wore. Kept so its settings still load.
    finish: hifi::Look,
    show: Show,
    /// Download up from the middle, upload down from it, rather than both
    /// up from the bottom.
    mirrored: bool,
    /// How many seconds the graph spans.
    history_s: u32,
    /// How often the counters are read, in milliseconds.
    interval_ms: u32,
    units: Units,
    prefix: Prefix,
    /// KiB/s and MiB/s rather than kB/s and MB/s.
    binary: bool,
    /// The top of the scale in bytes a second; 0 to follow the traffic.
    scale: f64,
    log_scale: bool,
    /// One adaptor's id, or every real adaptor.
    adaptor: Option<String>,
    /// The window's controls (a title strip in 0.1.0, the pill under the
    /// mouse since): off leaves only the graphs.
    controls: bool,
    on_top: bool,
    /// Use the system's title bar instead of the meter's own controls.
    system_title_bar: bool,
    /// Frosted or smoked, how see-through, and the traces' colours.
    glass: glass::Look,
    /// The dashboard (the expanded window) rather than the meter.
    expanded: bool,
    /// The window's size in each mode, to go back to.
    compact_size: Option<[f32; 2]>,
    expanded_size: Option<[f32; 2]>,
    /// The dashboard's tiles, in order.
    panels: Vec<Panel>,
    fahrenheit: bool,
    /// Say when a USB device is plugged in or out.
    usb_notices: bool,
}

impl Default for Saved {
    fn default() -> Self {
        Saved {
            finish: hifi::Look::default(),
            show: Show::Both,
            mirrored: false,
            history_s: 120,
            interval_ms: 1000,
            units: Units::Bits,
            prefix: Prefix::Auto,
            binary: false,
            scale: 0.0,
            log_scale: false,
            adaptor: None,
            controls: true,
            on_top: true,
            system_title_bar: false,
            glass: glass::Look::default(),
            expanded: false,
            compact_size: None,
            expanded_size: None,
            panels: Panel::DEFAULT.to_vec(),
            fahrenheit: false,
            usb_notices: true,
        }
    }
}

impl Saved {
    fn format(&self) -> Format {
        Format {
            units: self.units,
            prefix: self.prefix,
            binary: self.binary,
        }
    }

    fn span(&self) -> f64 {
        self.history_s.clamp(10, rates::HISTORY_MAX_S as u32) as f64
    }
}

pub struct NetMeterApp {
    s: Saved,
    sampler: Sampler,
    history: History,
    error: Option<String>,
    settings_open: bool,
    /// Where the settings window opens: beside the meter, worked out when
    /// it's opened (`None` where the system won't say where windows are).
    settings_at: Option<Pos2>,
    /// Whether the meter starts when the user logs in, as the system has it.
    start_at_login: bool,
    login_error: Option<String>,
    /// While a custom scale is being set: the unit it's typed in (1 kilo,
    /// 2 mega, 3 giga).
    custom_power: Option<i32>,
    /// The system as it is now.
    snap: Snapshot,
    /// The USB devices at the last reading, to tell what was plugged in.
    usb_seen: Option<Vec<UsbDevice>>,
    /// USB devices plugged in and out, newest last.
    usb_events: Vec<UsbEvent>,
    /// A passing message (USB), and when it came (egui's clock).
    notice: Option<(String, f64)>,
    /// egui's clock this frame.
    now: f64,
    /// The adaptors, for the menu: (id, label).
    adaptors: Vec<(String, String)>,
}

impl NetMeterApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        glass::install_fonts(&cc.egui_ctx);
        let s: Saved = cc
            .storage
            .and_then(|st| eframe::get_value(st, STORAGE_KEY))
            .unwrap_or_default();
        if s.on_top {
            cc.egui_ctx
                .send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
        }
        if s.system_title_bar {
            cc.egui_ctx
                .send_viewport_cmd(ViewportCommand::Decorations(true));
        }
        // Keep the login entry pointing at this copy, wherever it was run
        // from last.
        let start_at_login = autostart::is_enabled();
        if start_at_login {
            let _ = autostart::set(true);
        }
        let sampler = Sampler::start(s.interval_ms, s.adaptor.clone(), cc.egui_ctx.clone());
        NetMeterApp {
            s,
            sampler,
            history: History::default(),
            error: None,
            settings_open: false,
            settings_at: None,
            start_at_login,
            login_error: None,
            custom_power: None,
            snap: Snapshot::default(),
            usb_seen: None,
            usb_events: Vec::new(),
            notice: None,
            now: 0.0,
            adaptors: Vec::new(),
        }
    }

    fn refresh_adaptors(&mut self) {
        let mut v: Vec<(String, String)> = net::adaptors()
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.id != "lo")
            .map(|a| {
                let label = if a.physical {
                    a.label
                } else {
                    format!("{} (virtual)", a.label)
                };
                (a.id, label)
            })
            .collect();
        v.sort_by_key(|(_, l)| l.ends_with("(virtual)"));
        self.adaptors = v;
    }

    fn set_on_top(&mut self, ctx: &egui::Context, on: bool) {
        self.s.on_top = on;
        ctx.send_viewport_cmd_to(
            ViewportId::ROOT,
            ViewportCommand::WindowLevel(if on {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            }),
        );
    }

    fn take_samples(&mut self) {
        let now = self.now;
        while let Ok(r) = self.sampler.rx.try_recv() {
            self.history.push(r.sample);
            self.error = r.net_error;
            if let (Some(was), Some(is)) = (&self.usb_seen, &r.snap.usb) {
                for (dev, connected) in usb_changes(was, is) {
                    let what = match (&dev.speed_label(), connected) {
                        (Some(sp), true) => format!("{} · {sp}", dev.name),
                        _ => dev.name.clone(),
                    };
                    let msg = if connected {
                        format!("Plugged in: {what}")
                    } else {
                        format!("Unplugged: {what}")
                    };
                    if self.s.usb_notices {
                        self.notice = Some((msg.clone(), now));
                    }
                    self.usb_events.push(UsbEvent { at: now, text: msg });
                }
                let n = self.usb_events.len();
                if n > 20 {
                    self.usb_events.drain(..n - 20);
                }
            }
            if r.snap.usb.is_some() {
                self.usb_seen = r.snap.usb.clone();
            }
            self.snap = r.snap;
        }
    }

    fn open_settings(&mut self, ctx: &egui::Context) {
        self.settings_open = !self.settings_open;
        if self.settings_open {
            self.refresh_adaptors();
            self.start_at_login = autostart::is_enabled();
            let (meter, monitor) =
                ctx.input(|i| (i.viewport().outer_rect, i.viewport().monitor_size));
            self.settings_at = meter.map(|m| beside(m, SETTINGS_SIZE, monitor));
        }
    }

    // ------------------------------------------------------------ display

    fn display(&self, ui: &Ui, rect: Rect, t: &Tokens) {
        let p = ui.painter().with_clip_rect(rect);
        let pad = Vec2::new(
            (rect.width() * 0.035).clamp(8.0, 16.0),
            (rect.height() * 0.06).clamp(6.0, 14.0),
        );
        let inner = rect.shrink2(pad);
        if let Some(e) = &self.error {
            p.text(
                inner.center(),
                Align2::CENTER_CENTER,
                format!("Can't read the network: {e}"),
                glass::regular(13.0),
                t.ink2,
            );
            return;
        }
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
        let lanes = self.s.show.lanes(self.sampler.lan_known);
        let gap = (inner.height() * 0.06).clamp(6.0, 12.0);
        let h = (inner.height() - gap * (lanes.len() as f32 - 1.0)) / lanes.len() as f32;
        let hover_x = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|q| inner.contains(*q))
            .map(|q| q.x);
        for (i, lane) in lanes.into_iter().enumerate() {
            let r = Rect::from_min_size(
                Pos2::new(inner.left(), inner.top() + i as f32 * (h + gap)),
                Vec2::new(inner.width(), h),
            );
            self.lane(&p, r, lane, t, hover_x);
        }
    }

    fn lane(&self, p: &egui::Painter, r: Rect, lane: Lane, t: &Tokens, hover_x: Option<f32>) {
        let fmt = self.s.format();
        let span = self.s.span();
        // The figures grow with the window: 22 pt at the review's size.
        let fig = (r.height() * 0.2).clamp(12.0, 24.0);
        let head_h = fig + 6.0;
        let g = Rect::from_min_max(Pos2::new(r.left(), r.top() + head_h), r.max);
        let Some(newest) = self.history.latest().map(|s| s.at) else {
            return;
        };
        let x_of = |at: f64| g.right() - ((newest - at) / span) as f32 * g.width();

        let top = if self.s.scale > 0.0 {
            self.s.scale
        } else {
            fmt.nice_ceiling(self.history.peak(lane, span))
        };
        let log = self.s.log_scale;
        let mid_y = g.center().y;

        // The baseline (the middle, when mirrored), and the scale's top.
        let base = if self.s.mirrored {
            mid_y
        } else {
            g.bottom() - 0.5
        };
        p.line_segment(
            [Pos2::new(g.left(), base), Pos2::new(g.right(), base)],
            Stroke::new(1.0, t.grid),
        );
        if g.height() > 26.0 {
            p.text(
                Pos2::new(g.left(), g.top() + 1.0),
                Align2::LEFT_TOP,
                fmt.mark(top),
                glass::regular((fig * 0.5).clamp(9.5, 12.0)),
                t.ink2,
            );
        }

        let mut down = Vec::new();
        let mut up = Vec::new();
        for s in self.history.recent(span) {
            let Some(rate) = s.lane(lane) else { continue };
            let x = x_of(s.at);
            let (ld, lu) = (
                rates::level(rate.down, top, log),
                rates::level(rate.up, top, log),
            );
            if self.s.mirrored {
                down.push(Pos2::new(x, mid_y - ld * g.height() / 2.0));
                up.push(Pos2::new(x, mid_y + lu * g.height() / 2.0));
            } else {
                down.push(Pos2::new(x, g.bottom() - ld * g.height()));
                up.push(Pos2::new(x, g.bottom() - lu * g.height()));
            }
        }
        let plot = if self.s.mirrored {
            Rect::from_min_max(g.min, Pos2::new(g.right(), mid_y))
        } else {
            g
        };
        glass::trace(p, &down, plot, base, t.down, 0.5, 1.6);
        glass::trace(p, &up, plot, base, t.up, 0.0, 1.2);

        // The pointer's moment, or now.
        let mut when = None;
        let mut sample: &Sample = self.history.latest().unwrap();
        if let Some(x) = hover_x {
            let at = newest - ((g.right() - x) / g.width()) as f64 * span;
            if let Some(s) = self.history.nearest(at) {
                let sx = x_of(s.at);
                p.line_segment(
                    [Pos2::new(sx, g.top()), Pos2::new(sx, g.bottom())],
                    Stroke::new(1.0, t.ink2.gamma_multiply(0.6)),
                );
                let ago = (newest - s.at).round();
                when = Some(if ago < 1.0 {
                    "now".to_string()
                } else if ago < 120.0 {
                    format!("{ago:.0} s ago")
                } else {
                    format!("{:.0} min ago", ago / 60.0)
                });
                sample = s;
            }
        }
        let rate = sample.lane(lane).unwrap_or_default();
        self.heading(p, r, head_h, fig, lane, rate, when.as_deref(), t);
    }

    /// The lane's name in spaced capitals, and ▼ download ▲ upload in light
    /// figures on the right.
    #[allow(clippy::too_many_arguments)]
    fn heading(
        &self,
        p: &egui::Painter,
        r: Rect,
        head_h: f32,
        fig: f32,
        lane: Lane,
        rate: Rate,
        when: Option<&str>,
        t: &Tokens,
    ) {
        let fmt = self.s.format();
        let y = r.top() + head_h / 2.0 - 2.0;
        let mut x = r.right();
        for (v, up, c) in [(rate.up, true, t.up), (rate.down, false, t.down)] {
            let tr = p.text(
                Pos2::new(x, y),
                Align2::RIGHT_CENTER,
                fmt.rate(v),
                glass::light(fig),
                t.ink,
            );
            let a = fig * 0.55;
            glass::arrow(p, Pos2::new(tr.left() - a * 0.5 - 5.0, y + 1.0), up, a, c);
            x = tr.left() - a - fig * 0.75;
        }
        let name = match when {
            Some(w) => format!("{} · {w}", lane.name()),
            None => lane.name().to_string(),
        };
        let room = x - r.left();
        if room > 30.0 {
            glass::caps(
                p,
                Pos2::new(r.left(), y),
                &name,
                (fig * 0.48).clamp(9.0, 11.0),
                t.ink2,
                room,
            );
        }
    }

    /// The controls' pill, bottom right, while the mouse is over the window.
    fn controls(&mut self, ui: &mut Ui, screen: Rect, t: &Tokens) {
        let ctx = ui.ctx().clone();
        let n = 5.0;
        let (bw, h) = (26.0, 28.0);
        let inset = 10.0f32.min(screen.height() * 0.08);
        let r = Rect::from_min_size(
            Pos2::new(
                screen.right() - inset - bw * n - 12.0,
                screen.bottom() - inset - h,
            ),
            Vec2::new(bw * n + 12.0, h),
        );
        glass::strip(ui.painter(), r, t);
        let icons = [
            (Icon::Settings, "Settings"),
            (
                Icon::OnTop(self.s.on_top),
                if self.s.on_top {
                    "Staying on top (click to stop)"
                } else {
                    "Stay on top"
                },
            ),
            (
                Icon::Expand(self.s.expanded),
                if self.s.expanded {
                    "Back to the meter"
                } else {
                    "Expand into the dashboard"
                },
            ),
            (Icon::Minimise, "Minimise"),
            (Icon::Close, "Close"),
        ];
        for (i, (icon, tip)) in icons.into_iter().enumerate() {
            let b = Rect::from_min_size(
                Pos2::new(r.left() + 6.0 + i as f32 * bw, r.top() + 1.0),
                Vec2::new(bw, h - 2.0),
            );
            if glass::icon_button(ui, b, Id::new(("glass-btn", i)), t, icon, tip).clicked() {
                match icon {
                    Icon::Settings => self.open_settings(&ctx),
                    Icon::OnTop(on) => self.set_on_top(&ctx, !on),
                    Icon::Expand(_) => self.toggle_expanded(&ctx),
                    Icon::Minimise => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
                    Icon::Close => ctx.send_viewport_cmd(ViewportCommand::Close),
                }
            }
        }
    }

    /// Between the meter and the dashboard: each keeps its own size.
    fn toggle_expanded(&mut self, ctx: &egui::Context) {
        let size = ctx
            .input(|i| i.viewport().inner_rect)
            .map(|r| [r.width(), r.height()]);
        if self.s.expanded {
            self.s.expanded_size = size.or(self.s.expanded_size);
        } else {
            self.s.compact_size = size.or(self.s.compact_size);
        }
        self.s.expanded = !self.s.expanded;
        let next = if self.s.expanded {
            self.s.expanded_size.map_or(EXPANDED_SIZE, Vec2::from)
        } else {
            self.s.compact_size.map_or(DEFAULT_SIZE, Vec2::from)
        };
        ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::InnerSize(next));
    }

    /// The everyday choices, on a right-click: short, as the menu can't
    /// leave a small window.
    fn quick_menu(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.sampler.lan_known {
            ui.menu_button("Show", |ui| {
                for show in Show::ALL {
                    if ui.radio(self.s.show == show, show.name()).clicked() {
                        self.s.show = show;
                        ui.close();
                    }
                }
            });
        }
        ui.menu_button("Speeds in", |ui| {
            for (u, name) in [(Units::Bits, "Bits (Mb/s)"), (Units::Bytes, "Bytes (MB/s)")] {
                if ui.radio(self.s.units == u, name).clicked() {
                    self.set_units(u);
                    ui.close();
                }
            }
        });
        ui.menu_button("Glass", |ui| {
            for b in Build::ALL {
                if ui.radio(self.s.glass.build == b, b.name()).clicked() {
                    self.s.glass.build = b;
                    ui.close();
                }
            }
        });
        if ui.checkbox(&mut self.s.mirrored, "Mirrored").clicked() {
            ui.close();
        }
        let mut on_top = self.s.on_top;
        if ui.checkbox(&mut on_top, "Stay on top").clicked() {
            self.set_on_top(&ctx, on_top);
            ui.close();
        }
        let expand = if self.s.expanded {
            "Back to the meter"
        } else {
            "Expand into the dashboard"
        };
        if ui.button(expand).clicked() {
            self.toggle_expanded(&ctx);
            ui.close();
        }
        if ui.button("Settings…").clicked() {
            self.open_settings(&ctx);
            ui.close();
        }
    }

    /// Switching between bits and bytes keeps a fixed scale near where it
    /// was, on the new units' list.
    fn set_units(&mut self, u: Units) {
        if self.s.units == u {
            return;
        }
        let was_fixed = self.s.format().is_fixed(self.s.scale);
        self.s.units = u;
        // A custom scale stays as it is, only written in the new units.
        if self.s.scale > 0.0 && was_fixed {
            let was = self.s.scale;
            self.s.scale = self
                .s
                .format()
                .fixed_scales()
                .into_iter()
                .min_by(|a, b| (a / was).ln().abs().total_cmp(&(b / was).ln().abs()))
                .unwrap_or(0.0);
        }
    }

    // ------------------------------------------------------------ settings

    /// The settings, in a window of their own.
    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut builder = ViewportBuilder::default()
            .with_title("Net Meter settings")
            .with_app_id("taw-netmeter")
            .with_inner_size(SETTINGS_SIZE)
            .with_min_inner_size([320.0, 300.0])
            .with_icon(crate::icon::icon());
        if let Some(at) = self.settings_at {
            builder = builder.with_position(at);
        }
        ctx.show_viewport_immediate(
            ViewportId::from_hash_of("taw-netmeter-settings"),
            builder,
            |ctx, class| {
                if ctx.input(|i| i.viewport().close_requested())
                    || ctx.input(|i| i.key_pressed(Key::Escape))
                {
                    self.settings_open = false;
                }
                let t = Tokens::from_ctx(ctx, &self.s.glass);
                glass::apply_visuals(ctx, &t);
                if class == ViewportClass::EmbeddedWindow {
                    // Where the system can't open another window, the
                    // settings show as one inside the meter.
                    let mut open = self.settings_open;
                    egui::Window::new("Settings")
                        .open(&mut open)
                        .vscroll(true)
                        .show(ctx, |ui| self.settings_body(ui, &t));
                    self.settings_open &= open;
                } else {
                    egui::CentralPanel::default()
                        .frame(
                            egui::Frame::new()
                                .fill(t.window)
                                .inner_margin(egui::Margin::symmetric(18, 14)),
                        )
                        .show(ctx, |ui| {
                            ui.spacing_mut().scroll = egui::style::ScrollStyle::thin();
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| self.settings_body(ui, &t));
                        });
                }
            },
        );
    }

    fn settings_body(&mut self, ui: &mut Ui, t: &Tokens) {
        let ctx = ui.ctx().clone();
        let before_interval = self.s.interval_ms;
        let before_adaptor = self.s.adaptor.clone();
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
        let s = &mut self.s;

        heading(ui, t, "Look", true);
        row(ui, t, "Glass", |ui| {
            for b in Build::ALL {
                ui.selectable_value(&mut s.glass.build, b, b.name());
            }
        });
        row(ui, t, "Opacity", |ui| {
            ui.add(
                egui::Slider::new(&mut s.glass.opacity, glass::OPACITY_MIN..=1.0).show_value(false),
            );
        });
        ui.label(label_text(t, "Download"));
        let look = s.glass;
        if let Some(c) = glass::swatches(ui, t, &look, s.glass.down) {
            s.glass.down = c;
        }
        ui.label(label_text(t, "Upload"));
        if let Some(c) = glass::swatches(ui, t, &look, s.glass.up) {
            s.glass.up = c;
        }

        heading(ui, t, "Display", false);
        if self.sampler.lan_known {
            row(ui, t, "Show", |ui| {
                for v in Show::ALL {
                    ui.selectable_value(&mut s.show, v, v.name());
                }
            });
        } else {
            ui.label(
                egui::RichText::new(
                    "Internet and LAN apart need Linux for now; this shows all traffic.",
                )
                .font(glass::regular(12.5))
                .color(t.ink2),
            );
        }
        row(ui, t, "Graph", |ui| {
            ui.selectable_value(&mut s.mirrored, false, "Overlaid");
            ui.selectable_value(&mut s.mirrored, true, "Mirrored");
        });
        row(ui, t, "History", |ui| {
            for (v, n) in HISTORIES.iter().zip(["1 min", "2 min", "5 min", "10 min"]) {
                ui.selectable_value(&mut s.history_s, *v, n);
            }
        });
        let fmt = s.format();
        let fixed = fmt.fixed_scales();
        let mut custom = self.custom_power;
        if custom.is_none() && s.scale > 0.0 && !fmt.is_fixed(s.scale) {
            custom = Some(fmt.best_power(s.scale));
        }
        row(ui, t, "Scale", |ui| {
            let shown = if custom.is_some() {
                "Custom".to_string()
            } else if s.scale > 0.0 {
                fmt.mark(s.scale)
            } else {
                "Follow the traffic".to_string()
            };
            egui::ComboBox::from_id_salt("scale")
                .selected_text(shown)
                .width(170.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(custom.is_none() && s.scale <= 0.0, "Follow the traffic")
                        .clicked()
                    {
                        s.scale = 0.0;
                        custom = None;
                    }
                    for v in &fixed {
                        let on = custom.is_none() && (v / s.scale - 1.0).abs() < 1e-6;
                        if ui.selectable_label(on, fmt.mark(*v)).clicked() {
                            s.scale = *v;
                            custom = None;
                        }
                    }
                    if ui.selectable_label(custom.is_some(), "Custom…").clicked() {
                        // Start from the fixed scale chosen, or 100 Mb/s.
                        if s.scale <= 0.0 {
                            s.scale = 12_500_000.0;
                        }
                        custom = Some(fmt.best_power(s.scale));
                    }
                });
        });
        if let Some(mut power) = custom {
            row(ui, t, "", |ui| {
                let mut v = fmt.in_unit(s.scale, power);
                let speed = (v * 0.01).max(0.1);
                let r = ui.add(
                    egui::DragValue::new(&mut v)
                        .range(0.1..=9999.0)
                        .speed(speed)
                        .max_decimals(2),
                );
                let names = Format {
                    prefix: Prefix::Auto,
                    ..fmt
                };
                let before = power;
                egui::ComboBox::from_id_salt("scale-unit")
                    .selected_text(names.unit(power))
                    .width(72.0)
                    .show_ui(ui, |ui| {
                        for p in 1..=3 {
                            ui.selectable_value(&mut power, p, names.unit(p));
                        }
                    });
                // Picking another unit keeps the figure: 300 Mb/s becomes
                // 300 Gb/s.
                if r.changed() || power != before {
                    s.scale = fmt.bytes_from(v.clamp(0.1, 9999.0), power);
                }
            });
            custom = Some(power);
        }
        self.custom_power = custom;
        row(ui, t, "Logarithmic", |ui| {
            ui.checkbox(&mut s.log_scale, "");
        });

        heading(ui, t, "Speeds", false);
        let mut units = s.units;
        row(ui, t, "Count in", |ui| {
            ui.selectable_value(&mut units, Units::Bits, "Bits");
            ui.selectable_value(&mut units, Units::Bytes, "Bytes");
        });
        let names = Format {
            prefix: Prefix::Auto,
            ..s.format()
        };
        row(ui, t, "Unit", |ui| {
            for (i, p) in Prefix::ALL.into_iter().enumerate() {
                let n = if i == 0 {
                    "Auto".to_string()
                } else {
                    names.unit(i as i32)
                };
                ui.selectable_value(&mut s.prefix, p, n);
            }
        });
        if s.units == Units::Bytes {
            row(ui, t, "KiB, MiB", |ui| {
                ui.checkbox(&mut s.binary, "Powers of 1024");
            });
        }
        row(ui, t, "Update every", |ui| {
            for (v, n) in INTERVALS.iter().zip(["½ s", "1 s", "2 s"]) {
                ui.selectable_value(&mut s.interval_ms, *v, n);
            }
        });

        heading(ui, t, "Adaptor", false);
        let adaptors = self.adaptors.clone();
        row(ui, t, "Measure", |ui| {
            let shown = match &s.adaptor {
                None => "All adaptors".to_string(),
                Some(id) => adaptors
                    .iter()
                    .find(|(a, _)| a == id)
                    .map_or(format!("{id} (not found)"), |(_, l)| l.clone()),
            };
            egui::ComboBox::from_id_salt("adaptor")
                .selected_text(shown)
                .width(190.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.adaptor, None, "All adaptors");
                    for (id, l) in &adaptors {
                        ui.selectable_value(&mut s.adaptor, Some(id.clone()), l);
                    }
                });
        });

        heading(ui, t, "Dashboard", false);
        ui.label(
            egui::RichText::new("The tiles the expanded window shows. A tile with nothing to show on this computer stays hidden.")
                .font(glass::regular(12.5))
                .color(t.ink2),
        );
        ui.horizontal_wrapped(|ui| {
            for p in Panel::ALL {
                let mut on = s.panels.contains(&p);
                if ui.checkbox(&mut on, p.name()).changed() {
                    if on {
                        s.panels.push(p);
                        s.panels
                            .sort_by_key(|x| Panel::ALL.iter().position(|y| y == x));
                    } else {
                        s.panels.retain(|x| *x != p);
                    }
                }
            }
        });
        row(ui, t, "Temperatures", |ui| {
            ui.selectable_value(&mut s.fahrenheit, false, "°C");
            ui.selectable_value(&mut s.fahrenheit, true, "°F");
        });
        ui.checkbox(
            &mut s.usb_notices,
            "Say when a USB device is plugged in or out",
        );

        heading(ui, t, "Window", false);
        let mut on_top = s.on_top;
        let mut system_bar = s.system_title_bar;
        ui.checkbox(&mut on_top, "Stay on top");
        ui.checkbox(&mut s.controls, "Show the controls under the mouse");
        ui.checkbox(&mut system_bar, "Use the system title bar");
        let mut login = self.start_at_login;
        if ui.checkbox(&mut login, "Start when I log in").changed() {
            self.login_error = autostart::set(login).err();
            self.start_at_login = autostart::is_enabled();
        }
        if let Some(e) = &self.login_error {
            ui.label(
                egui::RichText::new(format!("Couldn't change that: {e}"))
                    .font(glass::regular(12.0))
                    .color(t.ink2),
            );
        }

        heading(ui, t, "Keys", false);
        for (k, what) in [
            ("H", "Hide or show the controls"),
            ("T", "Stay on top, or not"),
            ("Right-click", "Quick choices"),
        ] {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [84.0, 18.0],
                    egui::Label::new(egui::RichText::new(k).font(glass::medium(12.5))),
                );
                ui.label(
                    egui::RichText::new(what)
                        .font(glass::regular(12.5))
                        .color(t.ink2),
                );
            });
        }
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new(format!("NM-001 · Net Meter · v{VERSION}"))
                .font(glass::regular(11.0))
                .color(t.ink2),
        );

        self.set_units(units);
        if on_top != self.s.on_top {
            self.set_on_top(&ctx, on_top);
        }
        if system_bar != self.s.system_title_bar {
            self.s.system_title_bar = system_bar;
            ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Decorations(system_bar));
        }
        if self.s.interval_ms != before_interval {
            self.sampler.set_interval(self.s.interval_ms);
        }
        if self.s.adaptor != before_adaptor {
            self.sampler.set_adaptor(self.s.adaptor.clone());
            self.history.clear();
        }
    }
}

/// Where a window of `size` goes beside the meter at `meter`: to its right
/// if it fits on the meter's screen, otherwise to its left, its top level
/// with the meter's and kept on the screen. The screen is taken to be the
/// `monitor`-sized tile of the desktop the meter's top left is in, which is
/// right for one screen, however wide, and for screens side by side.
fn beside(meter: Rect, size: Vec2, monitor: Option<Vec2>) -> Pos2 {
    let right = Pos2::new(meter.right() + SETTINGS_GAP, meter.top());
    let Some(mon) = monitor.filter(|m| m.x > 0.0 && m.y > 0.0) else {
        return right;
    };
    let origin = Pos2::new(
        (meter.left() / mon.x).floor() * mon.x,
        (meter.top() / mon.y).floor() * mon.y,
    );
    let screen = Rect::from_min_size(origin, mon);
    let x = if right.x + size.x <= screen.right() {
        right.x
    } else {
        (meter.left() - SETTINGS_GAP - size.x).max(screen.left())
    };
    let y = meter.top().min(screen.bottom() - size.y).max(screen.top());
    Pos2::new(x, y)
}

/// A USB device plugged in or out.
struct UsbEvent {
    at: f64,
    text: String,
}

/// What was plugged in (`true`) and unplugged (`false`) between two lists.
fn usb_changes(was: &[UsbDevice], is: &[UsbDevice]) -> Vec<(UsbDevice, bool)> {
    let mut v: Vec<(UsbDevice, bool)> = is
        .iter()
        .filter(|d| !was.iter().any(|w| w.id == d.id))
        .map(|d| (d.clone(), true))
        .collect();
    v.extend(
        was.iter()
            .filter(|w| !is.iter().any(|d| d.id == w.id))
            .map(|w| (w.clone(), false)),
    );
    v
}

fn label_text(t: &Tokens, s: &str) -> egui::RichText {
    egui::RichText::new(s)
        .font(glass::regular(13.5))
        .color(t.ink)
}

/// A section heading in the settings: spaced capitals under a hairline.
fn heading(ui: &mut Ui, t: &Tokens, s: &str, first: bool) {
    if !first {
        ui.add_space(8.0);
        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(r, 0, t.grid);
        ui.add_space(4.0);
    }
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
    glass::caps(
        ui.painter(),
        Pos2::new(r.left(), r.center().y),
        s,
        10.5,
        t.ink2,
        r.width(),
    );
}

/// A row in the settings: a label, then its control.
fn row(ui: &mut Ui, t: &Tokens, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(96.0, 22.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(96.0);
                ui.label(label_text(t, label));
            },
        );
        add(ui);
    });
}

impl eframe::App for NetMeterApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let t = Tokens::from_ctx(&ctx, &self.s.glass);
        glass::apply_visuals(&ctx, &t);
        self.now = ui.input(|i| i.time);
        self.take_samples();

        let free = ui.memory(|m| m.focused().is_none());
        let (h, key_t) = ui.input(|i| (i.key_pressed(Key::H), i.key_pressed(Key::T)));
        if free && h {
            self.s.controls = !self.s.controls;
        }
        if free && key_t {
            self.set_on_top(&ctx, !self.s.on_top);
        }

        let screen = ui.max_rect();
        glass::pane(ui.painter(), screen, &t);
        // The glass moves the window, a double-click hides or shows the
        // controls, and a right-click gives the quick choices.
        let resp = ui.interact(screen, Id::new("display"), Sense::click_and_drag());
        if resp.double_clicked() {
            self.s.controls = !self.s.controls;
        } else if resp.drag_started_by(egui::PointerButton::Primary) && !self.s.system_title_bar {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            ctx.stop_dragging();
        }
        resp.context_menu(|ui| self.quick_menu(ui));
        if self.s.expanded {
            self.dashboard(ui, screen, &t);
        } else {
            self.display(ui, screen, &t);
            self.show_notice(ui, screen, &t);
        }
        let under_mouse = ui.input(|i| i.pointer.hover_pos()).is_some();
        if self.s.controls && !self.s.system_title_bar && under_mouse {
            self.controls(ui, screen, &t);
        }

        self.settings_window(&ctx);
        if !self.s.system_title_bar {
            hifi::resize_edges(ui, screen);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.s);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // See-through: the pane draws the glass, and the corners show the
        // desktop.
        [0.0; 4]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_load_from_nothing_and_from_part() {
        let s: Saved = serde_json::from_str("{}").unwrap();
        assert_eq!(s.history_s, 120);
        assert!(s.on_top);
        let s: Saved = serde_json::from_str(r#"{"units":"Bytes","prefix":"Mega"}"#).unwrap();
        assert_eq!(s.format().rate(2_500_000.0), "2.50 MB/s");
    }

    #[test]
    fn usb_plugged_in_and_out() {
        let d = |id: &str| UsbDevice {
            id: id.into(),
            name: id.into(),
            speed_mbps: None,
        };
        let c = usb_changes(&[d("a"), d("b")], &[d("b"), d("c")]);
        assert_eq!(c, vec![(d("c"), true), (d("a"), false)]);
    }

    #[test]
    fn settings_open_beside_the_meter() {
        let size = SETTINGS_SIZE;
        let wide = Some(Vec2::new(5120.0, 1440.0));
        // Room on the right: just to the right, level with the meter.
        let m = Rect::from_min_size(Pos2::new(100.0, 200.0), DEFAULT_SIZE);
        assert_eq!(beside(m, size, wide), Pos2::new(532.0, 200.0));
        // At the right edge of a super-wide screen: just to its left.
        let m = Rect::from_min_size(Pos2::new(4680.0, 40.0), DEFAULT_SIZE);
        assert_eq!(beside(m, size, wide), Pos2::new(4288.0, 40.0));
        // Near the bottom: lifted to stay on the screen.
        let m = Rect::from_min_size(Pos2::new(100.0, 1300.0), DEFAULT_SIZE);
        assert_eq!(beside(m, size, wide).y, 840.0);
        // The right-hand screen of two side by side.
        let two = Some(Vec2::new(1920.0, 1080.0));
        let m = Rect::from_min_size(Pos2::new(3800.0 - 420.0, 0.0), DEFAULT_SIZE);
        assert_eq!(beside(m, size, two).x, 3380.0 - 12.0 - 380.0);
        // Nothing known about the screen: to the right.
        assert_eq!(beside(m, size, None), Pos2::new(3812.0, 0.0));
    }

    #[test]
    fn version_0_1_settings_load_on_glass() {
        let s: Saved = serde_json::from_str(
            r#"{"finish":{"finish":"Spool","build":"Dark","accent":"Amber"},"show":"Lan","controls":false}"#,
        )
        .unwrap();
        assert_eq!(s.show, Show::Lan);
        assert!(!s.controls);
        assert_eq!(s.glass, glass::Look::default());
        assert_eq!(s.glass.down, glass::Colour::Ok);
        assert_eq!(s.glass.up, glass::Colour::Pen2);
    }
}
