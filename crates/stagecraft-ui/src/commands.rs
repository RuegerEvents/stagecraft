//! The GUI's own registry: `ui.*` commands for tool, camera, theme, panels and
//! screenshots. They change UI state only, never the document, and are not undoable.
//! In GUI mode the control server lists, describes and runs them next to the core
//! registry (see [`UiExtension`]).

use std::path::PathBuf;
use std::sync::OnceLock;

use schemars::JsonSchema;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use stagecraft_core::model::{P2, P3};
use stagecraft_core::{CmdError, CmdResult, Engine};
use stagecraft_geom::section::Plane;

use crate::state::{PanelKind, PendingShot, ThemePref, Tool, UiState, View};
use crate::view3d::{self, Camera3d, Projection, Section, StdView};

/// What a `ui.*` command function gets.
pub struct UiCtx<'a> {
    pub engine: &'a mut Engine,
    pub ui: &'a mut UiState,
}

type RunFn = Box<dyn Fn(&mut UiCtx, Value) -> CmdResult + Send + Sync>;

pub struct UiEntry {
    pub name: &'static str,
    pub summary: &'static str,
    schema: fn() -> Value,
    run: RunFn,
}

impl UiEntry {
    pub fn schema(&self) -> Value {
        (self.schema)()
    }
    pub fn run(&self, ctx: &mut UiCtx, args: Value) -> CmdResult {
        (self.run)(ctx, args)
    }
}

#[derive(Default)]
pub struct UiRegistry {
    entries: Vec<UiEntry>,
}

fn schema_of<A: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(A)).expect("schema serialises")
}

impl UiRegistry {
    fn add<A>(&mut self, name: &'static str, summary: &'static str, f: fn(&mut UiCtx, A) -> CmdResult)
    where
        A: DeserializeOwned + JsonSchema + 'static,
    {
        assert!(self.get(name).is_none(), "ui command `{name}` registered twice");
        let run: RunFn = Box::new(move |ctx, args| {
            let args = if args.is_null() { Value::Object(Default::default()) } else { args };
            let a: A = serde_json::from_value(args).map_err(|e| CmdError::invalid_args(format!("{name}: {e}")))?;
            f(ctx, a)
        });
        self.entries.push(UiEntry { name, summary, schema: schema_of::<A>, run });
    }

    pub fn get(&self, name: &str) -> Option<&UiEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    pub fn entries(&self) -> &[UiEntry] {
        &self.entries
    }
}

pub fn ui_registry() -> &'static UiRegistry {
    static REG: OnceLock<UiRegistry> = OnceLock::new();
    REG.get_or_init(|| {
        let mut r = UiRegistry::default();
        register(&mut r);
        r
    })
}

/// Kind reported for `ui.*` commands in `commands`/`describe`.
pub const UI_KIND: &str = "ui";

fn register(r: &mut UiRegistry) {
    r.add("ui.state", "Tool, view, camera, theme, panels, hidden tags, selection and status", state);
    r.add("ui.set_tool", "Pick the active tool (select, move, rotate, floor, wall, truss, fixture)", set_tool);
    r.add("ui.set_view", "Show the plan, the 3D view, or both side by side (split)", set_view);
    r.add("ui.zoom", "Zoom the plan by a factor or to a scale", zoom);
    r.add("ui.pan", "Move the plan view by an offset or centre it on a point", pan);
    r.add("ui.zoom_to_fit", "Fit the drawing (or the selection) into the views shown", zoom_to_fit);
    r.add(
        "ui.camera",
        "3D camera: standard view (top, front, side, iso …), eye/target, projection, orbit, fit",
        camera,
    );
    r.add(
        "ui.section",
        "3D section: a clip plane (axis or free) or a section box, with filled cut faces; or off",
        section,
    );
    r.add("ui.set_theme", "Light, dark, or follow the system", set_theme);
    r.add("ui.toggle_panel", "Show or hide a panel (layers, tags, inspector, table)", toggle_panel);
    r.add("ui.set_tag_visible", "Show or hide objects carrying a tag in the views", set_tag_visible);
    r.add("ui.set_active_layer", "Layer that new objects go on", set_active_layer);
    r.add("ui.set_snap", "Turn snaps on or off and set the grid", set_snap);
    r.add("ui.set_tool_options", "Truss profile and height, wall height and thickness for new objects", set_options);
    r.add("ui.palette", "Open or close the command palette", palette);
    r.add("ui.screenshot", "Write a PNG of the window to `path` (answered after the next frame)", screenshot);
}

