//! Menu bar, toolbar, status bar, layers and tags panels.

use egui::{RichText, Role, Ui};
use serde_json::{Value, json};
use stagecraft_core::{Id, Layer};

use crate::app::Studio;
use crate::i18n::{t, tf};
use crate::state::{FileRequest, PanelKind, ThemePref, Tool, View};
use crate::widgets::{self, Icon, button, edit_field, icon_button};

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

pub fn menu_bar(ui: &mut Ui, s: &mut Studio) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button(t("menu.file"), |ui| {
            if ui.button(t("menu.new")).clicked() {
                new_document(s);
            }
            if ui.button(t("menu.open")).clicked() {
                s.ui.file_request = Some(FileRequest::Open);
            }
            if ui.button(t("menu.save")).clicked() {
                save(s, false);
            }
            if ui.button(t("menu.save_as")).clicked() {
                save(s, true);
            }
            ui.separator();
            if ui.button(t("menu.import_mvr")).clicked() {
                s.ui.file_request = Some(FileRequest::ImportMvr);
            }
            if ui.button(t("menu.import_model")).clicked() {
                s.ui.file_request = Some(FileRequest::ImportModel);
            }
            if ui.button(t("menu.import_underlay")).clicked() {
                s.ui.file_request = Some(FileRequest::ImportUnderlay);
            }
            if ui.button(t("menu.export_mvr")).clicked() {
                s.ui.file_request = Some(FileRequest::ExportMvr);
            }
            ui.separator();
            if ui.add_enabled(!s.engine.doc().sheets.is_empty(), egui::Button::new(t("menu.export_pdf"))).clicked() {
                s.ui.file_request = Some(FileRequest::ExportPdf);
            }
        });
        ui.menu_button(t("menu.edit"), |ui| {
            if ui.add_enabled(s.engine.can_undo(), egui::Button::new(t("menu.undo"))).clicked() {
                let _ = s.run("history.undo", Value::Null);
            }
            if ui.add_enabled(s.engine.can_redo(), egui::Button::new(t("menu.redo"))).clicked() {
                let _ = s.run("history.redo", Value::Null);
            }
            ui.separator();
            let any = !s.engine.selection().is_empty();
            if ui.add_enabled(any, egui::Button::new(t("menu.delete"))).clicked() {
                let ids = s.engine.selection().to_vec();
                let _ = s.run("object.delete", json!({ "ids": ids }));
            }
            if ui.button(t("menu.palette")).clicked() {
                s.ui.palette.open();
            }
        });
        ui.menu_button(t("menu.view"), |ui| {
            for (view, key) in [
                (View::Plan, "view.plan"),
                (View::ThreeD, "view.3d"),
                (View::Split, "view.split"),
                (View::Sheets, "view.sheets"),
            ] {
                if ui.radio(s.ui.view == view, t(key)).clicked() {
                    let _ = s.run("ui.set_view", json!({ "view": view }));
                }
            }
            if ui.button(t("menu.zoom_fit")).clicked() {
                let _ = s.run("ui.zoom_to_fit", Value::Null);
            }
            ui.menu_button(t("menu.standard_views"), |ui| {
                for v in crate::view3d::StdView::ALL {
                    if ui.button(t(v.key())).clicked() {
                        let _ = s.run("ui.camera", json!({ "view": v }));
                    }
                }
            });
            ui.separator();
            for p in crate::dock::PANELS.into_iter().filter(|p| *p != PanelKind::Canvas) {
                let mut on = s.ui.layout.layout.contains(&p);
                if ui.checkbox(&mut on, crate::dock::label(p)).changed() {
                    let _ = s.run("ui.toggle_panel", json!({ "panel": p, "visible": on }));
                }
            }
            if ui.button(t("menu.reset_layout")).clicked() {
                let _ = s.run("ui.reset_layout", Value::Null);
            }
            ui.separator();
            for (theme, key) in [
                (ThemePref::System, "theme.system"),
                (ThemePref::Light, "theme.light"),
                (ThemePref::Dark, "theme.dark"),
            ] {
                if ui.radio(s.ui.theme == theme, t(key)).clicked() {
                    let _ = s.run("ui.set_theme", json!({ "theme": theme }));
                }
            }
        });
    });
}

pub fn toolbar(ui: &mut Ui, s: &mut Studio) {
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        let groups: Vec<&[Tool]> =
            if s.ui.view == View::Sheets { Vec::new() } else { vec![&Tool::DRAW[..], &Tool::STAGE[..]] };
        // Calibrate only matters with an underlay in the document.
        let underlays = s.engine.doc().objects.values().any(|o| o.kind.type_name() == "underlay");
        ui.spacing_mut().item_spacing.x = 5.0;
        for (i, group) in groups.into_iter().enumerate() {
            if i > 0 {
                ui.separator();
            }
            for &tool in group {
                if tool == Tool::Calibrate && !underlays && s.ui.tool != Tool::Calibrate {
                    continue;
                }
                let label = t(tool.key());
                let r = ui.selectable_label(s.ui.tool == tool, label);
                widgets::name(&r, Role::Button, tf("a11y.tool", &[("tool", &label)]));
                let tip = match tool.shortcut() {
                    Some(k) => tf("tool.tip", &[("tool", &label), ("key", &format!("{k:?}"))]),
                    None => label.to_owned(),
                };
                if r.on_hover_text(tip).clicked() {
                    let _ = s.run("ui.set_tool", json!({ "tool": tool }));
                }
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if button(ui, t("palette.button"), "Command palette").clicked() {
                s.ui.palette.open();
            }
            ui.separator();
            for (view, key) in [
                (View::Sheets, "view.sheets"),
                (View::Split, "view.split"),
                (View::ThreeD, "view.3d"),
                (View::Plan, "view.plan"),
            ] {
                let label = t(key);
                let r = ui.selectable_label(s.ui.view == view, label);
                widgets::name(&r, Role::Button, tf("a11y.view", &[("view", &label)]));
                if r.clicked() {
                    let _ = s.run("ui.set_view", json!({ "view": view }));
                }
            }
        });
    });
    if s.ui.view == View::Sheets {
        ui.label(RichText::new(t("hint.sheets")).weak());
    } else {
        ui.horizontal(|ui| tool_options(ui, s));
    }
    ui.add_space(3.0);
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
        Tool::Ceiling => mm_option(ui, s, "option.height", "ceiling_height", o.ceiling_height),
        _ => {}
    }
    if !matches!(
        s.ui.tool,
        Tool::Select | Tool::Move | Tool::Rotate | Tool::Floor | Tool::Fixture | Tool::Dimension | Tool::Calibrate
    ) {
        ui.separator();
    }
    ui.label(RichText::new(t(hint_key(s.ui.tool))).weak());
}

fn mm_option(ui: &mut Ui, s: &mut Studio, label: &str, field: &str, value: f64) {
    ui.label(t(label));
    let id = egui::Id::unique(("tool_option", field));
    if let Some(v) = edit_field(ui, id, &fmt_mm(value), 56.0, t(label))
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
    ui.horizontal(|ui| {
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
        });
    });
    let layers: Vec<Layer> = s.engine.doc().layers_sorted().into_iter().cloned().collect();
    let active = s.ui.active_layer.or(layers.first().map(|l| l.id));
    ui.horizontal(|ui| {
        ui.add_space(46.0);
        ui.label(RichText::new(t("layers.name")).small().weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(64.0);
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
