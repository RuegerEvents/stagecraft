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

/// Everything a panel can be. New panels (a tutorial) are a variant here, a label and a
/// body; [`Workspace::reconcile`] puts panels an old saved layout does not know where
/// [`Action::Open`] puts them.
pub const PANELS: [PanelKind; 8] = [
    PanelKind::Canvas,
    PanelKind::Layers,
    PanelKind::Tags,
    PanelKind::Sheets,
    PanelKind::Library,
    PanelKind::Share,
    PanelKind::Inspector,
    PanelKind::Table,
];

const LEFT_WIDTH: f32 = 272.0;
const RIGHT_WIDTH: f32 = 292.0;
const TABLE_HEIGHT: f32 = 230.0;

/// The arrangement of a new user and of View > Reset Layout: layers, tags and sheets as
/// tabs above the library on the left, the inspector on the right, the table under the
/// canvas.
pub fn default_layout() -> Layout<PanelKind> {
    use PanelKind::*;
    let left = Node::Dock {
        side: Side::Start,
        columns: vec![Column::new(
            vec![Group::new(vec![Layers, Tags, Sheets], 0), Group::new(vec![Library, Share], 0)],
            LEFT_WIDTH,
        )],
    };
    let right =
        Node::Dock { side: Side::End, columns: vec![Column::new(vec![Group::new(vec![Inspector], 0)], RIGHT_WIDTH)] };
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

pub fn default_workspace() -> Workspace<PanelKind> {
    Workspace::new(default_layout())
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
    underline_active(ui.ctx(), id, &layout);
    if !out.actions.is_empty()
        && let Err(e) = apply(&mut s.ui.layout, out.actions)
    {
        s.ui.error(e);
    }
}

/// The active tab of every group gets a 2 pt accent line along its bottom (Style A).
/// craft-ui fills the active tab; the line is drawn over it, on the tab's own layer.
fn underline_active(ctx: &egui::Context, id: egui::Id, layout: &Layout<PanelKind>) {
    fn walk(n: &Node<PanelKind>, out: &mut Vec<PanelKind>) {
        match n {
            Node::Tabs { panels, active } if panels.len() > 1 || panels.first() != Some(&PanelKind::Canvas) => {
                out.extend(panels.get(*active).copied())
            }
            Node::Split { first, second, .. } => {
                walk(first, out);
                walk(second, out);
            }
            Node::Dock { columns, .. } => {
                for g in columns.iter().filter(|c| !c.iconic).flat_map(|c| &c.groups) {
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
    for p in active {
        if let Some(r) = ctx.read_response(id.with(("tab", p))) {
            let line = egui::Rect::from_min_max(
                egui::pos2(r.rect.left() + 6.0, r.rect.bottom() - 2.0),
                r.rect.right_bottom() - egui::vec2(6.0, 0.0),
            );
            ctx.layer_painter(r.layer_id).with_clip_rect(r.interact_rect).rect_filled(line, 1.0, crate::theme::ACCENT);
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
            let c = crate::theme::chrome(ui.visuals().dark_mode);
            let r = ui.max_rect();
            ui.painter().rect_filled(r, 0.0, c.panel);
            // The tab strip's bottom edge.
            ui.painter().hline(r.x_range(), r.top() + 0.5, egui::Stroke::new(1.0, c.separator));
        }
        let scroll = |ui: &mut Ui, s: &mut Studio, f: fn(&mut Ui, &mut Studio)| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("dock_body", *panel))
                    .auto_shrink([false, false])
                    .show(ui, |ui| f(ui, s));
            });
        };
        match panel {
            PanelKind::Canvas => canvas(ui, s),
            PanelKind::Layers => scroll(ui, s, crate::panels::layers),
            PanelKind::Tags => scroll(ui, s, crate::panels::tags),
            PanelKind::Sheets => scroll(ui, s, crate::sheets::list_panel),
            PanelKind::Library => scroll(ui, s, crate::library::show),
            PanelKind::Share => scroll(ui, s, crate::library::share_panel),
            PanelKind::Inspector if s.ui.view == View::Sheets => scroll(ui, s, crate::sheets::inspector),
            PanelKind::Inspector => scroll(ui, s, crate::inspector::show),
            PanelKind::Table => {
                egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| crate::table::show(ui, s));
            }
        }
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
            PanelKind::Table => egui::vec2(320.0, 100.0),
            _ => egui::vec2(160.0, 60.0),
        };
        PanelLimits { min, ..Default::default() }
    }

    fn dock_heights(&self, panel: &PanelKind) -> Option<DockHeights> {
        match panel {
            PanelKind::Layers | PanelKind::Tags | PanelKind::Sheets => Some(DockHeights::new(300.0).compact(150.0)),
            PanelKind::Library | PanelKind::Share => Some(DockHeights::new(200.0).compact(120.0).fill(260.0)),
            _ => None,
        }
    }

    fn floating_size(&self, panel: &PanelKind) -> Option<egui::Vec2> {
        Some(match panel {
            PanelKind::Table => egui::vec2(720.0, 320.0),
            PanelKind::Inspector => egui::vec2(300.0, 520.0),
            _ => egui::vec2(280.0, 400.0),
        })
    }
}

/// The centre: plan, 3D, both side by side, or the sheet.
fn canvas(ui: &mut Ui, s: &mut Studio) {
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
}

/// What StageCraft keeps per user (`config.toml` in the platform's config folder, e.g.
/// `~/Library/Application Support/stagecraft/`; `STAGECRAFT_CONFIG_DIR` overrides it,
/// empty = keep nothing).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub appearance: Appearance,
    pub layout: SavedLayout,
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
            assert!(ws.layout.contains(&p), "{p:?}");
        }
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
        let prefs =
            Prefs { appearance: Appearance { theme: ThemePref::Dark }, layout: SavedLayout { workspace: Some(ws) } };
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
