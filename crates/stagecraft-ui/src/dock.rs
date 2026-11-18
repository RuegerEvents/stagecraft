//! Dockable panels through `craft_ui::docking`: the plan/3D/split/sheets canvas is a
//! protected centre, every other panel (layers, tags, sheets, library, GDTF Share,
//! inspector, table) is a tab that moves between docks, splits and floating windows.
//!
//! The layout lives in [`UiState::layout`](crate::state::UiState) as a
//! [`Workspace`]; the renderer only reports [`Action`]s, which go through
//! [`apply`] like the `ui.*` layout commands do. [`Prefs`] saves it per user with
//! `craft-settings`.

use craft_ui::docking::{
    Action, Column, DockArea, DockContent, DockHeights, DockStyle, Group, Layout, Node, PanelLimits, Permissions, Side,
    Workspace,
};
use craft_ui::layout::{SplitAxis, SplitSize};
use egui::Ui;
use serde::{Deserialize, Serialize};

use crate::app::Studio;
use crate::i18n::t;
use crate::state::{PanelKind, ThemePref, View};

/// Everything a panel can be. A new panel is a variant here, a label and a body;
/// [`Workspace::reconcile`] puts panels an old saved layout does not know where the
/// default layout has them (hidden ones stay hidden).
pub const PANELS: [PanelKind; 10] = [
    PanelKind::Canvas,
    PanelKind::Tools,
    PanelKind::Layers,
    PanelKind::Tags,
    PanelKind::Sheets,
    PanelKind::Library,
    PanelKind::Share,
    PanelKind::Inspector,
    PanelKind::Table,
    PanelKind::Tutorial,
];

const LEFT_WIDTH: f32 = 204.0;
const RIGHT_WIDTH: f32 = 300.0;
const TABLE_HEIGHT: f32 = 210.0;

/// The arrangement of a new user and of View > Reset Layout, as in CADCraft: the tool
/// palette on the left; on the right the browsers (Layers, Tags, Sheets, Library, GDTF
/// Share as icon tabs) above the Inspector, the tutorial above both when shown; the table
/// under the canvas.
pub fn default_layout() -> Layout<PanelKind> {
    use PanelKind::*;
    let left =
        Node::Dock { side: Side::Start, columns: vec![Column::new(vec![Group::new(vec![Tools], 0)], LEFT_WIDTH)] };
    let right = Node::Dock {
        side: Side::End,
        columns: vec![Column::new(
            vec![
                Group::new(vec![Tutorial], 0),
                Group::new(vec![Layers, Tags, Sheets, Library, Share], 0),
                Group::new(vec![Inspector], 0),
            ],
            RIGHT_WIDTH,
        )],
    };
    let centre = Node::Split {
        axis: SplitAxis::Vertical,
        size: SplitSize::FixedSecond(TABLE_HEIGHT),
        first: Box::new(Node::Tabs { panels: vec![Canvas], active: 0 }),
        second: Box::new(Node::Tabs { panels: vec![Table], active: 0 }),
    };
    Layout {
        root: Some(Node::Split {
            axis: SplitAxis::Horizontal,
            size: SplitSize::FixedFirst(LEFT_WIDTH),
            first: Box::new(left),
            second: Box::new(Node::Split {
                axis: SplitAxis::Horizontal,
                size: SplitSize::FixedSecond(RIGHT_WIDTH),
                first: Box::new(centre),
                second: Box::new(right),
            }),
        }),
        floating: Vec::new(),
    }
}

/// The default layout with the tutorial hidden; it opens above the inspector.
pub fn default_workspace() -> Workspace<PanelKind> {
    let mut ws = Workspace::new(default_layout());
    set_visible(&mut ws, PanelKind::Tutorial, false).expect("the tutorial can close");
    ws
}

pub fn permissions(p: &PanelKind) -> Permissions {
    match p {
        PanelKind::Canvas => Permissions { accepts_splits: true, ..Permissions::PROTECTED },
        _ => Permissions::default(),
    }
}

/// Apply layout actions all or none, as the renderer and the `ui.*` commands do.
pub fn apply(ws: &mut Workspace<PanelKind>, actions: Vec<Action<PanelKind>>) -> Result<(), String> {
    ws.apply_all(actions, permissions).map(drop).map_err(|e| format!("{e:?}"))
}

