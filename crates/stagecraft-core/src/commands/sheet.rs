//! Sheets, viewports, title blocks, label templates, object attributes and export.

use std::collections::BTreeMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{ensure_exists, resolve_layer, resolve_tag};
use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::model::{Document, Id, P2};
use crate::sheet::{
    LabelField, LabelRotation, LabelTemplate, Orientation, Paper, Sheet, Side, TitleBlock, TitleItem, ViewKind,
    Viewport, default_label_fields, exporter,
};

pub(super) fn register(r: &mut Registry) {
    r.add("sheet.list", Kind::Query, "Sheets in order with their viewports", list);
    r.add("sheet.get", Kind::Query, "Everything about one sheet", get);
    r.add("sheet.create", Kind::Edit, "Create a drawing sheet (A4–A0 or custom, portrait/landscape)", create);
    r.add("sheet.update", Kind::Edit, "Rename, renumber, resize or reorder a sheet; pick its title block", update);
    r.add("sheet.delete", Kind::Edit, "Delete a sheet and its viewports", delete);
    r.add(
        "sheet.export_pdf",
        Kind::Query,
        "Write sheets as one vector PDF, a page per sheet (all sheets in order by default)",
        export_pdf,
    );
    r.add("sheet.export_svg", Kind::Query, "Write one sheet as SVG", export_svg);
    r.add(
        "viewport.create",
        Kind::Edit,
        "Put a view (plan, elevation, section, rendered 3D, fixture list) on a sheet at a scale",
        vp_create,
    );
    r.add(
        "viewport.update",
        Kind::Edit,
        "Change a viewport: view, scale, crop, position, hidden tags/layers, line weights, label template",
        vp_update,
    );
    r.add("viewport.delete", Kind::Edit, "Remove a viewport from its sheet", vp_delete);
    r.add("label.list", Kind::Query, "Label templates", label_list);
    r.add("label.create", Kind::Edit, "Create a label template (fields, rotation mode)", label_create);
    r.add("label.update", Kind::Edit, "Change a label template's name, rotation mode, fields or gap", label_update);
    r.add("label.delete", Kind::Edit, "Delete a label template; viewports using it show no labels", label_delete);
    r.add(
        "label.move",
        Kind::Edit,
        "Drag a fixture's label in one viewport by hand (offset in paper mm), or reset it",
        label_move,
    );
    r.add("titleblock.list", Kind::Query, "Title block templates, the built-in generic one first", tb_list);
    r.add(
        "titleblock.create",
        Kind::Edit,
        "Create a title block template (a copy of the generic one by default)",
        tb_create,
    );
    r.add("titleblock.update", Kind::Edit, "Change a title block template's name, size or items", tb_update);
    r.add("titleblock.delete", Kind::Edit, "Delete a title block template; its sheets use the generic one", tb_delete);
    r.add(
        "object.set_attrs",
        Kind::Edit,
        "Set free attributes (gel, circuit, focus, note …) on objects; an empty value removes one",
        set_attrs,
    );
}

#[derive(Deserialize, JsonSchema)]
struct NoArgs {}

// ---- lookups ----

/// A sheet by id, number or name.
pub(crate) fn resolve_sheet(doc: &Document, r: &str) -> Result<Id, CmdError> {
    if let Ok(id) = r.parse::<Id>()
        && doc.sheets.contains_key(&id)
    {
        return Ok(id);
    }
    doc.sheets
        .values()
        .find(|s| s.number == r)
        .or_else(|| doc.sheets.values().find(|s| s.name == r))
        .map(|s| s.id)
        .ok_or_else(|| CmdError::not_found("sheet", r))
}

fn resolve_label(doc: &Document, r: &str) -> Result<Id, CmdError> {
    if let Ok(id) = r.parse::<Id>()
        && doc.label_templates.contains_key(&id)
    {
        return Ok(id);
    }
    doc.label_templates
        .values()
        .find(|t| t.name == r)
        .map(|t| t.id)
        .ok_or_else(|| CmdError::not_found("label template", r))
}

fn resolve_title_block(doc: &Document, r: &str) -> Result<Id, CmdError> {
    if let Ok(id) = r.parse::<Id>()
        && doc.title_blocks.contains_key(&id)
    {
        return Ok(id);
    }
    doc.title_blocks.values().find(|t| t.name == r).map(|t| t.id).ok_or_else(|| CmdError::not_found("title block", r))
}

