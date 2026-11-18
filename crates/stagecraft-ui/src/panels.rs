//! Menu bar, toolbar, status bar, layers and tags panels.

use egui::{RichText, Role, Ui};
use serde_json::{Value, json};
use stagecraft_core::{Id, Layer};

use crate::app::Studio;
use crate::i18n::{t, tf};
use crate::state::{FileRequest, Tool, View};
use crate::widgets::{self, Icon, edit_field, icon_button};

/// Room the macOS window buttons take at the toolbar's left with the inset title bar.
pub const TRAFFIC_LIGHTS: f32 = 72.0;

/// New document; with unsaved changes the app asks first (the answer comes on a later frame).
pub fn new_document(s: &mut Studio) {
    if s.engine.dirty() && s.ui.native_dialogs {
        s.ui.file_request = Some(FileRequest::New);
    } else {
        let _ = s.run("doc.new", json!({ "discard": true }));
    }
}

/// Save to the current file, or ask where.
pub fn save(s: &mut Studio, save_as: bool) {
    if save_as || s.engine.path().is_none() {
        s.ui.file_request = Some(FileRequest::SaveAs);
    } else if s.run("doc.save", Value::Null).is_ok() {
        s.ui.info(t("status.saved"));
    }
}

/// Height of the tool/title bar.
pub const BAR_HEIGHT: f32 = 40.0;
const TOOL_BUTTON: egui::Vec2 = egui::vec2(30.0, 28.0);
const TOOL_ICON: f32 = 18.0;

/// The tool groups of the toolbar, in order; Calibrate only with an underlay.
pub fn tool_groups(s: &Studio) -> Vec<Vec<Tool>> {
    if s.ui.view == View::Sheets {
        return Vec::new();
    }
    let underlays = s.engine.doc().objects.values().any(|o| o.kind.type_name() == "underlay");
    let mut draw = vec![Tool::Floor, Tool::Wall, Tool::Dimension];
    if underlays || s.ui.tool == Tool::Calibrate {
        draw.push(Tool::Calibrate);
    }
    vec![
        vec![Tool::Select, Tool::Move, Tool::Rotate],
        draw,
        vec![Tool::Truss, Tool::Run, Tool::Fixture],
        vec![Tool::Batten, Tool::Curtain, Tool::Proscenium, Tool::Wings],
        vec![Tool::Platform, Tool::Support, Tool::Hoist, Tool::Ceiling],
    ]
}

/// "Wall (W)": what a tool's tooltip says.
pub fn tool_tip(tool: Tool) -> String {
    let label = t(tool.key());
    match tool.shortcut() {
        Some(k) => tf("tool.tip", &[("tool", &label), ("key", &format!("{k:?}"))]),
        None => label.to_owned(),
    }
}

fn icon_style(c: &crate::theme::Chrome) -> craft_ui::buttons::IconButtonStyle {
    use craft_ui::buttons::{ButtonVisuals, IconButtonStyle};
    let v = |fill, icon| ButtonVisuals { fill, stroke: egui::Stroke::NONE, icon };
    IconButtonStyle {
        normal: v(egui::Color32::TRANSPARENT, c.icon),
        hovered: v(c.hover, c.text),
        pressed: Some(v(c.pressed, c.text)),
        selected: v(c.accent, c.on_accent),
        corner_radius: egui::CornerRadius::same(6),
        focus_stroke: egui::Stroke::new(1.5, c.accent),
    }
}

/// One icon button: icon, tooltip "Name (key)", accessible name "Tool Name".
fn tool_button(ui: &mut Ui, s: &mut Studio, tool: Tool, style: &craft_ui::buttons::IconButtonStyle) {
    let label = t(tool.key());
    let a11y = tf("a11y.tool", &[("tool", &label)]);
    let r = ui
        .push_id(("tool", tool.key()), |ui| {
            craft_ui::buttons::IconButton::new(&a11y, TOOL_BUTTON, style)
                .selected(s.ui.tool == tool)
                .show(ui, |ui, rect, colour| {
                    crate::icons::paint(ui, rect, crate::icons::tool_icon(tool), TOOL_ICON, colour)
                })
        })
        .inner;
    if r.on_hover_text(tool_tip(tool)).clicked() {
        let _ = s.run("ui.set_tool", json!({ "tool": tool }));
    }
}