#[derive(Deserialize, JsonSchema)]
struct NoArgs {}

fn state(c: &mut UiCtx, _: NoArgs) -> CmdResult {
    let ui = &*c.ui;
    let cam = &ui.camera;
    Ok(json!({
        "tool": ui.tool,
        "view": ui.view,
        "camera": { "center": cam.center, "scale": cam.scale,
                    "visible": [cam.to_world(cam.rect.left_bottom()), cam.to_world(cam.rect.right_top())] },
        "camera3d": camera3d_json(&ui.view3d.camera),
        "section": ui.view3d.section,
        "section_hatch": ui.view3d.hatch,
        "theme": ui.theme,
        "panels": ui.panels,
        "hidden_tags": ui.hidden_tags,
        "active_layer": ui.active_layer,
        "snap": ui.snap,
        "tool_options": ui.options,
        "drawing": ui.gesture.points,
        "typed": ui.gesture.dim,
        "palette_open": ui.palette.open,
        "selection": c.engine.selection(),
        "status": ui.status.as_ref().map(|s| &s.0),
    }))
}

#[derive(Deserialize, JsonSchema)]
struct ToolArgs {
    tool: Tool,
}

fn set_tool(c: &mut UiCtx, a: ToolArgs) -> CmdResult {
    c.ui.set_tool(a.tool);
    Ok(json!({ "tool": a.tool }))
}

#[derive(Deserialize, JsonSchema)]
struct ViewArgs {
    view: View,
}

fn set_view(c: &mut UiCtx, a: ViewArgs) -> CmdResult {
    c.ui.view = a.view;
    Ok(json!({ "view": a.view }))
}

#[derive(Deserialize, JsonSchema)]
struct ZoomArgs {
    /// Multiply the zoom by this (2 = twice as close). Ignored when `scale` is given.
    factor: Option<f64>,
    /// Screen points per mm, e.g. 0.05 shows 20 m across 1000 points.
    scale: Option<f64>,
}

fn zoom(c: &mut UiCtx, a: ZoomArgs) -> CmdResult {
    let cam = &mut c.ui.camera;
    match (a.scale, a.factor) {
        (Some(s), _) if s > 0.0 => {
            cam.scale = s.clamp(crate::state::MIN_SCALE, crate::state::MAX_SCALE);
        }
        (None, Some(f)) if f > 0.0 => {
            let at = cam.rect.center();
            cam.zoom_at(at, f);
        }
        _ => return Err(CmdError::invalid_args("give a positive `factor` or `scale`")),
    }
    c.ui.fit_pending = false;
    Ok(json!({ "scale": cam.scale }))
}

#[derive(Deserialize, JsonSchema)]
struct PanArgs {
    /// Move the view by this many mm (x right, y upstage).
    by: Option<P2>,
    /// Centre the view on this world point, mm.
    center: Option<P2>,
}

fn pan(c: &mut UiCtx, a: PanArgs) -> CmdResult {
    let cam = &mut c.ui.camera;
    match (a.center, a.by) {
        (Some(p), _) => cam.center = p,
        (None, Some(d)) => cam.center = [cam.center[0] + d[0], cam.center[1] + d[1]],
        _ => return Err(CmdError::invalid_args("give `by` or `center`")),
    }
    c.ui.fit_pending = false;
    Ok(json!({ "center": cam.center }))
}

#[derive(Deserialize, JsonSchema)]
struct FitArgs {
    /// Fit only the selected objects. Default false.
    #[serde(default)]
    selection: bool,
}

fn zoom_to_fit(c: &mut UiCtx, a: FitArgs) -> CmdResult {
    let ids: Vec<_> =
        if a.selection { c.engine.selection().to_vec() } else { crate::canvas::visible_ids(c.engine.doc(), c.ui) };
    let mut out = json!({});
    if c.ui.view.shows_plan() {
        let b = stagecraft_geom::pick::objects_bounds(c.engine.doc(), ids.clone(), &Default::default());
        c.ui.camera.fit(b);
        c.ui.fit_pending = false;
        out["center"] = json!(c.ui.camera.center);
        out["scale"] = json!(c.ui.camera.scale);
    }
    if c.ui.view.shows_3d() {
        view3d::sync_scene(c.engine, c.ui);
        c.ui.view3d.fit(a.selection.then_some(ids.as_slice()));
        out["camera3d"] = camera3d_json(&c.ui.view3d.camera);
    }
    Ok(out)
}