fn viewport_mut(doc: &mut Document, id: Id) -> Result<&mut Viewport, CmdError> {
    doc.viewport_mut(id).ok_or_else(|| CmdError::not_found("viewport", id))
}

fn paper_from(name: &str, width: Option<f64>, height: Option<f64>) -> Result<Paper, CmdError> {
    Ok(match name.to_ascii_lowercase().as_str() {
        "a4" => Paper::A4,
        "a3" => Paper::A3,
        "a2" => Paper::A2,
        "a1" => Paper::A1,
        "a0" => Paper::A0,
        "custom" => {
            let (Some(w), Some(h)) = (width, height) else {
                return Err(CmdError::invalid_args("a custom paper needs width and height (mm)"));
            };
            if !(w >= 50.0 && h >= 50.0 && w <= 5000.0 && h <= 5000.0) {
                return Err(CmdError::invalid_args("custom paper must be 50–5000 mm on each side"));
            }
            Paper::Custom { width: w, height: h }
        }
        other => {
            return Err(CmdError::invalid_args(format!("unknown paper `{other}`; use a4, a3, a2, a1, a0 or custom")));
        }
    })
}

fn sheet_json(doc: &Document, s: &Sheet) -> Value {
    let [w, h] = s.size();
    let tb = s.title_block.and_then(|t| doc.title_blocks.get(&t)).map_or("Generic".to_owned(), |t| t.name.clone());
    json!({
        "id": s.id, "name": s.name, "number": s.number, "paper": s.paper.name(),
        "orientation": s.orientation, "size": [w, h], "order": s.order,
        "title_block": if s.show_title_block { Some(tb) } else { None },
        "legend": s.show_legend,
        "viewports": s.viewports.iter().map(|v| json!({
            "id": v.id, "name": v.name, "view": v.view, "scale": format!("1:{}", v.scale),
            "position": v.position, "crop": v.crop,
        })).collect::<Vec<_>>(),
    })
}

// ---- sheets ----

fn list(ctx: &mut Ctx, _: NoArgs) -> CmdResult {
    let doc = ctx.doc();
    Ok(json!(doc.sheets_sorted().into_iter().map(|s| sheet_json(doc, s)).collect::<Vec<_>>()))
}

#[derive(Deserialize, JsonSchema)]
struct SheetRef {
    /// Sheet id, number or name.
    sheet: String,
}

fn get(ctx: &mut Ctx, a: SheetRef) -> CmdResult {
    let id = resolve_sheet(ctx.doc(), &a.sheet)?;
    Ok(json!(ctx.doc().sheets[&id]))
}

#[derive(Deserialize, JsonSchema)]
struct CreateArgs {
    /// Default "Sheet <n>".
    name: Option<String>,
    /// Default: the next free number ("1", "2" …).
    number: Option<String>,
    /// a4, a3, a2, a1, a0 or custom. Default a3.
    paper: Option<String>,
    /// For custom paper, mm.
    width: Option<f64>,
    height: Option<f64>,
    /// Default landscape.
    #[serde(default)]
    orientation: Orientation,
    /// Title block template id or name, `generic` (default) or `none`.
    title_block: Option<String>,
}

/// `(template, shown)` from a title block argument.
fn title_block_arg(doc: &Document, r: &str) -> Result<(Option<Id>, bool), CmdError> {
    match r {
        "generic" | "" => Ok((None, true)),
        "none" => Ok((None, false)),
        r => Ok((Some(resolve_title_block(doc, r)?), true)),
    }
}

fn create(ctx: &mut Ctx, a: CreateArgs) -> CmdResult {
    let doc = ctx.doc();
    let paper = paper_from(a.paper.as_deref().unwrap_or("a3"), a.width, a.height)?;
    let (title_block, show_title_block) = title_block_arg(doc, a.title_block.as_deref().unwrap_or("generic"))?;
    let number = match a.number {
        Some(n) => n,
        None => (1..).map(|i: u32| i.to_string()).find(|n| !doc.sheets.values().any(|s| &s.number == n)).expect("free"),
    };
    if doc.sheets.values().any(|s| s.number == number) {
        return Err(CmdError::invalid(format!("sheet number `{number}` is taken")));
    }
    let order = doc.sheets.values().map(|s| s.order + 1).max().unwrap_or(0);
    let s = Sheet {
        id: Uuid::new_v4(),
        name: a.name.unwrap_or_else(|| format!("Sheet {number}")),
        number,
        paper,
        orientation: a.orientation,
        title_block,
        show_title_block,
        show_legend: true,
        order,
        viewports: Vec::new(),
    };
    let id = s.id;
    ctx.doc_mut().sheets.insert(id, s);
    Ok(json!({ "id": id }))
}