fn group_width(n: usize, spacing: f32) -> f32 {
    n as f32 * TOOL_BUTTON.x + n.saturating_sub(1) as f32 * spacing
}

/// The tool/title bar: tool groups (the ones that do not fit go into a » menu), the view
/// switch and the command palette.
pub fn toolbar(ui: &mut Ui, s: &mut Studio) {
    let c = crate::theme::chrome(ui.visuals().dark_mode);
    let bar = ui.max_rect();
    if s.ui.inset_title_bar {
        // The strip is the window's title bar: drags on its empty parts move the window.
        craft_window::title_bar::empty_area(ui, bar);
    }
    let style = icon_style(&c);
    let groups = tool_groups(s);
    let spacing = 2.0;
    let gap = 13.0;
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = spacing;
        ui.add_space(if s.ui.inset_title_bar { TRAFFIC_LIGHTS } else { 6.0 });
        // The right end: view switch and palette, measured first so the tools know their room.
        let right = 300.0;
        let avail = (ui.available_width() - right).max(0.0);
        let widths: Vec<f32> = groups.iter().map(|g| group_width(g.len(), spacing) + gap).collect();
        let fit = craft_ui::layout::fit_toolbar(&widths, avail, 0.0, TOOL_BUTTON.x + gap).unwrap_or_else(|| {
            craft_ui::layout::ToolbarFit { overflow: (0..groups.len()).collect(), ..Default::default() }
        });
        for (n, (i, _)) in fit.shown.iter().enumerate() {
            if n > 0 {
                separator(ui, &c, gap);
            }
            for &tool in &groups[*i] {
                tool_button(ui, s, tool, &style);
            }
        }
        if !fit.overflow.is_empty() {
            if !fit.shown.is_empty() {
                separator(ui, &c, gap);
            }
            let more: Vec<Tool> = fit.overflow.iter().flat_map(|i| groups[*i].iter().copied()).collect();
            overflow_menu(ui, s, &more, &style);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            let search = t("palette.search_tip");
            let r = craft_ui::buttons::IconButton::new("Command palette", TOOL_BUTTON, &style)
                .show(ui, |ui, rect, colour| crate::icons::paint(ui, rect, "search", 17.0, colour));
            if r.on_hover_text(search).clicked() {
                s.ui.palette.open();
            }
            ui.add_space(6.0);
            let views = [
                (View::Plan, "view.plan"),
                (View::ThreeD, "view.3d"),
                (View::Split, "view.split"),
                (View::Sheets, "view.sheets"),
            ];
            let items: Vec<(String, String, bool)> = views
                .iter()
                .map(|(v, key)| {
                    let label = t(key).to_owned();
                    (label.clone(), tf("a11y.view", &[("view", &label)]), s.ui.view == *v)
                })
                .collect();
            if let Some(i) = crate::widgets::segmented(ui, &c, &items) {
                let _ = s.run("ui.set_view", json!({ "view": views[i].0 }));
            }
        });
    });
}

fn separator(ui: &mut Ui, c: &crate::theme::Chrome, gap: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(gap, 20.0), egui::Sense::hover());
    ui.painter().vline(rect.center().x.round() + 0.5, rect.y_range(), egui::Stroke::new(1.0, c.separator));
}

/// The tools that did not fit, as a menu of icons with names.
fn overflow_menu(ui: &mut Ui, s: &mut Studio, tools: &[Tool], style: &craft_ui::buttons::IconButtonStyle) {
    let r = craft_ui::buttons::IconButton::new("More tools", TOOL_BUTTON, style).show(ui, |ui, rect, colour| {
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "»", egui::FontId::proportional(16.0), colour);
    });
    let r = r.on_hover_text(t("toolbar.more"));
    egui::Popup::menu(&r).show(|ui| {
        for &tool in tools {
            let label = t(tool.key());
            let text = match tool.shortcut() {
                Some(k) => format!("{label}    {k:?}"),
                None => label.to_owned(),
            };
            let b = ui.add(egui::Button::selectable(s.ui.tool == tool, text));
            crate::widgets::name(&b, Role::Button, format!("Menu tool {label}"));
            if b.clicked() {
                let _ = s.run("ui.set_tool", json!({ "tool": tool }));
                ui.close();
            }
        }
    });
}

