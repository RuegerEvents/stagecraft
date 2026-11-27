//! Dockable panels through craft-ui's `DockArea` (`craft_ui::patterns::dock`) over the
//! `craft_behavior::dock` model: the plan/3D/split/sheets canvas is a protected centre, every
//! other panel (layers, tags, sheets, library, GDTF Share, inspector, table) is a tab that
//! moves between docks, splits and floating windows.
//!
//! The layout lives in [`UiState::layout`](crate::state::UiState) as a [`Workspace`]; the dock
//! shows a copy of it ([`Live`]) and only reports [`Action`]s, which go through [`apply`] like
//! the `ui.*` layout commands do. [`Prefs`] saves it per user with `craft-settings`. Every body
//! is StageCraft's own egui code (`PanelSpec::host_body`).

use std::sync::Arc;

use craft_behavior::dock::{Action, Column, DockEdge, Group, Layout, Node, Permissions, SplitSize, Workspace};
use craft_behavior::flow::Axis;
use craft_ui::contract::{Bind, BodyCx, MemoKey, MemoProps, PlainData, SizeLimits, Tracked};
use craft_ui::docking::{DockConfig, DockHeights, DockPanels, GroupAt, PanelSpec, PanelSpecs};
use craft_ui::patterns::dock::DockArea;
use craft_ui::primitives::{Glyph, IconSpec};
use craft_ui::style::UiTokens;
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

pub const LEFT_WIDTH: f32 = 204.0;
const RIGHT_WIDTH: f32 = 300.0;
const TABLE_HEIGHT: f32 = 210.0;

fn dock_column(side: DockEdge, width: f32, groups: Vec<Group<PanelKind>>) -> Node<PanelKind> {
    Node::dock(side, vec![Column::new(groups).with_width(width)])
}

/// The arrangement of a new user and of View > Reset Layout, as in CADCraft: the tool
/// palette on the left; on the right the browsers (Layers, Tags, Sheets as tabs, Library
/// and GDTF Share in a group below them) above the Inspector, the tutorial above both when shown; the table
/// under the canvas.
pub fn default_layout() -> Layout<PanelKind> {
    use PanelKind::*;
    let left = dock_column(DockEdge::Start, LEFT_WIDTH, vec![Group::new(vec![Tools], 0)]);
    let right = dock_column(
        DockEdge::End,
        RIGHT_WIDTH,
        vec![
            Group::new(vec![Tutorial], 0),
            Group::new(vec![Layers, Tags, Sheets], 0),
            Group::new(vec![Library, Share], 0),
            Group::new(vec![Inspector], 0),
        ],
    );
    let centre = Node::split(
        Axis::Y,
        SplitSize::FixedSecond(TABLE_HEIGHT),
        Node::tabs(vec![Canvas], 0),
        Node::tabs(vec![Table], 0),
    );
    Layout::new(
        Some(Node::split(
            Axis::X,
            SplitSize::FixedFirst(LEFT_WIDTH),
            left,
            Node::split(Axis::X, SplitSize::FixedSecond(RIGHT_WIDTH), centre, right),
        )),
        Vec::new(),
    )
}

/// The default layout with the tutorial hidden; it opens above the inspector.
pub fn default_workspace() -> Workspace<PanelKind> {
    let mut ws = Workspace::new(default_layout());
    set_visible(&mut ws, PanelKind::Tutorial, false).expect("the tutorial can close");
    ws
}

pub fn permissions(p: &PanelKind) -> Permissions {
    match p {
        PanelKind::Canvas => Permissions::PROTECTED.accepts_splits(true),
        _ => Permissions::default(),
    }
}

/// Apply layout actions all or none, as the renderer and the `ui.*` commands do.
pub fn apply(ws: &mut Workspace<PanelKind>, actions: Vec<Action<PanelKind>>) -> Result<(), String> {
    ws.apply_all(actions, false, permissions).map(drop).map_err(|e| format!("{e:?}"))
}

/// Show or hide a panel; a panel shown again goes back where it was.
pub fn set_visible(ws: &mut Workspace<PanelKind>, panel: PanelKind, visible: bool) -> Result<(), String> {
    ws.set_visible(panel, visible, None, false, permissions).map_err(|e| format!("{e:?}"))
}

/// Show a panel and make it the active tab of its group.
pub fn bring_forward(ws: &mut Workspace<PanelKind>, panel: PanelKind) {
    if !ws.layout().contains(&panel) {
        let _ = set_visible(ws, panel, true);
    }
    let _ = apply(ws, vec![Action::activate(panel)]);
}

