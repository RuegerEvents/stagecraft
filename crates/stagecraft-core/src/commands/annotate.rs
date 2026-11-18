//! Underlays (images, PDF pages, SVGs, DXFs under the plan) and text notes.

use std::path::PathBuf;

use glam::{DAffine3, DQuat, DVec3};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::create::{Common, insert};
use super::{object, resolve_layer};
use crate::asset::Asset;
use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::geom::world;
use crate::model::{Document, Id, Layer, ObjectKind, P2, Placement, UnderlayFormat};

pub(super) fn register(r: &mut Registry) {
    r.add(
        "underlay.import",
        Kind::Edit,
        "Import an image (PNG/JPEG), PDF page, SVG or DXF as an underlay on its own layer",
        import,
    );
    r.add(
        "underlay.calibrate",
        Kind::Edit,
        "Scale an underlay so two points of it are a known distance apart",
        calibrate,
    );
    r.add(
        "underlay.align",
        Kind::Edit,
        "Move, turn and scale an underlay so two of its points land on two target points",
        align,
    );
    r.add("underlay.update", Kind::Edit, "Change an underlay's opacity, lock or visibility", update);
    r.add(
        "note.create",
        Kind::Edit,
        "Place a text note in the plan (assumptions, remarks), optionally with a leader",
        note_create,
    );
    r.add("note.update", Kind::Edit, "Change a note's text, size, frame or leader", note_update);
}

/// Refuse an edit of a locked underlay.
pub(crate) fn ensure_unlocked(doc: &Document, ids: &[Id]) -> Result<(), CmdError> {
    for id in ids {
        if let Some(o) = doc.objects.get(id)
            && matches!(o.kind, ObjectKind::Underlay { locked: true, .. })
        {
            return Err(CmdError::invalid(format!(
                "underlay `{}` is locked; unlock it first (underlay.update with locked = false)",
                o.name
            )));
        }
    }
    Ok(())
}

fn format_of(f: stagecraft_underlay::Format) -> UnderlayFormat {
    match f {
        stagecraft_underlay::Format::Image => UnderlayFormat::Image,
        stagecraft_underlay::Format::Pdf => UnderlayFormat::Pdf,
        stagecraft_underlay::Format::Svg => UnderlayFormat::Svg,
        stagecraft_underlay::Format::Dxf => UnderlayFormat::Dxf,
        stagecraft_underlay::Format::Dwg => UnderlayFormat::Dwg,
    }
}

#[derive(Deserialize, JsonSchema)]
struct ImportArgs {
    /// Path to a `.png`, `.jpg`, `.pdf`, `.svg`, `.dxf` or `.dwg` file (DWG: desktop only).
    path: Option<PathBuf>,
    /// The file's bytes, base64, instead of `path` (needs `file_name`).
    data_base64: Option<String>,
    /// File name with extension for `data_base64`.
    file_name: Option<String>,
    /// Name of the underlay object. Default: the file name without extension.
    name: Option<String>,
    /// Layer id or name. Default: a new layer "Underlay" (or "Underlay 2" …) at
    /// elevation 0, ordered below every other layer.
    layer: Option<String>,
    /// PDF page, 1-based. Default 1.
    #[serde(default = "one")]
    page: u32,
    /// Drawing scale 1:N of images, PDFs and SVGs, taking a pixel or SVG unit as
    /// 1/96 inch and a PDF point as 1/72 inch. Default 100. For DXF the drawing units
    /// (`$INSUNITS`, mm when unset) are used and this is ignored unless given; then it
    /// is mm per drawing unit.
    scale: Option<f64>,
    /// Where the source origin goes in plan, mm (rasters: their lower left corner).
    /// Default 0, 0.
    #[serde(default)]
    position: P2,
    /// 0 to 1. Default 0.5.
    #[serde(default = "half")]
    opacity: f64,
}

fn one() -> u32 {
    1
}
fn half() -> f64 {
    0.5
}

/// A fresh layer below all others, named `base` or `base N`.
fn new_layer_below(doc: &mut Document, base: &str) -> Id {
    let mut name = base.to_owned();
    let mut n = 2;
    while doc.layers.values().any(|l| l.name == name) {
        name = format!("{base} {n}");
        n += 1;
    }
    let order = doc.layers.values().map(|l| l.order).min().unwrap_or(0) - 1;
    let l = Layer { id: Uuid::new_v4(), name, elevation: 0.0, order, visible: true };
    let id = l.id;
    doc.layers.insert(id, l);
    id
}