/// The row under the tool bar: the active tool's options and what it expects next.
pub fn options_bar(ui: &mut Ui, s: &mut Studio) {
    ui.horizontal_centered(|ui| {
        ui.add_space(4.0);
        if s.ui.view == View::Sheets {
            ui.label(RichText::new(t("hint.sheets")).color(crate::theme::chrome(ui.visuals().dark_mode).text2));
        } else {
            tool_options(ui, s);
        }
    });
}

fn tool_options(ui: &mut Ui, s: &mut Studio) {
    let o = s.ui.options.clone();
    let profile_box = |ui: &mut Ui, s: &mut Studio| {
        ui.label(t("option.profile"));
        let r = egui::ComboBox::from_id_salt("truss_profile").selected_text(&o.truss_profile).show_ui(ui, |ui| {
            for p in stagecraft_geom::truss_profiles() {
                let label = format!("{}  {} · {} mm", p.name, p.maker, p.width);
                if ui.selectable_label(p.name == o.truss_profile, label).clicked() {
                    let _ = s.run("ui.set_tool_options", json!({ "truss_profile": p.name }));
                }
            }
        });
        widgets::name(&r.response, egui::Role::ComboBox, format!("Option {}", t("option.profile")));
    };
    match s.ui.tool {
        Tool::Truss | Tool::Run => {
            profile_box(ui, s);
            mm_option(ui, s, "option.height", "truss_height", o.truss_height);
        }
        Tool::Wall => {
            mm_option(ui, s, "option.height", "wall_height", o.wall_height);
            mm_option(ui, s, "option.thickness", "wall_thickness", o.wall_thickness);
            mm_option(ui, s, "option.base", "wall_base", o.wall_base);
        }
        Tool::Floor => {
            mm_option(ui, s, "option.top_height", "floor_z", o.floor_z);
            mm_option(ui, s, "option.thickness", "floor_thickness", o.floor_thickness);
        }
        Tool::Batten => {
            mm_option(ui, s, "option.trim", "batten_trim", o.batten_trim);
            mm_option(ui, s, "option.length", "batten_length", o.batten_length);
        }
        Tool::Curtain => {
            mm_option(ui, s, "option.height", "curtain_height", o.curtain_height);
            mm_option(ui, s, "option.trim", "curtain_trim", o.curtain_trim);
        }
        Tool::Proscenium => {
            mm_option(ui, s, "option.width", "proscenium_width", o.proscenium_width);
            mm_option(ui, s, "option.height", "proscenium_height", o.proscenium_height);
        }
        Tool::Wings => {
            ui.label(t("option.count"));
            let id = egui::Id::unique(("tool_option", "wing_count"));
            if let Some(v) = edit_field(ui, id, &o.wing_count.to_string(), 32.0, t("option.count"))
                && let Ok(n) = v.trim().parse::<u32>()
            {
                let _ = s.run("ui.set_tool_options", json!({ "wing_count": n }));
            }
            mm_option(ui, s, "option.spacing", "wing_spacing", o.wing_spacing);
            mm_option(ui, s, "option.opening", "wing_opening", o.wing_opening);
        }
        Tool::Platform => {
            ui.label(t("option.size"));
            let r =
                egui::ComboBox::from_id_salt("platform_preset").selected_text(&o.platform_preset).show_ui(ui, |ui| {
                    for p in ["2x1", "1x1", "2x0.5", "1x0.5"] {
                        if ui.selectable_label(o.platform_preset == p, format!("{p} m")).clicked() {
                            let _ = s.run("ui.set_tool_options", json!({ "platform_preset": p }));
                        }
                    }
                });
            widgets::name(&r.response, egui::Role::ComboBox, format!("Option {}", t("option.size")));
            mm_option(ui, s, "option.height", "platform_height", o.platform_height);
            ui.label(t("option.system"));
            let shown =
                if o.platform_system.is_empty() { t("option.none").to_owned() } else { o.platform_system.clone() };
            let r = egui::ComboBox::from_id_salt("platform_system").selected_text(shown).show_ui(ui, |ui| {
                if ui.selectable_label(o.platform_system.is_empty(), t("option.none")).clicked() {
                    let _ = s.run("ui.set_tool_options", json!({ "platform_system": "" }));
                }
                for sys in stagecraft_geom::platform_systems() {
                    let label = format!("{} · {}–{} mm", sys.name, sys.min_height, sys.max_height);
                    if ui.selectable_label(o.platform_system == sys.name, label).clicked() {
                        let _ = s.run("ui.set_tool_options", json!({ "platform_system": sys.name }));
                    }
                }
            });
            widgets::name(&r.response, egui::Role::ComboBox, format!("Option {}", t("option.system")));
        }
        Tool::Support => {
            ui.label(t("option.preset"));
            let r = egui::ComboBox::from_id_salt("support_preset").selected_text(&o.support_preset).show_ui(ui, |ui| {
                for p in stagecraft_geom::supports() {
                    if ui.selectable_label(o.support_preset == p.name, &p.name).clicked() {
                        let _ = s.run("ui.set_tool_options", json!({ "support_preset": p.name }));
                    }
                }
            });
            widgets::name(&r.response, egui::Role::ComboBox, format!("Option {}", t("option.preset")));
        }
        Tool::Hoist => mm_option(ui, s, "option.height", "hoist_height", o.hoist_height),
        Tool::Ceiling => mm_option(ui, s, "option.underside", "ceiling_height", o.ceiling_height),
        _ => {}
    }
    if !matches!(
        s.ui.tool,
        Tool::Select | Tool::Move | Tool::Rotate | Tool::Fixture | Tool::Dimension | Tool::Calibrate
    ) {
        ui.separator();
    }
    ui.label(RichText::new(hint(s)).color(crate::theme::chrome(ui.visuals().dark_mode).text2));
}