/// Back to the default arrangement (View > Reset Layout).
pub fn reset(ws: &mut Workspace<PanelKind>) {
    ws.replace(Workspace::new(default_layout()));
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
/// list to the front and leaves out the table (so the page has room) and the fixture
/// library (so the Inspector has room for the viewport's settings). Actions the renderer
/// reports against it are panel-anchored and apply to the real layout as well.
fn display(ws: &Workspace<PanelKind>, view: View) -> Workspace<PanelKind> {
    let mut shown = ws.clone();
    if view == View::Sheets {
        for p in [PanelKind::Table, PanelKind::Library, PanelKind::Share] {
            let _ = shown.apply(Action::close(p), false, permissions);
        }
        if shown.layout().contains(&PanelKind::Sheets) {
            let _ = shown.apply(Action::activate(PanelKind::Sheets), false, permissions);
        }
    }
    shown
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

/// The icon of a panel collapsed to icons and of its flyout cell.
fn icon(p: PanelKind) -> &'static str {
    match p {
        PanelKind::Layers => "layers",
        PanelKind::Tags => "tags",
        PanelKind::Sheets => "sheet",
        PanelKind::Library => "library",
        PanelKind::Share => "cloud-download",
        PanelKind::Inspector => "sliders",
        PanelKind::Tutorial => "graduation-cap",
        PanelKind::Table => "table",
        PanelKind::Tools | PanelKind::Canvas => "box",
    }
}

/// A panel's icon, painted by StageCraft's own icon rasteriser in the dock's ink.
#[derive(Debug, PartialEq)]
struct PanelIcon(&'static str);

impl MemoProps for PanelIcon {
    fn key(&self, key: &mut MemoKey) {
        key.write(self.0.as_bytes());
    }
}

impl Glyph for PanelIcon {
    fn paint(
        &self,
        painter: &egui::Painter,
        _tokens: &UiTokens,
        rect: egui::Rect,
        color: egui::Color32,
        _mirrored: bool,
    ) {
        let size = if rect.height() >= 16.0 { 16.0 } else { 12.0 };
        let rect = egui::Rect::from_center_size(rect.center(), egui::vec2(size, size));
        crate::icons::paint_with(painter, painter.ctx(), rect, self.0, size, color);
    }
}

fn panel_icon(p: PanelKind) -> &'static PanelIcon {
    static ICONS: std::sync::OnceLock<Vec<(PanelKind, PanelIcon)>> = std::sync::OnceLock::new();
    let icons = ICONS.get_or_init(|| PANELS.iter().map(|p| (*p, PanelIcon(icon(*p)))).collect());
    &icons.iter().find(|(k, _)| *k == p).expect("every panel has an icon").1
}

/// What the dock reads of the panels. Their names, limits and heights do not change while
/// StageCraft runs, so the specs never move their revision.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Specs;

impl PlainData for Specs {}

impl PanelSpecs<PanelKind> for Specs {
    fn spec(&self, panel: &PanelKind) -> PanelSpec {
        let mut spec = PanelSpec::new(label(*panel).to_owned())
            .limits(limits(*panel))
            .host_body()
            .allow_native(false)
            .icon(IconSpec::glyph(panel_icon(*panel)));
        if let Some(h) = dock_heights(*panel) {
            spec = spec.heights(h);
        }
        spec = spec.float_size(floating_size(*panel));
        if *panel == PanelKind::Canvas { spec.canvas() } else { spec }
    }

    fn permissions(&self, panel: &PanelKind) -> Permissions {
        permissions(panel)
    }
}

fn limits(panel: PanelKind) -> SizeLimits {
    let min = match panel {
        PanelKind::Inspector | PanelKind::Layers | PanelKind::Library => egui::vec2(220.0, 80.0),
        PanelKind::Tools => egui::vec2(crate::toolsets::width_for(1), 120.0),
        PanelKind::Table => egui::vec2(320.0, 100.0),
        _ => egui::vec2(160.0, 60.0),
    };
    SizeLimits::at_least(min)
}

fn dock_heights(panel: PanelKind) -> Option<DockHeights> {
    match panel {
        PanelKind::Layers | PanelKind::Tags | PanelKind::Sheets => Some(DockHeights::new(190.0).compact(120.0)),
        // Below the browsers: lower, so the Inspector keeps its room.
        PanelKind::Library | PanelKind::Share => Some(DockHeights::new(150.0).compact(90.0)),
        PanelKind::Tutorial => Some(DockHeights::new(230.0).compact(150.0)),
        _ => None,
    }
}

fn floating_size(panel: PanelKind) -> egui::Vec2 {
    match panel {
        PanelKind::Table => egui::vec2(720.0, 320.0),
        PanelKind::Inspector => egui::vec2(300.0, 520.0),
        PanelKind::Tools => egui::vec2(190.0, 420.0),
        _ => egui::vec2(280.0, 400.0),
    }
}

/// The workspace the dock shows and the panels' specs, kept across frames: the dock reads
/// the specs again only when the shown workspace's revision moves, so an unchanged layout
/// keeps its `Arc`.
#[derive(Default)]
pub struct Live {
    shown: Option<Arc<Workspace<PanelKind>>>,
    specs: Tracked<Specs>,
}

impl Live {
    fn shown(&mut self, ws: Workspace<PanelKind>) -> Arc<Workspace<PanelKind>> {
        if let Some(shown) = self.shown.as_ref().filter(|s| ***s == ws) {
            return Arc::clone(shown);
        }
        let shown = Arc::new(ws);
        self.shown = Some(Arc::clone(&shown));
        shown
    }
}

pub const DOCK_ID: &str = "dock";

/// The dock area: everything below the toolbar and above the status bar.
pub fn show(ui: &mut Ui, s: &mut Studio) {
    let shown = s.dock.shown(display(&s.ui.layout, s.ui.view));
    let specs = std::mem::take(&mut s.dock.specs);
    let config = DockConfig::default().native_windows(false);
    let out = {
        let mut bodies = Bodies { s: &mut *s };
        DockArea::new(DOCK_ID, Bind::given(shown), &specs, &mut bodies, config).show(ui)
    };
    s.dock.specs = specs;
    let actions: Vec<_> = out.actions.into_iter().chain(out.requested).collect();
    if !actions.is_empty()
        && let Err(e) = apply(&mut s.ui.layout, actions)
    {
        s.ui.error(e);
    }
}

struct Bodies<'a> {
    s: &'a mut Studio,
}

