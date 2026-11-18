//! `sketch.*` and `dimension.*`: the parametric floor-plan sketch of each layer
//! (docs/spec/sketch.md). Every edit solves the sketch; an edit the solver cannot
//! satisfy fails with code `conflict` and a message naming the constraints, and the
//! engine rolls the document back. Lengths are mm, angles degrees, positions plan mm
//! in the layer's coordinates.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::create::{Common, insert};
use super::{layer_or_default, object};
use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::model::{Document, Id, ObjectKind, P2, Placement};
use crate::sketch::{
    self, Anchor, ConstraintId, ConstraintKind, DimAxis, DragTarget, EdgeId, Edit, InferOptions, ItemRef, LineGesture,
    Measure, PointId, RefDimension, SketchLink, SolveOptions, Status, describe, edit_options, infer_line, p2, v2,
};

pub(super) fn register(r: &mut Registry) {
    r.add("sketch.get", Kind::Query, "A layer's sketch: points, edges, constraints, dimensions, and its status", get);
    r.add(
        "sketch.status",
        Kind::Query,
        "Degrees of freedom and conflicting/redundant constraints of a layer's sketch",
        status,
    );
    r.add("sketch.add_point", Kind::Edit, "Add a point to a layer's sketch, optionally fixed", add_point);
    r.add("sketch.add_line", Kind::Edit, "Add a line between two sketch points or positions", add_line);
    r.add("sketch.add_arc", Kind::Edit, "Add a counter-clockwise arc (centre, start, end)", add_arc);
    r.add("sketch.add_polyline", Kind::Edit, "Add a chain of lines through points, optionally closed", add_polyline);
    r.add(
        "sketch.add_constraint",
        Kind::Edit,
        "Add a geometric constraint (horizontal, coincident, parallel …)",
        add_constraint,
    );
    r.add(
        "sketch.remove_constraint",
        Kind::Edit,
        "Remove constraints or dimensions of a layer's sketch",
        remove_constraint,
    );
    r.add("sketch.remove", Kind::Edit, "Remove sketch points, edges or constraints (and what depends on them)", remove);
    r.add(
        "sketch.drag",
        Kind::Edit,
        "Drag sketch points towards targets; they follow as far as the constraints allow",
        drag,
    );
    r.add(
        "sketch.draw_line",
        Kind::Edit,
        "Draw a line as a user would: snapping, horizontal/vertical and typed length become constraints",
        draw_line,
    );
    r.add(
        "sketch.draw",
        Kind::Edit,
        "Draw a polyline into the sketch with inferred constraints; optionally as a wall or floor",
        draw,
    );
    r.add("sketch.make_wall", Kind::Edit, "Make a wall that follows a chain of sketch edges", make_wall);
    r.add("sketch.make_floor", Kind::Edit, "Make a floor that follows a closed loop of sketch edges", make_floor);
    r.add("sketch.adopt", Kind::Edit, "Turn free walls and floors into sketch geometry they then follow", adopt);
    r.add(
        "sketch.unlink",
        Kind::Edit,
        "Detach walls and floors from the sketch; they keep their current geometry",
        unlink,
    );
    r.add("sketch.solve", Kind::Edit, "Solve a layer's sketch again and update what follows it", solve);
    r.add("sketch.edit", Kind::Edit, "Apply a raw sketch edit (any operation of stagecraft-sketch's Edit)", edit);
    r.add(
        "dimension.add",
        Kind::Edit,
        "Add a dimension to a layer's sketch: driving (constrains) or reference (measures)",
        dim_add,
    );
    r.add(
        "dimension.add_reference",
        Kind::Edit,
        "Add a reference dimension between points of any objects or sketches; it measures live",
        dim_add_reference,
    );
    r.add("dimension.set_value", Kind::Edit, "Change a driving dimension; everything attached follows", dim_set_value);
    r.add(
        "dimension.set_driving",
        Kind::Edit,
        "Turn a sketch dimension into a driving or a reference dimension",
        dim_set_driving,
    );
    r.add("dimension.move_label", Kind::Edit, "Move a dimension's label (and its dimension line)", dim_move_label);
    r.add(
        "dimension.name",
        Kind::Edit,
        "Name a dimension as a parameter (\"portal width\"), or clear the name",
        dim_name,
    );
    r.add("dimension.remove", Kind::Edit, "Remove dimensions", dim_remove);
    r.add("dimension.list", Kind::Query, "Every dimension with its current value", dim_list);
}

fn layer(doc: &Document, l: Option<&str>) -> Result<Id, CmdError> {
    layer_or_default(doc, l)
}

fn current(doc: &Document, layer: Id) -> sketch::Sketch {
    doc.sketches.get(&layer).cloned().unwrap_or_default()
}

/// A sketch point: an existing point id, or a plan position `[x, y]` in mm for a new
/// point.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
enum PointArg {
    Id(u32),
    At(P2),
}

