//! Window chrome around the dock area, as in the Craft suite: the macOS title row (28 pt,
//! the band where AppKit centres the traffic lights) with the centred window title, the tool
//! bar (file and edit commands, the view switch, the command palette), the tool options row,
//! and the status bar (Model and sheet tabs, coordinates, messages, snap and display toggles).
//!
//! The title row's geometry follows CADCraft's `chrome.rs` (storytold/cadcraft, MIT OR
//! Apache-2.0): AppKit draws the lights itself in the standard 28 pt band; moving them
//! would take AppKit calls, so the row is that band and its content is centred on the
//! lights' line.

use craft_menu::CommandRef;
use egui::{Rect, Response, Role, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::app::Studio;
use crate::i18n::{t, tf};
use crate::state::View;
use crate::theme::{self, Tokens};
use crate::widgets;

/// The macOS title band.
pub const TITLE_BAR: f32 = 28.0;
pub const TOOL_BAR: f32 = 34.0;
pub const OPTIONS_BAR: f32 = 28.0;
pub const STATUS_BAR: f32 = 24.0;

/// The traffic lights as AppKit lays them out: diameter, centre-to-centre pitch, the first
/// one's left edge.
pub const MAC_LIGHT: f32 = 12.0;
pub const MAC_LIGHT_PITCH: f32 = 20.0;
pub const MAC_LIGHTS_LEFT: f32 = 10.0;
/// Where title-row content may start: after the lights plus the gap they keep from the top.
pub const MAC_CONTENT_LEFT: f32 = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT + (TITLE_BAR - MAC_LIGHT) / 2.0;

/// Centres of the three lights in window points (for the headless screens, which paint them).
pub fn traffic_light_centres() -> [egui::Pos2; 3] {
    let c = |i: f32| pos2(MAC_LIGHTS_LEFT + MAC_LIGHT / 2.0 + i * MAC_LIGHT_PITCH, TITLE_BAR / 2.0);
    [c(0.0), c(1.0), c(2.0)]
}

/// Left edge of a title `text_w` wide in a row from `left`, `width` wide: centred, never
/// under the lights.
fn title_left(left: f32, width: f32, text_w: f32) -> f32 {
    (left + (width - text_w) / 2.0).max(left + MAC_CONTENT_LEFT)
}

/// The macOS title row: a window drag area with the title in the middle.
pub fn title_row(ui: &mut Ui, s: &Studio) {
    let c = theme::of(ui);
    let r = ui.max_rect();
    craft_window::title_bar::empty_area(ui, r);
    let galley = ui.painter().layout_no_wrap(s.window_title(), theme::proportional(12.5), c.text2);
    let x = title_left(r.left(), r.width(), galley.size().x);
    ui.painter().galley(pos2(x, r.center().y - galley.size().y / 2.0), galley, c.text2);
}

/// A tool bar command: icon, label key (tooltip and accessible name), what it runs.
struct Command {
    icon: &'static str,
    label: &'static str,
    run: &'static str,
    keys: Option<&'static str>,
}

const fn cmd(icon: &'static str, label: &'static str, run: &'static str, keys: Option<&'static str>) -> Command {
    Command { icon, label, run, keys }
}

/// The groups of the tool bar, in order. `app.import` opens the import menu.
const TOOLBAR: &[&[Command]] = &[
    &[
        cmd("file-plus", "menu.new", "app.new", Some("N")),
        cmd("folder-open", "menu.open", "app.open", Some("O")),
        cmd("save", "menu.save", "app.save", Some("S")),
    ],
    &[cmd("undo", "menu.undo", "history.undo", Some("Z")), cmd("redo", "menu.redo", "history.redo", Some("⇧Z"))],
    &[
        cmd("file-input", "toolbar.import", "app.import", None),
        cmd("printer", "menu.export_pdf", "app.export_pdf", None),
        cmd("file-output", "menu.export_mvr", "app.export_mvr", None),
    ],
    &[cmd("fit", "menu.zoom_fit", "ui.zoom_to_fit", Some("0"))],
    &[cmd("graduation-cap", "menu.tutorial", "ui.tutorial", None)],
];

const BUTTON: f32 = 26.0;
const ICON: f32 = 16.0;
const BUTTON_GAP: f32 = 2.0;
const GROUP_GAP: f32 = 17.0;

/// "Save (⌘S)": a tooltip with the key on this platform.
fn tip(ui: &Ui, label: &str, keys: Option<&str>) -> String {
    match keys {
        Some(k) if ui.ctx().os().is_mac() => format!("{label} (⌘{k})"),
        Some(k) => format!("{label} (Ctrl+{})", k.replace('⇧', "Shift+")),
        None => label.trim_end_matches('…').to_owned(),
    }
}

pub fn icon_style(c: &Tokens) -> craft_ui::buttons::IconButtonStyle {
    use craft_ui::buttons::{ButtonVisuals, IconButtonStyle};
    let v = |fill, icon| ButtonVisuals { fill, stroke: Stroke::NONE, icon };
    IconButtonStyle {
        normal: v(egui::Color32::TRANSPARENT, c.icon),
        hovered: v(c.hover.gamma_multiply(0.7), c.text),
        pressed: Some(v(c.pressed, c.text)),
        selected: v(c.accent, c.on_accent),
        corner_radius: egui::CornerRadius::same(3),
        focus_stroke: Stroke::new(1.5, c.accent),
    }
}

/// An icon button with an accessible name and a tooltip.
pub fn icon(
    ui: &mut Ui,
    name: &str,
    icon: &'static str,
    size: f32,
    icon_size: f32,
    selected: bool,
    tip: &str,
) -> Response {
    let c = theme::of(ui);
    let style = icon_style(&c);
    let r = craft_ui::buttons::IconButton::new(name, vec2(size, size), &style)
        .selected(selected)
        .show(ui, |ui, rect, colour| crate::icons::paint(ui, rect, icon, icon_size, colour));
    if crate::tutorial::mark(ui.ctx(), name, r.rect) {
        crate::tutorial::reveal(&r, name);
    }
    r.on_hover_text(tip)
}

fn enabled(s: &Studio, run: &str) -> bool {
    match run {
        "history.undo" => s.engine.can_undo(),
        "history.redo" => s.engine.can_redo(),
        "app.export_pdf" => !s.engine.doc().sheets.is_empty(),
        _ => true,
    }
}

fn run(s: &mut Studio, what: &str) {
    match what {
        "ui.tutorial" => {
            let _ = s.run("ui.tutorial", json!({ "action": "start" }));
        }
        other => crate::menu::run(s, &CommandRef::new(other)),
    }
}

/// The import menu behind the tool bar's Import button.
fn import_menu(ui: &mut Ui, s: &mut Studio) {
    for (key, what) in [
        ("menu.import_mvr", "app.import_mvr"),
        ("library.import", "app.import_gdtf"),
        ("menu.import_model", "app.import_model"),
        ("menu.import_underlay", "app.import_underlay"),
    ] {
        let b = ui.button(t(key));
        widgets::name(&b, Role::Button, format!("Menu {}", t(key)));
        if b.clicked() {
            run(s, what);
            ui.close();
        }
    }
}

/// The tool bar: command groups (those that do not fit go into a ⋯ menu), the view switch
/// and the command palette at the right end.
pub fn toolbar(ui: &mut Ui, s: &mut Studio) {
    let c = theme::of(ui);
    let bar = ui.max_rect();
    if s.ui.inset_title_bar {
        // Below the title row, but still window chrome: empty parts move the window.
        craft_window::title_bar::empty_area(ui, bar);
    }
    ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, c.separator));
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = BUTTON_GAP;
        ui.add_space(10.0);
        let right = 330.0;
        let avail = (ui.available_width() - right).max(0.0);
        let widths: Vec<f32> =
            TOOLBAR.iter().map(|g| g.len() as f32 * (BUTTON + BUTTON_GAP) - BUTTON_GAP + GROUP_GAP).collect();
        let fit = craft_ui::layout::fit_toolbar(&widths, avail, 0.0, BUTTON + GROUP_GAP).unwrap_or_else(|| {
            craft_ui::layout::ToolbarFit { overflow: (0..TOOLBAR.len()).collect(), ..Default::default() }
        });
        for (n, (i, _)) in fit.shown.iter().enumerate() {
            if n > 0 {
                separator(ui, &c);
            }
            for command in TOOLBAR[*i] {
                let label = t(command.label);
                let name = format!("Toolbar {}", label.trim_end_matches('…'));
                let r = ui
                    .add_enabled_ui(enabled(s, command.run), |ui| {
                        icon(ui, &name, command.icon, BUTTON, ICON, false, &tip(ui, label, command.keys))
                    })
                    .inner;
                if command.run == "app.import" {
                    egui::Popup::menu(&r).show(|ui| import_menu(ui, s));
                } else if r.clicked() {
                    run(s, command.run);
                }
            }
        }
        if !fit.overflow.is_empty() {
            if !fit.shown.is_empty() {
                separator(ui, &c);
            }
            let r = icon(ui, "More commands", "ellipsis", BUTTON, ICON, false, t("toolbar.more_commands"));
            egui::Popup::menu(&r).show(|ui| {
                for (n, i) in fit.overflow.iter().enumerate() {
                    if n > 0 {
                        ui.separator();
                    }
                    for command in TOOLBAR[*i] {
                        if command.run == "app.import" {
                            import_menu(ui, s);
                            continue;
                        }
                        let label = t(command.label);
                        let b = ui.add_enabled(enabled(s, command.run), egui::Button::new(label));
                        widgets::name(&b, Role::Button, format!("Menu {}", label.trim_end_matches('…')));
                        if b.clicked() {
                            run(s, command.run);
                            ui.close();
                        }
                    }
                }
            });
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            let r = icon(ui, "Command palette", "search", BUTTON, ICON, s.ui.palette.open, t("palette.search_tip"));
            if r.clicked() {
                s.ui.palette.open();
            }
            ui.add_space(8.0);
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
            if let Some(i) = widgets::segmented(ui, &c, &items) {
                let _ = s.run("ui.set_view", json!({ "view": views[i].0 }));
            }
        });
    });
}

