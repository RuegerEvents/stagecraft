//! Menu bar, toolbar, status bar, layers and tags panels.

use egui::{RichText, Ui};
use serde_json::{Value, json};
use stagecraft_core::{Id, Layer};

use crate::app::Studio;
use crate::i18n::{t, tf};
use crate::state::{FileRequest, Tool};
use crate::widgets::{self, Icon, edit_field, icon_button};

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

/// The sections of the tool palette (catalog key, tools), in order; Calibrate only with an
/// underlay.
pub fn tool_groups(s: &Studio) -> Vec<(&'static str, Vec<Tool>)> {
    let underlays = s.engine.doc().objects.values().any(|o| o.kind.type_name() == "underlay");
    let mut draw = vec![Tool::Select, Tool::Move, Tool::Rotate, Tool::Wall, Tool::Floor, Tool::Dimension];
    if underlays || s.ui.tool == Tool::Calibrate {
        draw.push(Tool::Calibrate);
    }
    vec![
        ("palette.draw", draw),
        ("palette.stage", vec![Tool::Proscenium, Tool::Curtain, Tool::Wings, Tool::Platform, Tool::Ceiling]),
        ("palette.rigging", vec![Tool::Batten, Tool::Truss, Tool::Run, Tool::Hoist, Tool::Support]),
        ("palette.lighting", vec![Tool::Fixture]),
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

/// Whether a tool has options on the options row.
pub fn has_options(tool: Tool) -> bool {
    !matches!(tool, Tool::Select | Tool::Move | Tool::Rotate | Tool::Fixture | Tool::Dimension | Tool::Calibrate)
}

pub fn tool_options(ui: &mut Ui, s: &mut Studio) {
    let o = s.ui.options.clone();
    let u = crate::lengths::of(s);
    let profile_box = |ui: &mut Ui, s: &mut Studio| {
        ui.label(t("option.profile"));
        let r = crate::widgets::combo_w(150.0, "truss_profile").selected_text(&o.truss_profile).show_ui(ui, |ui| {
            for p in stagecraft_geom::truss_profiles() {
                let label = format!("{}  {} · {}", p.name, p.maker, u.field_with_unit(p.width));
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
                crate::widgets::combo_w(150.0, "platform_preset").selected_text(&o.platform_preset).show_ui(ui, |ui| {
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
            let r = crate::widgets::combo_w(150.0, "platform_system").selected_text(shown).show_ui(ui, |ui| {
                if ui.selectable_label(o.platform_system.is_empty(), t("option.none")).clicked() {
                    let _ = s.run("ui.set_tool_options", json!({ "platform_system": "" }));
                }
                for sys in stagecraft_geom::platform_systems() {
                    let label =
                        format!("{} · {}–{}", sys.name, u.field(sys.min_height), u.field_with_unit(sys.max_height));
                    if ui.selectable_label(o.platform_system == sys.name, label).clicked() {
                        let _ = s.run("ui.set_tool_options", json!({ "platform_system": sys.name }));
                    }
                }
            });
            widgets::name(&r.response, egui::Role::ComboBox, format!("Option {}", t("option.system")));
        }
        Tool::Support => {
            ui.label(t("option.preset"));
            let r =
                crate::widgets::combo_w(150.0, "support_preset").selected_text(&o.support_preset).show_ui(ui, |ui| {
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
    tf(key, &[("unit", &crate::lengths::of(s).label())])
}

fn mm_option(ui: &mut Ui, s: &mut Studio, label: &str, field: &str, value: f64) {
    let r = ui.label(t(label));
    if field.ends_with("_z") || field.ends_with("_base") || field.ends_with("height") || field.ends_with("trim") {
        r.on_hover_text(t("option.above_layer"));
    }
    let u = crate::lengths::of(s);
    let feet = u.unit == stagecraft_core::units::LengthUnit::FtIn;
    let id = egui::Id::unique(("tool_option", field));
    let w = if feet { 92.0 } else { 56.0 };
    if let Some(v) = edit_field(ui, id, &u.field(value), w, &format!("Option {}", t(label)))
        && let Some(mm) = crate::lengths::parse_or_error(s, &v)
    {
        let _ = s.run("ui.set_tool_options", json!({ field: mm }));
    }
    // Feet and inches carry their marks.
    if !feet {
        ui.label(RichText::new(u.label()).weak());
    }
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

pub fn snap_key(k: stagecraft_geom::snap::SnapKind) -> &'static str {
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
    let u = crate::lengths::of(s);
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
            ui.label(RichText::new(tf("layers.elevation", &[("unit", &u.label())])).small().weak());
        });
    });
    for (i, l) in layers.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if widgets::eye_toggle(ui, l.visible, &format!("Visible {}", l.name))
                .on_hover_text(t("layers.visible"))
                .clicked()
            {
                let _ = s.run("layer.update", json!({ "layer": l.id, "visible": !l.visible }));
            }
            let is_active = active == Some(l.id);
            if icon_button(ui, Icon::Dot(is_active), &format!("Activate {}", l.name))
                .on_hover_text(t("layers.activate"))
                .clicked()
            {
                let _ = s.run("ui.set_active_layer", json!({ "layer": l.id }));
            }
            // Name takes what the elevation field and the three buttons leave.
            let elev_w = if u.unit == stagecraft_core::units::LengthUnit::FtIn { 76.0 } else { 48.0 };
            let name_w = (ui.available_width() - elev_w - 8.0 - 3.0 * 22.0 - 8.0).max(50.0);
            let id = egui::Id::unique(("layer_name", l.id));
            if let Some(name) = edit_field(ui, id, &l.name, name_w, &format!("Layer name {}", l.name))
                && !name.trim().is_empty()
            {
                let _ = s.run("layer.update", json!({ "layer": l.id, "name": name.trim() }));
            }
            let id = egui::Id::unique(("layer_elev", l.id));
            if let Some(v) = edit_field(ui, id, &u.field(l.elevation), elev_w, &format!("Elevation {}", l.name)) {
                match u.parse(&v) {
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
            let vis = !s.ui.hidden_tags.contains(&id);
            if widgets::eye_toggle(ui, vis, &format!("Show tag {name}")).clicked() {
                let _ = s.run("ui.set_tag_visible", json!({ "tag": id, "visible": !vis }));
            }
            ui.label(&name);
            ui.label(RichText::new(n.to_string()).color(crate::theme::of(ui).text3));
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
