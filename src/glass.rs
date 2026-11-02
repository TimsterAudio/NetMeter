//! The Glass look, from TAW-Panel's looks review (`docs/looks-review.html`,
//! 27 October 2026): frosted pale grey or smoked glass with the desktop
//! showing through, no faceplate, Outfit type with light figures, and the
//! controls only under the mouse.
//!
//! It lives here until a second utility wants it; then it moves into
//! TAW-Panel beside `hifi`.

use egui::epaint::{Mesh, Shape};
use egui::text::{LayoutJob, TextFormat};
use egui::{
    Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Id, Painter,
    Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The corner radius Glass uses throughout.
pub const RADIUS: u8 = 14;

/// Frosted (light) or smoked (dark) glass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Build {
    /// Follow the computer's light or dark setting.
    #[default]
    Automatic,
    Frosted,
    Smoked,
}

impl Build {
    pub const ALL: [Build; 3] = [Build::Automatic, Build::Frosted, Build::Smoked];

    pub fn name(self) -> &'static str {
        match self {
            Build::Automatic => "Automatic",
            Build::Frosted => "Frosted",
            Build::Smoked => "Smoked",
        }
    }
}

/// A colour for a trace, from Glass's palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Colour {
    /// Glass's "ok": green.
    Ok,
    /// Glass's pen: blue.
    Pen,
    /// Glass's second pen: orange.
    Pen2,
    /// Glass's "warn": gold.
    Warn,
    Red,
    Violet,
    /// The text colour: near black on frosted glass, near white on smoked.
    Ink,
}

impl Colour {
    pub const ALL: [Colour; 7] = [
        Colour::Ok,
        Colour::Pen,
        Colour::Pen2,
        Colour::Warn,
        Colour::Red,
        Colour::Violet,
        Colour::Ink,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Colour::Ok => "Green (Glass OK)",
            Colour::Pen => "Blue (Glass Pen)",
            Colour::Pen2 => "Orange (Glass Pen 2)",
            Colour::Warn => "Gold (Glass Warn)",
            Colour::Red => "Red",
            Colour::Violet => "Violet",
            Colour::Ink => "Ink",
        }
    }
}

/// What the user chose. Kept in the saved settings.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Look {
    pub build: Build,
    pub down: Colour,
    pub up: Colour,
    /// How solid the glass is, 0.2 (mostly desktop) to 1 (none).
    pub opacity: f32,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            build: Build::Automatic,
            down: Colour::Ok,
            up: Colour::Pen2,
            opacity: 0.66,
        }
    }
}

pub const OPACITY_MIN: f32 = 0.2;