fn point_of(s: &mut sketch::Sketch, p: PointArg) -> Result<PointId, CmdError> {
    match p {
        PointArg::Id(id) => {
            let id = PointId(id);
            s.point(id).ok_or_else(|| CmdError::not_found("sketch point", id))?;
            Ok(id)
        }
        PointArg::At(at) => Ok(s.add_point(v2(at))),
    }
}

/// Stores a sketch built directly (no constraint added) after solving it.
fn commit(ctx: &mut Ctx, layer: Id, s: sketch::Sketch) -> Result<(), CmdError> {
    let solved = s.solve(&edit_options()).map_err(sketch::solve_error)?;
    sketch::store(ctx.doc_mut(), layer, solved.sketch);
    Ok(())
}

fn status_json(doc: &Document, layer: Id) -> Result<Value, CmdError> {
    let s = current(doc, layer);
    let st: Status = s.analyse().map_err(sketch::solve_error)?;
    let conflicting: Vec<Value> =
        st.conflicting.iter().map(|&id| json!({ "id": id, "description": describe(&s, id) })).collect();
    Ok(json!({
        "layer": layer,
        "dof": st.dof,
        "fully_constrained": st.dof == 0 && st.conflicting.is_empty(),
        "points": s.points.len(),
        "edges": s.edges.len(),
        "constraints": s.constraints.len(),
        "dimensions": s.dimensions.len(),
        "conflicting": conflicting,
        "redundant": st.redundant,
        "free_points": st.points.iter().filter(|p| p.dof > 0).map(|p| p.id).collect::<Vec<_>>(),
    }))
}

#[derive(Deserialize, JsonSchema)]
struct LayerArg {
    /// Layer id or name. Default: the first layer.
    layer: Option<String>,
}

fn get(ctx: &mut Ctx, a: LayerArg) -> CmdResult {
    let doc = ctx.doc();
    let l = layer(doc, a.layer.as_deref())?;
    let objects: Vec<Value> = sketch::linked_on(doc, l)
        .into_iter()
        .map(|id| {
            let o = &doc.objects[&id];
            json!({ "id": id, "name": o.name, "type": o.kind.type_name(), "edges": o.sketch.as_ref().map(|s| &s.edges) })
        })
        .collect();
    Ok(json!({ "layer": l, "sketch": current(doc, l), "status": status_json(doc, l)?, "objects": objects }))
}

fn status(ctx: &mut Ctx, a: LayerArg) -> CmdResult {
    let doc = ctx.doc();
    let l = layer(doc, a.layer.as_deref())?;
    status_json(doc, l)
}

#[derive(Deserialize, JsonSchema)]
struct AddPointArgs {
    layer: Option<String>,
    /// Plan position, mm.
    at: P2,
    /// Pin the point where it is (a `fixed` constraint). Default false.
    #[serde(default)]
    fixed: bool,
}

fn add_point(ctx: &mut Ctx, a: AddPointArgs) -> CmdResult {
    let l = layer(ctx.doc(), a.layer.as_deref())?;
    let mut s = current(ctx.doc(), l);
    let id = s.add_point(v2(a.at));
    if a.fixed {
        s.fix(id);
    }
    commit(ctx, l, s)?;
    Ok(json!({ "id": id }))
}

#[derive(Deserialize, JsonSchema)]
struct AddLineArgs {
    layer: Option<String>,
    /// Start: a point id or a position `[x, y]` in mm (a new point).
    from: PointArg,
    /// End: a point id or a position `[x, y]` in mm.
    to: PointArg,
    /// Construction line (centre line, helper): constrains, but is no wall or floor
    /// edge. Default false.
    #[serde(default)]
    construction: bool,
}

fn add_line(ctx: &mut Ctx, a: AddLineArgs) -> CmdResult {
    let l = layer(ctx.doc(), a.layer.as_deref())?;
    let mut s = current(ctx.doc(), l);
    let pa = point_of(&mut s, a.from)?;
    let pb = point_of(&mut s, a.to)?;
    if pa == pb {
        return Err(CmdError::invalid_args("a line needs two different points"));
    }
    let id = s.add_line(pa, pb);
    s.edge_mut(id).expect("added").construction = a.construction;
    commit(ctx, l, s)?;
    Ok(json!({ "id": id, "points": [pa, pb] }))
}

#[derive(Deserialize, JsonSchema)]
struct AddArcArgs {
    layer: Option<String>,
    /// Centre: point id or `[x, y]` mm.
    center: PointArg,
    /// Start of the arc (counter-clockwise from here): point id or `[x, y]` mm.
    start: PointArg,
    /// End: point id or `[x, y]` mm. The solver keeps it at the start's radius.
    end: PointArg,
    #[serde(default)]
    construction: bool,
}

fn add_arc(ctx: &mut Ctx, a: AddArcArgs) -> CmdResult {
    let l = layer(ctx.doc(), a.layer.as_deref())?;
    let mut s = current(ctx.doc(), l);
    let c = point_of(&mut s, a.center)?;
    let p0 = point_of(&mut s, a.start)?;
    let p1 = point_of(&mut s, a.end)?;
    let id = s.add_arc(c, p0, p1);
    s.edge_mut(id).expect("added").construction = a.construction;
    commit(ctx, l, s)?;
    Ok(json!({ "id": id, "points": [c, p0, p1] }))
}