#[derive(Deserialize, JsonSchema)]
struct UpdateArgs {
    /// Sheet id, number or name.
    sheet: String,
    name: Option<String>,
    number: Option<String>,
    /// a4, a3, a2, a1, a0 or custom (with width and height, mm).
    paper: Option<String>,
    width: Option<f64>,
    height: Option<f64>,
    orientation: Option<Orientation>,
    /// Title block template id or name, `generic` or `none`.
    title_block: Option<String>,
    /// Show the legend of fixture types.
    legend: Option<bool>,
    order: Option<i32>,
}

fn update(ctx: &mut Ctx, a: UpdateArgs) -> CmdResult {
    let doc = ctx.doc();
    let id = resolve_sheet(doc, &a.sheet)?;
    if let Some(n) = &a.number
        && doc.sheets.values().any(|s| s.id != id && &s.number == n)
    {
        return Err(CmdError::invalid(format!("sheet number `{n}` is taken")));
    }
    let paper = match &a.paper {
        Some(p) => Some(paper_from(p, a.width, a.height)?),
        None => match (&doc.sheets[&id].paper, a.width, a.height) {
            (Paper::Custom { width, height }, w, h) if w.is_some() || h.is_some() => {
                Some(paper_from("custom", w.or(Some(*width)), h.or(Some(*height)))?)
            }
            _ => None,
        },
    };
    let tb = a.title_block.as_deref().map(|r| title_block_arg(doc, r)).transpose()?;
    let s = ctx.doc_mut().sheets.get_mut(&id).expect("resolved");
    if let Some(v) = a.name {
        s.name = v;
    }
    if let Some(v) = a.number {
        s.number = v;
    }
    if let Some(p) = paper {
        s.paper = p;
    }
    if let Some(o) = a.orientation {
        s.orientation = o;
    }
    if let Some((t, shown)) = tb {
        s.title_block = t;
        s.show_title_block = shown;
    }
    if let Some(l) = a.legend {
        s.show_legend = l;
    }
    if let Some(o) = a.order {
        s.order = o;
    }
    let s = s.clone();
    Ok(sheet_json(ctx.doc(), &s))
}

fn delete(ctx: &mut Ctx, a: SheetRef) -> CmdResult {
    let id = resolve_sheet(ctx.doc(), &a.sheet)?;
    ctx.doc_mut().sheets.remove(&id);
    Ok(json!({}))
}

#[derive(Deserialize, JsonSchema)]
struct ExportPdfArgs {
    /// Where to write the PDF.
    path: PathBuf,
    /// Sheet ids, numbers or names, in page order. Default: every sheet in order.
    sheets: Option<Vec<String>>,
}

fn export_pdf(ctx: &mut Ctx, a: ExportPdfArgs) -> CmdResult {
    let doc = ctx.doc();
    let ids: Vec<Id> = match &a.sheets {
        Some(v) => v.iter().map(|r| resolve_sheet(doc, r)).collect::<Result<_, _>>()?,
        None => doc.sheets_sorted().into_iter().map(|s| s.id).collect(),
    };
    if ids.is_empty() {
        return Err(CmdError::invalid("the document has no sheets; create one with sheet.create"));
    }
    let ex = exporter().ok_or_else(|| CmdError::invalid("no sheet exporter is installed in this build"))?;
    let bytes = ex.pdf(doc, &ids).map_err(CmdError::invalid)?;
    std::fs::write(&a.path, &bytes).map_err(|e| CmdError::io(format!("{}: {e}", a.path.display())))?;
    Ok(json!({ "path": a.path, "pages": ids.len(), "bytes": bytes.len() }))
}

#[derive(Deserialize, JsonSchema)]
struct ExportSvgArgs {
    /// Where to write the SVG.
    path: PathBuf,
    /// Sheet id, number or name.
    sheet: String,
}

