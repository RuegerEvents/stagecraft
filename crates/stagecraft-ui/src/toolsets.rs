//! The tool palette (left dock): the drawing, stage, rigging and lighting tools as icon
//! grids in collapsible sections, and the sketch constraints for what is picked. Each tool
//! button has the tooltip "Name (key)" and the accessible name "Tool Name".
//!
//! The rows hold as many buttons as fit (`ToolColumns::Auto`) or two or one
//! (`ui.tool_columns`, the ≡ menu's Compact entries). The panel can be dragged as narrow as
//! one button; section titles end in "…" when cut, and at one button per row a section header
//! is a rule with the title as tooltip.

use egui::{Role, Sense, Stroke, Ui, vec2};
use serde_json::json;

use crate::app::Studio;
use crate::i18n::t;
use crate::sketch_ui::Relation;
use crate::state::{ToolColumns, View};
use crate::theme;

const BUTTON: f32 = 30.0;
/// 16 pt: two thirds of Lucide's 24 grid, so most of its coordinates land on whole pixels at
/// 1× (`icons::rasterise` fits the rest).
const ICON: f32 = 16.0;
const GAP: f32 = 2.0;
const INSET: f32 = 6.0;
/// Room the body keeps right of the last button.
const RIGHT: f32 = 4.0;
/// The dock frame's margins around a panel body, left and right together.
const FRAME: f32 = 16.0;

/// The body width `n` buttons per row need.
fn body_for(n: usize) -> f32 {
    INSET + n as f32 * (BUTTON + GAP) - GAP + RIGHT
}

/// The docked palette's width for `n` buttons per row.
pub fn width_for(n: usize) -> f32 {
    body_for(n) + FRAME
}

/// Buttons per row in a body `width` wide: what fits, at most what the user chose.
pub fn per_row(width: f32, columns: ToolColumns) -> usize {
    let fit = (((width - INSET - RIGHT + GAP) / (BUTTON + GAP)).floor() as usize).max(1);
    match columns {
        ToolColumns::Auto => fit,
        ToolColumns::Two => fit.min(2),
        ToolColumns::One => 1,
    }
}

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

/// A section header (chevron and title, cut with "…"); returns whether the section is
/// open. With one button per row (`compact`) it is a rule; the title is the tooltip.
fn header(ui: &mut Ui, s: &mut Studio, key: &str, compact: bool) -> bool {
    let c = theme::of(ui);
    let open = !s.ui.collapsed_sections.contains(key);
    let h = if compact { 11.0 } else { 22.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    let title = t(key);
    resp.widget_info(|| egui::WidgetInfo::selected(Role::Button, true, open, format!("Section {title}")));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, c.hover.gamma_multiply(0.25));
    }
    let resp = if compact {
        let y = rect.center().y.round() + 0.5;
        let x = egui::Rangef::new(rect.left() + INSET, rect.left() + INSET + BUTTON);
        ui.painter().hline(x, y, Stroke::new(1.0, if open { c.separator } else { c.text3 }));
        resp.on_hover_text(title)
    } else {
        let chevron = egui::Rect::from_center_size(egui::pos2(rect.left() + 11.0, rect.center().y), vec2(12.0, 12.0));
        crate::icons::paint(ui, chevron, if open { "chevron-down" } else { "chevron-right" }, 12.0, c.text2);
        let mut job = egui::text::LayoutJob::single_section(
            title.to_owned(),
            egui::TextFormat::simple(theme::medium(theme::BODY), c.text),
        );
        job.wrap = egui::text::TextWrapping {
            max_width: (rect.width() - 25.0).max(1.0),
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        };
        let g = ui.fonts_mut(|f| f.layout_job(job));
        let elided = g.elided;
        ui.painter().galley(egui::pos2(rect.left() + 21.0, rect.center().y - g.size().y / 2.0), g, c.text);
        if elided { resp.on_hover_text(title) } else { resp }
    };
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

/// Icon buttons, `per_row` to a row.
fn grid<T: Copy>(ui: &mut Ui, items: &[T], per_row: usize, mut add: impl FnMut(&mut Ui, T)) {
    for row in items.chunks(per_row) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
            ui.add_space(INSET);
            for item in row {
                add(ui, *item);
            }
        });
        ui.add_space(2.0);
    }
    ui.add_space(4.0);
}

pub fn show(ui: &mut Ui, s: &mut Studio) {
    egui::ScrollArea::vertical().id_salt("toolsets").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let sheets = s.ui.view == View::Sheets;
        let n = per_row(ui.available_width(), s.ui.tool_columns);
        let compact = n == 1;
        for (key, tools) in crate::panels::tool_groups(s) {
            if header(ui, s, key, compact) {
                grid(ui, &tools, n, |ui, tool| {
                    ui.add_enabled_ui(!sheets, |ui| {
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
                    });
                });
            }
            if !compact {
                rule(ui);
            }
        }
        if header(ui, s, "palette.constraints", compact) {
            grid(ui, &Relation::ALL, n, |ui, r| {
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
            });
        }
        if !compact {
            rule(ui);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_fit_the_width_and_the_choice() {
        for n in 1..=6 {
            assert_eq!(per_row(body_for(n), ToolColumns::Auto), n, "{n} fit their width");
            assert_eq!(per_row(body_for(n) - 1.0, ToolColumns::Auto), (n - 1).max(1));
        }
        assert_eq!(per_row(body_for(6), ToolColumns::Two), 2);
        assert_eq!(per_row(body_for(1), ToolColumns::Two), 1);
        assert_eq!(per_row(body_for(6), ToolColumns::One), 1);
        assert_eq!(width_for(1), 56.0);
    }
}
