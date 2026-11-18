//! Layer sketches (block 9, `docs/spec/sketch.md`).
//!
//! Every layer can hold one parametric sketch ([`Document::sketches`], keyed by layer id)
//! solved by `stagecraft-sketch` (ezpz). Walls and floors with a [`SketchLink`] follow
//! it: their `path` / `outline` is derived from the linked edges after every sketch
//! edit ([`sync_layer`]), so plan, 3D, sheets, hidden lines and MVR export all see the
//! solved geometry without knowing about sketches. A linked object is a root with its
//! placement's x/y and rotation at zero; only the base height (placement z) is its own.
//!
//! Sketch coordinates are the layer's plan coordinates in mm (the layer only adds its
//! elevation). [`RefDimension`]s measure between points of objects, sketch points or
//! fixed points and drive nothing.

use glam::{DQuat, DVec3};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use stagecraft_sketch::*;

use crate::command::CmdError;
use crate::model::{Document, Id, ObjectKind, P2, P3};

/// The sketch edges a wall or floor is made of, in order along the wall or around the
/// floor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SketchLink {
    pub edges: Vec<EdgeId>,
}

/// One end of a reference dimension.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Anchor {
    /// A point of an object in its own (local) frame, mm. Default: its origin. It
    /// follows the object when it moves.
    Object {
        object: Id,
        #[serde(default)]
        local: P3,
    },
    /// A point of a layer's sketch.
    SketchPoint { layer: Id, point: PointId },
    /// A fixed point in world mm.
    Point { at: P3 },
}

/// What a reference dimension measures between its two anchors (in plan).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DimAxis {
    /// Straight distance in plan.
    #[default]
    Aligned,
    /// Along x only (|Δx|).
    Horizontal,
    /// Along y only (|Δy|).
    Vertical,
}

/// A dimension that measures live between two points of anything (truss to portal …).
/// It never drives geometry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RefDimension {
    pub id: Id,
    pub from: Anchor,
    pub to: Anchor,
    #[serde(default)]
    pub axis: DimAxis,
    /// Label position as an offset from the midpoint of the two points, plan mm. The
    /// dimension line runs through it.
    #[serde(default)]
    pub label_offset: P2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Layer it belongs to: hidden with it, deleted with it.
    pub layer: Id,
}

pub fn v2(p: P2) -> Vec2 {
    Vec2::new(p[0], p[1])
}
pub fn p2(v: Vec2) -> P2 {
    [v.x, v.y]
}

/// Where an anchor is in world mm, `None` if what it refers to is gone.
pub fn anchor_world(doc: &Document, a: &Anchor) -> Option<DVec3> {
    match *a {
        Anchor::Object { object, local } => {
            crate::geom::world(doc, object).map(|w| w.transform_point3(DVec3::from_array(local)))
        }
        Anchor::SketchPoint { layer, point } => {
            let p = doc.sketches.get(&layer)?.point(point)?.pos;
            let z = doc.layers.get(&layer).map_or(0.0, |l| l.elevation);
            Some(DVec3::new(p.x, p.y, z))
        }
        Anchor::Point { at } => Some(DVec3::from_array(at)),
    }
}

impl RefDimension {
    /// The two measured plan points (after projecting onto the axis) and the value, mm.
    pub fn measure(&self, doc: &Document) -> Option<(P2, P2, f64)> {
        let a = anchor_world(doc, &self.from)?;
        let b = anchor_world(doc, &self.to)?;
        let (a, b) = ([a.x, a.y], [b.x, b.y]);
        let v = match self.axis {
            DimAxis::Aligned => (b[0] - a[0]).hypot(b[1] - a[1]),
            DimAxis::Horizontal => (b[0] - a[0]).abs(),
            DimAxis::Vertical => (b[1] - a[1]).abs(),
        };
        Some((a, b, v))
    }

    /// True if the dimension refers to `id` (object anchors).
    pub fn uses_object(&self, id: Id) -> bool {
        [self.from, self.to].iter().any(|a| matches!(a, Anchor::Object { object, .. } if *object == id))
    }
}

/// The error a failed sketch edit becomes. Conflicts get the code `conflict` and a
/// message naming the constraints, the one the edit touched first.
pub fn solve_error(e: SolveError) -> CmdError {
    match e {
        SolveError::Conflict { .. } => CmdError::new("conflict", e.to_string()),
        SolveError::Missing(_) => CmdError::new("not_found", e.to_string()),
        SolveError::NotConverged => CmdError::new("conflict", e.to_string()),
        _ => CmdError::invalid(e.to_string()),
    }
}