fn export_svg(ctx: &mut Ctx, a: ExportSvgArgs) -> CmdResult {
    let doc = ctx.doc();
    let id = resolve_sheet(doc, &a.sheet)?;
    let ex = exporter().ok_or_else(|| CmdError::invalid("no sheet exporter is installed in this build"))?;
    let svg = ex.svg(doc, id).map_err(CmdError::invalid)?;
    std::fs::write(&a.path, svg.as_bytes()).map_err(|e| CmdError::io(format!("{}: {e}", a.path.display())))?;
    Ok(json!({ "path": a.path, "bytes": svg.len() }))
}

// ---- viewports ----

/// A view by short name or in full.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
enum ViewArg {
    /// `plan`, `front`, `back`, `left`, `right` (elevations), `rendered`,
    /// `fixture_list`, or `section_x` / `section_y` (cuts through the origin: x hides
    /// stage left of 0 and looks from there, y hides downstage and looks upstage).
    Name(String),
    Full(ViewKind),
}

impl ViewArg {
    fn resolve(self) -> Result<ViewKind, CmdError> {
        let ViewArg::Name(n) = self else {
            let ViewArg::Full(v) = self else { unreachable!() };
            return Ok(v);
        };
        Ok(match n.as_str() {
            "plan" => ViewKind::Plan,
            "front" => ViewKind::Elevation { side: Side::Front },
            "back" => ViewKind::Elevation { side: Side::Back },
            "left" => ViewKind::Elevation { side: Side::Left },
            "right" => ViewKind::Elevation { side: Side::Right },
            "rendered" | "3d" => ViewKind::Rendered { eye: None, target: None },
            "fixture_list" => ViewKind::FixtureList,
            "section_x" => ViewKind::Section { origin: [0.0; 3], normal: [1.0, 0.0, 0.0] },
            "section_y" => ViewKind::Section { origin: [0.0; 3], normal: [0.0, -1.0, 0.0] },
            other => return Err(CmdError::invalid_args(format!("unknown view `{other}`"))),
        })
    }
}

fn check_scale(s: f64) -> Result<(), CmdError> {
    if (1.0..=2000.0).contains(&s) {
        Ok(())
    } else {
        Err(CmdError::invalid_args("scale is the denominator (50 = 1:50), 1–2000"))
    }
}

fn check_crop(c: &[P2; 2]) -> Result<[P2; 2], CmdError> {
    let min = [c[0][0].min(c[1][0]), c[0][1].min(c[1][1])];
    let max = [c[0][0].max(c[1][0]), c[0][1].max(c[1][1])];
    if max[0] - min[0] < 1.0 || max[1] - min[1] < 1.0 || !min.iter().chain(&max).all(|v| v.is_finite()) {
        return Err(CmdError::invalid_args("crop must be two corners at least 1 mm apart"));
    }
    Ok([min, max])
}

fn tag_ids(doc: &Document, refs: &[String]) -> Result<Vec<Id>, CmdError> {
    refs.iter().map(|r| resolve_tag(doc, r).ok_or_else(|| CmdError::not_found("tag", r))).collect()
}

fn layer_ids(doc: &Document, refs: &[String]) -> Result<Vec<Id>, CmdError> {
    refs.iter().map(|r| resolve_layer(doc, r)).collect()
}

const LINE_CLASSES: [&str; 6] = ["thin", "medium", "thick", "hidden", "centre", "cut"];

fn check_weights(w: &BTreeMap<String, f64>) -> Result<(), CmdError> {
    for (k, v) in w {
        if !LINE_CLASSES.contains(&k.as_str()) {
            return Err(CmdError::invalid_args(format!("unknown line class `{k}`; use {}", LINE_CLASSES.join(", "))));
        }
        if !(0.01..=5.0).contains(v) {
            return Err(CmdError::invalid_args(format!("line weight for {k} must be 0.01–5 mm")));
        }
    }
    Ok(())
}