impl DockPanels<PanelKind> for Bodies<'_> {
    fn body(&mut self, panel: &PanelKind, ui: &mut Ui, _: &mut BodyCx<'_>) {
        let s = &mut *self.s;
        let scroll = |ui: &mut Ui, s: &mut Studio, f: fn(&mut Ui, &mut Studio)| {
            egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 8, top: 4, bottom: 6 }).show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("dock_body", *panel))
                    .auto_shrink([false, false])
                    .show(ui, |ui| f(ui, s));
            });
        };
        match panel {
            // The canvas is its group's `group_view`.
            PanelKind::Canvas => {}
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

    /// The plan, 3D view or sheet: the window's document canvas, edge to edge.
    fn group_view(
        &mut self,
        panels: &[PanelKind],
        _: usize,
        ui: &mut Ui,
        at: GroupAt,
        _: &mut BodyCx<'_>,
    ) -> Option<f32> {
        let GroupAt::Show(rect) = at else { return None };
        if !panels.contains(&PanelKind::Canvas) {
            return None;
        }
        canvas(ui, self.s);
        crate::framework::canvas_keys(ui.ctx(), crate::hud::kind(self.s).is_some());
        Some(rect.height())
    }

    /// ≡ before the tab strip: float, close the panel, reset the layout.
    fn group_header(&mut self, panels: &[PanelKind], active: usize, ui: &mut Ui, _: &mut BodyCx<'_>) {
        let Some(panel) = panels.get(active).copied() else { return };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, ui.available_height().max(20.0)), egui::Sense::hover());
        let b = egui::Rect::from_center_size(rect.center(), egui::vec2(20.0, 20.0));
        panel_menu_button(ui, self.s, panel, b);
    }
}