pub fn camera3d_json(cam: &Camera3d) -> Value {
    let r = |v: glam::DVec3| v.to_array().map(|x| (x * 1000.0).round() / 1000.0);
    json!({
        "eye": r(cam.eye()),
        "target": r(cam.target),
        "projection": cam.projection,
        "fov": cam.fov,
        "yaw": cam.yaw,
        "pitch": cam.pitch,
        "distance": cam.distance,
    })
}

#[derive(Deserialize, JsonSchema)]
struct CameraArgs {
    /// A standard view: top, front (from the audience), back, side (from stage
    /// right, +x), left, iso (front right, above). Fits the model unless `fit` is false
    /// or `eye`/`target` are given.
    view: Option<StdView>,
    /// Eye position, world mm.
    eye: Option<P3>,
    /// Point looked at and orbited around, world mm.
    target: Option<P3>,
    /// perspective or orthographic.
    projection: Option<Projection>,
    /// Vertical field of view in degrees (perspective), 10–120. Default 40.
    fov: Option<f64>,
    /// Orbit by [yaw, pitch] degrees around the target.
    orbit: Option<[f64; 2]>,
    /// Move closer by this factor (2 = half the distance).
    zoom: Option<f64>,
    /// Fit everything shown after the other changes.
    fit: Option<bool>,
}

fn camera(c: &mut UiCtx, a: CameraArgs) -> CmdResult {
    let placed = a.eye.is_some() || a.target.is_some();
    let cam = &mut c.ui.view3d.camera;
    if let Some(p) = a.projection {
        cam.projection = p;
    }
    if let Some(f) = a.fov {
        if !(10.0..=120.0).contains(&f) {
            return Err(CmdError::invalid_args("fov must be 10–120 degrees"));
        }
        cam.fov = f;
    }
    if let Some(v) = a.view {
        let (yaw, pitch) = v.angles();
        cam.set_angles(yaw, pitch);
    }
    if placed {
        let eye = a.eye.map_or(cam.eye(), glam::DVec3::from_array);
        let target = a.target.map_or(cam.target, glam::DVec3::from_array);
        if eye.distance(target) < 1.0 {
            return Err(CmdError::invalid_args("eye and target must be apart"));
        }
        cam.look(eye, target);
    }
    if let Some([dy, dp]) = a.orbit {
        cam.set_angles(cam.yaw + dy, cam.pitch + dp);
    }
    if let Some(z) = a.zoom {
        if z <= 0.0 {
            return Err(CmdError::invalid_args("zoom must be positive"));
        }
        let at = cam.rect.center();
        cam.zoom_at(at, z);
    }
    let fit = a.fit.unwrap_or(a.view.is_some() && !placed);
    if fit {
        view3d::sync_scene(c.engine, c.ui);
        c.ui.view3d.fit(None);
    } else {
        c.ui.view3d.fit_pending = false;
    }
    Ok(camera3d_json(&c.ui.view3d.camera))
}

#[derive(Deserialize, JsonSchema, Default)]
struct SectionBox {
    /// Lower corner, world mm. Default: the model's bounds, a tenth in from each side
    /// in plan, full height.
    min: Option<P3>,
    /// Upper corner, world mm.
    max: Option<P3>,
}