/// The colours a [`Look`] resolves to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub dark: bool,
    /// The glass itself.
    pub paper: Color32,
    /// Raised things on the glass: the control strip, menus.
    pub sheet: Color32,
    /// The settings window: solid, as it isn't a gadget.
    pub window: Color32,
    pub grid: Color32,
    pub edge: Color32,
    pub ink: Color32,
    pub ink2: Color32,
    pub pen: Color32,
    pub down: Color32,
    pub up: Color32,
    /// Cool, warm and hot, for temperatures and full disks.
    pub ok: Color32,
    pub warn: Color32,
    pub hot: Color32,
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn rgba(v: u32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (v >> 16) as u8,
        (v >> 8) as u8,
        v as u8,
        (a.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

impl Tokens {
    /// `system_dark` is the computer's setting, for [`Build::Automatic`].
    pub fn new(look: &Look, system_dark: bool) -> Tokens {
        let dark = match look.build {
            Build::Automatic => system_dark,
            Build::Frosted => false,
            Build::Smoked => true,
        };
        let a = look.opacity.clamp(OPACITY_MIN, 1.0);
        // The review's palettes, light and dark.
        let (paper, sheet, window, grid, edge, ink, ink2) = if dark {
            (
                rgba(0x0e1116, a),
                rgba(0x1e222a, 0.75_f32.max(a)),
                hex(0x1a1e25),
                rgba(0xffffff, 0.10),
                rgba(0xffffff, 0.14),
                hex(0xeef3f8),
                hex(0x9aa6b2),
            )
        } else {
            (
                rgba(0xeef1f5, a),
                rgba(0xffffff, 0.6_f32.max(a)),
                hex(0xf3f5f8),
                rgba(0x101820, 0.12),
                rgba(0xffffff, 0.55),
                hex(0x101820),
                hex(0x4f5a66),
            )
        };
        let colour = |c: Colour| match (c, dark) {
            (Colour::Ok, false) => hex(0x2e8b57),
            (Colour::Ok, true) => hex(0x6fd49a),
            (Colour::Pen, false) => hex(0x0a7fc2),
            (Colour::Pen, true) => hex(0x5ccfff),
            (Colour::Pen2, false) => hex(0xc8701e),
            (Colour::Pen2, true) => hex(0xffb35c),
            (Colour::Warn, false) => hex(0xb7791f),
            (Colour::Warn, true) => hex(0xe6b450),
            (Colour::Red, false) => hex(0xc0392b),
            (Colour::Red, true) => hex(0xff6b5e),
            (Colour::Violet, false) => hex(0x6a45b8),
            (Colour::Violet, true) => hex(0xb49cf0),
            (Colour::Ink, _) => ink,
        };
        Tokens {
            dark,
            paper,
            sheet,
            window,
            grid,
            edge,
            ink,
            ink2,
            pen: colour(Colour::Pen),
            down: colour(look.down),
            ok: colour(Colour::Ok),
            warn: colour(Colour::Warn),
            hot: colour(Colour::Red),
            up: colour(look.up),
        }
    }

    pub fn from_ctx(ctx: &egui::Context, look: &Look) -> Tokens {
        Tokens::new(look, ctx.system_theme() == Some(egui::Theme::Dark))
    }

    /// Green to gold to red, for 0 (cool or empty) to 1 (hot or full).
    pub fn heat(&self, h: f32) -> Color32 {
        let lerp = |a: Color32, b: Color32, k: f32| {
            let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * k).round() as u8;
            Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
        };
        let h = h.clamp(0.0, 1.0);
        if h < 0.5 {
            lerp(self.ok, self.warn, h * 2.0)
        } else {
            lerp(self.warn, self.hot, (h - 0.5) * 2.0)
        }
    }

    /// A palette colour as it shows in this build, for the swatches.
    pub fn colour(&self, look: &Look, c: Colour) -> Color32 {
        Tokens::new(&Look { down: c, ..*look }, self.dark).down
    }
}

// ---------------------------------------------------------------- type

const F_LIGHT: &str = "glass-light";
const F_REGULAR: &str = "glass";
const F_MEDIUM: &str = "glass-medium";

/// Outfit Light: the big figures.
pub fn light(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(F_LIGHT.into()))
}
/// Outfit Regular: everything else.
pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(F_REGULAR.into()))
}
/// Outfit Medium: the spaced capitals over each graph.
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(F_MEDIUM.into()))
}

/// Outfit, built in; egui's own fonts behind it for anything it lacks.
pub fn install_fonts(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let faces: [(&str, &'static [u8]); 3] = [
        (F_LIGHT, include_bytes!("../assets/fonts/Outfit-Light.ttf")),
        (
            F_REGULAR,
            include_bytes!("../assets/fonts/Outfit-Regular.ttf"),
        ),
        (
            F_MEDIUM,
            include_bytes!("../assets/fonts/Outfit-Medium.ttf"),
        ),
    ];
    for (name, bytes) in faces {
        defs.font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
        let mut chain = vec![name.to_owned()];
        chain.extend(defs.families[&FontFamily::Proportional].iter().cloned());
        defs.families.insert(FontFamily::Name(name.into()), chain);
    }
    defs.families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .insert(0, F_REGULAR.to_owned());
    ctx.set_fonts(defs);
}

/// egui's own widgets (menus, the settings window) in Glass's colours.
pub fn apply_visuals(ctx: &egui::Context, t: &Tokens) {
    let mut v = if t.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let solid = |c: Color32| Color32::from_rgb(c.r(), c.g(), c.b());
    let field = if t.dark { hex(0x252a33) } else { hex(0xffffff) };
    let hover = if t.dark { hex(0x2e3540) } else { hex(0xe6eaf0) };
    v.panel_fill = t.window;
    v.window_fill = t.window;
    v.window_stroke = Stroke::new(1.0, t.grid);
    v.extreme_bg_color = field;
    v.faint_bg_color = field;
    v.override_text_color = Some(t.ink);
    v.selection.bg_fill = solid(t.pen).gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, t.pen);
    v.hyperlink_color = t.pen;
    v.slider_trailing_fill = true;
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(8);
    }
    v.widgets.inactive.weak_bg_fill = field;
    v.widgets.inactive.bg_fill = field;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.grid);
    v.widgets.hovered.weak_bg_fill = hover;
    v.widgets.hovered.bg_fill = hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.ink2);
    v.widgets.active.weak_bg_fill = hover;
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(10);
    if ctx.global_style().visuals != v {
        ctx.set_visuals(v);
    }
}