#[derive(Deserialize, JsonSchema)]
struct VpCreateArgs {
    /// Sheet id, number or name.
    sheet: String,
    /// What it shows: `plan`, `front`, `back`, `left`, `right`, `section_x`, `section_y`,
    /// `rendered`, `fixture_list`, or a full view object such as
    /// `{"type":"section","origin":[0,4000,0],"normal":[0,-1,0]}`.
    view: ViewArg,
    /// Scale denominator: 50 = 1:50. Default 50 (fixture lists ignore it).
    #[serde(default = "d50")]
    scale: f64,
    /// Default: named after the view.
    name: Option<String>,
    /// Model mm `[[min_x, min_y], [max_x, max_y]]` in view coordinates (plan: world
    /// x/y; elevations/sections: across and up). Default: fit what is shown.
    crop: Option<[P2; 2]>,
    /// Centre on the paper, mm from the lower left. Default: centre of the space left
    /// of the title block.
    position: Option<P2>,
    /// Tag ids or names left out of this viewport.
    #[serde(default)]
    hidden_tags: Vec<String>,
    /// Layer ids or names left out of this viewport.
    #[serde(default)]
    hidden_layers: Vec<String>,
    /// Label template id or name (plan viewports).
    label_template: Option<String>,
    /// Line class → pen width mm, e.g. `{"thick": 0.7}`.
    #[serde(default)]
    line_weights: BTreeMap<String, f64>,
}

fn d50() -> f64 {
    50.0
}

fn default_name(v: &ViewKind) -> String {
    match v {
        ViewKind::Plan => "Plan".into(),
        ViewKind::Elevation { side } => match side {
            Side::Front => "Front elevation".into(),
            Side::Back => "Back elevation".into(),
            Side::Left => "Left elevation".into(),
            Side::Right => "Right elevation".into(),
        },
        ViewKind::Section { .. } => "Section".into(),
        ViewKind::Rendered { .. } => "3D view".into(),
        ViewKind::FixtureList => "Fixture list".into(),
    }
}

fn vp_create(ctx: &mut Ctx, a: VpCreateArgs) -> CmdResult {
    let doc = ctx.doc();
    let sid = resolve_sheet(doc, &a.sheet)?;
    let view = a.view.resolve()?;
    check_scale(a.scale)?;
    check_weights(&a.line_weights)?;
    let crop = a.crop.as_ref().map(check_crop).transpose()?;
    let hidden_tags = tag_ids(doc, &a.hidden_tags)?;
    let hidden_layers = layer_ids(doc, &a.hidden_layers)?;
    let label_template = a.label_template.as_deref().map(|r| resolve_label(doc, r)).transpose()?;
    let sheet = &doc.sheets[&sid];
    let [w, h] = sheet.size();
    let position = a.position.unwrap_or([(w - 190.0).max(w / 2.0) / 2.0 + 5.0, h / 2.0 + 10.0]);
    let vp = Viewport {
        id: Uuid::new_v4(),
        name: a.name.unwrap_or_else(|| default_name(&view)),
        view,
        scale: a.scale,
        crop,
        position,
        hidden_tags,
        hidden_layers,
        line_weights: a.line_weights,
        label_template,
        label_offsets: BTreeMap::new(),
        show_title: true,
        scale_bar: !matches!(view, ViewKind::FixtureList | ViewKind::Rendered { .. }),
    };
    let id = vp.id;
    ctx.doc_mut().sheets.get_mut(&sid).expect("resolved").viewports.push(vp);
    Ok(json!({ "id": id }))
}

#[derive(Deserialize, JsonSchema)]
struct VpUpdateArgs {
    viewport: Id,
    name: Option<String>,
    view: Option<ViewArg>,
    /// Scale denominator.
    scale: Option<f64>,
    /// New crop, model mm; `null` keeps it. Use `fit: true` to go back to fitting.
    crop: Option<[P2; 2]>,
    /// Drop the crop and fit everything shown.
    #[serde(default)]
    fit: bool,
    /// Centre on the paper, mm.
    position: Option<P2>,
    /// Move by this on the paper, mm.
    by: Option<P2>,
    /// Replace the hidden tags (ids or names).
    hidden_tags: Option<Vec<String>>,
    hidden_layers: Option<Vec<String>>,
    /// Label template id or name; empty string = no labels.
    label_template: Option<String>,
    /// Line class → pen width mm; replaces the map (empty map = defaults).
    line_weights: Option<BTreeMap<String, f64>>,
    show_title: Option<bool>,
    scale_bar: Option<bool>,
    /// Forget all hand-dragged label positions of this viewport.
    #[serde(default)]
    reset_labels: bool,
    /// Move the viewport to the front (drawn last).
    #[serde(default)]
    to_front: bool,
}

