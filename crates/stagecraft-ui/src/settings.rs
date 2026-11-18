//! Project Settings (File > Project Settings…, ⌘, , ⌘K): the document's own settings in
//! one non-blocking window: the project unit and its precision, sheet language,
//! dimension format, decimal separator and the title block fields. Every change is a
//! command (`doc.set_units`, `doc.set_meta`); `ui.project_settings` opens and closes it.

use egui::{Align2, RichText, Ui, vec2};
use serde_json::json;
use stagecraft_core::units::{LengthUnit, Units};

use crate::app::Studio;
use crate::i18n::{t, tf};
use crate::theme;
use crate::widgets::{self, button, edit_field};

/// What a precision is called: "3 decimals", "1/16\"".
pub fn precision_label(unit: LengthUnit, p: u8) -> String {
    match unit {
        LengthUnit::FtIn if p == 1 => t("settings.whole_inches").to_owned(),
        LengthUnit::FtIn => format!("1/{p}\""),
        _ => tf("settings.decimals", &[("n", &p)]),
    }
}

/// The window, when it is open. Drawn after the panels, above them; the rest of the
/// window keeps working.
pub fn show(ctx: &egui::Context, s: &mut Studio) {
    if !s.ui.project_settings {
        return;
    }
    let c = theme::tokens(ctx.theme() == egui::Theme::Dark);
    let frame = egui::Frame::NONE
        .fill(c.pal_bg)
        .stroke(egui::Stroke::new(1.0, c.pal_border))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::same(16))
        .shadow(egui::Shadow { offset: [0, 14], blur: 36, spread: 0, color: c.shadow });
    let mut open = true;
    egui::Window::new(t("settings.title"))
        .id(egui::Id::unique("project_settings"))
        .title_bar(false)
        .frame(frame)
        .resizable(false)
        .default_pos(ctx.content_rect().center() - vec2(216.0, 290.0))
        .pivot(Align2::LEFT_TOP)
        .show(ctx, |ui| {
            ui.set_width(400.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                crate::icons::paint(ui, r, "settings-2", 16.0, c.accent);
                ui.add_space(2.0);
                ui.label(RichText::new(t("settings.title")).font(theme::bold(14.0)).color(c.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::icon_button(ui, widgets::Icon::Close, "Close settings window").clicked() {
                        open = false;
                    }
                });
            });
            body(ui, s);
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let done = egui::Button::new(RichText::new(t("settings.done")).color(c.on_accent).strong())
                    .fill(c.accent)
                    .corner_radius(5.0)
                    .min_size(vec2(64.0, 24.0));
                let r = ui.add(done);
                widgets::name(&r, egui::Role::Button, "Close Project Settings");
                if r.clicked() {
                    s.ui.project_settings = false;
                }
            });
        });
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ctx.text_edit_focused() && !s.ui.palette.open;
    if !open || esc {
        s.ui.project_settings = false;
    }
}

fn body(ui: &mut Ui, s: &mut Studio) {
    let c = theme::of(ui);
    let meta = s.engine.doc().meta.clone();
    let u = Units::of(&meta);

    widgets::section(ui, t("settings.units"));
    egui::Grid::new("settings_units").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
        widgets::prop_label(ui, t("settings.length_unit"));
        let r = widgets::combo_w(ui.available_width() - 8.0, "settings_length_unit")
            .selected_text(t(&format!("unit.{}", u.unit.code())))
            .show_ui(ui, |ui| {
                for unit in LengthUnit::ALL {
                    if ui.selectable_label(u.unit == unit, t(&format!("unit.{}", unit.code()))).clicked() {
                        let _ = s.run("doc.set_units", json!({ "length_unit": unit }));
                    }
                }
            });
        widgets::name(&r.response, egui::Role::ComboBox, "Settings length unit");
        ui.end_row();

        widgets::prop_label(ui, t("settings.precision"));
        let r = widgets::combo_w(ui.available_width() - 8.0, "settings_precision")
            .selected_text(precision_label(u.unit, u.precision))
            .show_ui(ui, |ui| {
                for &p in u.unit.precisions() {
                    if ui.selectable_label(u.precision == p, precision_label(u.unit, p)).clicked() {
                        let _ = s.run("doc.set_units", json!({ "precision": p }));
                    }
                }
            });
        widgets::name(&r.response, egui::Role::ComboBox, "Settings precision");
        ui.end_row();

        widgets::prop_label(ui, t("settings.example"));
        let r = ui.label(RichText::new(u.with_unit(6155.0)).color(c.text));
        widgets::name(&r, egui::Role::Label, format!("Settings example {}", u.with_unit(6155.0)));
        ui.end_row();
    });

    widgets::section(ui, t("settings.sheet_text"));
    egui::Grid::new("settings_sheet_text").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
        crate::sheets::sheet_text_rows(ui, s, "Settings");
    });

    widgets::section(ui, t("settings.title_block"));
    egui::Grid::new("settings_title_block").num_columns(2).spacing([6.0, 3.0]).show(ui, |ui| {
        for (key, field, v) in crate::sheets::meta_texts(&meta) {
            widgets::prop_label(ui, t(key));
            let id = egui::Id::unique(("settings_meta", field));
            if let Some(v) = edit_field(ui, id, &v, f32::INFINITY, &format!("Settings {}", t(key))) {
                let _ = s.run("doc.set_meta", json!({ field: crate::sheets::meta_value(field, v) }));
            }
            ui.end_row();
        }
        widgets::prop_label(ui, t("meta.logo"));
        ui.horizontal(|ui| {
            if button(ui, t("sheets.choose_logo"), "Settings choose logo").clicked() {
                s.ui.file_request = Some(crate::state::FileRequest::Logo);
            }
            if meta.logo.is_some() && button(ui, t("sheets.remove_logo"), "Settings remove logo").clicked() {
                let _ = s.run("doc.set_meta", json!({ "logo_file": "" }));
            }
        });
        ui.end_row();
    });
}