/// What the tool expects next. Outline tools say how to close once they can.
pub fn hint(s: &Studio) -> String {
    let n = s.ui.gesture.points.len();
    let key = match (s.ui.tool, n) {
        (Tool::Floor | Tool::Ceiling, 0) => "hint.outline.start",
        (Tool::Floor | Tool::Ceiling, 1 | 2) => "hint.outline.next",
        (Tool::Floor, _) => "hint.floor.close",
        (Tool::Ceiling, _) => "hint.ceiling.close",
        (Tool::Wall, 1 | 2) => "hint.wall.next",
        (Tool::Wall, n) if n >= 3 => "hint.wall.close",
        (tool, _) => hint_key(tool),
    };
    t(key).to_owned()
}

fn mm_option(ui: &mut Ui, s: &mut Studio, label: &str, field: &str, value: f64) {
    let r = ui.label(t(label));
    if field.ends_with("_z") || field.ends_with("_base") || field.ends_with("height") || field.ends_with("trim") {
        r.on_hover_text(t("option.above_layer"));
    }
    let id = egui::Id::unique(("tool_option", field));
    if let Some(v) = edit_field(ui, id, &fmt_mm(value), 56.0, &format!("Option {}", t(label)))
        && let Some(mm) = parse_mm(&v)
    {
        let _ = s.run("ui.set_tool_options", json!({ field: mm }));
    }
    ui.label(RichText::new("mm").weak());
}

fn hint_key(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "hint.select",
        Tool::Move => "hint.move",
        Tool::Rotate => "hint.rotate",
        Tool::Floor => "hint.floor",
        Tool::Wall => "hint.wall",
        Tool::Dimension => "hint.dimension",
        Tool::Calibrate => "hint.calibrate",
        Tool::Truss => "hint.truss",
        Tool::Fixture => "hint.fixture",
        Tool::Run => "hint.run",
        Tool::Batten => "hint.batten",
        Tool::Curtain => "hint.curtain",
        Tool::Proscenium => "hint.proscenium",
        Tool::Wings => "hint.wings",
        Tool::Platform => "hint.platform",
        Tool::Support => "hint.support",
        Tool::Hoist => "hint.hoist",
        Tool::Ceiling => "hint.ceiling",
    }
}

