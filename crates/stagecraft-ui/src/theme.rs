//! Design tokens (the Craft suite look): slate chrome, a near-black navy model space, one
//! blue accent; a light variant with light grey chrome and a light canvas. Every widget,
//! the plan canvas and the 3D view read their colours from [`Tokens`]; egui's own widgets
//! get them through the styles set in [`apply`]. Which theme is live follows macOS unless
//! the user picks one (`ui.set_theme`).
//!
//! The chrome colours, sizes and the 28 pt title band follow CADCraft's `theme.rs` and
//! `chrome.rs` (storytold/cadcraft, MIT OR Apache-2.0), so StageCraft sits in the same suite.

use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Theme, ThemePreference};
use stagecraft_geom::{FillClass, LineClass};

use crate::state::ThemePref;

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn rgba(hex: u32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, a)
}

/// Everything the window paints with, for one theme.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
    /// Title row, tool bar, status bar.
    pub bar: Color32,
    /// Docked panels.
    pub panel: Color32,
    /// Insets on panels: section headers, list backgrounds, the segmented track.
    pub panel_dark: Color32,
    /// Lines between chrome areas and around controls (1 pt).
    pub separator: Color32,
    pub text: Color32,
    /// Labels, inactive tabs, hints.
    pub text2: Color32,
    /// Placeholders, disabled text.
    pub text3: Color32,
    pub icon: Color32,
    /// Text fields.
    pub field: Color32,
    /// Buttons and drop-downs at rest.
    pub control: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    /// The active tab, the chosen segment.
    pub tab_active: Color32,
    pub accent: Color32,
    pub on_accent: Color32,
    /// Selected list rows and text.
    pub select_bg: Color32,
    /// Status bar toggles that are on.
    pub toggle_on: Color32,
    /// The floating command bar.
    pub cmd_bg: Color32,
    pub cmd_border: Color32,
    pub cmd_history: Color32,
    /// Autocomplete list.
    pub list_bg: Color32,
    pub list_row: Color32,
    /// Patch conflicts and other problems.
    pub warning: Color32,
    /// Around the paper in the Sheets mode.
    pub desk: Color32,
    /// The axis triad and the plan's axis icon: x, y, z.
    pub axes: [Color32; 3],
    pub plan: Palette,
    pub scene: Palette3d,
}

impl Tokens {
    pub const DARK: Tokens = Tokens {
        dark: true,
        bar: rgb(0x3b4453),
        panel: rgb(0x3c4452),
        panel_dark: rgb(0x333a47),
        separator: rgb(0x232930),
        text: rgb(0xd7dbe0),
        text2: rgb(0xa3aab4),
        text3: rgb(0x7e8590),
        icon: rgb(0xcfd4db),
        field: rgb(0x2e3540),
        control: rgb(0x4b5567),
        hover: rgb(0x5c677c),
        pressed: rgb(0x66728a),
        tab_active: rgb(0x4e5a6e),
        accent: rgb(0x3d8bfd),
        on_accent: Color32::WHITE,
        select_bg: rgb(0x2f5e9e),
        toggle_on: rgb(0x3d8bfd),
        cmd_bg: rgba(0x343c4a, 0xf2),
        cmd_border: rgb(0x55606f),
        cmd_history: rgba(0x2a303a, 0xc8),
        list_bg: rgb(0x2b313b),
        list_row: rgb(0x3a4250),
        warning: rgb(0xff6a52),
        desk: rgb(0x2b313a),
        axes: [rgb(0xe0605a), rgb(0x5cc46e), rgb(0x5a9bff)],
        plan: Palette {
            canvas: rgb(0x22282f),
            grid_minor: rgb(0x272e37),
            grid_major: rgb(0x303844),
            axis_x: rgb(0x5a2c2e),
            axis_y: rgb(0x2a4c31),
            line: rgb(0xd7dbe0),
            text: rgb(0xe4e7eb),
            floor: rgb(0x2a313b),
            wall: rgb(0x8a94a3),
            body: rgb(0x38414e),
            masking: rgb(0x58616f),
            deck: rgb(0x313945),
            selected: rgb(0x4a8fff),
            hover: rgb(0x9cc6ff),
            preview: rgb(0xffa83a),
            snap: rgb(0x2fd062),
            box_window: rgba(0x4a8fff, 40),
            box_crossing: rgba(0x2fd062, 40),
            sketch: rgb(0xc8d2e2),
            constrained: rgb(0x4cd07d),
            conflict: rgb(0xff5a4a),
            dim: rgb(0x55c8e0),
            dim_reference: rgb(0x98a1ae),
            ink: rgb(0xa3aab4),
        },
        scene: Palette3d {
            background: rgb(0x22282f),
            generic: rgb(0xa0a6b0),
            floor: rgb(0x6a6358),
            wall: rgb(0x9a9ea6),
            metal: rgb(0xc4c8ce),
            fixture: rgb(0x4a4f58),
            lens: rgb(0xffe4a8),
            curtain: rgb(0x8b1a1a),
            masking: rgb(0x2a2b2e),
            deck: rgb(0x3a3836),
            cut: rgb(0xd25a4a),
            hatch: rgb(0x7a2c22),
            grid_minor: rgba(0x5a6676, 50),
            grid_major: rgba(0x7a879a, 110),
            selected: rgb(0x4a8fff),
        },
    };

