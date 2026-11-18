//! UI state: everything the GUI remembers that is not the document. `ui.*` commands
//! read and change it, so an AI can see and steer the running app.

use std::collections::BTreeSet;
use std::path::PathBuf;

use egui::{Pos2, Rect, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};
use stagecraft_core::Id;
use stagecraft_core::model::P2;
use stagecraft_geom::Bounds;
use stagecraft_geom::snap::{SnapSettings, Snapped};

use crate::palette::PaletteState;
use crate::table::TableState;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    #[default]
    Select,
    Move,
    Rotate,
    Floor,
    Wall,
    Truss,
    Fixture,
    /// A run of truss along clicked points, corners at the bends.
    Run,
    Batten,
    Curtain,
    Proscenium,
    /// A wing set (legs and borders) from the first wing.
    Wings,
    Platform,
    Support,
    Hoist,
    Ceiling,
}

impl Tool {
    /// Drawing tools, then stage objects (the toolbar's two groups).
    pub const DRAW: [Tool; 7] =
        [Tool::Select, Tool::Move, Tool::Rotate, Tool::Floor, Tool::Wall, Tool::Truss, Tool::Fixture];
    pub const STAGE: [Tool; 9] = [
        Tool::Run,
        Tool::Batten,
        Tool::Curtain,
        Tool::Proscenium,
        Tool::Wings,
        Tool::Platform,
        Tool::Support,
        Tool::Hoist,
        Tool::Ceiling,
    ];
    pub const ALL: [Tool; 16] = [
        Tool::Select,
        Tool::Move,
        Tool::Rotate,
        Tool::Floor,
        Tool::Wall,
        Tool::Truss,
        Tool::Fixture,
        Tool::Run,
        Tool::Batten,
        Tool::Curtain,
        Tool::Proscenium,
        Tool::Wings,
        Tool::Platform,
        Tool::Support,
        Tool::Hoist,
        Tool::Ceiling,
    ];