fn vp_update(ctx: &mut Ctx, a: VpUpdateArgs) -> CmdResult {
    let doc = ctx.doc();
    let sheet = doc.sheet_of_viewport(a.viewport).ok_or_else(|| CmdError::not_found("viewport", a.viewport))?.id;
    let view = a.view.map(ViewArg::resolve).transpose()?;
    if let Some(s) = a.scale {
        check_scale(s)?;
    }
    if let Some(w) = &a.line_weights {
        check_weights(w)?;
    }
    let crop = a.crop.as_ref().map(check_crop).transpose()?;
    let tags = a.hidden_tags.as_deref().map(|t| tag_ids(doc, t)).transpose()?;
    let layers = a.hidden_layers.as_deref().map(|t| layer_ids(doc, t)).transpose()?;
    let label = match a.label_template.as_deref() {
        None => None,
        Some("") => Some(None),
        Some(r) => Some(Some(resolve_label(doc, r)?)),
    };
    let doc = ctx.doc_mut();
    if a.to_front {
        let s = doc.sheets.get_mut(&sheet).expect("found");
        let i = s.viewports.iter().position(|v| v.id == a.viewport).expect("found");
        let v = s.viewports.remove(i);
        s.viewports.push(v);
    }
    let v = viewport_mut(doc, a.viewport)?;
    if let Some(x) = a.name {
        v.name = x;
    }
    if let Some(x) = view {
        v.view = x;
    }
    if let Some(x) = a.scale {
        v.scale = x;
    }
    if a.fit {
        v.crop = None;
    }
    if let Some(c) = crop {
        v.crop = Some(c);
    }
    if let Some(p) = a.position {
        v.position = p;
    }
    if let Some(d) = a.by {
        v.position = [v.position[0] + d[0], v.position[1] + d[1]];
    }
    if let Some(t) = tags {
        v.hidden_tags = t;
    }
    if let Some(l) = layers {
        v.hidden_layers = l;
    }
    if let Some(l) = label {
        v.label_template = l;
    }
    if let Some(w) = a.line_weights {
        v.line_weights = w;
    }
    if let Some(b) = a.show_title {
        v.show_title = b;
    }
    if let Some(b) = a.scale_bar {
        v.scale_bar = b;
    }
    if a.reset_labels {
        v.label_offsets.clear();
    }
    Ok(json!(v))
}

#[derive(Deserialize, JsonSchema)]
struct VpRef {
    viewport: Id,
}

fn vp_delete(ctx: &mut Ctx, a: VpRef) -> CmdResult {
    let sheet = ctx.doc().sheet_of_viewport(a.viewport).ok_or_else(|| CmdError::not_found("viewport", a.viewport))?.id;
    ctx.doc_mut().sheets.get_mut(&sheet).expect("found").viewports.retain(|v| v.id != a.viewport);
    Ok(json!({}))
}

// ---- label templates ----

fn label_list(ctx: &mut Ctx, _: NoArgs) -> CmdResult {
    let mut v: Vec<_> = ctx.doc().label_templates.values().cloned().collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(json!(v))
}

const LABEL_FIELDS: [&str; 12] =
    ["fid", "cid", "address", "mode", "gel", "circuit", "focus", "note", "name", "type", "weight", "footprint"];

fn check_fields(fields: &[LabelField]) -> Result<(), CmdError> {
    for f in fields {
        if !(LABEL_FIELDS.contains(&f.field.as_str()) || f.field.strip_prefix("attr:").is_some_and(|k| !k.is_empty())) {
            return Err(CmdError::invalid_args(format!(
                "unknown label field `{}`; use {} or attr:<key>",
                f.field,
                LABEL_FIELDS.join(", ")
            )));
        }
        if !(0.5..=20.0).contains(&f.size) {
            return Err(CmdError::invalid_args("label text size must be 0.5–20 mm"));
        }
        if !f.anchor.iter().all(|v| v.is_finite()) {
            return Err(CmdError::invalid_args("label anchor must be finite"));
        }
    }
    Ok(())
}

#[derive(Deserialize, JsonSchema)]
struct LabelCreateArgs {
    name: String,
    /// `anchored_upright` (default), `sheet_fixed` or `follow`.
    #[serde(default)]
    rotation: LabelRotation,
    /// Fields. Default: FID boxed on the fixture, gel in front, address behind.
    fields: Option<Vec<LabelField>>,
    /// Gap between anchor and text, paper mm. Default 0.8.
    gap: Option<f64>,
}