/// Options for edits from commands: no analysis (status is a separate query).
pub fn edit_options() -> SolveOptions {
    SolveOptions { analyse: false, ..SolveOptions::default() }
}

/// How far a new dimension's line sits from what it measures, mm.
pub const LABEL_OFFSET: f64 = 600.0;

/// A default label offset for a new dimension: beside the measured line, on the side
/// away from the middle of the sketch (outside a room). Zero for angles and radii.
pub fn default_label_offset(s: &Sketch, m: &Measure) -> Vec2 {
    let ends = match *m {
        Measure::Length { line } => s.line_ends(line),
        Measure::Distance { a, b } => Some((a, b)),
        Measure::HorizontalDistance { .. } => return outward(s, m, Vec2::new(0.0, 1.0)),
        Measure::VerticalDistance { .. } => return outward(s, m, Vec2::new(1.0, 0.0)),
        _ => None,
    };
    let Some((a, b)) = ends else { return Vec2::ZERO };
    let (Some(pa), Some(pb)) = (s.point(a), s.point(b)) else { return Vec2::ZERO };
    outward(s, m, (pb.pos - pa.pos).perp().normalized())
}

fn outward(s: &Sketch, m: &Measure, n: Vec2) -> Vec2 {
    let Some(mid) = s.measure_anchor(m) else { return Vec2::ZERO };
    let k = s.points.len().max(1) as f64;
    let centre = s.points.iter().fold(Vec2::ZERO, |acc, p| acc + p.pos) * (1.0 / k);
    let side = if (mid - centre).dot(n) < 0.0 { -1.0 } else { 1.0 };
    n * (side * LABEL_OFFSET)
}

/// Polyline of one edge from its `ends().0` to `ends().1`, arcs split every 5° at most.
fn edge_polyline(s: &Sketch, e: &Edge) -> Option<Vec<P2>> {
    match e.geom {
        EdgeGeom::Line { a, b } => Some(vec![p2(s.point(a)?.pos), p2(s.point(b)?.pos)]),
        EdgeGeom::Arc { center, start, end } => {
            let c = s.point(center)?.pos;
            let (p0, p1) = (s.point(start)?.pos, s.point(end)?.pos);
            let r = c.distance(p0);
            let a0 = (p0 - c).angle();
            let mut sweep = (p1 - c).angle() - a0;
            while sweep <= 1e-12 {
                sweep += std::f64::consts::TAU;
            }
            let n = ((sweep.to_degrees() / 5.0).ceil() as usize).max(1);
            let mut v: Vec<P2> = (0..n)
                .map(|i| {
                    let t = a0 + sweep * i as f64 / n as f64;
                    [c.x + r * t.cos(), c.y + r * t.sin()]
                })
                .collect();
            v.push(p2(p1));
            Some(v)
        }
    }
}

fn touches(s: &Sketch, a: PointId, b: PointId) -> bool {
    a == b || matches!((s.point(a), s.point(b)), (Some(p), Some(q)) if p.pos.distance(q.pos) < 1e-6)
}

/// The points along a chain of edges in order, and whether the chain closes. `None` if
/// an edge is missing or the edges do not join end to end.
pub fn chain(s: &Sketch, edges: &[EdgeId]) -> Option<(Vec<P2>, bool)> {
    let segs: Vec<(PointId, PointId, Vec<P2>)> = edges
        .iter()
        .map(|&id| {
            let e = s.edge(id)?;
            let (a, b) = e.geom.ends();
            Some((a, b, edge_polyline(s, e)?))
        })
        .collect::<Option<_>>()?;
    let (first_a, first_b, first_pts) = segs.first()?.clone();
    let reverse_first = segs.get(1).is_some_and(|n| touches(s, first_a, n.0) || touches(s, first_a, n.1));
    let (start, mut cur, mut out) = if reverse_first {
        (first_b, first_a, first_pts.into_iter().rev().collect::<Vec<_>>())
    } else {
        (first_a, first_b, first_pts)
    };
    for (a, b, pts) in segs.iter().skip(1) {
        if touches(s, cur, *a) {
            out.extend(pts.iter().skip(1));
            cur = *b;
        } else if touches(s, cur, *b) {
            out.extend(pts.iter().rev().skip(1));
            cur = *a;
        } else {
            return None;
        }
    }
    let closed = segs.len() >= 2 && touches(s, cur, start) && out.len() >= 4;
    if closed {
        let first = out[0];
        *out.last_mut().expect("non-empty") = first;
    }
    Some((out, closed))
}