#[derive(Deserialize, JsonSchema)]
struct AddPolylineArgs {
    layer: Option<String>,
    /// Point ids or positions `[x, y]` (mm), at least 2.
    points: Vec<PointArg>,
    /// Close back to the first point. Default false.
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    construction: bool,
}

fn add_polyline(ctx: &mut Ctx, a: AddPolylineArgs) -> CmdResult {
    if a.points.len() < 2 {
        return Err(CmdError::invalid_args("a polyline needs at least 2 points"));
    }
    let l = layer(ctx.doc(), a.layer.as_deref())?;
    let mut s = current(ctx.doc(), l);
    let pts: Vec<PointId> = a.points.iter().map(|p| point_of(&mut s, *p)).collect::<Result<_, _>>()?;
    let n = pts.len();
    let segs = if a.closed && n >= 3 { n } else { n - 1 };
    let mut edges = Vec::new();
    for i in 0..segs {
        let (p, q) = (pts[i], pts[(i + 1) % n]);
        if p == q {
            continue;
        }
        let e = s.add_line(p, q);
        s.edge_mut(e).expect("added").construction = a.construction;
        edges.push(e);
    }
    commit(ctx, l, s)?;
    Ok(json!({ "points": pts, "edges": edges }))
}

#[derive(Deserialize, JsonSchema)]
struct AddConstraintArgs {
    layer: Option<String>,
    /// The constraint, e.g. `{"type": "horizontal", "a": 1, "b": 2}`,
    /// `{"type": "fixed", "point": 1, "at": {"x": 0, "y": 0}}`,
    /// `{"type": "parallel", "a": 5, "b": 7}` (edge ids). Positions mm.
    constraint: ConstraintKind,
}

fn add_constraint(ctx: &mut Ctx, a: AddConstraintArgs) -> CmdResult {
    let l = layer(ctx.doc(), a.layer.as_deref())?;
    let solved = sketch::apply(ctx.doc_mut(), l, &Edit::AddConstraint { kind: a.constraint }, &edit_options())?;
    Ok(json!({ "id": first_constraint(&solved.created) }))
}

fn first_constraint(created: &[ItemRef]) -> Option<ConstraintId> {
    created.iter().find_map(|c| match c {
        ItemRef::Constraint(id) => Some(*id),
        _ => None,
    })
}

#[derive(Deserialize, JsonSchema)]
struct RemoveConstraintArgs {
    layer: Option<String>,
    /// Constraint or dimension ids.
    ids: Vec<u32>,
}

fn remove_constraint(ctx: &mut Ctx, a: RemoveConstraintArgs) -> CmdResult {
    let l = layer(ctx.doc(), a.layer.as_deref())?;
    let edits = a.ids.iter().map(|&id| Edit::Remove { item: ItemRef::Constraint(ConstraintId(id)) }).collect();
    sketch::apply(ctx.doc_mut(), l, &Edit::Batch { edits }, &edit_options())?;
    Ok(json!({ "removed": a.ids }))
}

#[derive(Deserialize, JsonSchema)]
struct RemoveArgs {
    layer: Option<String>,
    /// What to remove, e.g. `{"type": "point", "id": 3}`, `{"type": "edge", "id": 5}`,
    /// `{"type": "constraint", "id": 9}`. Removing a point removes its edges; removing
    /// edges removes the constraints on them. Walls and floors using a removed edge
    /// keep their geometry but no longer follow the sketch.
    items: Vec<ItemRef>,
}

fn remove(ctx: &mut Ctx, a: RemoveArgs) -> CmdResult {
    let l = layer(ctx.doc(), a.layer.as_deref())?;
    let edits = a.items.iter().map(|&item| Edit::Remove { item }).collect();
    let before: Vec<Id> = sketch::linked_on(ctx.doc(), l);
    sketch::apply(ctx.doc_mut(), l, &Edit::Batch { edits }, &edit_options())?;
    let unlinked: Vec<Id> =
        before.into_iter().filter(|id| ctx.doc().objects.get(id).is_some_and(|o| o.sketch.is_none())).collect();
    Ok(json!({ "unlinked": unlinked }))
}

#[derive(Deserialize, JsonSchema)]
struct DragArgs {
    layer: Option<String>,
    /// Points and where to pull them, plan mm. Each follows as far as the constraints
    /// allow.
    targets: Vec<DragTarget>,
}

fn drag(ctx: &mut Ctx, a: DragArgs) -> CmdResult {
    let l = layer(ctx.doc(), a.layer.as_deref())?;
    let solved = sketch::apply(ctx.doc_mut(), l, &Edit::Drag { targets: a.targets }, &edit_options())?;
    Ok(json!({ "reached": solved.drag_reached }))
}

fn default_snap() -> f64 {
    100.0
}