fn separator(ui: &mut Ui, c: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(GROUP_GAP, 18.0), Sense::hover());
    ui.painter().vline(rect.center().x.round() + 0.5, rect.y_range(), Stroke::new(1.0, c.separator));
}

/// The row under the tool bar: the active tool and its options.
pub fn options_row(ui: &mut Ui, s: &mut Studio) {
    let c = theme::of(ui);
    let r = ui.max_rect();
    ui.painter().rect_filled(r, 0.0, c.panel);
    ui.painter().hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, c.separator));
    ui.horizontal_centered(|ui| {
        ui.add_space(10.0);
        if s.ui.view == View::Sheets {
            crate::icons::paint_inline(ui, "sheet", 15.0, c.icon);
            ui.label(egui::RichText::new(t("view.sheets")).font(theme::medium(theme::BODY)).color(c.text));
            ui.add_space(8.0);
            ui.label(egui::RichText::new(t("hint.sheets")).color(c.text2));
            return;
        }
        let tool = s.ui.tool;
        crate::icons::paint_inline(ui, crate::icons::tool_icon(tool), 15.0, c.icon);
        ui.label(egui::RichText::new(t(tool.key())).font(theme::medium(theme::BODY)).color(c.text));
        if crate::panels::has_options(tool) {
            ui.add_space(4.0);
            let (rect, _) = ui.allocate_exact_size(vec2(9.0, 16.0), Sense::hover());
            ui.painter().vline(rect.center().x.round() + 0.5, rect.y_range(), Stroke::new(1.0, c.separator));
            crate::panels::tool_options(ui, s);
        }
    });
}