fn import(ctx: &mut Ctx, a: ImportArgs) -> CmdResult {
    let (name, bytes) = super::mvr::read_input(&a.path, &a.data_base64, &a.file_name, "underlay")?;
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_owned(), e.to_ascii_lowercase()),
        None => (name.clone(), String::new()),
    };
    let format = stagecraft_underlay::Format::from_ext(&ext)
        .ok_or_else(|| CmdError::invalid_args(format!("`{name}`: use a .png, .jpg, .pdf, .svg, .dxf or .dwg file")))?;
    if !(0.0..=1.0).contains(&a.opacity) {
        return Err(CmdError::invalid_args("opacity is between 0 and 1"));
    }
    if let Some(s) = a.scale
        && !(s.is_finite() && s > 0.0)
    {
        return Err(CmdError::invalid_args("scale must be positive"));
    }
    let conv = stagecraft_underlay::convert(format, &bytes, a.page)
        .map_err(|e| CmdError::invalid_args(format!("`{name}`: {e}")))?;
    let scale = match format {
        stagecraft_underlay::Format::Dxf | stagecraft_underlay::Format::Dwg => a.scale.unwrap_or(conv.unit_mm),
        _ => conv.unit_mm * a.scale.unwrap_or(100.0),
    };
    let source = Asset::new(&name, &ext, bytes);
    let mut raster = None;
    let mut linework = None;
    let doc = ctx.doc_mut();
    match (&conv.raster, &conv.linework) {
        (Some(r), _) => {
            let png = Asset::new(&format!("{stem}.png"), "png", r.png.clone());
            raster = Some(png.hash.clone());
            doc.assets.entry(png.hash.clone()).or_insert(png);
        }
        (None, Some(l)) => {
            let j = Asset::new(&format!("{stem}.lines.json"), "json", l.to_json());
            linework = Some(j.hash.clone());
            doc.assets.entry(j.hash.clone()).or_insert(j);
        }
        (None, None) => raster = Some(source.hash.clone()),
    }
    let source_hash = source.hash.clone();
    doc.assets.entry(source.hash.clone()).or_insert(source);
    let layer = match &a.layer {
        Some(l) => resolve_layer(ctx.doc(), l)?,
        None => new_layer_below(ctx.doc_mut(), "Underlay"),
    };
    let kind = ObjectKind::Underlay {
        format: format_of(format),
        source: source_hash,
        raster,
        linework,
        page: a.page.max(1),
        extent: conv.extent,
        scale,
        opacity: a.opacity,
        locked: false,
    };
    let common = Common { name: Some(a.name.unwrap_or(stem)), layer: Some(layer.to_string()), ..Default::default() };
    let p = Placement { position: [a.position[0], a.position[1], 0.0], ..Default::default() };
    let mut out = insert(ctx, common, "Underlay", p, kind)?;
    let [lo, hi] = conv.extent;
    out["layer"] = json!(layer);
    out["scale"] = json!(scale);
    out["size"] = json!([(hi[0] - lo[0]) * scale, (hi[1] - lo[1]) * scale]);
    out["pages"] = json!(conv.pages);
    if !conv.notes.is_empty() {
        out["notes"] = json!(conv.notes);
    }
    Ok(out)
}

fn underlay_scale(doc: &Document, id: Id) -> Result<f64, CmdError> {
    match &object(doc, id)?.kind {
        ObjectKind::Underlay { scale, .. } => Ok(*scale),
        _ => Err(CmdError::invalid(format!("{id} is not an underlay"))),
    }
}

/// Apply a similarity transform (about the world origin, in plan) to an underlay:
/// its placement and its scale.
fn transform_underlay(doc: &mut Document, id: Id, m: DAffine3, factor: f64) -> Result<(), CmdError> {
    let o = object(doc, id)?;
    if o.parent.is_some() {
        return Err(CmdError::invalid("the underlay is attached to another object; detach it first"));
    }
    let w = world(doc, id).expect("exists");
    let elevation = doc.layers.get(&o.layer).map_or(0.0, |l| l.elevation);
    // the world transform without its scale: m includes the scale factor
    let (_, rot, t) = (m * w).to_scale_rotation_translation();
    let placement =
        crate::geom::affine_to_placement(&DAffine3::from_rotation_translation(rot, t - DVec3::Z * elevation));
    let o = doc.objects.get_mut(&id).expect("exists");
    o.placement = placement;
    if let ObjectKind::Underlay { scale, .. } = &mut o.kind {
        *scale *= factor;
    }
    Ok(())
}