#[derive(Deserialize, JsonSchema)]
struct SectionArgs {
    /// Turn the section off.
    #[serde(default)]
    off: bool,
    /// An axis plane: x hides what is right of `offset` (stage left), y hides what is
    /// downstage of it (look from the audience at the cut), z hides what is above
    /// it. `flip` hides the other side.
    axis: Option<Axis>,
    /// Position of an axis plane along its axis, mm. Default: the model's centre.
    offset: Option<f64>,
    /// Hide the other side of an axis or free plane.
    #[serde(default)]
    flip: bool,
    /// A free plane: a point on it, world mm …
    origin: Option<P3>,
    /// … and its normal, which points to the hidden side.
    normal: Option<P3>,
    /// A section box: everything outside is hidden.
    #[serde(rename = "box")]
    section_box: Option<SectionBox>,
    /// Hatch the cut faces (true) or fill them solid (false). Unchanged when absent.
    hatch: Option<bool>,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Axis {
    X,
    Y,
    Z,
}

fn section(c: &mut UiCtx, a: SectionArgs) -> CmdResult {
    if let Some(h) = a.hatch {
        c.ui.view3d.hatch = h;
    }
    view3d::sync_scene(c.engine, c.ui);
    let (lo, hi) = c.ui.view3d.scene.bounds().unwrap_or((glam::DVec3::ZERO, glam::DVec3::splat(1000.0)));
    let centre = (lo + hi) / 2.0;
    let sign = if a.flip { -1.0 } else { 1.0 };
    let new = if a.off {
        Section::Off
    } else if let Some(b) = a.section_box {
        let q = (hi - lo) / 10.0;
        let min = b.min.unwrap_or([lo.x + q.x, lo.y + q.y, lo.z - 1.0]);
        let max = b.max.unwrap_or([hi.x - q.x, hi.y - q.y, hi.z + 1.0]);
        if (0..3).any(|i| min[i] >= max[i]) {
            return Err(CmdError::invalid_args("box: min must be below max on every axis"));
        }
        Section::Box { min, max }
    } else if let Some(axis) = a.axis {
        let (i, n) = match axis {
            Axis::X => (0, [1.0, 0.0, 0.0]),
            Axis::Y => (1, [0.0, -1.0, 0.0]),
            Axis::Z => (2, [0.0, 0.0, 1.0]),
        };
        let mut origin = [0.0; 3];
        origin[i] = a.offset.unwrap_or(centre[i]);
        Section::Plane { plane: Plane::new(origin, n.map(|x| x * sign)) }
    } else if let Some(n) = a.normal {
        if glam::DVec3::from_array(n).length() < 1e-9 {
            return Err(CmdError::invalid_args("normal must not be zero"));
        }
        let origin = a.origin.unwrap_or(centre.to_array());
        Section::Plane { plane: Plane::new(origin, n.map(|x| x * sign)) }
    } else if a.hatch.is_some() {
        c.ui.view3d.section
    } else {
        return Err(CmdError::invalid_args("give `off`, `axis`, `normal` (with `origin`) or `box`"));
    };
    c.ui.view3d.section = new;
    Ok(json!({ "section": new, "hatch": c.ui.view3d.hatch }))
}

#[derive(Deserialize, JsonSchema)]
struct ThemeArgs {
    theme: ThemePref,
}

fn set_theme(c: &mut UiCtx, a: ThemeArgs) -> CmdResult {
    c.ui.theme = a.theme;
    Ok(json!({ "theme": a.theme }))
}

#[derive(Deserialize, JsonSchema)]
struct PanelArgs {
    panel: PanelKind,
    /// Show (true) or hide (false). Default: flip.
    visible: Option<bool>,
}

fn toggle_panel(c: &mut UiCtx, a: PanelArgs) -> CmdResult {
    let v = c.ui.panels.get_mut(a.panel);
    *v = a.visible.unwrap_or(!*v);
    Ok(json!({ "panel": a.panel, "visible": *v }))
}

#[derive(Deserialize, JsonSchema)]
struct TagVisibleArgs {
    /// Tag id or name.
    tag: String,
    visible: bool,
}

fn set_tag_visible(c: &mut UiCtx, a: TagVisibleArgs) -> CmdResult {
    let doc = c.engine.doc();
    let id = doc
        .tags
        .values()
        .find(|t| t.name == a.tag || t.id.to_string() == a.tag)
        .map(|t| t.id)
        .ok_or_else(|| CmdError::not_found("tag", &a.tag))?;
    if a.visible {
        c.ui.hidden_tags.remove(&id);
    } else {
        c.ui.hidden_tags.insert(id);
    }
    Ok(json!({ "hidden_tags": c.ui.hidden_tags }))
}

#[derive(Deserialize, JsonSchema)]
struct LayerArgs {
    /// Layer id or name.
    layer: String,
}

fn set_active_layer(c: &mut UiCtx, a: LayerArgs) -> CmdResult {
    let doc = c.engine.doc();
    let id = doc
        .layers
        .values()
        .find(|l| l.name == a.layer || l.id.to_string() == a.layer)
        .map(|l| l.id)
        .ok_or_else(|| CmdError::not_found("layer", &a.layer))?;
    c.ui.active_layer = Some(id);
    Ok(json!({ "active_layer": id }))
}

#[derive(Deserialize, JsonSchema)]
struct SnapArgs {
    /// Grid spacing in mm; 0 turns grid snapping off.
    grid: Option<f64>,
    endpoints: Option<bool>,
    midpoints: Option<bool>,
    /// Snap onto truss axes.
    truss: Option<bool>,
}

fn set_snap(c: &mut UiCtx, a: SnapArgs) -> CmdResult {
    let s = &mut c.ui.snap;
    if let Some(g) = a.grid {
        s.grid = (g > 0.0).then_some(g);
    }
    if let Some(v) = a.endpoints {
        s.endpoints = v;
    }
    if let Some(v) = a.midpoints {
        s.midpoints = v;
    }
    if let Some(v) = a.truss {
        s.truss = v;
    }
    Ok(json!(s))
}

#[derive(Deserialize, JsonSchema)]
struct OptionsArgs {
    /// Truss profile for new truss, e.g. F34.
    truss_profile: Option<String>,
    /// Height of new truss above the layer, mm.
    truss_height: Option<f64>,
    /// mm.
    wall_height: Option<f64>,
    /// mm.
    wall_thickness: Option<f64>,
}

fn set_options(c: &mut UiCtx, a: OptionsArgs) -> CmdResult {
    let o = &mut c.ui.options;
    if let Some(v) = a.truss_profile {
        o.truss_profile = v;
    }
    if let Some(v) = a.truss_height {
        o.truss_height = v;
    }
    for (slot, v) in [(&mut o.wall_height, a.wall_height), (&mut o.wall_thickness, a.wall_thickness)] {
        if let Some(v) = v {
            if v <= 0.0 {
                return Err(CmdError::invalid_args("wall sizes must be positive mm"));
            }
            *slot = v;
        }
    }
    Ok(json!(o))
}

#[derive(Deserialize, JsonSchema)]
struct PaletteArgs {
    /// Default true.
    #[serde(default = "yes")]
    open: bool,
}

fn yes() -> bool {
    true
}

fn palette(c: &mut UiCtx, a: PaletteArgs) -> CmdResult {
    if a.open {
        c.ui.palette.open();
    } else {
        c.ui.palette.close();
    }
    Ok(json!({ "open": a.open }))
}

#[derive(Deserialize, JsonSchema)]
pub struct ScreenshotArgs {
    /// Where to write the PNG. Parent directories must exist.
    pub path: PathBuf,
}

fn screenshot(c: &mut UiCtx, a: ScreenshotArgs) -> CmdResult {
    check_shot_path(&a.path)?;
    c.ui.shots.push(PendingShot { path: a.path.clone(), reply: None });
    Ok(json!({ "path": a.path, "pending": true }))
}

pub fn check_shot_path(p: &std::path::Path) -> Result<(), CmdError> {
    match p.parent() {
        Some(d) if d.as_os_str().is_empty() || d.is_dir() => Ok(()),
        _ => Err(CmdError::invalid_args(format!("{}: the directory does not exist", p.display()))),
    }
}

/// Lets the control server answer `ui.*` next to the core registry.
pub struct UiExtension<'a> {
    pub ui: &'a mut UiState,
}

impl stagecraft_control::Extension for UiExtension<'_> {
    fn commands(&self) -> Vec<Value> {
        ui_registry()
            .entries()
            .iter()
            .map(|e| json!({ "name": e.name, "kind": UI_KIND, "summary": e.summary }))
            .collect()
    }

    fn describe(&self, name: &str) -> Option<Value> {
        let e = ui_registry().get(name)?;
        Some(json!({ "name": e.name, "kind": UI_KIND, "summary": e.summary, "args": e.schema() }))
    }

    fn execute(&mut self, engine: &mut Engine, name: &str, args: Value) -> Option<Result<Value, CmdError>> {
        let e = ui_registry().get(name)?;
        Some(e.run(&mut UiCtx { engine, ui: self.ui }, args))
    }
}