#[derive(Deserialize, JsonSchema)]
struct DrawLineArgs {
    layer: Option<String>,
    /// Where the line starts, plan mm. Snaps to a sketch point (shared, i.e.
    /// coincident) or onto a line (point-on-line) within `snap_radius`.
    start: P2,
    /// Where the pointer was released, plan mm. Snaps like `start` unless `length`
    /// is given.
    end: P2,
    /// Shift held: horizontal or vertical, whichever is closer. Lines within 2° of an
    /// axis get it anyway. Default false.
    #[serde(default)]
    axis_lock: bool,
    /// Typed length, mm: becomes a driving length dimension.
    length: Option<f64>,
    /// mm. Default 100.
    #[serde(default = "default_snap")]
    snap_radius: f64,
}

fn draw_line(ctx: &mut Ctx, a: DrawLineArgs) -> CmdResult {
    let l = layer(ctx.doc(), a.layer.as_deref())?;
    let s = current(ctx.doc(), l);
    let g = LineGesture { start: v2(a.start), end: v2(a.end), axis_lock: a.axis_lock, length: a.length };
    let plan = infer_line(&s, &g, &InferOptions { snap_radius: a.snap_radius, ..Default::default() });
    let solved = sketch::apply(ctx.doc_mut(), l, &Edit::DrawLine { plan }, &edit_options())?;
    Ok(json!({ "plan": plan, "created": solved.created }))
}