#[derive(Deserialize, JsonSchema)]
struct CalibrateArgs {
    /// The underlay.
    id: Id,
    /// First point, picked on the underlay, world plan mm. It stays where it is.
    p1: P2,
    /// Second point, world plan mm.
    p2: P2,
    /// What the distance between them really is, mm.
    distance_mm: f64,
}

fn calibrate(ctx: &mut Ctx, a: CalibrateArgs) -> CmdResult {
    let doc = ctx.doc();
    underlay_scale(doc, a.id)?;
    super::annotate::ensure_unlocked(doc, &[a.id])?;
    let d = (a.p2[0] - a.p1[0]).hypot(a.p2[1] - a.p1[1]);
    if d < 1e-6 {
        return Err(CmdError::invalid_args("p1 and p2 are the same point"));
    }
    if !(a.distance_mm.is_finite() && a.distance_mm > 0.0) {
        return Err(CmdError::invalid_args("distance_mm must be positive"));
    }
    let f = a.distance_mm / d;
    let c = DVec3::new(a.p1[0], a.p1[1], 0.0);
    let m =
        DAffine3::from_translation(c) * DAffine3::from_scale(DVec3::new(f, f, 1.0)) * DAffine3::from_translation(-c);
    transform_underlay(ctx.doc_mut(), a.id, m, f)?;
    Ok(json!({ "factor": f, "scale": underlay_scale(ctx.doc(), a.id)? }))
}

#[derive(Deserialize, JsonSchema)]
struct AlignArgs {
    /// The underlay.
    id: Id,
    /// Two points on the underlay as it lies now, world plan mm.
    from: [P2; 2],
    /// Where they should go, world plan mm.
    to: [P2; 2],
    /// Scale too, so the distance matches (default true). Otherwise only move and turn
    /// (the first point lands exactly, the second on the line).
    #[serde(default = "yes")]
    scale: bool,
}

fn yes() -> bool {
    true
}

fn align(ctx: &mut Ctx, a: AlignArgs) -> CmdResult {
    let doc = ctx.doc();
    underlay_scale(doc, a.id)?;
    ensure_unlocked(doc, &[a.id])?;
    let v = |p: [P2; 2]| DVec3::new(p[1][0] - p[0][0], p[1][1] - p[0][1], 0.0);
    let (vf, vt) = (v(a.from), v(a.to));
    if vf.length() < 1e-6 || vt.length() < 1e-6 {
        return Err(CmdError::invalid_args("the two points of `from` and of `to` must differ"));
    }
    let angle = vt.y.atan2(vt.x) - vf.y.atan2(vf.x);
    let f = if a.scale { vt.length() / vf.length() } else { 1.0 };
    let m = DAffine3::from_translation(DVec3::new(a.to[0][0], a.to[0][1], 0.0))
        * DAffine3::from_quat(DQuat::from_rotation_z(angle))
        * DAffine3::from_scale(DVec3::new(f, f, 1.0))
        * DAffine3::from_translation(DVec3::new(-a.from[0][0], -a.from[0][1], 0.0));
    transform_underlay(ctx.doc_mut(), a.id, m, f)?;
    Ok(json!({ "factor": f, "angle": angle.to_degrees(), "scale": underlay_scale(ctx.doc(), a.id)? }))
}

#[derive(Deserialize, JsonSchema)]
struct UpdateArgs {
    id: Id,
    /// 0 to 1.
    opacity: Option<f64>,
    /// Locked underlays cannot be picked, moved, calibrated or deleted.
    locked: Option<bool>,
    /// Show or hide it in the plan.
    visible: Option<bool>,
}

fn update(ctx: &mut Ctx, a: UpdateArgs) -> CmdResult {
    underlay_scale(ctx.doc(), a.id)?;
    if let Some(op) = a.opacity
        && !(0.0..=1.0).contains(&op)
    {
        return Err(CmdError::invalid_args("opacity is between 0 and 1"));
    }
    let o = ctx.doc_mut().objects.get_mut(&a.id).expect("checked");
    if let ObjectKind::Underlay { opacity, locked, .. } = &mut o.kind {
        if let Some(v) = a.opacity {
            *opacity = v;
        }
        if let Some(v) = a.locked {
            *locked = v;
        }
    }
    if let Some(v) = a.visible {
        o.style.hidden = !v;
    }
    Ok(json!({}))
}

