//! Creating objects, and fixture patch edits.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::tag::ensure_tags;
use super::{layer_or_default, object};
use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::model::{Id, Object, ObjectKind, P2, P3, Placement};

pub(super) fn register(r: &mut Registry) {
    r.add("floor.create", Kind::Edit, "Create a floor area from a closed outline", floor);
    r.add("wall.create", Kind::Edit, "Create a wall along a polyline", wall);
    r.add("truss.create", Kind::Edit, "Create a straight truss piece from start to end", truss);
    r.add("floor.update", Kind::Edit, "Change outline or thickness of one floor", floor_update);
    r.add("wall.update", Kind::Edit, "Change path, height or thickness of one wall", wall_update);
    r.add("truss.update", Kind::Edit, "Change length or profile of one truss piece", truss_update);
}

/// Fields every create command takes.
#[derive(Deserialize, JsonSchema, Default)]
pub(crate) struct Common {
    pub name: Option<String>,
    /// Layer id or name. Default: the first layer.
    pub layer: Option<String>,
    /// Tag ids or names; unknown names are created.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Attach to this object. Positions are then relative to it.
    pub parent: Option<Id>,
}

pub(crate) fn insert(
    ctx: &mut Ctx,
    c: Common,
    default_name: &str,
    placement: Placement,
    kind: ObjectKind,
) -> CmdResult {
    let doc = ctx.doc();
    let layer = match (c.parent, &c.layer) {
        (Some(p), None) => object(doc, p)?.layer,
        (Some(p), Some(_)) => {
            object(doc, p)?;
            layer_or_default(doc, c.layer.as_deref())?
        }
        (None, l) => layer_or_default(doc, l.as_deref())?,
    };
    let tags = ensure_tags(ctx.doc_mut(), &c.tags)?;
    let o = Object {
        id: Uuid::new_v4(),
        name: c.name.unwrap_or_else(|| default_name.to_owned()),
        layer,
        tags,
        parent: c.parent,
        placement,
        kind,
        style: Default::default(),
        attrs: Default::default(),
    };
    let id = o.id;
    ctx.doc_mut().objects.insert(id, o);
    Ok(json!({ "id": id }))
}

#[derive(Deserialize, JsonSchema)]
struct FloorArgs {
    #[serde(flatten)]
    common: Common,
    /// Closed outline in plan, mm, at least 3 points (do not repeat the first).
    outline: Vec<P2>,
    /// mm. Default 20.
    #[serde(default = "d20")]
    thickness: f64,
    /// Height of the top surface above the layer, mm. Default 0.
    #[serde(default)]
    z: f64,
}

fn d20() -> f64 {
    20.0
}

fn floor(ctx: &mut Ctx, a: FloorArgs) -> CmdResult {
    if a.outline.len() < 3 {
        return Err(CmdError::invalid_args("a floor outline needs at least 3 points"));
    }
    positive("thickness", a.thickness)?;
    let p = Placement { position: [0.0, 0.0, a.z], ..Default::default() };
    insert(ctx, a.common, "Floor", p, ObjectKind::Floor { outline: a.outline, thickness: a.thickness })
}

#[derive(Deserialize, JsonSchema)]
struct WallArgs {
    #[serde(flatten)]
    common: Common,
    /// Centre line in plan, mm, at least 2 points.
    path: Vec<P2>,
    /// mm. Default 3000.
    #[serde(default = "d3000")]
    height: f64,
    /// mm. Default 150.
    #[serde(default = "d150")]
    thickness: f64,
    /// Base height above the layer, mm. Default 0.
    #[serde(default)]
    z: f64,
}

fn d3000() -> f64 {
    3000.0
}
fn d150() -> f64 {
    150.0
}

fn wall(ctx: &mut Ctx, a: WallArgs) -> CmdResult {
    if a.path.len() < 2 {
        return Err(CmdError::invalid_args("a wall path needs at least 2 points"));
    }
    positive("height", a.height)?;
    positive("thickness", a.thickness)?;
    let p = Placement { position: [0.0, 0.0, a.z], ..Default::default() };
    insert(ctx, a.common, "Wall", p, ObjectKind::Wall { path: a.path, height: a.height, thickness: a.thickness })
}

#[derive(Deserialize, JsonSchema)]
struct TrussArgs {
    #[serde(flatten)]
    common: Common,
    /// mm, relative to the parent or layer.
    start: P3,
    /// mm, relative to the parent or layer.
    end: P3,
    /// Truss profile name, e.g. F34, F44P, H30V. Default F34.
    #[serde(default = "f34")]
    profile: String,
}

fn f34() -> String {
    "F34".into()
}