    /// Catalog key of the tool's name.
    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "tool.select",
            Tool::Move => "tool.move",
            Tool::Rotate => "tool.rotate",
            Tool::Floor => "tool.floor",
            Tool::Wall => "tool.wall",
            Tool::Truss => "tool.truss",
            Tool::Fixture => "tool.fixture",
            Tool::Run => "tool.run",
            Tool::Batten => "tool.batten",
            Tool::Curtain => "tool.curtain",
            Tool::Proscenium => "tool.proscenium",
            Tool::Wings => "tool.wings",
            Tool::Platform => "tool.platform",
            Tool::Support => "tool.support",
            Tool::Hoist => "tool.hoist",
            Tool::Ceiling => "tool.ceiling",
        }
    }

    /// Single-key shortcut, if the tool has one.
    pub fn shortcut(self) -> Option<egui::Key> {
        use egui::Key;
        Some(match self {
            Tool::Select => Key::V,
            Tool::Move => Key::M,
            Tool::Rotate => Key::R,
            Tool::Floor => Key::F,
            Tool::Wall => Key::W,
            Tool::Truss => Key::T,
            Tool::Fixture => Key::L,
            Tool::Run => Key::U,
            Tool::Batten => Key::B,
            Tool::Curtain => Key::C,
            Tool::Platform => Key::P,
            Tool::Hoist => Key::H,
            Tool::Proscenium | Tool::Wings | Tool::Support | Tool::Ceiling => return None,
        })
    }

    /// Tools that take typed lengths after their first point.
    pub fn draws(self) -> bool {
        matches!(
            self,
            Tool::Move
                | Tool::Rotate
                | Tool::Floor
                | Tool::Wall
                | Tool::Truss
                | Tool::Run
                | Tool::Curtain
                | Tool::Ceiling
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThemePref {
    /// Follow macOS.
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum View {
    /// Top view of the layers (2D).
    #[default]
    Plan,
    /// The 3D model.
    #[serde(rename = "3d")]
    ThreeD,
    /// Plan on the left, 3D on the right.
    Split,
    /// Drawing sheets: white paper with viewports, title block and labels.
    Sheets,
}

impl View {
    pub fn shows_plan(self) -> bool {
        matches!(self, View::Plan | View::Split)
    }

    pub fn shows_3d(self) -> bool {
        matches!(self, View::ThreeD | View::Split)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PanelKind {
    Layers,
    Tags,
    Inspector,
    Table,
    /// Fixture types of the document and GDTF Share.
    Library,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Panels {
    pub layers: bool,
    pub tags: bool,
    pub inspector: bool,
    pub table: bool,
    pub library: bool,
}

impl Default for Panels {
    fn default() -> Self {
        Self { layers: true, tags: true, inspector: true, table: true, library: true }
    }
}

impl Panels {
    pub fn get_mut(&mut self, p: PanelKind) -> &mut bool {
        match p {
            PanelKind::Layers => &mut self.layers,
            PanelKind::Tags => &mut self.tags,
            PanelKind::Inspector => &mut self.inspector,
            PanelKind::Table => &mut self.table,
            PanelKind::Library => &mut self.library,
        }
    }
}

/// The plan camera: world mm at the centre of the canvas and pixels per mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub center: P2,
    /// Screen points per mm.
    pub scale: f64,
    /// Where the canvas was drawn last frame.
    pub rect: Rect,
}

impl Default for Camera {
    fn default() -> Self {
        Self { center: [0.0, 0.0], scale: 0.05, rect: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)) }
    }
}

pub const MIN_SCALE: f64 = 0.0005;
pub const MAX_SCALE: f64 = 20.0;

impl Camera {
    /// World mm (y up) to screen points (y down).
    pub fn to_screen(&self, p: P2) -> Pos2 {
        let c = self.rect.center();
        pos2(c.x + ((p[0] - self.center[0]) * self.scale) as f32, c.y - ((p[1] - self.center[1]) * self.scale) as f32)
    }

    pub fn to_world(&self, s: Pos2) -> P2 {
        let c = self.rect.center();
        [self.center[0] + (s.x - c.x) as f64 / self.scale, self.center[1] - (s.y - c.y) as f64 / self.scale]
    }

    /// Zoom by `factor`, keeping the world point under `at` where it is.
    pub fn zoom_at(&mut self, at: Pos2, factor: f64) {
        let before = self.to_world(at);
        self.scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let after = self.to_world(at);
        self.center = [self.center[0] + before[0] - after[0], self.center[1] + before[1] - after[1]];
    }

    /// Move the view by a screen offset (content follows the pointer).
    pub fn pan_screen(&mut self, d: Vec2) {
        self.center = [self.center[0] - d.x as f64 / self.scale, self.center[1] + d.y as f64 / self.scale];
    }

    /// Show `b` with a margin. An empty box shows 20 × 15 m around the origin.
    pub fn fit(&mut self, b: Bounds) {
        let b = if b.is_empty() { Bounds::from_points([-10_000.0, -7_500.0], [10_000.0, 7_500.0]) } else { b };
        let size = b.size();
        let w = (self.rect.width() as f64 * 0.9).max(10.0);
        let h = (self.rect.height() as f64 * 0.9).max(10.0);
        let sx = if size[0] > 1.0 { w / size[0] } else { MAX_SCALE };
        let sy = if size[1] > 1.0 { h / size[1] } else { MAX_SCALE };
        self.scale = sx.min(sy).clamp(MIN_SCALE, 1.0);
        self.center = b.center();
    }
}

/// What the user is typing while drawing: "4.5", Tab, "30", Enter.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DimInput {
    pub length: String,
    pub angle: String,
    pub on_angle: bool,
}

/// A press on the canvas that may become a drag.
#[derive(Clone, Debug, PartialEq)]
pub enum Drag {
    /// Box selection from this world point.
    Box { from: P2 },
    /// Moving the selection; `from` is the snapped grab point.
    Move { from: P2, ids: Vec<Id> },
}

/// The drawing gesture in progress.
#[derive(Clone, Debug, Default)]
pub struct Gesture {
    /// Points clicked so far (wall/floor vertices, truss start, move base, rotate pivot
    /// and reference).
    pub points: Vec<P2>,
    pub dim: Option<DimInput>,
    /// The snapped pointer, world mm.
    pub cursor: Option<Snapped>,
    pub drag: Option<Drag>,
}

/// A screenshot asked for by `ui.screenshot`; written when the frame arrives.
pub struct PendingShot {
    pub path: PathBuf,
    pub reply: Option<stagecraft_control::Call>,
}

/// Options of the drawing tools.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolOptions {
    pub truss_profile: String,
    /// Height of new truss above the layer, mm.
    pub truss_height: f64,
    pub wall_height: f64,
    pub wall_thickness: f64,
    /// Trim height of new battens, mm.
    pub batten_trim: f64,
    pub batten_length: f64,
    /// Top edge of new curtains not hung on a batten, mm.
    pub curtain_trim: f64,
    pub curtain_height: f64,
    pub proscenium_width: f64,
    pub proscenium_height: f64,
    pub wing_count: u32,
    pub wing_spacing: f64,
    pub wing_opening: f64,
    /// `2x1` style, metres.
    pub platform_preset: String,
    pub platform_height: f64,
    /// Catalogue system; empty = none.
    pub platform_system: String,
    /// Support catalogue entry.
    pub support_preset: String,
    pub hoist_height: f64,
    pub ceiling_height: f64,
    /// Library key of the type the fixture tool places; empty = generic fixture.
    pub fixture_type: String,
    pub fixture_mode: String,
}

impl ToolOptions {
    /// Width of the truss profile new truss gets, mm.
    pub fn truss_profile_width(&self) -> f64 {
        stagecraft_geom::truss_profile_or_fallback(&self.truss_profile).width
    }
}

impl Default for ToolOptions {
    fn default() -> Self {
        Self {
            truss_profile: "F34".into(),
            truss_height: 6000.0,
            wall_height: 3000.0,
            wall_thickness: 150.0,
            batten_trim: 8000.0,
            batten_length: 14000.0,
            curtain_trim: 8000.0,
            curtain_height: 7000.0,
            proscenium_width: 10000.0,
            proscenium_height: 6000.0,
            wing_count: 3,
            wing_spacing: 2000.0,
            wing_opening: 10000.0,
            platform_preset: "2x1".into(),
            platform_height: 400.0,
            platform_system: String::new(),
            support_preset: "Base plate 600".into(),
            hoist_height: 9000.0,
            ceiling_height: 9000.0,
            fixture_type: String::new(),
            fixture_mode: String::new(),
        }
    }
}

pub struct UiState {
    pub tool: Tool,
    pub view: View,
    pub camera: Camera,
    /// Camera, section and mesh cache of the 3D view.
    pub view3d: crate::view3d::View3d,
    /// Fit the drawing on the next frame (after the canvas knows its size).
    pub fit_pending: bool,
    pub theme: ThemePref,
    pub panels: Panels,
    pub hidden_tags: BTreeSet<Id>,
    /// New objects without a parent go here.
    pub active_layer: Option<Id>,
    pub snap: SnapSettings,
    pub options: ToolOptions,
    pub gesture: Gesture,
    pub palette: PaletteState,
    pub table: TableState,
    /// The library panel: GDTF Share search and downloads.
    pub library: crate::library::LibraryState,
    /// Last message for the status bar; `true` = error.
    pub status: Option<(String, bool)>,
    /// The Sheets mode: current sheet, selected viewport, paper camera.
    pub sheets: crate::sheets::SheetsState,
    /// Label template the plan canvas draws fixture labels with (`None` = FIDs only).
    pub plan_labels: Option<Id>,
    pub shots: Vec<PendingShot>,
    /// Set by the File menu; the app shows a native dialog outside the frame.
    pub file_request: Option<FileRequest>,
    /// Native file and message dialogs (off in headless tests).
    pub native_dialogs: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileRequest {
    Open,
    SaveAs,
    /// Export every sheet as one PDF.
    ExportPdf,
    /// Pick a logo for the title block.
    Logo,
    /// Import `.gdtf` files into the library.
    ImportGdtf,
    /// Import an `.mvr` file into the document.
    ImportMvr,
    /// Write the document as `.mvr`.
    ExportMvr,
    /// Import a 3D model (`.glb`, `.gltf`, `.obj`, `.3ds`).
    ImportModel,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            tool: Tool::Select,
            view: View::Plan,
            camera: Camera::default(),
            view3d: Default::default(),
            fit_pending: true,
            theme: ThemePref::System,
            panels: Panels::default(),
            hidden_tags: BTreeSet::new(),
            active_layer: None,
            snap: SnapSettings::default(),
            options: ToolOptions::default(),
            gesture: Gesture::default(),
            palette: PaletteState::default(),
            table: TableState::default(),
            library: Default::default(),
            status: None,
            sheets: Default::default(),
            plan_labels: None,
            shots: Vec::new(),
            file_request: None,
            native_dialogs: false,
        }
    }
}

impl UiState {
    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.gesture = Gesture::default();
    }

    pub fn info(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), false));
    }

    pub fn error(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), true));
    }
}