/// The status bar: Model and sheet tabs, the readout, snap and display toggles.
pub fn status_bar(ui: &mut Ui, s: &mut Studio) {
    let c = theme::of(ui);
    let r = ui.max_rect();
    ui.painter().rect_filled(r, 0.0, c.bar);
    ui.painter().hline(r.x_range(), r.top() + 0.5, Stroke::new(1.0, c.separator));
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(6.0);
        sheet_tabs(ui, s, &c);
        ui.add_space(10.0);
        ui.spacing_mut().item_spacing.x = 8.0;
        if let Some(cur) = s.ui.gesture.cursor
            && s.ui.view != View::Sheets
        {
            let f = |v: f64| crate::panels::fmt_mm(v);
            ui.label(
                egui::RichText::new(format!("X {}   Y {}", f(cur.point[0]), f(cur.point[1])))
                    .font(theme::proportional(theme::BODY))
                    .color(c.text),
            );
            ui.label(egui::RichText::new(t(crate::panels::snap_key(cur.kind))).color(c.text2));
        }
        if let Some((msg, err)) = &s.ui.status {
            let text = egui::RichText::new(msg).color(if *err { c.warning } else { c.text2 });
            ui.add(egui::Label::new(text).truncate());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            toggles(ui, s, &c);
            ui.add_space(8.0);
            let n = s.engine.selection().len();
            if n > 0 {
                ui.label(egui::RichText::new(tf("status.selected", &[("n", &n)])).color(c.text2));
            }
            let grid = s.ui.snap.grid.map_or_else(
                || t("status.grid_off").to_owned(),
                |g| tf("status.grid", &[("mm", &crate::panels::fmt_mm(g))]),
            );
            ui.label(egui::RichText::new(grid).color(c.text2));
        });
    });
}