// ---------------------------------------------------------------- drawing

/// The glass: a rounded, see-through panel with a light edge.
pub fn pane(p: &Painter, r: Rect, t: &Tokens) {
    p.rect_filled(r, CornerRadius::same(RADIUS), t.paper);
    p.rect_stroke(
        r,
        CornerRadius::same(RADIUS),
        Stroke::new(1.0, t.edge),
        StrokeKind::Inside,
    );
}

/// Spaced capitals: "I N T E R N E T", cut to `max_w`.
pub fn caps(p: &Painter, pos: Pos2, s: &str, size: f32, c: Color32, max_w: f32) -> Rect {
    let mut job = LayoutJob::default();
    job.append(
        &s.to_uppercase(),
        0.0,
        TextFormat {
            font_id: medium(size),
            color: c,
            extra_letter_spacing: size * 0.18,
            ..Default::default()
        },
    );
    job.wrap.max_width = max_w.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let g = p.layout_job(job);
    let r = Align2::LEFT_CENTER.anchor_size(pos, g.size());
    p.galley(r.min, g, c);
    r
}

/// A trace: a fill fading from `fill` at the top of the plot to nothing at
/// `base_y`, under a line. `fill` 0 draws the line alone.
pub fn trace(p: &Painter, pts: &[Pos2], plot: Rect, base_y: f32, c: Color32, fill: f32, w: f32) {
    if pts.len() < 2 {
        return;
    }
    if fill > 0.0 {
        let span = (base_y - plot.top()).abs().max(1.0);
        let mut mesh = Mesh::default();
        for (i, q) in pts.iter().enumerate() {
            let k = 1.0 - ((q.y - plot.top()).abs() / span).clamp(0.0, 1.0);
            mesh.colored_vertex(*q, c.gamma_multiply(fill * k.max(0.08)));
            mesh.colored_vertex(Pos2::new(q.x, base_y), Color32::TRANSPARENT);
            if i > 0 {
                let n = (i as u32) * 2;
                mesh.add_triangle(n - 2, n - 1, n);
                mesh.add_triangle(n - 1, n + 1, n);
            }
        }
        p.add(Shape::mesh(mesh));
    }
    p.add(Shape::line(pts.to_vec(), Stroke::new(w, c)));
}

/// A small arrow, down for download and up for upload.
pub fn arrow(p: &Painter, c: Pos2, up: bool, size: f32, colour: Color32) {
    let s = if up { -1.0 } else { 1.0 };
    let h = size * 0.5;
    p.add(Shape::convex_polygon(
        vec![
            c + Vec2::new(-h, -h * 0.8 * s),
            c + Vec2::new(h, -h * 0.8 * s),
            c + Vec2::new(0.0, h * 0.9 * s),
        ],
        colour,
        Stroke::NONE,
    ));
}

/// The controls' pill, at the bottom right under the mouse.
pub fn strip(p: &Painter, r: Rect, t: &Tokens) {
    let rad = CornerRadius::same((r.height() / 2.0) as u8);
    p.rect_filled(
        r.translate(Vec2::new(0.0, 2.0)).expand(1.0),
        rad,
        Color32::from_black_alpha(if t.dark { 60 } else { 22 }),
    );
    p.rect_filled(r, rad, t.sheet);
    p.rect_stroke(r, rad, Stroke::new(1.0, t.edge), StrokeKind::Inside);
}