/// How one drawn segment was made.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
struct SegmentHint {
    /// Shift was held: horizontal or vertical.
    #[serde(default)]
    axis_lock: bool,
    /// Typed length in mm: a driving dimension.
    length: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Make {
    /// Sketch lines only.
    #[default]
    None,
    /// A wall along the lines.
    Wall,
    /// A floor inside the closed polyline.
    Floor,
}

#[derive(Deserialize, JsonSchema)]
struct DrawArgs {
    #[serde(flatten)]
    common: Common,
    /// Clicked points, plan mm, at least 2 (3 for a floor). The first point and every
    /// end snap to sketch points or lines within `snap_radius`.
    points: Vec<P2>,
    /// Per segment (from point i to i+1; with `closed` the last one goes back to the
    /// first): Shift and typed length. Missing entries: no lock, no length.
    #[serde(default)]
    segments: Vec<SegmentHint>,
    /// Close back to the first point. Floors are always closed. Default false.
    #[serde(default)]
    closed: bool,
    /// mm. Default 100.
    #[serde(default = "default_snap")]
    snap_radius: f64,
    /// What the lines become: none, wall or floor. Default none.
    #[serde(default)]
    make: Make,
    /// Wall height, mm. Default 3000.
    height: Option<f64>,
    /// Wall thickness (default 150) or floor thickness (default 20), mm.
    thickness: Option<f64>,
    /// Wall base or floor top above the layer, mm. Default 0.
    #[serde(default)]
    z: f64,
}

fn draw(ctx: &mut Ctx, a: DrawArgs) -> CmdResult {
    let closed = a.closed || a.make == Make::Floor;
    let min = if closed { 3 } else { 2 };
    if a.points.len() < min {
        return Err(CmdError::invalid_args(format!("drawing needs at least {min} points")));
    }
    let l = layer_for(ctx.doc(), &a.common)?;
    let mut s = current(ctx.doc(), l);
    let n = a.points.len();
    let segs = if closed { n } else { n - 1 };
    let infer = InferOptions { snap_radius: a.snap_radius, ..Default::default() };
    let (mut first, mut prev): (Option<PointId>, Option<PointId>) = (None, None);
    let mut edges: Vec<EdgeId> = Vec::new();
    let mut created = Vec::new();
    for i in 0..segs {
        let hint = a.segments.get(i).copied().unwrap_or_default();
        let g = LineGesture {
            start: v2(a.points[i]),
            end: v2(a.points[(i + 1) % n]),
            axis_lock: hint.axis_lock,
            length: hint.length,
        };
        let mut plan = infer_line(&s, &g, &infer);
        if let Some(p) = prev {
            plan.start = sketch::Endpoint::Existing { point: p };
        }
        if closed
            && i == segs - 1
            && let Some(f) = first
        {
            plan.end = sketch::Endpoint::Existing { point: f };
        }
        if let (sketch::Endpoint::Existing { point: x }, sketch::Endpoint::Existing { point: y }) =
            (plan.start, plan.end)
            && x == y
        {
            continue; // zero-length segment (double click on the same spot)
        }
        let solved = s.apply(&Edit::DrawLine { plan }, &edit_options()).map_err(|e| {
            let err = sketch::solve_error(e);
            CmdError::new(err.code, format!("segment {}: {}", i + 1, err.message))
        })?;
        let edge = solved
            .created
            .iter()
            .find_map(|c| match c {
                ItemRef::Edge(e) => Some(*e),
                _ => None,
            })
            .expect("draw_line creates an edge");
        created.extend(solved.created.iter().copied());
        s = solved.sketch;
        let (pa, pb) = s.edge(edge).expect("created").geom.ends();
        first.get_or_insert(pa);
        prev = Some(pb);
        edges.push(edge);
    }
    if edges.is_empty() {
        return Err(CmdError::invalid_args("nothing to draw: every segment has zero length"));
    }
    demote_redundant(&mut s, &created);
    sketch::store(ctx.doc_mut(), l, s);
    let object = match a.make {
        Make::None => None,
        Make::Wall => Some(linked_wall(ctx, a.common, l, edges.clone(), a.height, a.thickness, a.z)?),
        Make::Floor => Some(linked_floor(ctx, a.common, l, edges.clone(), a.thickness, a.z)?),
    };
    Ok(json!({ "id": object, "edges": edges, "created": created }))
}

/// Typed lengths that the other constraints already imply (the last side of a closed
/// rectangle) become reference dimensions, as in FreeCAD, so changing one side later
/// does not conflict with them. The newest goes first.
fn demote_redundant(s: &mut sketch::Sketch, created: &[ItemRef]) {
    let dims: Vec<ConstraintId> = created
        .iter()
        .filter_map(|c| match c {
            ItemRef::Constraint(id) if s.dimension(*id).is_some() => Some(*id),
            _ => None,
        })
        .collect();
    for _ in 0..dims.len() {
        let Ok(st) = s.analyse() else { return };
        let Some(&id) =
            dims.iter().rev().find(|d| st.redundant.contains(d) && s.dimension(**d).is_some_and(|d| d.driving))
        else {
            return;
        };
        s.dimension_mut(id).expect("listed").driving = false;
    }
}

fn layer_for(doc: &Document, c: &Common) -> Result<Id, CmdError> {
    if c.parent.is_some() {
        return Err(CmdError::invalid_args(
            "sketch geometry sits on the layer; walls and floors from it have no parent",
        ));
    }
    layer(doc, c.layer.as_deref())
}

fn linked_wall(
    ctx: &mut Ctx,
    mut common: Common,
    l: Id,
    edges: Vec<EdgeId>,
    height: Option<f64>,
    thickness: Option<f64>,
    z: f64,
) -> Result<Id, CmdError> {
    let (height, thickness) = (height.unwrap_or(3000.0), thickness.unwrap_or(150.0));
    super::create::positive("height", height)?;
    super::create::positive("thickness", thickness)?;
    let s = current(ctx.doc(), l);
    let (path, _) = sketch::chain(&s, &edges).ok_or_else(|| CmdError::invalid("the edges do not form a chain"))?;
    common.layer = Some(l.to_string());
    let p = Placement { position: [0.0, 0.0, z], ..Default::default() };
    let v = insert(ctx, common, "Wall", p, ObjectKind::Wall { path, height, thickness })?;
    let id = super::id_of(&v);
    ctx.doc_mut().objects.get_mut(&id).expect("created").sketch = Some(SketchLink { edges });
    Ok(id)
}

fn linked_floor(
    ctx: &mut Ctx,
    mut common: Common,
    l: Id,
    edges: Vec<EdgeId>,
    thickness: Option<f64>,
    z: f64,
) -> Result<Id, CmdError> {
    let thickness = thickness.unwrap_or(20.0);
    super::create::positive("thickness", thickness)?;
    let s = current(ctx.doc(), l);
    let outline =
        sketch::loop_outline(&s, &edges).ok_or_else(|| CmdError::invalid("the edges do not form a closed loop"))?;
    common.layer = Some(l.to_string());
    let p = Placement { position: [0.0, 0.0, z], ..Default::default() };
    let v = insert(ctx, common, "Floor", p, ObjectKind::Floor { outline, thickness })?;
    let id = super::id_of(&v);
    ctx.doc_mut().objects.get_mut(&id).expect("created").sketch = Some(SketchLink { edges });
    Ok(id)
}

#[derive(Deserialize, JsonSchema)]
struct MakeWallArgs {
    #[serde(flatten)]
    common: Common,
    /// Sketch edge ids in order along the wall; they must join end to end.
    edges: Vec<u32>,
    /// mm. Default 3000.
    height: Option<f64>,
    /// mm. Default 150.
    thickness: Option<f64>,
    /// Base height above the layer, mm. Default 0.
    #[serde(default)]
    z: f64,
}

fn make_wall(ctx: &mut Ctx, a: MakeWallArgs) -> CmdResult {
    let l = layer_for(ctx.doc(), &a.common)?;
    let edges = a.edges.into_iter().map(EdgeId).collect();
    let id = linked_wall(ctx, a.common, l, edges, a.height, a.thickness, a.z)?;
    Ok(json!({ "id": id }))
}

#[derive(Deserialize, JsonSchema)]
struct MakeFloorArgs {
    #[serde(flatten)]
    common: Common,
    /// Sketch edge ids around the floor, in order. Either this or `at`.
    edges: Option<Vec<u32>>,
    /// A plan point (mm) inside a closed loop of the sketch: the smallest loop around
    /// it becomes the floor.
    at: Option<P2>,
    /// mm. Default 20.
    thickness: Option<f64>,
    /// Height of the top surface above the layer, mm. Default 0.
    #[serde(default)]
    z: f64,
}

fn make_floor(ctx: &mut Ctx, a: MakeFloorArgs) -> CmdResult {
    let l = layer_for(ctx.doc(), &a.common)?;
    let edges: Vec<EdgeId> = match (a.edges, a.at) {
        (Some(e), _) => e.into_iter().map(EdgeId).collect(),
        (None, Some(at)) => {
            let s = current(ctx.doc(), l);
            let lp = s
                .closed_loops()
                .into_iter()
                .filter(|lp| point_in(&s.loop_outline(lp, 5.0).into_iter().map(p2).collect::<Vec<_>>(), at))
                .min_by(|x, y| x.area.total_cmp(&y.area))
                .ok_or_else(|| CmdError::invalid(format!("no closed loop of the sketch around {at:?}")))?;
            lp.edges.iter().map(|e| e.edge).collect()
        }
        (None, None) => return Err(CmdError::invalid_args("give edges or a point inside the loop (at)")),
    };
    let id = linked_floor(ctx, a.common, l, edges, a.thickness, a.z)?;
    Ok(json!({ "id": id }))
}

fn point_in(poly: &[P2], p: P2) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
    }
    inside
}

