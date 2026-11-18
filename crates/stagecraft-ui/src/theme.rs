//! The look: Roboto, quiet panels, one accent colour, a light and a dark palette.
//! The plan canvas reads its colours from [`Palette`]; egui widgets from the styles set
//! in [`apply`]. Which one is live follows macOS unless the user picks one.

use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Theme, ThemePreference};
use stagecraft_geom::{FillClass, LineClass};

use crate::state::ThemePref;

/// The one accent (Style A · Studio).
pub const ACCENT: Color32 = Color32::from_rgb(0x3d, 0x7b, 0xfd);

/// Colours of the window chrome around the canvas (Style A · Studio): neutral greys, one
/// blue accent. The light variant is the same neutral greys turned over, not warm paper.
#[derive(Clone, Copy, Debug)]
pub struct Chrome {
    /// Title/tool bar.
    pub bar: Color32,
    /// Panels and docks.
    pub panel: Color32,
    /// The drawing area around the plan.
    pub canvas: Color32,
    pub separator: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub icon: Color32,
    /// Text fields, combo boxes, buttons at rest.
    pub field: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    /// The track of segmented controls.
    pub segment: Color32,
    /// The chosen segment (a raised panel colour).
    pub segment_on: Color32,
    pub status: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
}

pub fn chrome(dark: bool) -> Chrome {
    let c = |hex: u32| Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    if dark {
        Chrome {
            bar: c(0x2a2b30),
            panel: c(0x232428),
            canvas: c(0x1b1c1f),
            separator: c(0x34363b),
            text: c(0xe6e6e8),
            text2: c(0x8d9097),
            icon: c(0xc9cbd0),
            field: c(0x2e3035),
            hover: c(0x383a40),
            pressed: c(0x42444b),
            segment: c(0x1b1c1f),
            segment_on: c(0x3a3c42),
            status: c(0x1f2023),
            accent: ACCENT,
            on_accent: Color32::WHITE,
        }
    } else {
        Chrome {
            bar: c(0xececee),
            panel: c(0xf6f6f7),
            canvas: c(0xffffff),
            separator: c(0xdadbde),
            text: c(0x1c1d20),
            text2: c(0x6d7078),
            icon: c(0x3c3e44),
            field: c(0xffffff),
            hover: c(0xe2e3e6),
            pressed: c(0xd6d7db),
            segment: c(0xdfe0e3),
            segment_on: c(0xffffff),
            status: c(0xefeff1),
            accent: ACCENT,
            on_accent: Color32::WHITE,
        }
    }
}
/// Patch conflicts and other problems, readable on light and dark.
pub const WARNING: Color32 = Color32::from_rgb(0xe0, 0x4a, 0x2f);