/// Show or hide a panel; a panel shown again goes back where it was.
pub fn set_visible(ws: &mut Workspace<PanelKind>, panel: PanelKind, visible: bool) -> Result<(), String> {
    ws.set_visible(panel, visible, None, permissions).map_err(|e| format!("{e:?}"))
}

/// Show a panel and make it the active tab of its group.
pub fn bring_forward(ws: &mut Workspace<PanelKind>, panel: PanelKind) {
    if !ws.layout.contains(&panel) {
        let _ = set_visible(ws, panel, true);
    }
    let _ = apply(ws, vec![Action::Activate { panel }]);
}

/// A saved layout fitted to the panels of this build; the default if it does not validate.
pub fn load(saved: Option<Workspace<PanelKind>>) -> Workspace<PanelKind> {
    let default = default_workspace();
    let Some(mut ws) = saved else { return default };
    if ws.validate().is_err() {
        return default;
    }
    ws.reconcile(|p| PANELS.contains(p), permissions, &default);
    if ws.validate().is_err() { default } else { ws }
}

/// What the window shows: the user's layout, except that the Sheets mode brings the sheet
/// list to the front and leaves out the table, so the page has room. Actions the renderer
/// reports against it are panel-anchored and apply to the real layout as well.
fn display(ws: &Workspace<PanelKind>, view: View) -> Layout<PanelKind> {
    let mut layout = ws.layout.clone();
    if view == View::Sheets {
        let _ = layout.apply_with_permissions(Action::Close { panel: PanelKind::Table }, permissions);
        if layout.contains(&PanelKind::Sheets) {
            let _ = layout.apply(Action::Activate { panel: PanelKind::Sheets });
        }
    }
    layout
}

pub fn label(p: PanelKind) -> &'static str {
    t(match p {
        PanelKind::Canvas => "panel.canvas",
        PanelKind::Layers => "panel.layers",
        PanelKind::Tags => "panel.tags",
        PanelKind::Sheets => "panel.sheets",
        PanelKind::Library => "panel.library",
        PanelKind::Share => "panel.share",
        PanelKind::Inspector => "panel.inspector",
        PanelKind::Table => "panel.table",
        PanelKind::Tutorial => "panel.tutorial",
        PanelKind::Tools => "panel.tools",
    })
}

/// The icon of a panel's tab (the right dock's panels show icons, the name in the body's
/// title row); `None` keeps a text tab.
pub fn icon(p: PanelKind) -> Option<&'static str> {
    Some(match p {
        PanelKind::Layers => "layers",
        PanelKind::Tags => "tags",
        PanelKind::Sheets => "sheet",
        PanelKind::Library => "library",
        PanelKind::Share => "cloud-download",
        PanelKind::Canvas | PanelKind::Inspector | PanelKind::Tutorial | PanelKind::Table | PanelKind::Tools => {
            return None;
        }
    })
}

/// The icon of a panel collapsed to icons.
fn iconic(p: PanelKind) -> &'static str {
    icon(p).unwrap_or(match p {
        PanelKind::Inspector => "sliders",
        PanelKind::Tutorial => "graduation-cap",
        PanelKind::Table => "table",
        _ => "box",
    })
}

pub const DOCK_ID: &str = "dock";

/// The dock area: everything below the toolbar and above the status bar.
pub fn show(ui: &mut Ui, s: &mut Studio) {
    let layout = display(&s.ui.layout, s.ui.view);
    let mut style = DockStyle::from_ui(ui);
    crate::theme::dock_style(&mut style, ui.visuals().dark_mode);
    let id = egui::Id::unique(DOCK_ID);
    let out = DockArea::new(id).show_content(ui, &layout, &style, &mut Bodies { s });
    tab_icons(ui, id, &layout);
    if !out.actions.is_empty()
        && let Err(e) = apply(&mut s.ui.layout, out.actions)
    {
        s.ui.error(e);
    }
}

/// The active panel of every tab group.
fn active_panels(layout: &Layout<PanelKind>) -> Vec<PanelKind> {
    fn walk(n: &Node<PanelKind>, out: &mut Vec<PanelKind>) {
        match n {
            Node::Tabs { panels, active } => out.extend(panels.get(*active).copied()),
            Node::Split { first, second, .. } => {
                walk(first, out);
                walk(second, out);
            }
            Node::Dock { columns, .. } => {
                for g in columns.iter().flat_map(|c| &c.groups) {
                    out.extend(g.active_panel().copied())
                }
            }
            Node::Stack { entries } => out.extend(entries.iter().filter(|e| e.open).map(|e| e.panel)),
            _ => {}
        }
    }
    let mut active = Vec::new();
    for n in layout.root.iter().chain(layout.floating.iter().map(|f| &f.root)) {
        walk(n, &mut active);
    }
    active
}