#[derive(Deserialize, JsonSchema)]
struct NoteArgs {
    #[serde(flatten)]
    common: Common,
    /// The text; `\n` starts a new line.
    text: String,
    /// Upper left corner of the text, world plan mm.
    at: P2,
    /// Cap height in model mm. Default 250 (2.5 mm on paper at 1:100).
    #[serde(default = "d250")]
    height: f64,
    /// Wrap longer lines at this width, mm. Default 0 = only at `\n`.
    #[serde(default)]
    width: f64,
    /// Draw a box around the text. Default false.
    #[serde(default)]
    frame: bool,
    /// A leader from the note to this world plan point, mm.
    leader: Option<P2>,
    /// Degrees counter-clockwise. Default 0.
    #[serde(default)]
    rotation: f64,
}

fn d250() -> f64 {
    250.0
}

fn check_note(text: &str, height: f64, width: f64) -> Result<(), CmdError> {
    if text.trim().is_empty() {
        return Err(CmdError::invalid_args("a note needs text"));
    }
    if !(height.is_finite() && height > 0.0) {
        return Err(CmdError::invalid_args("height must be positive"));
    }
    if !(width.is_finite() && width >= 0.0) {
        return Err(CmdError::invalid_args("width must be 0 or positive"));
    }
    Ok(())
}

/// A world plan point in the note's local frame.
fn to_local(w: &DAffine3, p: P2) -> P2 {
    let l = w.inverse().transform_point3(DVec3::new(p[0], p[1], w.translation.z));
    [l.x, l.y]
}

fn note_create(ctx: &mut Ctx, a: NoteArgs) -> CmdResult {
    check_note(&a.text, a.height, a.width)?;
    let mut common = a.common;
    if common.parent.is_some() {
        return Err(CmdError::invalid_args("notes are not attached to objects"));
    }
    if common.name.is_none() {
        common.name = Some(a.text.lines().next().unwrap_or("Note").chars().take(40).collect());
    }
    let p = Placement { position: [a.at[0], a.at[1], 0.0], rotation: [0.0, 0.0, a.rotation] };
    let w = crate::geom::placement_to_affine(&p);
    let leader = a.leader.map(|l| to_local(&w, l));
    let kind = ObjectKind::Note { text: a.text, height: a.height, width: a.width, frame: a.frame, leader };
    insert(ctx, common, "Note", p, kind)
}

#[derive(Deserialize, JsonSchema)]
struct NoteUpdateArgs {
    id: Id,
    text: Option<String>,
    /// Cap height, mm.
    height: Option<f64>,
    /// Wrap width, mm; 0 = only at `\n`.
    width: Option<f64>,
    frame: Option<bool>,
    /// New leader end, world plan mm.
    leader: Option<P2>,
    /// Remove the leader.
    #[serde(default)]
    no_leader: bool,
}

fn note_update(ctx: &mut Ctx, a: NoteUpdateArgs) -> CmdResult {
    let doc = ctx.doc();
    let o = object(doc, a.id)?;
    let ObjectKind::Note { text, height, width, .. } = &o.kind else {
        return Err(CmdError::invalid(format!("{} is not a note", a.id)));
    };
    check_note(a.text.as_deref().unwrap_or(text), a.height.unwrap_or(*height), a.width.unwrap_or(*width))?;
    let w = world(doc, a.id).expect("exists");
    let local = a.leader.map(|l| to_local(&w, l));
    let o = ctx.doc_mut().objects.get_mut(&a.id).expect("checked");
    if let ObjectKind::Note { text, height, width, frame, leader } = &mut o.kind {
        if let Some(v) = a.text {
            *text = v;
        }
        if let Some(v) = a.height {
            *height = v;
        }
        if let Some(v) = a.width {
            *width = v;
        }
        if let Some(v) = a.frame {
            *frame = v;
        }
        if local.is_some() {
            *leader = local;
        }
        if a.no_leader {
            *leader = None;
        }
    }
    Ok(json!({}))
}