    pub const LIGHT: Tokens = Tokens {
        dark: false,
        bar: rgb(0xe4e7eb),
        panel: rgb(0xeef0f3),
        panel_dark: rgb(0xe2e5ea),
        separator: rgb(0xc3c9d1),
        text: rgb(0x1d232b),
        text2: rgb(0x4f5864),
        text3: rgb(0x7d8591),
        icon: rgb(0x343c48),
        field: rgb(0xffffff),
        control: rgb(0xdce0e6),
        hover: rgb(0xcbd2dc),
        pressed: rgb(0xbac3cf),
        tab_active: rgb(0xd3dbe6),
        accent: rgb(0x2f7cf6),
        on_accent: Color32::WHITE,
        select_bg: rgb(0xc4dafb),
        toggle_on: rgb(0x2f7cf6),
        cmd_bg: rgba(0xf9fafb, 0xf2),
        cmd_border: rgb(0xa9b0ba),
        cmd_history: rgba(0xf0f2f5, 0xdc),
        list_bg: rgb(0xfbfcfd),
        list_row: rgb(0xe3e8ef),
        warning: rgb(0xd5381f),
        desk: rgb(0xd5d9df),
        axes: [rgb(0xd03a32), rgb(0x2a9a40), rgb(0x2f6fe0)],
        plan: Palette {
            canvas: rgb(0xfbfbfc),
            grid_minor: rgb(0xf0f1f4),
            grid_major: rgb(0xdfe2e7),
            axis_x: rgb(0xe3a8a8),
            axis_y: rgb(0x9ccfa5),
            line: rgb(0x1e2228),
            text: rgb(0x1e2228),
            floor: rgb(0xeef0f2),
            wall: rgb(0x555b64),
            body: rgb(0xf6f7f8),
            masking: rgb(0x3c4048),
            deck: rgb(0xe4e6ea),
            selected: rgb(0x2f7cf6),
            hover: rgb(0x86b3fa),
            preview: rgb(0xe07a00),
            snap: rgb(0x12a15c),
            box_window: rgba(0x2f7cf6, 30),
            box_crossing: rgba(0x12a15c, 30),
            sketch: rgb(0x3a4860),
            constrained: rgb(0x129a4a),
            conflict: rgb(0xd92d20),
            dim: rgb(0x0f7f9c),
            dim_reference: rgb(0x6e737d),
            ink: rgb(0x47505c),
        },
        scene: Palette3d {
            background: rgb(0xeef0f3),
            generic: rgb(0xb4b8bf),
            floor: rgb(0xc9bfae),
            wall: rgb(0xe4e2dd),
            metal: rgb(0x9ca2ab),
            fixture: rgb(0x3a3d44),
            lens: rgb(0xffd880),
            curtain: rgb(0x8b1a1a),
            masking: rgb(0x262628),
            deck: rgb(0x44413e),
            cut: rgb(0xc84b3a),
            hatch: rgb(0xf3c2b8),
            grid_minor: rgba(0xa8adb6, 55),
            grid_major: rgba(0x888d96, 120),
            selected: rgb(0x2f7cf6),
        },
    };