/// What a button in the strip shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Settings,
    /// Stay on top: a ring, with a dot when on.
    OnTop(bool),
    /// Grow into the dashboard (`false`), or back to the meter (`true`).
    Expand(bool),
    Minimise,
    Close,
}

pub fn icon_button(ui: &mut Ui, r: Rect, id: Id, t: &Tokens, icon: Icon, tip: &str) -> Response {
    let resp = ui.interact(r, id, Sense::click()).on_hover_text(tip);
    let p = ui.painter();
    let close = icon == Icon::Close;
    if resp.hovered() {
        let bg = if close {
            hex(0xd9443a)
        } else {
            t.ink.gamma_multiply(0.10)
        };
        p.circle_filled(r.center(), r.height() / 2.0 - 1.0, bg);
    }
    let c = if close && resp.hovered() {
        Color32::WHITE
    } else if resp.hovered() {
        t.ink
    } else {
        t.ink2
    };
    let s = Stroke::new(1.4, c);
    let m = r.center();
    match icon {
        Icon::Settings => {
            p.circle_stroke(m, 3.2, s);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let d = Vec2::angled(a);
                p.line_segment([m + d * 5.0, m + d * 7.0], Stroke::new(1.8, c));
            }
        }
        Icon::OnTop(on) => {
            p.circle_stroke(m, 6.0, s);
            if on {
                p.circle_filled(m, 2.8, t.pen);
            } else {
                p.circle_stroke(m, 2.2, Stroke::new(1.2, c));
            }
        }
        Icon::Expand(expanded) => {
            // Two arrows to the corners: out to grow, in to shrink.
            for d in [Vec2::new(1.0, -1.0), Vec2::new(-1.0, 1.0)] {
                let (tip, tail, k) = if expanded {
                    (m + d * 1.5, m + d * 6.0, 1.0)
                } else {
                    (m + d * 6.0, m + d * 1.5, -1.0)
                };
                p.line_segment([tail, tip], s);
                p.line_segment([tip, tip + Vec2::new(d.x * 3.0 * k, 0.0)], s);
                p.line_segment([tip, tip + Vec2::new(0.0, d.y * 3.0 * k)], s);
            }
        }
        Icon::Minimise => {
            p.line_segment([m + Vec2::new(-5.0, 0.5), m + Vec2::new(5.0, 0.5)], s);
        }
        Icon::Close => {
            p.line_segment([m + Vec2::new(-4.5, -4.5), m + Vec2::new(4.5, 4.5)], s);
            p.line_segment([m + Vec2::new(4.5, -4.5), m + Vec2::new(-4.5, 4.5)], s);
        }
    }
    resp
}

/// A row of colour swatches; returns the one clicked.
pub fn swatches(ui: &mut Ui, t: &Tokens, look: &Look, current: Colour) -> Option<Colour> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for c in Colour::ALL {
            let (r, resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
            let resp = resp.on_hover_text(c.name());
            let p = ui.painter();
            let col = t.colour(look, c);
            p.circle_filled(r.center(), 8.0, col);
            if c == current {
                p.circle_stroke(r.center(), 10.5, Stroke::new(2.0, t.ink));
            } else if resp.hovered() {
                p.circle_stroke(r.center(), 10.5, Stroke::new(1.0, t.ink2));
            }
            if resp.clicked() {
                picked = Some(c);
            }
        }
    });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_users_choice() {
        let l = Look::default();
        assert_eq!(l.down, Colour::Ok);
        assert_eq!(l.up, Colour::Pen2);
        let t = Tokens::new(&l, false);
        assert_eq!(t.down, hex(0x2e8b57));
        assert_eq!(t.up, hex(0xc8701e));
        let t = Tokens::new(&l, true);
        assert_eq!(t.down, hex(0x6fd49a));
        assert_eq!(t.up, hex(0xffb35c));
    }

    #[test]
    fn builds_and_opacity() {
        let l = Look {
            build: Build::Smoked,
            opacity: 0.0,
            ..Look::default()
        };
        let t = Tokens::new(&l, false);
        assert!(t.dark);
        assert_eq!(t.paper.a(), (OPACITY_MIN * 255.0).round() as u8);
        assert!(!Tokens::new(&Look::default(), false).dark);
        assert!(Tokens::new(&Look::default(), true).dark);
    }
}
