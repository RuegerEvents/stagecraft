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
    r.add("ui.set_view", "Show the plan, the 3D view, both side by side (split), or the sheets", set_view);
    r.add(
        "ui.sheet",
        "Sheets mode: show a sheet (id, number or name), select a viewport, open its editor, fit the paper",
        sheet,
    );
    r.add(
        "ui.set_label_template",
        "Draw fixture labels in the plan with a label template (or FIDs only with null)",
        set_label_template,
    );
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
    r.add("ui.toggle_panel", "Show or hide a panel (layers, tags, inspector, table, library)", toggle_panel);
    r.add(
        "ui.share_search",
        "Search GDTF Share in the library panel (loads the cached list off the UI thread; returns hits once loaded)",
        share_search,
    );
    r.add(
        "ui.share_add",
        "Download a GDTF Share revision (rid) and import it into the document, in the background",
        share_add,
    );
    r.add("ui.set_tag_visible", "Show or hide objects carrying a tag in the views", set_tag_visible);
    r.add("ui.set_active_layer", "Layer that new objects go on", set_active_layer);
    r.add("ui.set_snap", "Turn snaps on or off and set the grid", set_snap);
    r.add(
        "ui.set_tool_options",
        "Options for new objects: fixture type, truss, wall, batten, curtain, proscenium, wings, platform, support, hoist, ceiling",
        set_options,
    );
    r.add(
        "ui.sketch",
        "Sketch overlay in the plan: show lines/points, FreeCAD colours (green = fully constrained, red = conflicting), dimensions",
        sketch,
    );
    r.add(
        "ui.dimension",
        "Select a dimension in the plan (id, UUID or name) and optionally open its value editor; null clears",
        dimension,
    );
    r.add("ui.palette", "Open or close the command palette", palette);
    r.add(
        "ui.file_dialog",
        "Show a file dialog: open, save_as, export_pdf, import_gdtf, import_mvr, export_mvr, import_model",
        file_dialog,
    );
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
        "sheet": crate::sheets::current(c.engine.doc(), &ui.sheets),
        "sheet_viewport": ui.sheets.viewport,
        "viewport_editor": ui.sheets.editor,
        "plan_labels": ui.plan_labels,
        "sketch": { "show": ui.sketch.show, "colours": ui.sketch.colours, "dimensions": ui.sketch.dimensions,
                    "selected_dimension": ui.sketch.selected.map(crate::sketch_ui::dim_args),
                    "editing": ui.sketch.editing.as_ref().map(|e| json!({ "dimension": crate::sketch_ui::dim_args(e.source), "text": e.text })) },
    }))
}

#[derive(Deserialize, JsonSchema)]
struct SketchViewArgs {
    /// Draw sketch lines, points and dimensions over the plan.
    show: Option<bool>,
    /// Colour by constraint state: fully constrained green, conflicting red.
    colours: Option<bool>,
    /// Draw dimensions (sketch and reference).
    dimensions: Option<bool>,
}

fn sketch(c: &mut UiCtx, a: SketchViewArgs) -> CmdResult {
    let v = &mut c.ui.sketch;
    if let Some(b) = a.show {
        v.show = b;
    }
    if let Some(b) = a.colours {
        v.colours = b;
    }
    if let Some(b) = a.dimensions {
        v.dimensions = b;
    }
    Ok(json!({ "show": v.show, "colours": v.colours, "dimensions": v.dimensions }))
}

#[derive(Deserialize, JsonSchema)]
struct DimensionArgs {
    /// Sketch dimension id (number), reference dimension UUID, or a dimension's name;
    /// null clears the selection.
    id: Option<Value>,
    /// Layer id or name, for sketch dimension ids that exist on several layers.
    layer: Option<String>,
    /// Open the value editor (driving dimensions), as a double-click does.
    #[serde(default)]
    edit: bool,
}