/// The closed outline of a floor from its edges (first point not repeated).
pub fn loop_outline(s: &Sketch, edges: &[EdgeId]) -> Option<Vec<P2>> {
    let (mut pts, closed) = chain(s, edges)?;
    if !closed {
        return None;
    }
    pts.pop();
    (pts.len() >= 3).then_some(pts)
}

/// Objects on `layer` with a sketch link.
pub fn linked_on(doc: &Document, layer: Id) -> Vec<Id> {
    doc.objects.values().filter(|o| o.layer == layer && o.sketch.is_some()).map(|o| o.id).collect()
}

/// The linked objects that use an edge.
pub fn objects_using_edge(doc: &Document, layer: Id, edge: EdgeId) -> Vec<Id> {
    doc.objects
        .values()
        .filter(|o| o.layer == layer && o.sketch.as_ref().is_some_and(|l| l.edges.contains(&edge)))
        .map(|o| o.id)
        .collect()
}

/// Brings the geometry of every linked object on `layer` up to date with the sketch.
/// Objects whose edges no longer exist lose their link and keep their last geometry.
/// Only objects whose geometry really changes are touched, so unchanged parts of the
/// document stay shared (sheet caches key on that).
pub fn sync_layer(doc: &mut Document, layer: Id) {
    let ids = linked_on(doc, layer);
    sync_objects(doc, &ids);
}

pub fn sync_objects(doc: &mut Document, ids: &[Id]) {
    for &id in ids {
        let Some(o) = doc.objects.get(&id) else { continue };
        let Some(link) = &o.sketch else { continue };
        let Some(s) = doc.sketches.get(&o.layer) else {
            doc.objects.get_mut(&id).expect("exists").sketch = None;
            continue;
        };
        if link.edges.iter().any(|e| s.edge(*e).is_none()) {
            doc.objects.get_mut(&id).expect("exists").sketch = None;
            continue;
        }
        let new_kind = match &o.kind {
            ObjectKind::Wall { path, height, thickness } => match chain(s, &link.edges) {
                Some((p, _)) if p != *path => {
                    Some(ObjectKind::Wall { path: p, height: *height, thickness: *thickness })
                }
                _ => None,
            },
            ObjectKind::Floor { outline, thickness } => match loop_outline(s, &link.edges) {
                Some(p) if p != *outline => Some(ObjectKind::Floor { outline: p, thickness: *thickness }),
                _ => None,
            },
            _ => None,
        };
        let placement_off = o.parent.is_some()
            || o.placement.position[0] != 0.0
            || o.placement.position[1] != 0.0
            || o.placement.rotation != [0.0; 3];
        if new_kind.is_none() && !placement_off {
            continue;
        }
        let o = doc.objects.get_mut(&id).expect("exists");
        if let Some(k) = new_kind {
            o.kind = k;
        }
        if placement_off {
            o.parent = None;
            o.placement.position[0] = 0.0;
            o.placement.position[1] = 0.0;
            o.placement.rotation = [0.0; 3];
        }
    }
}

/// Stores a solved sketch for a layer and lets the linked objects follow.
pub fn store(doc: &mut Document, layer: Id, mut sketch: Sketch) {
    // A sketch's first point is locked where it is, so the first dimension change
    // moves the far side instead of spreading over both (FreeCAD users lock a corner
    // by hand; here it is the default). Only when nothing is locked yet.
    let was_empty = doc.sketches.get(&layer).is_none_or(|s| s.points.is_empty());
    if was_empty
        && let Some(first) = sketch.points.first().map(|p| p.id)
        && !sketch.constraints.iter().any(|c| matches!(c.kind, stagecraft_sketch::ConstraintKind::Fixed { .. }))
    {
        sketch.fix(first);
    }
    doc.sketches.insert(layer, sketch);
    sync_layer(doc, layer);
    // Reference dimensions on vanished sketch points are dropped.
    let dead: Vec<Id> = doc
        .dimensions
        .values()
        .filter(|d| {
            [d.from, d.to].iter().any(|a| matches!(a, Anchor::SketchPoint { layer: l, .. } if *l == layer))
                && anchor_world(doc, &d.from).zip(anchor_world(doc, &d.to)).is_none()
        })
        .map(|d| d.id)
        .collect();
    for d in dead {
        doc.dimensions.remove(&d);
    }
}