/// Colours of the plan canvas.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub canvas: Color32,
    pub grid_minor: Color32,
    pub grid_major: Color32,
    pub axis: Color32,
    pub line: Color32,
    pub text: Color32,
    pub floor: Color32,
    pub wall: Color32,
    pub body: Color32,
    pub masking: Color32,
    pub deck: Color32,
    pub selected: Color32,
    pub hover: Color32,
    pub preview: Color32,
    pub snap: Color32,
    pub box_window: Color32,
    pub box_crossing: Color32,
    /// Sketch lines and points that can still move.
    pub sketch: Color32,
    /// Fully constrained sketch geometry (FreeCAD green).
    pub constrained: Color32,
    /// Conflicting constraints (red).
    pub conflict: Color32,
    /// Driving dimensions.
    pub dim: Color32,
    /// Reference dimensions.
    pub dim_reference: Color32,
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            canvas: Color32::from_rgb(0x1b, 0x1c, 0x1f),
            grid_minor: Color32::from_rgb(0x25, 0x26, 0x2a),
            grid_major: Color32::from_rgb(0x31, 0x33, 0x38),
            axis: Color32::from_rgb(0x4a, 0x4d, 0x55),
            line: Color32::from_rgb(0xd8, 0xda, 0xde),
            text: Color32::from_rgb(0xe8, 0xe8, 0xea),
            floor: Color32::from_rgb(0x2a, 0x2e, 0x33),
            wall: Color32::from_rgb(0x8a, 0x8e, 0x96),
            body: Color32::from_rgb(0x3a, 0x3d, 0x44),
            masking: Color32::from_rgb(0x5c, 0x5f, 0x66),
            deck: Color32::from_rgb(0x30, 0x33, 0x38),
            selected: Color32::from_rgb(0x4d, 0x97, 0xff),
            hover: Color32::from_rgb(0x9c, 0xc3, 0xff),
            preview: Color32::from_rgb(0xff, 0xa8, 0x3a),
            snap: Color32::from_rgb(0x3d, 0xd6, 0x8c),
            box_window: Color32::from_rgba_unmultiplied(0x4d, 0x97, 0xff, 40),
            box_crossing: Color32::from_rgba_unmultiplied(0x3d, 0xd6, 0x8c, 40),
            sketch: Color32::from_rgb(0xc8, 0xd2, 0xe2),
            constrained: Color32::from_rgb(0x4c, 0xd0, 0x7d),
            conflict: Color32::from_rgb(0xff, 0x5a, 0x4a),
            dim: Color32::from_rgb(0x8a, 0xb6, 0xff),
            dim_reference: Color32::from_rgb(0x9a, 0x9e, 0xa8),
        }
    } else {
        Palette {
            canvas: Color32::from_rgb(0xff, 0xff, 0xff),
            grid_minor: Color32::from_rgb(0xf0, 0xf1, 0xf3),
            grid_major: Color32::from_rgb(0xdf, 0xe1, 0xe5),
            axis: Color32::from_rgb(0xb8, 0xbc, 0xc4),
            line: Color32::from_rgb(0x1e, 0x20, 0x24),
            text: Color32::from_rgb(0x1e, 0x20, 0x24),
            floor: Color32::from_rgb(0xee, 0xec, 0xe6),
            wall: Color32::from_rgb(0x55, 0x59, 0x60),
            body: Color32::from_rgb(0xf6, 0xf6, 0xf8),
            masking: Color32::from_rgb(0x3c, 0x3e, 0x44),
            deck: Color32::from_rgb(0xe6, 0xe2, 0xda),
            selected: ACCENT,
            hover: Color32::from_rgb(0x86, 0xb3, 0xfa),
            preview: Color32::from_rgb(0xe0, 0x7a, 0x00),
            snap: Color32::from_rgb(0x12, 0xa1, 0x5c),
            box_window: Color32::from_rgba_unmultiplied(0x2f, 0x7c, 0xf6, 30),
            box_crossing: Color32::from_rgba_unmultiplied(0x12, 0xa1, 0x5c, 30),
            sketch: Color32::from_rgb(0x3a, 0x48, 0x60),
            constrained: Color32::from_rgb(0x12, 0x9a, 0x4a),
            conflict: Color32::from_rgb(0xd9, 0x2d, 0x20),
            dim: Color32::from_rgb(0x1f, 0x5f, 0xc8),
            dim_reference: Color32::from_rgb(0x6e, 0x73, 0x7d),
        }
    }
}

impl Palette {
    pub fn fill(&self, f: FillClass) -> Color32 {
        match f {
            FillClass::Floor => self.floor,
            FillClass::Wall => self.wall,
            FillClass::Body => self.body,
            FillClass::Masking => self.masking,
            FillClass::Deck => self.deck,
        }
    }
}

/// Screen width of a line class, points.
pub fn line_width(c: LineClass) -> f32 {
    match c {
        LineClass::Thin => 0.6,
        LineClass::Medium => 1.0,
        LineClass::Thick => 1.8,
        LineClass::Hidden => 0.8,
        LineClass::Centre => 0.7,
    }
}