#[derive(Deserialize, JsonSchema)]
struct AdoptArgs {
    /// Free walls and floors (roots, not tilted).
    ids: Vec<Id>,
    /// Give new lines that are exactly horizontal or vertical that constraint.
    /// Default true.
    #[serde(default = "yes")]
    infer_axes: bool,
}

fn yes() -> bool {
    true
}

fn adopt(ctx: &mut Ctx, a: AdoptArgs) -> CmdResult {
    let ids = sketch::adopt(ctx.doc_mut(), &a.ids, a.infer_axes)?;
    Ok(json!({ "adopted": ids }))
}

#[derive(Deserialize, JsonSchema)]
struct IdsArgs {
    ids: Vec<Id>,
}

fn unlink(ctx: &mut Ctx, a: IdsArgs) -> CmdResult {
    for id in &a.ids {
        object(ctx.doc(), *id)?;
    }
    let doc = ctx.doc_mut();
    for id in &a.ids {
        doc.objects.get_mut(id).expect("checked").sketch = None;
    }
    Ok(json!({}))
}

fn solve(ctx: &mut Ctx, a: LayerArg) -> CmdResult {
    let l = layer(ctx.doc(), a.layer.as_deref())?;
    sketch::apply(ctx.doc_mut(), l, &Edit::Resolve, &edit_options())?;
    Ok(json!({}))
}

#[derive(Deserialize, JsonSchema)]
struct EditArgs {
    layer: Option<String>,
    /// One edit, tagged by `op`: resolve, drag, set_dimension, set_driving,
    /// set_label_offset, set_dimension_name, add_point, add_edge, add_constraint,
    /// add_dimension, remove, set_edge_tag, set_construction, draw_line, batch.
    edit: Edit,
}

fn edit(ctx: &mut Ctx, a: EditArgs) -> CmdResult {
    let l = layer(ctx.doc(), a.layer.as_deref())?;
    let solved = sketch::apply(ctx.doc_mut(), l, &a.edit, &edit_options())?;
    Ok(json!({ "created": solved.created, "reached": solved.drag_reached }))
}

// ---- dimensions -------------------------------------------------------------------

#[derive(Deserialize, JsonSchema)]
struct DimAddArgs {
    layer: Option<String>,
    /// What to measure in the sketch, e.g. `{"type": "length", "line": 5}`,
    /// `{"type": "distance", "a": 1, "b": 2}`, `{"type": "horizontal_distance", "a": 1,
    /// "b": 2}`, `{"type": "point_line_distance", "point": 3, "line": 5}`,
    /// `{"type": "angle", "a": 5, "b": 6}`, `{"type": "radius", "arc": 8}`.
    measure: Measure,
    /// mm (degrees for angles). Default: the current measurement.
    value: Option<f64>,
    /// true: driving (constrains the geometry); false: reference (measures only).
    /// Default: driving, unless that would over-constrain the sketch (then reference,
    /// as FreeCAD does).
    driving: Option<bool>,
    /// Parameter name, e.g. "portal width".
    name: Option<String>,
    /// Label position as an offset from the measured geometry's midpoint, plan mm.
    label_offset: Option<P2>,
}