/// Applies an edit to a layer's sketch (empty if it has none) and stores the result.
/// On error the document is unchanged.
pub fn apply(doc: &mut Document, layer: Id, edit: &Edit, opts: &SolveOptions) -> Result<Solved, CmdError> {
    let empty = Sketch::new();
    let sk = doc.sketches.get(&layer).unwrap_or(&empty);
    let solved = sk.apply(edit, opts).map_err(solve_error)?;
    store(doc, layer, solved.sketch.clone());
    Ok(solved)
}

/// The plan transform of a root object's placement (x/y and rotation about z only).
fn plan_of(doc: &Document, id: Id) -> Result<impl Fn(P2) -> P2 + use<>, CmdError> {
    let o = crate::commands::object(doc, id)?;
    if o.parent.is_some() {
        return Err(CmdError::invalid(format!("{} is attached to another object; detach it first", o.name)));
    }
    let [rx, ry, rz] = o.placement.rotation;
    if rx.abs() > 1e-9 || ry.abs() > 1e-9 {
        return Err(CmdError::invalid(format!(
            "{} is tilted; only level walls and floors can join the sketch",
            o.name
        )));
    }
    let q = DQuat::from_rotation_z(rz.to_radians());
    let [px, py, _] = o.placement.position;
    Ok(move |p: P2| {
        let v = q * DVec3::new(p[0], p[1], 0.0);
        [v.x + px, v.y + py]
    })
}

/// Turns free walls and floors into sketch geometry: their corners become sketch points
/// (reusing points within 1 mm, so walls sharing a corner join), their segments lines
/// (reusing a line between the same two points). With `infer_axes`, new lines that are
/// horizontal or vertical get that constraint. Returns the adopted ids.
pub fn adopt(doc: &mut Document, ids: &[Id], infer_axes: bool) -> Result<Vec<Id>, CmdError> {
    let mut adopted = Vec::new();
    let mut layers = Vec::new();
    for &id in ids {
        let o = crate::commands::object(doc, id)?;
        if o.sketch.is_some() {
            continue;
        }
        let (pts, closed) = match &o.kind {
            ObjectKind::Wall { path, .. } => {
                let mut p = path.clone();
                p.dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6);
                let closed = p.len() > 3 && (p[0][0] - p[p.len() - 1][0]).hypot(p[0][1] - p[p.len() - 1][1]) < 1e-6;
                if closed {
                    p.pop();
                }
                (p, closed)
            }
            ObjectKind::Floor { outline, .. } => (outline.clone(), true),
            _ => {
                return Err(CmdError::invalid(format!(
                    "{} is a {}; only walls and floors join the sketch",
                    o.name,
                    o.kind.type_name()
                )));
            }
        };
        if pts.len() < 2 {
            return Err(CmdError::invalid(format!("{} has too few points", o.name)));
        }
        let layer = o.layer;
        let to_layer = plan_of(doc, id)?;
        let world: Vec<P2> = pts.iter().map(|p| to_layer(*p)).collect();
        let mut s = doc.sketches.get(&layer).cloned().unwrap_or_default();
        let pids: Vec<PointId> = world
            .iter()
            .map(|&p| {
                let near = s.points.iter().find(|q| q.pos.distance(v2(p)) <= 1.0).map(|q| q.id);
                near.unwrap_or_else(|| s.add_point(v2(p)))
            })
            .collect();
        let n = pids.len();
        let segs = if closed { n } else { n - 1 };
        let mut edges = Vec::new();
        for i in 0..segs {
            let (a, b) = (pids[i], pids[(i + 1) % n]);
            if a == b {
                continue;
            }
            let existing = s
                .edges
                .iter()
                .find(|e| matches!(e.geom, EdgeGeom::Line { a: x, b: y } if (x == a && y == b) || (x == b && y == a)));
            let e = match existing {
                Some(e) => e.id,
                None => {
                    let e = s.add_line(a, b);
                    if infer_axes {
                        let d = s.pos(b) - s.pos(a);
                        let ang = d.y.abs().atan2(d.x.abs()).to_degrees();
                        if ang < 0.01 {
                            s.add_constraint(ConstraintKind::Horizontal { a, b });
                        } else if ang > 89.99 {
                            s.add_constraint(ConstraintKind::Vertical { a, b });
                        }
                    }
                    e
                }
            };
            edges.push(e);
        }
        if edges.is_empty() {
            return Err(CmdError::invalid(format!("{} has no length", doc.objects[&id].name)));
        }
        let solved = s.solve(&edit_options()).map_err(solve_error)?;
        doc.sketches.insert(layer, solved.sketch);
        let o = doc.objects.get_mut(&id).expect("exists");
        o.sketch = Some(SketchLink { edges });
        o.placement.position[0] = 0.0;
        o.placement.position[1] = 0.0;
        o.placement.rotation = [0.0; 3];
        adopted.push(id);
        if !layers.contains(&layer) {
            layers.push(layer);
        }
    }
    for l in layers {
        sync_layer(doc, l);
    }
    Ok(adopted)
}