/// "Model" and one tab per sheet, then "+": the Model tab shows the last model view, a sheet
/// tab that sheet in the Sheets mode.
fn sheet_tabs(ui: &mut Ui, s: &mut Studio, c: &Tokens) {
    let doc = s.engine.doc();
    let current = crate::sheets::current(doc, &s.ui.sheets);
    let mut tabs: Vec<(String, String, bool, Option<stagecraft_core::Id>)> =
        vec![(t("status.model").to_owned(), "Model tab".to_owned(), s.ui.view != View::Sheets, None)];
    for sh in doc.sheets_sorted() {
        let on = s.ui.view == View::Sheets && current == Some(sh.id);
        tabs.push((sh.number.clone(), format!("Sheet tab {}", sh.number), on, Some(sh.id)));
    }
    let mut clicked = None;
    for (label, a11y, on, id) in tabs {
        let galley = ui.painter().layout_no_wrap(label.clone(), theme::body(), if on { c.text } else { c.text2 });
        let size = vec2(galley.size().x + 22.0, STATUS_BAR - 5.0);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        widgets::name(&resp, Role::Tab, &a11y);
        let fill = if on {
            c.tab_active
        } else if resp.hovered() {
            c.control.gamma_multiply(0.5)
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 2.0, fill);
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, c.text);
        let resp = match id {
            Some(id) => resp.on_hover_text(doc.sheets.get(&id).map_or(String::new(), |sh| sh.name.clone())),
            None => resp.on_hover_text(t("status.model_tip")),
        };
        if resp.clicked() {
            clicked = Some(id);
        }
        if id.is_none() {
            let r = icon(ui, "New sheet tab", "plus", 18.0, 12.0, false, t("sheets.new"));
            if r.clicked() {
                crate::sheets::new_sheet(s);
                let _ = s.run("ui.set_view", json!({ "view": View::Sheets }));
                return;
            }
        }
    }
    match clicked {
        Some(None) if s.ui.view == View::Sheets => {
            let _ = s.run("ui.set_view", json!({ "view": s.ui.model_view }));
        }
        Some(Some(id)) => {
            let _ = s.run("ui.set_view", json!({ "view": View::Sheets }));
            let _ = s.run("ui.sheet", json!({ "sheet": id }));
            s.ui.sheets.viewport = None;
        }
        _ => {}
    }
}