/// Icons on the icon tabs, painted over craft-ui's tab (whose label is blank) on the tab's
/// own layer; the tab keeps the panel's name for accessibility and gets it as a tooltip.
fn tab_icons(ui: &Ui, id: egui::Id, layout: &Layout<PanelKind>) {
    let c = crate::theme::of(ui);
    let active = active_panels(layout);
    for p in layout.panels() {
        let Some(icon) = icon(*p) else { continue };
        let Some(r) = ui.ctx().read_response(id.with(("tab", *p))) else { continue };
        let on = active.contains(p);
        let painter = ui.ctx().layer_painter(r.layer_id).with_clip_rect(r.interact_rect);
        let colour = if on { c.text } else { c.text2 };
        let rect = egui::Rect::from_center_size(r.rect.center(), egui::vec2(16.0, 16.0));
        crate::icons::paint_with(&painter, ui.ctx(), rect, icon, 16.0, colour);
        if r.hovered() {
            r.on_hover_text(label(*p));
        }
    }
}

struct Bodies<'a> {
    s: &'a mut Studio,
}

impl DockContent<PanelKind> for Bodies<'_> {
    fn body(&mut self, ui: &mut Ui, panel: &PanelKind) {
        let s = &mut *self.s;
        if *panel != PanelKind::Canvas {
            let c = crate::theme::of(ui);
            let r = ui.max_rect();
            ui.painter().rect_filled(r, 0.0, c.panel);
            // The tab strip's bottom edge.
            ui.painter().hline(r.x_range(), r.top() + 0.5, egui::Stroke::new(1.0, c.separator));
        }
        if icon(*panel).is_some() {
            title_row(ui, s, *panel);
        }
        let scroll = |ui: &mut Ui, s: &mut Studio, f: fn(&mut Ui, &mut Studio)| {
            egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 8, top: 4, bottom: 6 }).show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("dock_body", *panel))
                    .auto_shrink([false, false])
                    .show(ui, |ui| f(ui, s));
            });
        };
        match panel {
            PanelKind::Canvas => canvas(ui, s),
            PanelKind::Tools => crate::toolsets::show(ui, s),
            PanelKind::Layers => scroll(ui, s, crate::panels::layers),
            PanelKind::Tags => scroll(ui, s, crate::panels::tags),
            PanelKind::Sheets => scroll(ui, s, crate::sheets::list_panel),
            PanelKind::Library => scroll(ui, s, crate::library::show),
            PanelKind::Share => scroll(ui, s, crate::library::share_panel),
            PanelKind::Inspector if s.ui.view == View::Sheets => scroll(ui, s, crate::sheets::inspector),
            PanelKind::Inspector => scroll(ui, s, crate::inspector::show),
            PanelKind::Tutorial => scroll(ui, s, crate::tutorial::panel),
            PanelKind::Table => {
                egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| crate::table::show(ui, s));
            }
        }
    }

    fn label(
        &mut self,
        ui: &Ui,
        panel: &PanelKind,
        logical: &str,
        font: &egui::FontId,
        color: egui::Color32,
    ) -> egui::WidgetText {
        let _ = (ui, color);
        if icon(*panel).is_some() {
            // Room for the icon `tab_icons` paints; the name stays the accessible label.
            return egui::RichText::new("   ").font(font.clone()).into();
        }
        logical.into()
    }

    fn paint_icon(
        &mut self,
        painter: &egui::Painter,
        rect: egui::Rect,
        panel: &PanelKind,
        color: egui::Color32,
    ) -> bool {
        crate::icons::paint_with(painter, painter.ctx(), rect, iconic(*panel), rect.height().min(16.0), color);
        true
    }

    fn group_header_width(&self, _panels: &[PanelKind], active: &PanelKind, _floating: bool, _strip: f32) -> f32 {
        if icon(*active).is_none() && *active != PanelKind::Canvas { 26.0 } else { 0.0 }
    }

    fn group_header(&mut self, ui: &mut Ui, _panels: &[PanelKind], active: &PanelKind, _floating: bool) {
        let r = ui.max_rect();
        let b = egui::Rect::from_center_size(r.center(), egui::vec2(20.0, 20.0));
        panel_menu_button(ui, self.s, *active, b);
    }

    fn logical_label(&self, panel: &PanelKind) -> String {
        label(*panel).to_owned()
    }

    fn permissions(&self, panel: &PanelKind) -> Permissions {
        permissions(panel)
    }

    fn limits(&self, panel: &PanelKind) -> PanelLimits {
        let min = match panel {
            PanelKind::Inspector | PanelKind::Layers | PanelKind::Library => egui::vec2(220.0, 80.0),
            PanelKind::Tools => egui::vec2(120.0, 120.0),
            PanelKind::Table => egui::vec2(320.0, 100.0),
            _ => egui::vec2(160.0, 60.0),
        };
        PanelLimits { min, ..Default::default() }
    }

    fn dock_heights(&self, panel: &PanelKind) -> Option<DockHeights> {
        match panel {
            PanelKind::Layers | PanelKind::Tags | PanelKind::Sheets | PanelKind::Library | PanelKind::Share => {
                Some(DockHeights::new(190.0).compact(120.0))
            }
            PanelKind::Tutorial => Some(DockHeights::new(230.0).compact(150.0)),
            _ => None,
        }
    }

    fn floating_size(&self, panel: &PanelKind) -> Option<egui::Vec2> {
        Some(match panel {
            PanelKind::Table => egui::vec2(720.0, 320.0),
            PanelKind::Inspector => egui::vec2(300.0, 520.0),
            PanelKind::Tools => egui::vec2(190.0, 420.0),
            _ => egui::vec2(280.0, 400.0),
        })
    }
}