/// Moves the sketch points of linked objects as `f` moves plan points (object moves
/// and rotations). The points are dragged, so constraints hold; locks (`fixed`) on the
/// moved object's own points move with it, since moving the object is explicit. Locks
/// elsewhere in the sketch stay.
pub fn transform_linked(doc: &mut Document, ids: &[Id], f: impl Fn(P2) -> P2) -> Result<(), CmdError> {
    let mut by_layer: std::collections::BTreeMap<Id, Vec<PointId>> = Default::default();
    for id in ids {
        let Some(o) = doc.objects.get(id) else { continue };
        let (Some(link), Some(s)) = (&o.sketch, doc.sketches.get(&o.layer)) else { continue };
        let pts = by_layer.entry(o.layer).or_default();
        for e in &link.edges {
            if let Some(e) = s.edge(*e) {
                for p in e.geom.points() {
                    if !pts.contains(&p) {
                        pts.push(p);
                    }
                }
            }
        }
    }
    for (layer, pts) in by_layer {
        let mut s = doc.sketches[&layer].clone();
        for c in &mut s.constraints {
            if let stagecraft_sketch::ConstraintKind::Fixed { point, at } = &mut c.kind
                && pts.contains(point)
            {
                *at = v2(f(p2(*at)));
            }
        }
        let targets = pts.iter().map(|&p| DragTarget { point: p, to: v2(f(p2(s.pos(p)))) }).collect();
        let solved = s.apply(&Edit::Drag { targets }, &edit_options()).map_err(solve_error)?;
        store(doc, layer, solved.sketch);
    }
    Ok(())
}

/// Removes a layer's sketch and everything that refers to it (layer deletion).
pub fn forget_layer(doc: &mut Document, layer: Id) {
    doc.sketches.remove(&layer);
    let ids: Vec<Id> = doc.objects.values().filter(|o| o.sketch.is_some() && o.layer == layer).map(|o| o.id).collect();
    for id in ids {
        doc.objects.get_mut(&id).expect("listed").sketch = None;
    }
    let dead: Vec<Id> = doc
        .dimensions
        .values()
        .filter(|d| {
            d.layer == layer
                || [d.from, d.to].iter().any(|a| matches!(a, Anchor::SketchPoint { layer: l, .. } if *l == layer))
        })
        .map(|d| d.id)
        .collect();
    for d in dead {
        doc.dimensions.remove(&d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chains_follow_shared_points_in_any_direction() {
        let mut s = Sketch::new();
        let (p, e) = s.add_polyline(
            &[Vec2::new(0.0, 0.0), Vec2::new(1000.0, 0.0), Vec2::new(1000.0, 500.0), Vec2::new(0.0, 500.0)],
            true,
        );
        let (pts, closed) = chain(&s, &e).unwrap();
        assert!(closed);
        assert_eq!(pts.len(), 5);
        assert_eq!(pts[0], pts[4]);
        // Reversed order still chains.
        let rev: Vec<_> = e.iter().rev().copied().collect();
        assert!(chain(&s, &rev).unwrap().1);
        assert_eq!(loop_outline(&s, &e).unwrap().len(), 4);
        // Open chain of two.
        let (pts, closed) = chain(&s, &e[1..3]).unwrap();
        assert!(!closed);
        assert_eq!(pts, vec![[1000.0, 0.0], [1000.0, 500.0], [0.0, 500.0]]);
        // Disjoint edges do not chain.
        assert!(chain(&s, &[e[0], e[2]]).is_none());
        let _ = p;
    }

    #[test]
    fn arcs_are_split_in_chains() {
        let mut s = Sketch::new();
        let c = s.add_point(Vec2::new(0.0, 0.0));
        let a = s.add_point(Vec2::new(1000.0, 0.0));
        let b = s.add_point(Vec2::new(0.0, 1000.0));
        let arc = s.add_arc(c, a, b);
        let (pts, _) = chain(&s, &[arc]).unwrap();
        assert!(pts.len() >= 19);
        for p in &pts {
            assert!(((p[0]).hypot(p[1]) - 1000.0).abs() < 1e-6);
        }
    }
}