    pub fn of(dark: bool) -> &'static Tokens {
        if dark { &Tokens::DARK } else { &Tokens::LIGHT }
    }
}

/// The tokens of the theme `ui` shows.
pub fn tokens(dark: bool) -> Tokens {
    *Tokens::of(dark)
}

/// The tokens of the theme `ui` shows.
pub fn of(ui: &egui::Ui) -> Tokens {
    tokens(ui.visuals().dark_mode)
}

/// Colours of the plan canvas.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub canvas: Color32,
    pub grid_minor: Color32,
    pub grid_major: Color32,
    /// The setting line (x axis, y = 0).
    pub axis_x: Color32,
    /// The centre line (y axis, x = 0).
    pub axis_y: Color32,
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
    /// Labels drawn straight onto the canvas (axis icon, view name).
    pub ink: Color32,
}

/// Colours of the 3D view.
#[derive(Clone, Copy, Debug)]
pub struct Palette3d {
    pub background: Color32,
    pub generic: Color32,
    pub floor: Color32,
    pub wall: Color32,
    pub metal: Color32,
    pub fixture: Color32,
    pub lens: Color32,
    pub curtain: Color32,
    pub masking: Color32,
    pub deck: Color32,
    pub cut: Color32,
    pub hatch: Color32,
    pub grid_minor: Color32,
    pub grid_major: Color32,
    pub selected: Color32,
}