fn label_create(ctx: &mut Ctx, a: LabelCreateArgs) -> CmdResult {
    let name = a.name.trim().to_owned();
    if name.is_empty() {
        return Err(CmdError::invalid_args("label template name is empty"));
    }
    if ctx.doc().label_templates.values().any(|t| t.name == name) {
        return Err(CmdError::invalid(format!("a label template named `{name}` exists")));
    }
    let fields = a.fields.unwrap_or_else(default_label_fields);
    check_fields(&fields)?;
    let t = LabelTemplate {
        id: Uuid::new_v4(),
        name,
        rotation: a.rotation,
        fields,
        gap: a.gap.unwrap_or(0.8).clamp(0.0, 20.0),
    };
    let id = t.id;
    ctx.doc_mut().label_templates.insert(id, t);
    Ok(json!({ "id": id }))
}

#[derive(Deserialize, JsonSchema)]
struct LabelUpdateArgs {
    /// Template id or name.
    template: String,
    name: Option<String>,
    rotation: Option<LabelRotation>,
    /// Replaces all fields.
    fields: Option<Vec<LabelField>>,
    gap: Option<f64>,
}

fn label_update(ctx: &mut Ctx, a: LabelUpdateArgs) -> CmdResult {
    let doc = ctx.doc();
    let id = resolve_label(doc, &a.template)?;
    if let Some(n) = &a.name
        && doc.label_templates.values().any(|t| t.id != id && &t.name == n)
    {
        return Err(CmdError::invalid(format!("a label template named `{n}` exists")));
    }
    if let Some(f) = &a.fields {
        check_fields(f)?;
    }
    let t = ctx.doc_mut().label_templates.get_mut(&id).expect("resolved");
    if let Some(v) = a.name {
        t.name = v;
    }
    if let Some(v) = a.rotation {
        t.rotation = v;
    }
    if let Some(v) = a.fields {
        t.fields = v;
    }
    if let Some(v) = a.gap {
        t.gap = v.clamp(0.0, 20.0);
    }
    Ok(json!(t))
}

#[derive(Deserialize, JsonSchema)]
struct LabelRef {
    /// Template id or name.
    template: String,
}

fn label_delete(ctx: &mut Ctx, a: LabelRef) -> CmdResult {
    let id = resolve_label(ctx.doc(), &a.template)?;
    let doc = ctx.doc_mut();
    doc.label_templates.remove(&id);
    doc.edit_viewports(|v| {
        if v.label_template == Some(id) {
            v.label_template = None;
        }
    });
    Ok(json!({}))
}

#[derive(Deserialize, JsonSchema)]
struct LabelMoveArgs {
    viewport: Id,
    fixture: Id,
    /// Offset from where the template puts the label, paper mm (x right, y up).
    /// Missing or `null` = back to automatic placement.
    offset: Option<P2>,
    /// Add to the current offset instead of replacing it.
    #[serde(default)]
    relative: bool,
}

fn label_move(ctx: &mut Ctx, a: LabelMoveArgs) -> CmdResult {
    ensure_exists(ctx.doc(), &[a.fixture])?;
    let v = viewport_mut(ctx.doc_mut(), a.viewport)?;
    match a.offset {
        None => {
            v.label_offsets.remove(&a.fixture);
        }
        Some(o) if a.relative => {
            let cur = v.label_offsets.get(&a.fixture).copied().unwrap_or_default();
            v.label_offsets.insert(a.fixture, [cur[0] + o[0], cur[1] + o[1]]);
        }
        Some(o) => {
            v.label_offsets.insert(a.fixture, o);
        }
    }
    Ok(json!({ "offset": v.label_offsets.get(&a.fixture) }))
}

// ---- title blocks ----

fn tb_list(ctx: &mut Ctx, _: NoArgs) -> CmdResult {
    let mut v = vec![json!(TitleBlock::generic())];
    let mut own: Vec<_> = ctx.doc().title_blocks.values().cloned().collect();
    own.sort_by(|a, b| a.name.cmp(&b.name));
    v.extend(own.into_iter().map(|t| json!(t)));
    Ok(json!(v))
}