fn dim_add(ctx: &mut Ctx, a: DimAddArgs) -> CmdResult {
    let l = layer(ctx.doc(), a.layer.as_deref())?;
    let auto = a.driving.is_none();
    let edit = Edit::AddDimension {
        measure: a.measure,
        value: a.value,
        driving: a.driving.unwrap_or(true),
        name: a.name,
        label_offset: v2(a.label_offset.unwrap_or_default()),
    };
    let opts = if auto { SolveOptions::default() } else { edit_options() };
    let solved = sketch::apply(ctx.doc_mut(), l, &edit, &opts)?;
    let id = first_constraint(&solved.created).expect("add_dimension creates one");
    let mut driving = a.driving.unwrap_or(true);
    if auto && solved.status.as_ref().is_some_and(|st| st.redundant.contains(&id)) {
        sketch::apply(ctx.doc_mut(), l, &Edit::SetDriving { id, driving: false }, &edit_options())?;
        driving = false;
    }
    let value = ctx.doc().sketches[&l].dimension(id).map(|d| d.value);
    Ok(json!({ "id": id, "driving": driving, "value": value }))
}

#[derive(Deserialize, JsonSchema)]
struct DimAddRefArgs {
    /// First end: `{"type": "object", "object": <id>, "local": [x, y, z]}` (a point of
    /// an object in its own frame, mm; default its origin), `{"type": "sketch_point",
    /// "layer": <id>, "point": 3}` or `{"type": "point", "at": [x, y, z]}` (world mm).
    from: Anchor,
    /// Second end, same forms.
    to: Anchor,
    /// aligned (straight, default), horizontal (along x) or vertical (along y).
    #[serde(default)]
    axis: DimAxis,
    /// Label offset from the midpoint, plan mm.
    #[serde(default)]
    label_offset: P2,
    name: Option<String>,
    /// Layer the dimension belongs to (hidden and deleted with it). Default: the
    /// layer of the first object anchor, else the first layer.
    layer: Option<String>,
}

fn dim_add_reference(ctx: &mut Ctx, a: DimAddRefArgs) -> CmdResult {
    let doc = ctx.doc();
    for an in [&a.from, &a.to] {
        if sketch::anchor_world(doc, an).is_none() {
            return Err(CmdError::not_found("dimension anchor", serde_json::to_string(an).unwrap_or_default()));
        }
    }
    let l = match &a.layer {
        Some(l) => layer(doc, Some(l))?,
        None => [a.from, a.to]
            .iter()
            .find_map(|an| match an {
                Anchor::Object { object, .. } => doc.objects.get(object).map(|o| o.layer),
                Anchor::SketchPoint { layer, .. } => Some(*layer),
                Anchor::Point { .. } => None,
            })
            .map_or_else(|| layer(doc, None), Ok)?,
    };
    let d = RefDimension {
        id: Uuid::new_v4(),
        from: a.from,
        to: a.to,
        axis: a.axis,
        label_offset: a.label_offset,
        name: a.name,
        layer: l,
    };
    let value = d.measure(doc).map(|m| m.2);
    let id = d.id;
    ctx.doc_mut().dimensions.insert(id, d);
    Ok(json!({ "id": id, "value": value }))
}

/// A dimension: a sketch dimension id (number), a reference dimension id (UUID), or a
/// dimension's name.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
enum DimRef {
    Sketch(u32),
    Text(String),
}

enum Found {
    Sketch(Id, ConstraintId),
    Ref(Id),
}

fn find_dim(doc: &Document, r: &DimRef, l: Option<&str>) -> Result<Found, CmdError> {
    let layers: Vec<Id> = match l {
        Some(l) => vec![layer(doc, Some(l))?],
        None => doc.layers_sorted().iter().map(|l| l.id).collect(),
    };
    let hits: Vec<Found> = match r {
        DimRef::Sketch(n) => layers
            .iter()
            .filter(|l| doc.sketches.get(*l).is_some_and(|s| s.dimension(ConstraintId(*n)).is_some()))
            .map(|&l| Found::Sketch(l, ConstraintId(*n)))
            .collect(),
        DimRef::Text(t) => {
            if let Ok(id) = t.parse::<Uuid>()
                && doc.dimensions.contains_key(&id)
            {
                return Ok(Found::Ref(id));
            }
            let mut v: Vec<Found> = layers
                .iter()
                .filter_map(|l| doc.sketches.get(l).and_then(|s| s.dimension_named(t)).map(|d| Found::Sketch(*l, d.id)))
                .collect();
            v.extend(doc.dimensions.values().filter(|d| d.name.as_deref() == Some(t)).map(|d| Found::Ref(d.id)));
            v
        }
    };
    let what = match r {
        DimRef::Sketch(n) => n.to_string(),
        DimRef::Text(t) => t.clone(),
    };
    match hits.len() {
        0 => Err(CmdError::not_found("dimension", what)),
        1 => Ok(hits.into_iter().next().expect("one")),
        _ => Err(CmdError::invalid(format!("dimension {what} exists on several layers; pass layer"))),
    }
}

#[derive(Deserialize, JsonSchema)]
struct DimSetValueArgs {
    /// Sketch dimension id, or its name.
    id: DimRef,
    /// mm, or degrees for angles.
    value: f64,
    /// Layer id or name; needed only when the id exists on several layers.
    layer: Option<String>,
}