/// The placement and length of a straight piece from `start` to `end`.
pub(super) fn line_placement(start: P3, end: P3) -> (Placement, f64) {
    let [dx, dy, dz] = [end[0] - start[0], end[1] - start[1], end[2] - start[2]];
    let horiz = dx.hypot(dy);
    let length = horiz.hypot(dz);
    let p = Placement {
        position: start,
        // Pan towards the end in plan, then pitch up; Ry positive tips +x down.
        rotation: [0.0, -dz.atan2(horiz).to_degrees(), dy.atan2(dx).to_degrees()],
    };
    (p, length)
}

fn truss(ctx: &mut Ctx, a: TrussArgs) -> CmdResult {
    let (p, length) = line_placement(a.start, a.end);
    if length < 1.0 {
        return Err(CmdError::invalid_args("truss start and end are the same point"));
    }
    insert(ctx, a.common, "Truss", p, ObjectKind::Truss { length, profile: a.profile })
}

pub(super) fn positive(what: &str, v: f64) -> Result<(), CmdError> {
    if v > 0.0 && v.is_finite() {
        Ok(())
    } else {
        Err(CmdError::invalid_args(format!("{what} must be a positive number of mm")))
    }
}

#[derive(Deserialize, JsonSchema)]
struct FloorUpdateArgs {
    id: Id,
    /// New outline in plan, mm, relative to the floor's placement; at least 3 points.
    outline: Option<Vec<P2>>,
    /// mm.
    thickness: Option<f64>,
}

fn floor_update(ctx: &mut Ctx, a: FloorUpdateArgs) -> CmdResult {
    if a.outline.as_ref().is_some_and(|o| o.len() < 3) {
        return Err(CmdError::invalid_args("a floor outline needs at least 3 points"));
    }
    if let Some(t) = a.thickness {
        positive("thickness", t)?;
    }
    let o = kind_mut(ctx, a.id, "floor")?;
    let ObjectKind::Floor { outline, thickness } = &mut o.kind else { unreachable!() };
    if let Some(v) = a.outline {
        *outline = v;
    }
    if let Some(v) = a.thickness {
        *thickness = v;
    }
    Ok(json!(o.kind))
}

#[derive(Deserialize, JsonSchema)]
struct WallUpdateArgs {
    id: Id,
    /// New centre line in plan, mm, relative to the wall's placement; at least 2 points.
    path: Option<Vec<P2>>,
    /// mm.
    height: Option<f64>,
    /// mm.
    thickness: Option<f64>,
}

fn wall_update(ctx: &mut Ctx, a: WallUpdateArgs) -> CmdResult {
    if a.path.as_ref().is_some_and(|p| p.len() < 2) {
        return Err(CmdError::invalid_args("a wall path needs at least 2 points"));
    }
    for (what, v) in [("height", a.height), ("thickness", a.thickness)] {
        if let Some(v) = v {
            positive(what, v)?;
        }
    }
    let o = kind_mut(ctx, a.id, "wall")?;
    let ObjectKind::Wall { path, height, thickness } = &mut o.kind else { unreachable!() };
    if let Some(v) = a.path {
        *path = v;
    }
    if let Some(v) = a.height {
        *height = v;
    }
    if let Some(v) = a.thickness {
        *thickness = v;
    }
    Ok(json!(o.kind))
}

#[derive(Deserialize, JsonSchema)]
struct TrussUpdateArgs {
    id: Id,
    /// mm, along the piece from its start.
    length: Option<f64>,
    /// Profile name from the truss catalogue, e.g. F34.
    profile: Option<String>,
}

fn truss_update(ctx: &mut Ctx, a: TrussUpdateArgs) -> CmdResult {
    if let Some(l) = a.length {
        positive("length", l)?;
    }
    if a.profile.as_ref().is_some_and(|p| p.trim().is_empty()) {
        return Err(CmdError::invalid_args("profile is empty"));
    }
    let o = kind_mut(ctx, a.id, "truss")?;
    let ObjectKind::Truss { length, profile } = &mut o.kind else { unreachable!() };
    if let Some(v) = a.length {
        *length = v;
    }
    if let Some(v) = a.profile {
        *profile = v;
    }
    Ok(json!(o.kind))
}

/// The object, mutable, after checking it is of type `ty`.
pub(super) fn kind_mut<'c>(ctx: &'c mut Ctx, id: Id, ty: &str) -> Result<&'c mut Object, CmdError> {
    let o = object(ctx.doc(), id)?;
    if o.kind.type_name() != ty {
        return Err(CmdError::invalid(format!("{id} is not a {ty}")));
    }
    Ok(ctx.doc_mut().objects.get_mut(&id).expect("checked"))
}