/// Snap and display toggles at the right end of the status bar.
fn toggles(ui: &mut Ui, s: &mut Studio, c: &Tokens) {
    let snap = &s.ui.snap;
    let theme_label = match s.ui.theme {
        crate::state::ThemePref::System => t("theme.system"),
        crate::state::ThemePref::Light => t("theme.light"),
        crate::state::ThemePref::Dark => t("theme.dark"),
    };
    // Right to left: the theme ends the row.
    let items: [(&'static str, &str, bool, String); 6] = [
        ("sun-moon", "Toggle theme", false, tf("toggle.theme", &[("theme", &theme_label)])),
        ("spline", "Toggle constraint colours", s.ui.sketch.colours, t("toggle.colours").to_owned()),
        ("waypoints", "Toggle sketch", s.ui.sketch.show, t("toggle.sketch").to_owned()),
        ("truss", "Toggle truss snap", snap.truss, t("toggle.truss").to_owned()),
        ("magnet", "Toggle object snap", snap.endpoints || snap.midpoints, t("toggle.objects").to_owned()),
        ("grid", "Toggle grid snap", snap.grid.is_some(), t("toggle.grid").to_owned()),
    ];
    let mut hit = None;
    for (i, (ic, name, on, tip)) in items.iter().enumerate() {
        let size = vec2(22.0, 18.0);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(Role::CheckBox, true, *on, *name));
        let p = ui.painter();
        if *on {
            p.rect_filled(rect, 3.0, c.toggle_on.gamma_multiply(0.3));
        } else if resp.hovered() {
            p.rect_filled(rect, 3.0, c.hover.gamma_multiply(0.5));
        }
        let colour = if *on { c.text } else { c.text2 };
        crate::icons::paint(ui, Rect::from_center_size(rect.center(), vec2(14.0, 14.0)), ic, 14.0, colour);
        if *on {
            ui.painter().hline(rect.x_range().shrink(5.0), rect.bottom() - 0.5, Stroke::new(1.5, c.toggle_on));
        }
        if resp.on_hover_text(tip).clicked() {
            hit = Some(i);
        }
    }
    let Some(i) = hit else { return };
    let snap = s.ui.snap.clone();
    let _ = match i {
        0 => {
            let next = match s.ui.theme {
                crate::state::ThemePref::System if ui.visuals().dark_mode => "light",
                crate::state::ThemePref::System => "dark",
                crate::state::ThemePref::Light => "dark",
                crate::state::ThemePref::Dark => "light",
            };
            s.run("ui.set_theme", json!({ "theme": next }))
        }
        1 => s.run("ui.sketch", json!({ "colours": !s.ui.sketch.colours })),
        2 => s.run("ui.sketch", json!({ "show": !s.ui.sketch.show })),
        3 => s.run("ui.set_snap", json!({ "truss": !snap.truss })),
        4 => {
            let on = !(snap.endpoints || snap.midpoints);
            s.run("ui.set_snap", json!({ "endpoints": on, "midpoints": on }))
        }
        _ => {
            let grid = if snap.grid.is_some() { 0.0 } else { s.ui.grid_step };
            s.run("ui.set_snap", json!({ "grid": grid }))
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_row_centres_and_clears_the_traffic_lights() {
        let margin = (TITLE_BAR - MAC_LIGHT) / 2.0;
        let lights_end = MAC_LIGHTS_LEFT + 2.0 * MAC_LIGHT_PITCH + MAC_LIGHT;
        assert_eq!(MAC_CONTENT_LEFT - lights_end, margin);
        for p in traffic_light_centres() {
            assert_eq!(p.y, TITLE_BAR / 2.0, "the lights sit on the row's centre line");
        }
        assert_eq!(title_left(0.0, 1000.0, 200.0), 400.0);
        assert_eq!(title_left(0.0, 300.0, 280.0), MAC_CONTENT_LEFT);
    }
}