pub fn palette(dark: bool) -> Palette {
    Tokens::of(dark).plan
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

/// Text sizes of the chrome.
pub const BODY: f32 = 11.5;
pub const SMALL: f32 = 10.5;

pub fn proportional(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Inter SemiBold: panel titles, section headers, the active tab.
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// Inter Medium.
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

pub fn body() -> FontId {
    proportional(BODY)
}

pub fn small() -> FontId {
    proportional(SMALL)
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
    add("Inter", include_bytes!("../../../assets/fonts/Inter-Regular.ttf"));
    add("Inter Medium", include_bytes!("../../../assets/fonts/Inter-Medium.ttf"));
    add("Inter SemiBold", include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"));
    add("Roboto Mono", include_bytes!("../../../assets/fonts/RobotoMono-Regular.ttf"));
    add("DejaVu Symbols", include_bytes!("../../../assets/fonts/DejaVuSans-Symbols.ttf"));
    // Ours first, then the DejaVu cut for arrows and key symbols (⌘ ⇧ ⌥) the faces may
    // lack; egui's own stay behind them.
    for (family, face) in [(FontFamily::Proportional, "Inter"), (FontFamily::Monospace, "Roboto Mono")] {
        let list = fonts.families.get_mut(&family).expect("egui has it");
        list.insert(0, face.into());
        list.insert(1, "DejaVu Symbols".into());
    }
    for (family, face) in [("bold", "Inter SemiBold"), ("medium", "Inter Medium")] {
        let mut list = vec![face.to_string()];
        list.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
        fonts.families.insert(FontFamily::Name(family.into()), list);
    }
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
    let medium = FontFamily::Name("medium".into());
    if ctx.fonts(|f| f.definitions().families.contains_key(&medium)) {
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
    let c = tokens(dark);
    style.visuals = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    let v = &mut style.visuals;
    v.panel_fill = c.panel;
    v.window_fill = c.bar;
    v.window_stroke = Stroke::new(1.0, c.separator);
    v.extreme_bg_color = c.field;
    v.text_edit_bg_color = Some(c.field);
    v.faint_bg_color = c.panel_dark;
    v.code_bg_color = c.field;
    v.selection.bg_fill = c.select_bg;
    v.selection.stroke = Stroke::new(1.0, c.text);
    v.hyperlink_color = c.accent;
    v.weak_text_color = Some(c.text2);
    v.error_fg_color = c.warning;
    v.warn_fg_color = if dark { rgb(0xffd080) } else { rgb(0x8a5300) };
    v.override_text_color = None;
    v.window_corner_radius = egui::CornerRadius::same(6);
    v.menu_corner_radius = egui::CornerRadius::same(5);
    let shadow = Color32::from_black_alpha(if dark { 90 } else { 40 });
    v.window_shadow = egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: shadow };
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 12, spread: 0, color: shadow };
    craft_ui::theme::WidgetPalette {
        noninteractive: c.panel,
        inactive: c.control,
        hovered: c.hover,
        active: c.pressed,
        open: c.hover,
    }
    .apply_to(&mut v.widgets, egui::CornerRadius::same(3), Stroke::new(1.0, c.text));
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, c.separator);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, c.text);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, c.text);
    let border = if dark { Stroke::NONE } else { Stroke::new(1.0, c.separator) };
    v.widgets.inactive.bg_stroke = border;
    v.widgets.hovered.bg_stroke = border;
    v.widgets.active.bg_stroke = Stroke::new(1.0, c.accent);
    v.widgets.open.bg_stroke = border;
    for w in [&mut v.widgets.hovered, &mut v.widgets.active] {
        w.expansion = 0.0;
    }
    v.striped = false;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.interact_cursor = None;
    v.collapsing_header_frame = false;
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(6.0, 2.0);
    style.spacing.interact_size = egui::vec2(32.0, 19.0);
    style.spacing.menu_margin = egui::Margin::same(4);
    style.spacing.window_margin = egui::Margin::same(8);
    style.spacing.combo_width = 120.0;
    style.spacing.icon_width = 13.0;
    style.spacing.icon_width_inner = 7.0;
    style.spacing.icon_spacing = 5.0;
    style.spacing.scroll = egui::style::ScrollStyle::thin();
    style.text_styles = [
        (TextStyle::Small, proportional(SMALL)),
        (TextStyle::Body, proportional(BODY)),
        (TextStyle::Button, proportional(BODY)),
        (TextStyle::Heading, bold(13.0)),
        (TextStyle::Monospace, FontId::new(11.0, FontFamily::Monospace)),
    ]
    .into();
}

/// Colours and sizes of the docking chrome: tab strips in the bar colour, the active tab
/// raised, 1 pt separator gaps between groups.
pub fn dock_style(style: &mut craft_ui::docking::DockStyle, dark: bool) {
    let c = tokens(dark);
    style.accent = c.accent;
    style.tab_height = 26.0;
    style.gap = 1.0;
    style.background = c.separator;
    style.tab_background = c.bar;
    style.active_background = c.tab_active;
    style.text = c.text;
    style.inactive_text = c.text2;
    style.border = Stroke::NONE;
    style.font = proportional(BODY);
    style.floating_background = c.panel;
    style.floating_stroke = Stroke::new(1.0, c.separator);
    style.title_height = 24.0;
    style.column_bar_height = 0.0;
    style.icon_size = 16.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Color32) -> f32 {
        (0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b())) / 255.0
    }

    #[test]
    fn text_reads_on_every_surface() {
        for t in [Tokens::DARK, Tokens::LIGHT] {
            for bg in [t.bar, t.panel, t.field, t.control, t.plan.canvas] {
                let d = (luminance(t.text) - luminance(bg)).abs();
                assert!(d > 0.45, "dark {}: text on {bg:?} only {d}", t.dark);
                let d2 = (luminance(t.text2) - luminance(bg)).abs();
                assert!(d2 > 0.25, "dark {}: dim text on {bg:?} only {d2}", t.dark);
            }
            assert_eq!(luminance(t.plan.canvas) < 0.5, t.dark, "the canvas follows the theme");
        }
    }
}
