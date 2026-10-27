//! The Net Meter (NM-001): a small window of scrolling graphs, download and
//! upload, for the Internet and the local network, to keep on the desktop.
//!
//! The panel is the TAW hi-fi look (TAW-Panel's `hifi`), cut down to a
//! slim strip along the top (badge, settings, window buttons) over the
//! display window. H, or a double-click on the display, hides the strip so
//! only the display is left; drag the display to move the window.
//! Right-click the display for the everyday choices.

use crate::net;
use crate::rates::{self, Format, History, Lane, Prefix, Rate, Sample, Show, Units};
use crate::sampler::Sampler;
use egui::{
    Align2, Color32, Id, Key, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
    ViewportCommand, WindowLevel,
};
use serde::{Deserialize, Serialize};
use taw_panel::hifi::{self, Seg, Tokens};

const STORAGE_KEY: &str = "taw-netmeter";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_SIZE: Vec2 = Vec2::new(420.0, 180.0);
pub const MIN_SIZE: Vec2 = Vec2::new(200.0, 90.0);
/// The strip along the top: drag it to move the window.
const STRIP_H: f32 = 34.0;
/// The line over each graph with its name and the rates.
const HEAD_H: f32 = 18.0;
const HISTORIES: [u32; 4] = [60, 120, 300, 600];
const INTERVALS: [u32; 3] = [500, 1000, 2000];

/// Everything remembered between runs.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    /// Finish, light or dark, and accent.
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
    /// The strip along the top; off leaves only the display.
    controls: bool,
    on_top: bool,
    /// Use the system's title bar instead of the meter's own strip.
    system_title_bar: bool,
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
    /// The adaptors, for the menu: (id, label).
    adaptors: Vec<(String, String)>,
}

