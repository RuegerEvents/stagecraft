//! The tool palette (left dock): the drawing, stage, rigging and lighting tools as icon
//! grids in collapsible sections, and the sketch constraints for what is picked. Each tool
//! button has the tooltip "Name (key)" and the accessible name "Tool Name".

use egui::{Role, Sense, Stroke, Ui, vec2};
use serde_json::json;

use crate::app::Studio;
use crate::i18n::t;
use crate::sketch_ui::Relation;
use crate::state::View;
use crate::theme;

const BUTTON: f32 = 32.0;
const ICON: f32 = 18.0;

fn relation_icon(r: Relation) -> &'static str {
    match r {
        Relation::Horizontal => "c-horizontal",
        Relation::Vertical => "c-vertical",
        Relation::Parallel => "c-parallel",
        Relation::Perpendicular => "c-perpendicular",
        Relation::Equal => "c-equal",
        Relation::Symmetric => "c-symmetric",
        Relation::Lock => "lock",
    }
}

/// A section header (chevron and title); returns whether the section is open.
fn header(ui: &mut Ui, s: &mut Studio, key: &str) -> bool {
    let c = theme::of(ui);
    let open = !s.ui.collapsed_sections.contains(key);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
    let title = t(key);
    resp.widget_info(|| egui::WidgetInfo::selected(Role::Button, true, open, format!("Section {title}")));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, c.hover.gamma_multiply(0.25));
    }
    let chevron = egui::Rect::from_center_size(egui::pos2(rect.left() + 11.0, rect.center().y), vec2(12.0, 12.0));
    crate::icons::paint(ui, chevron, if open { "chevron-down" } else { "chevron-right" }, 12.0, c.text2);
    ui.painter().text(
        egui::pos2(rect.left() + 21.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        theme::medium(theme::BODY),
        c.text,
    );
    if resp.clicked() {
        if open {
            s.ui.collapsed_sections.insert(key.to_owned());
        } else {
            s.ui.collapsed_sections.remove(key);
        }
    }
    open
}

fn rule(ui: &mut Ui) {
    let c = theme::of(ui);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(r.x_range().shrink(6.0), r.center().y, Stroke::new(1.0, c.separator));
}

fn grid(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
        ui.add_space(6.0);
        ui.set_max_width(ui.available_width() - 4.0);
        add(ui);
    });
    ui.add_space(6.0);
}

pub fn show(ui: &mut Ui, s: &mut Studio) {
    egui::ScrollArea::vertical().id_salt("toolsets").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let sheets = s.ui.view == View::Sheets;
        for (key, tools) in crate::panels::tool_groups(s) {
            if header(ui, s, key) {
                grid(ui, |ui| {
                    ui.add_enabled_ui(!sheets, |ui| {
                        for tool in tools {
                            let label = t(tool.key());
                            let a11y = crate::i18n::tf("a11y.tool", &[("tool", &label)]);
                            let r = crate::chrome::icon(
                                ui,
                                &a11y,
                                crate::icons::tool_icon(tool),
                                BUTTON,
                                ICON,
                                s.ui.tool == tool,
                                &crate::panels::tool_tip(tool),
                            );
                            if r.clicked() {
                                let _ = s.run("ui.set_tool", json!({ "tool": tool }));
                            }
                        }
                    });
                });
            }
            rule(ui);
        }
        if header(ui, s, "palette.constraints") {
            grid(ui, |ui| {
                for r in Relation::ALL {
                    let label = t(r.key());
                    let ok = !sheets && crate::sketch_ui::relation_available(s, r);
                    let resp = ui
                        .add_enabled_ui(ok, |ui| {
                            let tip = format!("{label}\n{}", t("palette.constraints_tip"));
                            crate::chrome::icon(
                                ui,
                                &format!("Constraint {label}"),
                                relation_icon(r),
                                BUTTON,
                                ICON,
                                false,
                                &tip,
                            )
                        })
                        .inner;
                    if resp.clicked() {
                        crate::sketch_ui::apply_relation(s, r);
                    }
                }
            });
        }
        rule(ui);
    });
}
