//! Creating objects, and fixture patch edits.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::tag::ensure_tags;
use super::{ensure_exists, layer_or_default, object};
use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::model::{Document, Id, Object, ObjectKind, P2, P3, Placement, format_address, parse_address};

pub(super) fn register(r: &mut Registry) {
    r.add("floor.create", Kind::Edit, "Create a floor area from a closed outline", floor);
    r.add("wall.create", Kind::Edit, "Create a wall along a polyline", wall);
    r.add("truss.create", Kind::Edit, "Create a straight truss piece from start to end", truss);
    r.add("fixture.create", Kind::Edit, "Create a fixture, optionally hung on a parent", fixture);
    r.add("fixture.update", Kind::Edit, "Change FID, CID, mode or address of one fixture", fixture_update);
    r.add("fixture.number", Kind::Edit, "Give fixtures consecutive FIDs in the given order", fixture_number);
}

/// Fields every create command takes.
#[derive(Deserialize, JsonSchema, Default)]
struct Common {
    name: Option<String>,
    /// Layer id or name. Default: the first layer.
    layer: Option<String>,
    /// Tag ids or names; unknown names are created.
    #[serde(default)]
    tags: Vec<String>,
    /// Attach to this object. Positions are then relative to it.
    parent: Option<Id>,
}