pub fn proportional(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// The sheet font (Arimo), for drawing sheets like the PDF.
pub fn sheet_font(size: f32, bold: bool) -> FontId {
    FontId::new(size, FontFamily::Name(if bold { "sheet_bold" } else { "sheet" }.into()))
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.into(), std::sync::Arc::new(FontData::from_static(bytes)));
    };
    add("Roboto", include_bytes!("../../../assets/fonts/Roboto-Regular.ttf"));
    add("Roboto Bold", include_bytes!("../../../assets/fonts/Roboto-Bold.ttf"));
    add("Roboto Mono", include_bytes!("../../../assets/fonts/RobotoMono-Regular.ttf"));
    add("DejaVu Symbols", include_bytes!("../../../assets/fonts/DejaVuSans-Symbols.ttf"));
    // Ours first, then the DejaVu cut for arrows and key symbols (⌘ ⇧ ⌥) that Roboto
    // lacks; egui's own stay behind them.
    for (family, face) in [(FontFamily::Proportional, "Roboto"), (FontFamily::Monospace, "Roboto Mono")] {
        let list = fonts.families.get_mut(&family).expect("egui has it");
        list.insert(0, face.into());
        list.insert(1, "DejaVu Symbols".into());
    }
    let mut bold = vec!["Roboto Bold".to_string()];
    bold.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts.families.insert(FontFamily::Name("bold".into()), bold);
    add("Arimo", include_bytes!("../../../assets/fonts/Arimo-Regular.ttf"));
    add("Arimo Bold", include_bytes!("../../../assets/fonts/Arimo-Bold.ttf"));
    for (family, face) in [("sheet", "Arimo"), ("sheet_bold", "Arimo Bold")] {
        let mut list = vec![face.to_string()];
        list.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
        fonts.families.insert(FontFamily::Name(family.into()), list);
    }
    fonts
}

/// Whether fonts and styles are in place. Fonts take effect from the frame after they
/// are set, so the first frame only sets them and asks for another.
pub fn ready(ctx: &egui::Context) -> bool {
    let bold = FontFamily::Name("bold".into());
    if ctx.fonts(|f| f.definitions().families.contains_key(&bold)) {
        return true;
    }
    apply(ctx);
    ctx.request_repaint();
    false
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.style_mut_of(Theme::Light, |s| style(s, false));
    ctx.style_mut_of(Theme::Dark, |s| style(s, true));
}

pub fn set_preference(ctx: &egui::Context, pref: ThemePref) {
    let want = match pref {
        ThemePref::System => ThemePreference::System,
        ThemePref::Light => ThemePreference::Light,
        ThemePref::Dark => ThemePreference::Dark,
    };
    if ctx.options(|o| o.theme_preference) != want {
        ctx.set_theme(want);
    }
}

fn style(style: &mut egui::Style, dark: bool) {
    style.visuals = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    let v = &mut style.visuals;
    if dark {
        v.panel_fill = Color32::from_rgb(0x24, 0x25, 0x29);
        v.window_fill = Color32::from_rgb(0x2b, 0x2c, 0x31);
        v.extreme_bg_color = Color32::from_rgb(0x18, 0x19, 0x1c);
        v.faint_bg_color = Color32::from_rgb(0x2a, 0x2b, 0x30);
    } else {
        v.panel_fill = Color32::from_rgb(0xf5, 0xf5, 0xf6);
        v.window_fill = Color32::from_rgb(0xff, 0xff, 0xff);
        v.extreme_bg_color = Color32::from_rgb(0xff, 0xff, 0xff);
        v.faint_bg_color = Color32::from_rgb(0xee, 0xef, 0xf1);
    }
    v.selection.bg_fill = if dark { Color32::from_rgb(0x2c, 0x4f, 0x86) } else { Color32::from_rgb(0xcf, 0xe0, 0xfd) };
    v.selection.stroke = Stroke::new(1.0, if dark { Color32::WHITE } else { Color32::from_rgb(0x10, 0x3a, 0x80) });
    v.hyperlink_color = ACCENT;
    let r = egui::CornerRadius::same(4);
    v.window_corner_radius = egui::CornerRadius::same(8);
    v.menu_corner_radius = egui::CornerRadius::same(6);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 3.0);
    style.spacing.interact_size.y = 22.0;
    style.text_styles = [
        (TextStyle::Small, proportional(11.0)),
        (TextStyle::Body, proportional(13.0)),
        (TextStyle::Button, proportional(13.0)),
        (TextStyle::Heading, bold(15.0)),
        (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
    ]
    .into();
}

/// Colours of the docking chrome (tab strips, borders, floating windows).
pub fn dock_style(style: &mut craft_ui::docking::DockStyle, _dark: bool) {
    style.accent = ACCENT;
}