/// ≡: float, close the panel, reset the layout.
fn panel_menu_button(ui: &mut Ui, s: &mut Studio, panel: PanelKind, rect: egui::Rect) {
    let c = crate::theme::of(ui);
    let resp = ui.interact(rect, ui.scope_id().with(("panel_menu", panel)), egui::Sense::click());
    crate::widgets::name(&resp, egui::Role::Button, format!("Panel menu {}", label(panel)));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, c.hover.gamma_multiply(0.6));
    }
    crate::icons::paint(ui, rect, "menu", 16.0, if resp.hovered() { c.text } else { c.text2 });
    let resp = resp.on_hover_text(t("panel.menu"));
    egui::Popup::menu(&resp).show(|ui| {
        if panel == PanelKind::Tools {
            use crate::state::ToolColumns;
            ui.label(egui::RichText::new(t("panel.columns")).color(crate::theme::of(ui).text2));
            for (cols, key, name) in [
                (ToolColumns::Auto, "panel.columns.auto", "Columns auto"),
                (ToolColumns::Two, "panel.columns.two", "Columns two"),
                (ToolColumns::One, "panel.columns.one", "Columns one"),
            ] {
                let r = ui.radio(s.ui.tool_columns == cols, t(key));
                crate::widgets::name(&r, egui::Role::RadioButton, name);
                if r.clicked() {
                    let _ = s.run("ui.tool_columns", serde_json::json!({ "columns": cols }));
                    ui.close();
                }
            }
            ui.separator();
        }
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
    /// The drawing tools' options (heights, sizes, profiles), without the fixture type,
    /// which belongs to a document's library.
    pub tools: crate::state::ToolOptions,
    pub palette: PalettePrefs,
    pub tool_palette: ToolPalettePrefs,
    /// Whether to offer the tutorial on launch.
    pub onboarding: crate::onboarding::OnboardingPrefs,
}

/// The tool palette: buttons per row.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolPalettePrefs {
    pub columns: crate::state::ToolColumns,
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
    /// 2: the panel layout is craft-behavior's dock model (craft-libs after the framework
    /// merge); a layout saved by version 1 cannot be read and gives way to the default.
    const SCHEMA_VERSION: u32 = 2;

    fn migrate(from: u32, doc: &mut craft_settings::DocumentMut) -> Result<(), craft_settings::MigrateError> {
        if from == 1
            && let Some(layout) = doc.get_mut("layout").and_then(|l| l.as_table_like_mut())
        {
            layout.remove("workspace");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_behavior::dock::DockPlacement;

    #[test]
    fn the_default_layout_is_valid_and_has_every_panel() {
        let ws = default_workspace();
        ws.validate().unwrap();
        for p in PANELS {
            assert!(ws.layout().contains(&p) || ws.hidden().iter().any(|(h, _)| *h == p), "{p:?}");
        }
        assert!(!ws.layout().contains(&PanelKind::Tutorial), "the tutorial opens on demand");
        let mut shown = ws.clone();
        set_visible(&mut shown, PanelKind::Tutorial, true).unwrap();
        assert!(shown.layout().contains(&PanelKind::Tutorial));
    }

    #[test]
    fn layouts_survive_config_toml() {
        let mut ws = default_workspace();
        let rect = egui::Rect::from_min_size(egui::pos2(300.0, 200.0), egui::vec2(280.0, 360.0));
        apply(&mut ws, vec![Action::float(PanelKind::Library, rect)]).unwrap();
        set_visible(&mut ws, PanelKind::Tags, false).unwrap();
        apply(&mut ws, vec![Action::move_beside(PanelKind::Share, PanelKind::Inspector, DockPlacement::tab(None))])
            .unwrap();
        let tools = crate::state::ToolOptions { floor_z: 1000.0, floor_thickness: 200.0, ..Default::default() };
        let prefs = Prefs {
            appearance: Appearance { theme: ThemePref::Dark },
            layout: SavedLayout { workspace: Some(ws) },
            tools,
            palette: PalettePrefs { recent: vec!["patch.auto".into(), "ui.set_view {\"view\":\"3d\"}".into()] },
            tool_palette: ToolPalettePrefs { columns: crate::state::ToolColumns::Two },
            onboarding: crate::onboarding::OnboardingPrefs { tutorial: crate::onboarding::TutorialOffer::Never },
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
    fn a_layout_from_before_the_framework_is_dropped_and_the_rest_kept() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "schema_version = 1\n[appearance]\ntheme = \"dark\"\n[layout.workspace.layout.root.Split]\naxis = \"Horizontal\"\n",
        )
        .unwrap();
        let (store, report) = craft_settings::SettingsStore::<Prefs>::open_at(dir.path());
        assert!(!report.has_problems(), "{report:?}");
        assert_eq!(store.get().appearance.theme, ThemePref::Dark);
        assert!(store.get().layout.workspace.is_none());
    }

    #[test]
    fn an_old_layout_gets_new_panels_and_a_broken_one_the_default() {
        let mut old = default_workspace();
        set_visible(&mut old, PanelKind::Share, false).unwrap();
        old.forget_hidden(|_| true);
        let ws = load(Some(old));
        assert!(ws.layout().contains(&PanelKind::Share) || ws.hidden().iter().any(|(p, _)| *p == PanelKind::Share));
        // A layout from a newer StageCraft (a format version this build does not know).
        let mut newer = serde_json::to_value(default_workspace()).unwrap();
        newer["version"] = serde_json::json!(99);
        let newer: Workspace<PanelKind> = serde_json::from_value(newer).unwrap();
        assert_eq!(load(Some(newer)), default_workspace());
    }
}