/// mm without trailing zeros.
pub fn fmt_mm(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r.fract() == 0.0 { format!("{r:.0}") } else { format!("{r:.1}") }
}

/// A number of mm; "m"/"cm" suffixes allowed ("4.5m").
pub fn parse_mm(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.ends_with("mm") || !s.ends_with('m') {
        let n = s.trim_end_matches("mm").trim().replace(',', ".");
        return n.parse().ok().filter(|v: &f64| v.is_finite());
    }
    crate::tools::parse_length(s)
}

pub fn status_bar(ui: &mut Ui, s: &mut Studio) {
    ui.horizontal(|ui| {
        if let Some(c) = s.ui.gesture.cursor {
            ui.monospace(format!("x {:>9}  y {:>9}", fmt_mm(c.point[0]), fmt_mm(c.point[1])));
            ui.label(RichText::new(t(snap_key(c.kind))).weak());
        }
        ui.separator();
        let grid =
            s.ui.snap
                .grid
                .map_or_else(|| t("status.grid_off").to_owned(), |g| tf("status.grid", &[("mm", &fmt_mm(g))]));
        ui.label(RichText::new(grid).weak());
        ui.separator();
        let n = s.engine.selection().len();
        ui.label(tf("status.selected", &[("n", &n)]));
        if let Some((msg, err)) = &s.ui.status {
            ui.separator();
            let text = RichText::new(msg);
            ui.label(if *err { text.color(ui.visuals().error_fg_color) } else { text });
        }
    });
}

fn snap_key(k: stagecraft_geom::snap::SnapKind) -> &'static str {
    use stagecraft_geom::snap::SnapKind::*;
    match k {
        Endpoint => "snap.endpoint",
        Midpoint => "snap.midpoint",
        Truss => "snap.truss",
        Grid => "snap.grid",
        Orthogonal => "snap.orthogonal",
        Free => "snap.free",
    }
}

pub fn layers(ui: &mut Ui, s: &mut Studio) {
    let layers: Vec<Layer> = s.engine.doc().layers_sorted().into_iter().cloned().collect();
    let active = s.ui.active_layer.or(layers.first().map(|l| l.id));
    ui.horizontal(|ui| {
        ui.add_space(46.0);
        ui.label(RichText::new(t("layers.name")).small().weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_button(ui, Icon::Plus, "Add layer").on_hover_text(t("layers.add")).clicked() {
                let n = s.engine.doc().layers.len() + 1;
                let mut name = tf("layers.new_name", &[("n", &n)]);
                let mut i = n;
                while s.engine.doc().layers.values().any(|l| l.name == name) {
                    i += 1;
                    name = tf("layers.new_name", &[("n", &i)]);
                }
                let _ = s.run("layer.create", json!({ "name": name }));
            }
            ui.add_space(42.0);
            ui.label(RichText::new(t("layers.elevation")).small().weak());
        });
    });
    for (i, l) in layers.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let mut vis = l.visible;
            let r = ui.checkbox(&mut vis, "");
            widgets::name(&r, Role::CheckBox, format!("Visible {}", l.name));
            if r.on_hover_text(t("layers.visible")).changed() {
                let _ = s.run("layer.update", json!({ "layer": l.id, "visible": vis }));
            }
            let is_active = active == Some(l.id);
            if icon_button(ui, Icon::Dot(is_active), &format!("Activate {}", l.name))
                .on_hover_text(t("layers.activate"))
                .clicked()
            {
                let _ = s.run("ui.set_active_layer", json!({ "layer": l.id }));
            }
            // Name takes what the elevation field and the three buttons leave.
            let name_w = (ui.available_width() - 56.0 - 3.0 * 22.0 - 8.0).max(50.0);
            let id = egui::Id::unique(("layer_name", l.id));
            if let Some(name) = edit_field(ui, id, &l.name, name_w, &format!("Layer name {}", l.name))
                && !name.trim().is_empty()
            {
                let _ = s.run("layer.update", json!({ "layer": l.id, "name": name.trim() }));
            }
            let id = egui::Id::unique(("layer_elev", l.id));
            if let Some(v) = edit_field(ui, id, &fmt_mm(l.elevation), 48.0, &format!("Elevation {}", l.name)) {
                match parse_mm(&v) {
                    Some(mm) => {
                        let _ = s.run("layer.update", json!({ "layer": l.id, "elevation": mm }));
                    }
                    None => s.ui.error(tf("status.bad_number", &[("text", &v)])),
                }
            }
            ui.add_enabled_ui(i > 0, |ui| {
                if icon_button(ui, Icon::Up, &format!("Raise {}", l.name)).clicked() {
                    swap_order(s, &layers[i - 1], l);
                }
            });
            ui.add_enabled_ui(i + 1 < layers.len(), |ui| {
                if icon_button(ui, Icon::Down, &format!("Lower {}", l.name)).clicked() {
                    swap_order(s, l, &layers[i + 1]);
                }
            });
            ui.add_enabled_ui(layers.len() > 1, |ui| {
                if icon_button(ui, Icon::Close, &format!("Delete layer {}", l.name))
                    .on_hover_text(t("layers.delete"))
                    .clicked()
                {
                    let other = layers.iter().find(|o| o.id != l.id).map(|o| o.id);
                    let _ = s.run("layer.delete", json!({ "layer": l.id, "move_objects_to": other }));
                }
            });
        });
    }
}