/// The row at the top of an icon-tabbed panel: its name, and the ≡ panel menu.
fn title_row(ui: &mut Ui, s: &mut Studio, panel: PanelKind) {
    let c = crate::theme::of(ui);
    let full = ui.max_rect();
    let row = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), 26.0));
    ui.painter().text(
        egui::pos2(row.left() + 10.0, row.center().y),
        egui::Align2::LEFT_CENTER,
        label(panel),
        crate::theme::bold(12.5),
        c.text,
    );
    let b = egui::Rect::from_center_size(egui::pos2(row.right() - 16.0, row.center().y), egui::vec2(20.0, 20.0));
    panel_menu_button(ui, s, panel, b);
    ui.advance_cursor_after_rect(row);
}

/// ≡: float, close the panel, reset the layout.
fn panel_menu_button(ui: &mut Ui, s: &mut Studio, panel: PanelKind, rect: egui::Rect) {
    let c = crate::theme::of(ui);
    let resp = ui.interact(rect, ui.scope_id().with(("panel_menu", panel)), egui::Sense::click());
    crate::widgets::name(&resp, egui::Role::Button, format!("Panel menu {}", label(panel)));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, c.hover.gamma_multiply(0.6));
    }
    crate::icons::paint(ui, rect, "menu", 14.0, if resp.hovered() { c.text } else { c.text2 });
    let resp = resp.on_hover_text(t("panel.menu"));
    egui::Popup::menu(&resp).show(|ui| {
        let float = ui.button(t("panel.float"));
        crate::widgets::name(&float, egui::Role::Button, format!("Float {}", label(panel)));
        if float.clicked() {
            let _ = s.run("ui.float_panel", serde_json::json!({ "panel": panel }));
            ui.close();
        }
        let close = ui.button(t("panel.close"));
        crate::widgets::name(&close, egui::Role::Button, format!("Close {}", label(panel)));
        if close.clicked() {
            let _ = s.run("ui.toggle_panel", serde_json::json!({ "panel": panel, "visible": false }));
            ui.close();
        }
        ui.separator();
        let reset = ui.button(t("menu.reset_layout"));
        crate::widgets::name(&reset, egui::Role::Button, "Reset layout");
        if reset.clicked() {
            let _ = s.run("ui.reset_layout", serde_json::Value::Null);
            ui.close();
        }
    });
}