fn dim_set_value(ctx: &mut Ctx, a: DimSetValueArgs) -> CmdResult {
    match find_dim(ctx.doc(), &a.id, a.layer.as_deref())? {
        Found::Sketch(l, id) => {
            sketch::apply(ctx.doc_mut(), l, &Edit::SetDimension { id, value: a.value }, &edit_options())?;
            Ok(json!({ "id": id, "layer": l, "value": a.value }))
        }
        Found::Ref(_) => Err(CmdError::invalid("reference dimensions between objects measure only; move the objects")),
    }
}

#[derive(Deserialize, JsonSchema)]
struct DimSetDrivingArgs {
    id: DimRef,
    /// true: driving; false: reference.
    driving: bool,
    layer: Option<String>,
}

fn dim_set_driving(ctx: &mut Ctx, a: DimSetDrivingArgs) -> CmdResult {
    match find_dim(ctx.doc(), &a.id, a.layer.as_deref())? {
        Found::Sketch(l, id) => {
            sketch::apply(ctx.doc_mut(), l, &Edit::SetDriving { id, driving: a.driving }, &edit_options())?;
            Ok(json!({}))
        }
        Found::Ref(_) => Err(CmdError::invalid("reference dimensions between objects cannot drive")),
    }
}

#[derive(Deserialize, JsonSchema)]
struct DimMoveLabelArgs {
    id: DimRef,
    /// Offset of the label from the measured geometry's midpoint, plan mm.
    offset: P2,
    layer: Option<String>,
}

fn dim_move_label(ctx: &mut Ctx, a: DimMoveLabelArgs) -> CmdResult {
    match find_dim(ctx.doc(), &a.id, a.layer.as_deref())? {
        Found::Sketch(l, id) => {
            let doc = ctx.doc_mut();
            let mut s = current(doc, l);
            s.dimension_mut(id).expect("found").label_offset = v2(a.offset);
            doc.sketches.insert(l, s);
        }
        Found::Ref(id) => ctx.doc_mut().dimensions.get_mut(&id).expect("found").label_offset = a.offset,
    }
    Ok(json!({}))
}

#[derive(Deserialize, JsonSchema)]
struct DimNameArgs {
    id: DimRef,
    /// The parameter name; empty or missing clears it.
    name: Option<String>,
    layer: Option<String>,
}

fn dim_name(ctx: &mut Ctx, a: DimNameArgs) -> CmdResult {
    let name = a.name.filter(|n| !n.trim().is_empty());
    match find_dim(ctx.doc(), &a.id, a.layer.as_deref())? {
        Found::Sketch(l, id) => {
            let doc = ctx.doc_mut();
            let mut s = current(doc, l);
            s.dimension_mut(id).expect("found").name = name;
            doc.sketches.insert(l, s);
        }
        Found::Ref(id) => ctx.doc_mut().dimensions.get_mut(&id).expect("found").name = name,
    }
    Ok(json!({}))
}

#[derive(Deserialize, JsonSchema)]
struct DimRemoveArgs {
    /// Dimension ids (numbers for sketch dimensions, UUIDs for reference dimensions)
    /// or names.
    ids: Vec<DimRef>,
    layer: Option<String>,
}

fn dim_remove(ctx: &mut Ctx, a: DimRemoveArgs) -> CmdResult {
    for r in &a.ids {
        match find_dim(ctx.doc(), r, a.layer.as_deref())? {
            Found::Sketch(l, id) => {
                sketch::apply(ctx.doc_mut(), l, &Edit::Remove { item: ItemRef::Constraint(id) }, &edit_options())?;
            }
            Found::Ref(id) => {
                ctx.doc_mut().dimensions.remove(&id);
            }
        }
    }
    Ok(json!({}))
}

#[derive(Deserialize, JsonSchema)]
struct DimListArgs {
    /// Only this layer. Default: all.
    layer: Option<String>,
}

fn dim_list(ctx: &mut Ctx, a: DimListArgs) -> CmdResult {
    let doc = ctx.doc();
    let only = a.layer.as_deref().map(|l| layer(doc, Some(l))).transpose()?;
    let mut out = Vec::new();
    for l in doc.layers_sorted() {
        if only.is_some_and(|o| o != l.id) {
            continue;
        }
        if let Some(s) = doc.sketches.get(&l.id) {
            for d in &s.dimensions {
                out.push(json!({
                    "id": d.id, "kind": "sketch", "layer": l.name, "driving": d.driving,
                    "value": s.measure(&d.measure).unwrap_or(d.value), "name": d.name, "measure": d.measure,
                    "unit": if d.measure.is_angle() { "deg" } else { "mm" },
                }));
            }
        }
        for d in doc.dimensions.values().filter(|d| d.layer == l.id) {
            out.push(json!({
                "id": d.id, "kind": "reference", "layer": l.name, "driving": false,
                "value": d.measure(doc).map(|m| m.2), "name": d.name, "from": d.from, "to": d.to, "axis": d.axis,
                "unit": "mm",
            }));
        }
    }
    Ok(json!(out))
}