/// Swap two neighbouring layers in the list, as one undo step.
fn swap_order(s: &mut Studio, a: &Layer, b: &Layer) {
    let (oa, ob) = if a.order == b.order { (a.order, a.order + 1) } else { (a.order, b.order) };
    let _ = s.run(
        "batch",
        json!({ "commands": [
            { "command": "layer.update", "args": { "layer": a.id, "order": ob } },
            { "command": "layer.update", "args": { "layer": b.id, "order": oa } },
        ]}),
    );
}

pub fn tags(ui: &mut Ui, s: &mut Studio) {
    let mut tags: Vec<(Id, String, usize)> = {
        let doc = s.engine.doc();
        doc.tags
            .values()
            .map(|t| (t.id, t.name.clone(), doc.objects.values().filter(|o| o.tags.contains(&t.id)).count()))
            .collect()
    };
    tags.sort_by(|a, b| a.1.cmp(&b.1));
    if tags.is_empty() {
        ui.label(RichText::new(t("tags.none")).weak());
    }
    for (id, name, n) in tags {
        ui.horizontal(|ui| {
            let mut vis = !s.ui.hidden_tags.contains(&id);
            let r = ui.checkbox(&mut vis, format!("{name}  ({n})"));
            widgets::name(&r, Role::CheckBox, format!("Show tag {name}"));
            if r.changed() {
                let _ = s.run("ui.set_tag_visible", json!({ "tag": id, "visible": vis }));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Close, &format!("Delete tag {name}")).on_hover_text(t("tags.delete")).clicked()
                {
                    let _ = s.run("tag.delete", json!({ "tag": id }));
                }
            });
        });
    }
    ui.horizontal(|ui| {
        let id = egui::Id::unique("new_tag");
        if let Some(name) = crate::widgets::edit_field_hint(ui, id, "", 140.0, "New tag", t("tags.new_hint"))
            && !name.trim().is_empty()
        {
            let _ = s.run("tag.create", json!({ "name": name.trim() }));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mm_fields_take_plain_mm_and_metres() {
        assert_eq!(parse_mm("150"), Some(150.0));
        assert_eq!(parse_mm("150 mm"), Some(150.0));
        assert_eq!(parse_mm("4.5m"), Some(4500.0));
        assert_eq!(parse_mm("x"), None);
        assert_eq!(fmt_mm(1500.0), "1500");
        assert_eq!(fmt_mm(12.34), "12.3");
    }
}