/// The centre: plan, 3D, both side by side, or the sheet.
fn canvas(ui: &mut Ui, s: &mut Studio) {
    let rect = ui.max_rect();
    match s.ui.view {
        View::Plan => crate::canvas::show(ui, s),
        View::ThreeD => crate::view3d::show(ui, s),
        View::Sheets => crate::sheets::show(ui, s),
        View::Split => {
            let (left, right) = ui.max_rect().split_left_right_at_fraction(0.5);
            ui.scope_builder(egui::UiBuilder::new().max_rect(left.shrink2(egui::vec2(0.5, 0.0))), |ui| {
                crate::canvas::show(ui, s)
            });
            ui.scope_builder(egui::UiBuilder::new().max_rect(right.shrink2(egui::vec2(0.5, 0.0))), |ui| {
                crate::view3d::show(ui, s)
            });
            let x = left.right();
            ui.painter().vline(x, left.y_range(), ui.visuals().widgets.noninteractive.bg_stroke);
        }
    }
    crate::cmdline::show(ui, s, rect);
}

/// What StageCraft keeps per user (`config.toml` in the platform's config folder, e.g.
/// `~/Library/Application Support/stagecraft/`; `STAGECRAFT_CONFIG_DIR` overrides it,
/// empty = keep nothing).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub appearance: Appearance,
    pub layout: SavedLayout,
    /// The drawing tools' options (heights, sizes, profiles), without the fixture type,
    /// which belongs to a document's library.
    pub tools: crate::state::ToolOptions,
    pub palette: PalettePrefs,
}

/// The command palette's memory: recently run commands, newest first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PalettePrefs {
    pub recent: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: ThemePref,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedLayout {
    pub workspace: Option<Workspace<PanelKind>>,
}

impl craft_settings::Settings for Prefs {
    const APP_ID: &'static str = "stagecraft";
    const SCHEMA_VERSION: u32 = 1;

    fn migrate(_from: u32, _doc: &mut craft_settings::DocumentMut) -> Result<(), craft_settings::MigrateError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_ui::docking::Placement;

    #[test]
    fn the_default_layout_is_valid_and_has_every_panel() {
        let ws = default_workspace();
        ws.validate().unwrap();
        for p in PANELS {
            assert!(ws.layout.contains(&p) || ws.hidden.iter().any(|(h, _)| *h == p), "{p:?}");
        }
        assert!(!ws.layout.contains(&PanelKind::Tutorial), "the tutorial opens on demand");
        let mut shown = ws.clone();
        set_visible(&mut shown, PanelKind::Tutorial, true).unwrap();
        assert!(shown.layout.contains(&PanelKind::Tutorial));
    }

    #[test]
    fn layouts_survive_config_toml() {
        let mut ws = default_workspace();
        apply(&mut ws, vec![Action::Float { panel: PanelKind::Library, rect: [300.0, 200.0, 280.0, 360.0] }]).unwrap();
        set_visible(&mut ws, PanelKind::Tags, false).unwrap();
        apply(
            &mut ws,
            vec![Action::Move {
                panel: PanelKind::Share,
                anchor: PanelKind::Inspector,
                placement: Placement::Tab { before: None },
            }],
        )
        .unwrap();
        let tools = crate::state::ToolOptions { floor_z: 1000.0, floor_thickness: 200.0, ..Default::default() };
        let prefs = Prefs {
            appearance: Appearance { theme: ThemePref::Dark },
            layout: SavedLayout { workspace: Some(ws) },
            tools,
            palette: PalettePrefs { recent: vec!["patch.auto".into(), "ui.set_view {\"view\":\"3d\"}".into()] },
        };
        let dir = tempfile::tempdir().unwrap();
        {
            let (mut store, _) = craft_settings::SettingsStore::<Prefs>::open_at(dir.path());
            store.update(|p| *p = prefs.clone());
            store.flush().unwrap();
        }
        let (store, report) = craft_settings::SettingsStore::<Prefs>::open_at(dir.path());
        assert!(!report.has_problems(), "{report:?}");
        assert_eq!(store.get(), &prefs);
        let back = load(store.get().layout.workspace.clone());
        assert_eq!(Some(&back), prefs.layout.workspace.as_ref());
    }

    #[test]
    fn an_old_layout_gets_new_panels_and_a_broken_one_the_default() {
        let mut old = default_workspace();
        set_visible(&mut old, PanelKind::Share, false).unwrap();
        old.hidden.clear();
        let ws = load(Some(old));
        assert!(ws.layout.contains(&PanelKind::Share) || ws.hidden.iter().any(|(p, _)| *p == PanelKind::Share));
        let mut broken = default_workspace();
        broken.version = 99;
        assert_eq!(load(Some(broken)), default_workspace());
    }
}