fn insert(ctx: &mut Ctx, c: Common, default_name: &str, placement: Placement, kind: ObjectKind) -> CmdResult {
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

fn truss(ctx: &mut Ctx, a: TrussArgs) -> CmdResult {
    let [dx, dy, dz] = [a.end[0] - a.start[0], a.end[1] - a.start[1], a.end[2] - a.start[2]];
    let horiz = dx.hypot(dy);
    let length = horiz.hypot(dz);
    if length < 1.0 {
        return Err(CmdError::invalid_args("truss start and end are the same point"));
    }
    let p = Placement {
        position: a.start,
        // Pan towards the end in plan, then pitch up; Ry positive tips +x down.
        rotation: [0.0, -dz.atan2(horiz).to_degrees(), dy.atan2(dx).to_degrees()],
    };
    insert(ctx, a.common, "Truss", p, ObjectKind::Truss { length, profile: a.profile })
}

#[derive(Deserialize, JsonSchema)]
struct FixtureArgs {
    #[serde(flatten)]
    common: Common,
    /// mm, relative to the parent or layer.
    #[serde(default)]
    position: P3,
    /// Degrees about x, y, z (Rz · Ry · Rx).
    #[serde(default)]
    rotation: P3,
    /// Fixture ID, unique in the document.
    fid: Option<u32>,
    /// Custom ID (MVR CustomId).
    cid: Option<String>,
    /// DMX address: `1.001`, `2.17` or an absolute number.
    address: Option<AddressArg>,
    mode: Option<String>,
    /// Name of an embedded GDTF file.
    gdtf: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
enum AddressArg {
    Absolute(u32),
    Text(String),
}

impl AddressArg {
    fn resolve(&self) -> Result<u32, CmdError> {
        let n = match self {
            AddressArg::Absolute(n) => (*n >= 1).then_some(*n),
            AddressArg::Text(s) => parse_address(s),
        };
        n.ok_or_else(|| CmdError::invalid_args("address must be like 1.001 or an absolute number ≥ 1"))
    }
}

fn check_fid(doc: &Document, fid: u32, except: Option<Id>) -> Result<(), CmdError> {
    let taken = doc
        .objects
        .values()
        .find(|o| Some(o.id) != except && matches!(o.kind, ObjectKind::Fixture { fid: Some(f), .. } if f == fid));
    match taken {
        Some(o) => Err(CmdError::invalid(format!("FID {fid} is already used by {} ({})", o.name, o.id))),
        None => Ok(()),
    }
}

fn fixture(ctx: &mut Ctx, a: FixtureArgs) -> CmdResult {
    if let Some(f) = a.fid {
        check_fid(ctx.doc(), f, None)?;
    }
    let address = a.address.as_ref().map(AddressArg::resolve).transpose()?;
    let p = Placement { position: a.position, rotation: a.rotation };
    let name = match a.fid {
        Some(f) => format!("Fixture {f}"),
        None => "Fixture".into(),
    };
    insert(ctx, a.common, &name, p, ObjectKind::Fixture { fid: a.fid, cid: a.cid, gdtf: a.gdtf, mode: a.mode, address })
}

#[derive(Deserialize, JsonSchema)]
struct FixtureUpdateArgs {
    id: Id,
    /// New FID; 0 clears it.
    fid: Option<u32>,
    /// New CID; empty string clears it.
    cid: Option<String>,
    /// `1.001`, absolute number, or empty string / 0 to unpatch.
    address: Option<AddressArg>,
    mode: Option<String>,
}

fn fixture_update(ctx: &mut Ctx, a: FixtureUpdateArgs) -> CmdResult {
    let o = object(ctx.doc(), a.id)?;
    if !matches!(o.kind, ObjectKind::Fixture { .. }) {
        return Err(CmdError::invalid(format!("{} is not a fixture", a.id)));
    }
    if let Some(f) = a.fid.filter(|&f| f != 0) {
        check_fid(ctx.doc(), f, Some(a.id))?;
    }
    let address = match &a.address {
        None => None,
        Some(AddressArg::Absolute(0)) => Some(None),
        Some(AddressArg::Text(s)) if s.is_empty() => Some(None),
        Some(x) => Some(Some(x.resolve()?)),
    };
    let o = ctx.doc_mut().objects.get_mut(&a.id).expect("checked");
    let ObjectKind::Fixture { fid, cid, mode, address: addr, .. } = &mut o.kind else { unreachable!() };
    if let Some(f) = a.fid {
        *fid = (f != 0).then_some(f);
    }
    if let Some(c) = a.cid {
        *cid = (!c.is_empty()).then_some(c);
    }
    if let Some(m) = a.mode {
        *mode = Some(m);
    }
    if let Some(x) = address {
        *addr = x;
    }
    let shown = addr.map(format_address);
    Ok(json!({ "fid": fid, "cid": cid, "mode": mode, "address": shown }))
}

#[derive(Deserialize, JsonSchema)]
struct NumberArgs {
    /// Fixtures in the order they get numbers.
    ids: Vec<Id>,
    start: u32,
    /// Default 1.
    #[serde(default = "one")]
    step: u32,
}

fn one() -> u32 {
    1
}

fn fixture_number(ctx: &mut Ctx, a: NumberArgs) -> CmdResult {
    let doc = ctx.doc();
    ensure_exists(doc, &a.ids)?;
    if a.start == 0 || a.step == 0 {
        return Err(CmdError::invalid_args("start and step must be at least 1"));
    }
    let fids: Vec<u32> = (0..a.ids.len() as u32).map(|i| a.start + i * a.step).collect();
    for (id, f) in a.ids.iter().zip(&fids) {
        if !matches!(doc.objects[id].kind, ObjectKind::Fixture { .. }) {
            return Err(CmdError::invalid(format!("{id} is not a fixture")));
        }
        // Clashes with fixtures outside the renumbered set are errors.
        if let Some(o) = doc
            .objects
            .values()
            .find(|o| !a.ids.contains(&o.id) && matches!(o.kind, ObjectKind::Fixture { fid: Some(x), .. } if x == *f))
        {
            return Err(CmdError::invalid(format!("FID {f} is already used by {} ({})", o.name, o.id)));
        }
    }
    let doc = ctx.doc_mut();
    for (id, f) in a.ids.iter().zip(&fids) {
        if let ObjectKind::Fixture { fid, .. } = &mut doc.objects.get_mut(id).expect("checked").kind {
            *fid = Some(*f);
        }
    }
    Ok(json!({ "fids": fids }))
}

fn positive(what: &str, v: f64) -> Result<(), CmdError> {
    if v > 0.0 && v.is_finite() {
        Ok(())
    } else {
        Err(CmdError::invalid_args(format!("{what} must be a positive number of mm")))
    }
}