fn check_tb(width: f64, height: f64, items: &[TitleItem]) -> Result<(), CmdError> {
    if !(10.0..=1000.0).contains(&width) || !(5.0..=1000.0).contains(&height) {
        return Err(CmdError::invalid_args("title block width 10–1000 mm, height 5–1000 mm"));
    }
    if items.len() > 500 {
        return Err(CmdError::invalid_args("at most 500 title block items"));
    }
    Ok(())
}

#[derive(Deserialize, JsonSchema)]
struct TbCreateArgs {
    name: String,
    /// Copy this template (id or name); default the generic one.
    from: Option<String>,
    /// mm; default from the copied template.
    width: Option<f64>,
    height: Option<f64>,
    /// Replace the items.
    items: Option<Vec<TitleItem>>,
}

fn tb_create(ctx: &mut Ctx, a: TbCreateArgs) -> CmdResult {
    let doc = ctx.doc();
    let name = a.name.trim().to_owned();
    if name.is_empty() || name == "Generic" {
        return Err(CmdError::invalid_args("give the title block a name other than Generic"));
    }
    if doc.title_blocks.values().any(|t| t.name == name) {
        return Err(CmdError::invalid(format!("a title block named `{name}` exists")));
    }
    let base = match a.from.as_deref() {
        None | Some("generic") | Some("Generic") => TitleBlock::generic(),
        Some(r) => doc.title_blocks[&resolve_title_block(doc, r)?].clone(),
    };
    let t = TitleBlock {
        id: Uuid::new_v4(),
        name,
        width: a.width.unwrap_or(base.width),
        height: a.height.unwrap_or(base.height),
        items: a.items.unwrap_or(base.items),
    };
    check_tb(t.width, t.height, &t.items)?;
    let id = t.id;
    ctx.doc_mut().title_blocks.insert(id, t);
    Ok(json!({ "id": id }))
}

#[derive(Deserialize, JsonSchema)]
struct TbUpdateArgs {
    /// Template id or name.
    template: String,
    name: Option<String>,
    width: Option<f64>,
    height: Option<f64>,
    items: Option<Vec<TitleItem>>,
}

fn tb_update(ctx: &mut Ctx, a: TbUpdateArgs) -> CmdResult {
    let id = resolve_title_block(ctx.doc(), &a.template)?;
    let mut t = ctx.doc().title_blocks[&id].clone();
    if let Some(v) = a.name {
        t.name = v;
    }
    if let Some(v) = a.width {
        t.width = v;
    }
    if let Some(v) = a.height {
        t.height = v;
    }
    if let Some(v) = a.items {
        t.items = v;
    }
    check_tb(t.width, t.height, &t.items)?;
    ctx.doc_mut().title_blocks.insert(id, t.clone());
    Ok(json!(t))
}

#[derive(Deserialize, JsonSchema)]
struct TbRef {
    /// Template id or name.
    template: String,
}

fn tb_delete(ctx: &mut Ctx, a: TbRef) -> CmdResult {
    let id = resolve_title_block(ctx.doc(), &a.template)?;
    let doc = ctx.doc_mut();
    doc.title_blocks.remove(&id);
    let ids: Vec<Id> = doc.sheets.values().filter(|s| s.title_block == Some(id)).map(|s| s.id).collect();
    for s in ids {
        doc.sheets.get_mut(&s).expect("listed").title_block = None;
    }
    Ok(json!({}))
}

// ---- attributes ----

#[derive(Deserialize, JsonSchema)]
struct AttrArgs {
    ids: Vec<Id>,
    /// key → value. Keys labels know: `gel`, `circuit`, `focus`, `note`; any other key
    /// works with `attr:<key>`. An empty value removes the key.
    attrs: BTreeMap<String, String>,
}

fn set_attrs(ctx: &mut Ctx, a: AttrArgs) -> CmdResult {
    ensure_exists(ctx.doc(), &a.ids)?;
    if a.attrs.keys().any(|k| k.trim().is_empty()) {
        return Err(CmdError::invalid_args("attribute keys must not be empty"));
    }
    let doc = ctx.doc_mut();
    for id in &a.ids {
        let o = doc.objects.get_mut(id).expect("checked");
        for (k, v) in &a.attrs {
            if v.is_empty() {
                o.attrs.remove(k);
            } else {
                o.attrs.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(json!({ "changed": a.ids.len() }))
}