impl NetMeterApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        hifi::install_fonts(&cc.egui_ctx);
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
        let sampler = Sampler::start(s.interval_ms, s.adaptor.clone(), cc.egui_ctx.clone());
        NetMeterApp {
            s,
            sampler,
            history: History::default(),
            error: None,
            settings_open: false,
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
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(if on {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        }));
    }

    fn take_samples(&mut self) {
        while let Ok(r) = self.sampler.rx.try_recv() {
            match r {
                Ok(s) => {
                    self.history.push(s);
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
    }

    // ------------------------------------------------------------ display

    fn display(&self, ui: &Ui, rect: Rect, t: &Tokens) {
        hifi::display(ui.painter(), rect, t);
        let p = ui.painter().with_clip_rect(rect);
        let inner = rect.shrink2(Vec2::new(10.0, 6.0));
        if let Some(e) = &self.error {
            hifi::ink_text(
                &p,
                inner.center(),
                Align2::CENTER_CENTER,
                &format!("Can't read the network: {e}"),
                hifi::ui_font(12.5),
                t,
            );
            return;
        }
        if self.history.latest().is_none() {
            hifi::ink_text(
                &p,
                inner.center(),
                Align2::CENTER_CENTER,
                "Measuring…",
                hifi::ui_font(12.5),
                t,
            );
            return;
        }
        let lanes = self.s.show.lanes(self.sampler.lan_known);
        let gap = 8.0;
        let h = (inner.height() - gap * (lanes.len() as f32 - 1.0)) / lanes.len() as f32;
        // Where the pointer is over the graph, as a time.
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

    /// The colours of download and upload.
    fn colours(t: &Tokens) -> (Color32, Color32) {
        let up = if t.ink == t.accent { t.text } else { t.accent };
        (t.ink, up)
    }

    fn lane(&self, p: &Painter, r: Rect, lane: Lane, t: &Tokens, hover_x: Option<f32>) {
        let fmt = self.s.format();
        let span = self.s.span();
        let (down_c, up_c) = Self::colours(t);
        let g = Rect::from_min_max(Pos2::new(r.left(), r.top() + HEAD_H), r.max);
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

        // The grid: the top of the scale and the middle, faintly.
        let faint = Stroke::new(1.0, t.ink_faint(0.12));
        let mid_y = g.center().y;
        p.line_segment([g.left_top(), g.right_top()], faint);
        p.line_segment(
            [Pos2::new(g.left(), mid_y), Pos2::new(g.right(), mid_y)],
            Stroke::new(1.0, t.ink_faint(if self.s.mirrored { 0.22 } else { 0.07 })),
        );
        if g.height() > 28.0 {
            hifi::label(
                p,
                Pos2::new(g.left() + 2.0, g.top() + 2.0),
                Align2::LEFT_TOP,
                &fmt.mark(top),
                hifi::num(9.5),
                t.ink_faint(0.55),
            );
        }

        // The traffic.
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
        if self.s.mirrored {
            hifi::ink_curve(p, &down, mid_y, down_c, 0.40, 1.5);
            hifi::ink_curve(p, &up, mid_y, up_c, 0.30, 1.5);
        } else {
            hifi::ink_curve(p, &down, g.bottom(), down_c, 0.38, 1.5);
            hifi::ink_curve(p, &up, g.bottom(), up_c, 0.12, 1.5);
        }

        // The pointer's moment, or now.
        let mut shown: Option<(&Sample, String)> = None;
        if let Some(x) = hover_x {
            let at = newest - ((g.right() - x) / g.width()) as f64 * span;
            if let Some(s) = self.history.nearest(at) {
                let sx = x_of(s.at);
                p.line_segment(
                    [Pos2::new(sx, g.top()), Pos2::new(sx, g.bottom())],
                    Stroke::new(1.0, t.ink_faint(0.45)),
                );
                let ago = (newest - s.at).round();
                let when = if ago < 1.0 {
                    "now".to_string()
                } else if ago < 120.0 {
                    format!("{ago:.0} s ago")
                } else {
                    format!("{:.0} min ago", ago / 60.0)
                };
                shown = Some((s, when));
            }
        }
        let (sample, when) = match shown {
            Some((s, w)) => (s, Some(w)),
            None => (self.history.latest().unwrap(), None),
        };
        let rate = sample.lane(lane).unwrap_or_default();
        self.heading(p, r, lane, rate, when.as_deref(), t);
    }

    /// The lane's name on the left, and ↓ download ↑ upload on the right.
    fn heading(
        &self,
        p: &Painter,
        r: Rect,
        lane: Lane,
        rate: Rate,
        when: Option<&str>,
        t: &Tokens,
    ) {
        let fmt = self.s.format();
        let (down_c, up_c) = Self::colours(t);
        let y = r.top() + HEAD_H / 2.0 - 1.0;
        let font = hifi::num(11.0);
        let mut x = r.right();
        let right_part = |x: &mut f32, v: f64, up: bool, c: Color32| {
            let tr = hifi::ink_text(
                p,
                Pos2::new(*x, y),
                Align2::RIGHT_CENTER,
                &fmt.rate(v),
                font.clone(),
                t,
            );
            arrow(p, Pos2::new(tr.left() - 7.0, y), up, c);
            *x = tr.left() - 20.0;
        };
        right_part(&mut x, rate.up, true, up_c);
        right_part(&mut x, rate.down, false, down_c);
        let name = match when {
            Some(w) => format!("{} · {w}", lane.name().to_uppercase()),
            None => lane.name().to_uppercase(),
        };
        let room = x - r.left() - 6.0;
        if room > 40.0 {
            hifi::label_fit(
                p,
                Pos2::new(r.left(), y),
                Align2::LEFT_CENTER,
                &name,
                hifi::semi(10.5),
                t.ink_faint(0.8),
                room,
            );
        }
    }

    // ------------------------------------------------------------ top

    fn strip(&mut self, ui: &mut Ui, screen: Rect, t: &Tokens) {
        let strip = Rect::from_min_size(screen.min, Vec2::new(screen.width(), STRIP_H));
        if !self.s.system_title_bar {
            hifi::drag_to_move(ui, strip);
        }
        let mut right = screen.right() - 6.0;
        if !self.s.system_title_bar {
            let b = hifi::window_buttons(ui, Pos2::new(right, screen.top() + 5.0), t);
            right = b.left() - 4.0;
        }
        let gear = Rect::from_min_size(
            Pos2::new(right - 26.0, screen.top() + 4.0),
            Vec2::splat(26.0),
        );
        if hifi::gear_button(ui, gear, Id::new("settings"), t, self.settings_open).clicked() {
            self.open_settings();
        }
        let room = gear.left() - screen.left() - 20.0;
        let at = Pos2::new(screen.left() + 10.0, screen.top() + 6.0);
        if room > 210.0 {
            hifi::badge(
                ui.painter(),
                Pos2::new(at.x + badge_width(ui, t) / 2.0, at.y),
                t,
                "Net Meter",
            );
        } else if room > 70.0 {
            hifi::label(
                ui.painter(),
                Pos2::new(at.x, screen.top() + STRIP_H / 2.0),
                Align2::LEFT_CENTER,
                "Net Meter",
                hifi::bold(12.0),
                t.text,
            );
        }
    }

    fn open_settings(&mut self) {
        self.settings_open = !self.settings_open;
        if self.settings_open {
            self.refresh_adaptors();
        }
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
        if ui.checkbox(&mut self.s.mirrored, "Mirrored").clicked() {
            ui.close();
        }
        let mut on_top = self.s.on_top;
        if ui.checkbox(&mut on_top, "Stay on top").clicked() {
            self.set_on_top(&ctx, on_top);
            ui.close();
        }
        let strip = if self.s.controls {
            "Hide the title strip"
        } else {
            "Show the title strip"
        };
        if ui.button(strip).clicked() {
            self.s.controls = !self.s.controls;
            ui.close();
        }
        if ui.button("Settings…").clicked() {
            self.open_settings();
            ui.close();
        }
    }

    /// Switching between bits and bytes keeps a fixed scale near where it
    /// was, on the new units' list.
    fn set_units(&mut self, u: Units) {
        if self.s.units == u {
            return;
        }
        self.s.units = u;
        if self.s.scale > 0.0 {
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

    fn settings(&mut self, ctx: &egui::Context, t: &Tokens) {
        let mut open = self.settings_open;
        let before_interval = self.s.interval_ms;
        let before_adaptor = self.s.adaptor.clone();
        let mut on_top = self.s.on_top;
        let mut system_bar = self.s.system_title_bar;
        let mut units = self.s.units;
        let lan_known = self.sampler.lan_known;
        let adaptors = self.adaptors.clone();
        let s = &mut self.s;
        hifi::drawer(ctx, t, "Settings", &mut open, |ui| {
            hifi::look_settings(ui, t, &mut s.finish);

            hifi::drawer_heading(ui, t, "Display");
            if lan_known {
                let names: Vec<&str> = Show::ALL.iter().map(|v| v.name()).collect();
                let sel = Show::ALL.iter().position(|v| *v == s.show).unwrap_or(0);
                if let Some(i) = seg_row(ui, t, "Show", "show", &names, sel) {
                    s.show = Show::ALL[i];
                }
            } else {
                ui.label(
                    egui::RichText::new(
                        "Internet and LAN apart need Linux for now; this shows all traffic.",
                    )
                    .font(hifi::ui_font(12.0))
                    .color(t.dim),
                );
            }
            if let Some(i) = seg_row(
                ui,
                t,
                "Graph",
                "graph",
                &["Overlaid", "Mirrored"],
                s.mirrored as usize,
            ) {
                s.mirrored = i == 1;
            }
            let names = ["1 min", "2 min", "5 min", "10 min"];
            let sel = HISTORIES
                .iter()
                .position(|h| *h == s.history_s)
                .unwrap_or(1);
            if let Some(i) = seg_row(ui, t, "History", "history", &names, sel) {
                s.history_s = HISTORIES[i];
            }
            let fmt = s.format();
            let fixed = fmt.fixed_scales();
            let mut items = vec!["Follow the traffic".to_string()];
            items.extend(fixed.iter().map(|v| fmt.mark(*v)));
            let sel = if s.scale <= 0.0 {
                0
            } else {
                match fixed.iter().position(|v| (v / s.scale - 1.0).abs() < 1e-6) {
                    Some(i) => i + 1,
                    // A scale from the other units, until another is chosen.
                    None => {
                        items.push(fmt.mark(s.scale));
                        items.len() - 1
                    }
                }
            };
            let r = hifi::drawer_row(ui, t, "Scale", 170.0);
            if let Some(i) = hifi::select(ui, r, "scale", t, &items, sel) {
                s.scale = if i == 0 {
                    0.0
                } else {
                    fixed.get(i - 1).copied().unwrap_or(s.scale)
                };
            }
            switch_row(ui, t, "Logarithmic scale", "log", &mut s.log_scale);

            hifi::drawer_heading(ui, t, "Speeds");
            if let Some(i) = seg_row(
                ui,
                t,
                "Count in",
                "units",
                &["Bits", "Bytes"],
                (units == Units::Bytes) as usize,
            ) {
                units = [Units::Bits, Units::Bytes][i];
            }
            let shown = Format {
                prefix: Prefix::Auto,
                ..s.format()
            };
            let names = [
                "Auto".to_string(),
                shown.unit(1),
                shown.unit(2),
                shown.unit(3),
            ];
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            let sel = Prefix::ALL.iter().position(|p| *p == s.prefix).unwrap_or(0);
            if let Some(i) = seg_row(ui, t, "Unit", "prefix", &names, sel) {
                s.prefix = Prefix::ALL[i];
            }
            if s.units == Units::Bytes {
                switch_row(ui, t, "Powers of 1024 (KiB, MiB)", "binary", &mut s.binary);
            }
            let names = ["½ s", "1 s", "2 s"];
            let sel = INTERVALS
                .iter()
                .position(|v| *v == s.interval_ms)
                .unwrap_or(1);
            if let Some(i) = seg_row(ui, t, "Update every", "interval", &names, sel) {
                s.interval_ms = INTERVALS[i];
            }

            hifi::drawer_heading(ui, t, "Adaptor");
            let mut items = vec!["All adaptors".to_string()];
            items.extend(adaptors.iter().map(|(_, l)| l.clone()));
            let mut sel = match &s.adaptor {
                None => 0,
                Some(id) => adaptors
                    .iter()
                    .position(|(a, _)| a == id)
                    .map_or(usize::MAX, |i| i + 1),
            };
            if sel == usize::MAX {
                items.push(format!(
                    "{} (not found)",
                    s.adaptor.as_deref().unwrap_or("")
                ));
                sel = items.len() - 1;
            }
            let r = hifi::drawer_row(ui, t, "Measure", 190.0);
            if let Some(i) = hifi::select(ui, r, "adaptor", t, &items, sel) {
                if i == 0 {
                    s.adaptor = None;
                } else if let Some((id, _)) = adaptors.get(i - 1) {
                    s.adaptor = Some(id.clone());
                }
            }

            hifi::drawer_heading(ui, t, "Window");
            switch_row(ui, t, "Stay on top", "on-top", &mut on_top);
            switch_row(
                ui,
                t,
                "Use the system title bar",
                "system-bar",
                &mut system_bar,
            );

            hifi::drawer_heading(ui, t, "Keys");
            for (k, what) in [
                ("H", "Hide or show the title strip"),
                ("T", "Stay on top, or not"),
                ("Right-click", "Quick choices"),
            ] {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [84.0, 18.0],
                        egui::Label::new(
                            egui::RichText::new(k).font(hifi::num(12.0)).color(t.text),
                        ),
                    );
                    ui.label(
                        egui::RichText::new(what)
                            .font(hifi::ui_font(12.5))
                            .color(t.dim),
                    );
                });
            }
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(format!("NM-001 · NET METER · V{VERSION}"))
                    .font(hifi::semi(9.5))
                    .color(t.dim),
            );
        });
        self.settings_open = open;
        self.set_units(units);
        if on_top != self.s.on_top {
            self.set_on_top(ctx, on_top);
        }
        if system_bar != self.s.system_title_bar {
            self.s.system_title_bar = system_bar;
            ctx.send_viewport_cmd(ViewportCommand::Decorations(system_bar));
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

/// A row in the drawer with a segmented control on the right.
fn seg_row(
    ui: &mut Ui,
    t: &Tokens,
    label: &str,
    id: &str,
    names: &[&str],
    sel: usize,
) -> Option<usize> {
    let r = hifi::drawer_row(ui, t, label, 0.0);
    let (_, picked) = hifi::segmented(
        ui,
        Pos2::new(r.right(), r.center().y),
        true,
        Id::new(id),
        t,
        names,
        sel,
        Seg::Plain,
        &[],
        &[],
    );
    picked
}

/// A row in the drawer with a switch on the right.
fn switch_row(ui: &mut Ui, t: &Tokens, label: &str, id: &str, on: &mut bool) {
    let r = hifi::drawer_row(ui, t, label, 32.0);
    let r = Rect::from_min_size(r.min + Vec2::new(0.0, 4.0), Vec2::new(32.0, 18.0));
    if hifi::switch(ui, r, Id::new(id), t, *on).clicked() {
        *on = !*on;
    }
}

/// The badge's width, to place it by its left edge.
fn badge_width(ui: &Ui, t: &Tokens) -> f32 {
    let p = ui.painter();
    let w = |s: &str, f| p.layout_no_wrap(s.to_string(), f, t.text).size().x;
    w("Timster's Audio Works - ", hifi::ui_font(12.0)) + w("Net Meter", hifi::bold(12.0)) + 18.0
}

/// A small arrow, down for download and up for upload, in the line's
/// colour.
fn arrow(p: &Painter, c: Pos2, up: bool, colour: Color32) {
    let s = if up { -1.0 } else { 1.0 };
    p.add(Shape::convex_polygon(
        vec![
            c + Vec2::new(-4.0, -3.0 * s),
            c + Vec2::new(4.0, -3.0 * s),
            c + Vec2::new(0.0, 3.5 * s),
        ],
        colour,
        Stroke::NONE,
    ));
}

impl eframe::App for NetMeterApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let t = Tokens::from_ctx(&ctx, &self.s.finish);
        hifi::apply_visuals(&ctx, &t);
        self.take_samples();

        let free = ui.memory(|m| m.focused().is_none()) && !self.settings_open;
        let (h, key_t) = ui.input(|i| (i.key_pressed(Key::H), i.key_pressed(Key::T)));
        if free && h {
            self.s.controls = !self.s.controls;
        }
        if free && key_t {
            self.set_on_top(&ctx, !self.s.on_top);
        }

        let screen = ui.max_rect();
        hifi::faceplate(ui.painter(), screen, &t);
        let display = if self.s.controls {
            self.strip(ui, screen, &t);
            Rect::from_min_max(
                Pos2::new(screen.left() + 8.0, screen.top() + STRIP_H),
                screen.max - Vec2::splat(8.0),
            )
        } else {
            screen.shrink(4.0)
        };
        // The display moves the window, a double-click hides or shows the
        // strip, and a right-click gives the quick choices.
        let resp = ui.interact(display, Id::new("display"), Sense::click_and_drag());
        if resp.double_clicked() {
            self.s.controls = !self.s.controls;
        } else if resp.drag_started_by(egui::PointerButton::Primary) && !self.s.system_title_bar {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            ctx.stop_dragging();
        }
        resp.context_menu(|ui| self.quick_menu(ui));
        self.display(ui, display, &t);
        ui.painter()
            .rect_stroke(screen, 0, Stroke::new(1.0, t.edge), StrokeKind::Inside);

        self.settings(&ctx, &t);
        if !self.s.system_title_bar {
            hifi::resize_edges(ui, screen);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.s);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        Tokens::new(&self.s.finish, false)
            .face_bottom
            .to_normalized_gamma_f32()
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
}