fn dimension(c: &mut UiCtx, a: DimensionArgs) -> CmdResult {
    use stagecraft_geom::dims::DimSource;
    let Some(id) = a.id.filter(|v| !v.is_null()) else {
        c.ui.sketch.selected = None;
        c.ui.sketch.editing = None;
        return Ok(json!({}));
    };
    let doc = c.engine.doc();
    let only = match &a.layer {
        Some(l) => Some(
            doc.layers
                .values()
                .find(|x| x.name == *l || x.id.to_string() == *l)
                .map(|x| x.id)
                .ok_or_else(|| CmdError::not_found("layer", l))?,
        ),
        None => None,
    };
    let mut found: Vec<DimSource> = Vec::new();
    for (layer, s) in doc.sketches.iter().filter(|(l, _)| only.is_none_or(|o| o == **l)) {
        for d in &s.dimensions {
            let hit = match &id {
                Value::Number(n) => n.as_u64() == Some(d.id.0 as u64),
                Value::String(t) => d.name.as_deref() == Some(t.as_str()),
                _ => false,
            };
            if hit {
                found.push(DimSource::Sketch { layer: *layer, id: d.id });
            }
        }
    }
    if let Value::String(t) = &id {
        for d in doc.dimensions.values() {
            if d.id.to_string() == *t || d.name.as_deref() == Some(t.as_str()) {
                found.push(DimSource::Reference(d.id));
            }
        }
    }
    let src = match found.as_slice() {
        [one] => *one,
        [] => return Err(CmdError::not_found("dimension", &id)),
        _ => return Err(CmdError::invalid("several dimensions match; pass layer")),
    };
    c.ui.sketch.selected = Some(src);
    if a.edit {
        let DimSource::Sketch { layer, id } = src else {
            return Err(CmdError::invalid("reference dimensions have no value to edit"));
        };
        let d = c.engine.doc().sketches[&layer].dimension(id).cloned().expect("found");
        if !d.driving {
            return Err(CmdError::invalid("reference dimensions have no value to edit; make it driving first"));
        }
        let text = if d.measure.is_angle() { format!("{:.1}", d.value) } else { format!("{}", d.value / 1000.0) };
        c.ui.sketch.editing = Some(crate::sketch_ui::DimEdit { source: src, text, focus: true });
    }
    Ok(json!({ "selected": crate::sketch_ui::dim_args(src) }))
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
struct SheetArgs {
    /// Sheet id, number or name. Default: the one shown (or the first).
    sheet: Option<String>,
    /// Select this viewport (on the sheet); `null` clears the selection.
    #[serde(default, deserialize_with = "some")]
    viewport: Option<Option<stagecraft_core::Id>>,
    /// Open (true) or close the viewport editor for the selected viewport.
    editor: Option<bool>,
    /// Fit the paper into the window. Default true when the sheet changes.
    fit: Option<bool>,
}

/// Tells a missing field from an explicit `null`.
fn some<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

fn sheet(c: &mut UiCtx, a: SheetArgs) -> CmdResult {
    let doc = c.engine.doc();
    let before = crate::sheets::current(doc, &c.ui.sheets);
    if let Some(r) = &a.sheet {
        let id = if let Ok(id) = r.parse::<stagecraft_core::Id>()
            && doc.sheets.contains_key(&id)
        {
            id
        } else {
            doc.sheets
                .values()
                .find(|s| &s.number == r || &s.name == r)
                .map(|s| s.id)
                .ok_or_else(|| CmdError::not_found("sheet", r))?
        };
        c.ui.sheets.current = Some(id);
    }
    let cur =
        crate::sheets::current(doc, &c.ui.sheets).ok_or_else(|| CmdError::invalid("the document has no sheets"))?;
    if before != Some(cur) {
        c.ui.sheets.viewport = None;
        c.ui.sheets.fit_pending = true;
    }
    if let Some(v) = a.viewport {
        if let Some(v) = v
            && doc.sheets[&cur].viewport(v).is_none()
        {
            return Err(CmdError::not_found("viewport on this sheet", v));
        }
        c.ui.sheets.viewport = v;
    }
    if let Some(e) = a.editor {
        c.ui.sheets.editor = e && c.ui.sheets.viewport.is_some();
    }
    if let Some(f) = a.fit {
        c.ui.sheets.fit_pending = f;
    }
    c.ui.view = View::Sheets;
    Ok(json!({ "sheet": cur, "viewport": c.ui.sheets.viewport, "editor": c.ui.sheets.editor }))
}

#[derive(Deserialize, JsonSchema)]
struct LabelTemplateArgs {
    /// Template id or name; `null` = plain FIDs.
    template: Option<String>,
}

fn set_label_template(c: &mut UiCtx, a: LabelTemplateArgs) -> CmdResult {
    let doc = c.engine.doc();
    c.ui.plan_labels = match a.template {
        None => None,
        Some(r) => Some(
            doc.label_templates
                .values()
                .find(|t| t.id.to_string() == r || t.name == r)
                .map(|t| t.id)
                .ok_or_else(|| CmdError::not_found("label template", &r))?,
        ),
    };
    Ok(json!({ "template": c.ui.plan_labels }))
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
    if c.ui.view == View::Sheets {
        c.ui.sheets.fit_pending = true;
    }
    if c.ui.view.shows_plan() {
        let mut b = stagecraft_geom::pick::objects_bounds(c.engine.doc(), ids.clone(), &Default::default());
        if !a.selection {
            b = b.union(&crate::sketch_ui::bounds(c.engine.doc()));
        }
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
struct ShareSearchArgs {
    /// Words that must all appear in manufacturer, fixture or revision.
    #[serde(default)]
    query: String,
    /// Fetch a new list from the server instead of the cached one.
    #[serde(default)]
    refresh: bool,
}

fn share_search(c: &mut UiCtx, a: ShareSearchArgs) -> CmdResult {
    let lib = &mut c.ui.library;
    lib.query = a.query;
    c.ui.panels.library = true;
    let started = lib.load_list(a.refresh);
    let hits: Vec<Value> = lib
        .hits(40)
        .iter()
        .map(|e| {
            let modes: Vec<Value> = e.modes.iter().map(|m| json!({ "name": m.name, "footprint": m.dmx_footprint })).collect();
            json!({ "rid": e.rid, "manufacturer": e.manufacturer, "fixture": e.fixture, "revision": e.revision, "modes": modes })
        })
        .collect();
    Ok(
        json!({ "loading": started || lib.busy(), "list_loaded": lib.list.is_some(), "hits": hits, "message": lib.message.as_ref().map(|m| &m.0) }),
    )
}

#[derive(Deserialize, JsonSchema)]
struct ShareAddArgs {
    /// GDTF Share revision id from `ui.share_search`.
    rid: u64,
}

fn share_add(c: &mut UiCtx, a: ShareAddArgs) -> CmdResult {
    let lib = &mut c.ui.library;
    let entry = lib
        .list
        .as_ref()
        .and_then(|l| l.iter().find(|e| e.rid == a.rid).cloned())
        .unwrap_or_else(|| stagecraft_gdtf::share::ShareEntry { rid: a.rid, ..Default::default() });
    lib.add(entry);
    Ok(json!({ "started": true, "rid": a.rid }))
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
    /// Trim height of new battens, mm.
    batten_trim: Option<f64>,
    /// mm.
    batten_length: Option<f64>,
    /// Top edge of new curtains that are not hung on a batten, mm.
    curtain_trim: Option<f64>,
    /// mm.
    curtain_height: Option<f64>,
    /// Opening of new prosceniums, mm.
    proscenium_width: Option<f64>,
    /// mm.
    proscenium_height: Option<f64>,
    /// Wings per new wing set.
    wing_count: Option<u32>,
    /// mm between wings.
    wing_spacing: Option<f64>,
    /// Clear width between the legs, mm.
    wing_opening: Option<f64>,
    /// Size preset of new platforms in metres, e.g. `2x1`.
    platform_preset: Option<String>,
    /// Deck top above the base, mm.
    platform_height: Option<f64>,
    /// Platform system from the catalogue; empty = none.
    platform_system: Option<String>,
    /// Support catalogue entry, e.g. `Base plate 600`.
    support_preset: Option<String>,
    /// Hook height of new hoists, mm.
    hoist_height: Option<f64>,
    /// Underside of new ceilings, mm.
    ceiling_height: Option<f64>,
    /// Fixture type (library key) the fixture tool places; empty = generic.
    fixture_type: Option<String>,
    /// Mode of new fixtures; empty = the type's first mode.
    fixture_mode: Option<String>,
}

fn set_options(c: &mut UiCtx, a: OptionsArgs) -> CmdResult {
    if let Some(k) = a.fixture_type.as_deref().filter(|k| !k.is_empty())
        && !c.engine.doc().fixture_types.contains_key(k)
    {
        return Err(CmdError::not_found("fixture type", k));
    }
    let o = &mut c.ui.options;
    if let Some(v) = a.fixture_type {
        o.fixture_type = v;
        o.fixture_mode.clear();
    }
    if let Some(v) = a.fixture_mode {
        o.fixture_mode = v;
    }
    if let Some(v) = a.truss_profile {
        o.truss_profile = v;
    }
    if let Some(v) = a.truss_height {
        o.truss_height = v;
    }
    for (slot, v) in [
        (&mut o.wall_height, a.wall_height),
        (&mut o.wall_thickness, a.wall_thickness),
        (&mut o.batten_length, a.batten_length),
        (&mut o.curtain_height, a.curtain_height),
        (&mut o.proscenium_width, a.proscenium_width),
        (&mut o.proscenium_height, a.proscenium_height),
        (&mut o.wing_spacing, a.wing_spacing),
        (&mut o.wing_opening, a.wing_opening),
        (&mut o.platform_height, a.platform_height),
    ] {
        if let Some(v) = v {
            if v <= 0.0 || !v.is_finite() {
                return Err(CmdError::invalid_args("sizes must be positive mm"));
            }
            *slot = v;
        }
    }
    for (slot, v) in [
        (&mut o.batten_trim, a.batten_trim),
        (&mut o.curtain_trim, a.curtain_trim),
        (&mut o.hoist_height, a.hoist_height),
        (&mut o.ceiling_height, a.ceiling_height),
    ] {
        if let Some(v) = v {
            *slot = v;
        }
    }
    if let Some(n) = a.wing_count {
        if !(1..=20).contains(&n) {
            return Err(CmdError::invalid_args("wing_count must be 1–20"));
        }
        o.wing_count = n;
    }
    for (slot, v) in [
        (&mut o.platform_preset, a.platform_preset),
        (&mut o.platform_system, a.platform_system),
        (&mut o.support_preset, a.support_preset),
    ] {
        if let Some(v) = v {
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
struct FileDialogArgs {
    /// `open`, `save_as`, `export_pdf`, `import_gdtf`, `import_mvr`, `export_mvr` or
    /// `import_model`. The dialog shows after this frame; the GUI then runs the
    /// matching command (`mvr.import`, `model.import` …).
    dialog: String,
}

fn file_dialog(c: &mut UiCtx, a: FileDialogArgs) -> CmdResult {
    use crate::state::FileRequest as F;
    let r = match a.dialog.as_str() {
        "open" => F::Open,
        "save_as" => F::SaveAs,
        "export_pdf" => F::ExportPdf,
        "import_gdtf" => F::ImportGdtf,
        "import_mvr" => F::ImportMvr,
        "export_mvr" => F::ExportMvr,
        "import_model" => F::ImportModel,
        d => return Err(CmdError::invalid_args(format!("unknown dialog `{d}`"))),
    };
    c.ui.file_request = Some(r);
    Ok(json!({ "dialog": a.dialog }))
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
