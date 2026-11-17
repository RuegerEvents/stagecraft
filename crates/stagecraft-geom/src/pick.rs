//! Selection: bounds, hit tests and box selection on plan primitives.

use stagecraft_core::model::P2;
use stagecraft_core::{Document, Id, Object, ObjectKind};

use crate::kinds::truss::profile_of;
use crate::{Bounds, PlanPrim, PlanStyle, math, plan};

/// The box around a set of primitives.
pub fn bounds(prims: &[PlanPrim]) -> Bounds {
    prims.iter().fold(Bounds::EMPTY, |b, p| b.union(&p.extent()))
}

pub fn object_bounds(doc: &Document, obj: &Object, style: &PlanStyle) -> Bounds {
    bounds(&plan(doc, obj, style))
}

/// The box around several objects; empty if none has geometry.
pub fn objects_bounds(doc: &Document, ids: impl IntoIterator<Item = Id>, style: &PlanStyle) -> Bounds {
    ids.into_iter()
        .filter_map(|id| doc.objects.get(&id))
        .fold(Bounds::EMPTY, |b, o| b.union(&object_bounds(doc, o, style)))
}

/// Which object wins when several are under the pointer: small things on top.
pub fn rank(kind: &ObjectKind) -> u8 {
    match kind {
        ObjectKind::Fixture { .. } => 0,
        ObjectKind::Truss { .. } => 1,
        ObjectKind::Wall { .. } => 2,
        ObjectKind::Floor { .. } => 3,
    }
}

/// Distance from `p` to the object's plan in mm; 0 inside filled areas.
pub fn distance(prims: &[PlanPrim], p: P2) -> f64 {
    prims.iter().map(|q| q.distance(p)).fold(f64::INFINITY, f64::min)
}

/// The object under `p` among `ids`, within `tolerance` mm. Rank first (fixtures over
/// truss over walls over floors), then distance.
pub fn pick(doc: &Document, ids: impl IntoIterator<Item = Id>, p: P2, tolerance: f64, style: &PlanStyle) -> Option<Id> {
    let mut best: Option<(u8, f64, Id)> = None;
    for id in ids {
        let Some(o) = doc.objects.get(&id) else { continue };
        let prims = plan(doc, o, style);
        if bounds(&prims).distance(p) > tolerance {
            continue;
        }
        let d = distance(&prims, p);
        if d > tolerance {
            continue;
        }
        let key = (rank(&o.kind), d, id);
        if best.is_none_or(|b| (key.0, key.1) < (b.0, b.1)) {
            best = Some(key);
        }
    }
    best.map(|b| b.2)
}

/// How a selection box decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxMode {
    /// Only objects entirely inside (dragged left to right).
    Window,
    /// Everything the box touches (dragged right to left).
    Crossing,
}

pub fn in_box(
    doc: &Document,
    ids: impl IntoIterator<Item = Id>,
    rect: Bounds,
    mode: BoxMode,
    style: &PlanStyle,
) -> Vec<Id> {
    ids.into_iter()
        .filter(|id| {
            let Some(o) = doc.objects.get(id) else { return false };
            let prims = plan(doc, o, style);
            let b = bounds(&prims);
            match mode {
                BoxMode::Window => rect.contains_bounds(&b),
                BoxMode::Crossing => rect.intersects(&b) && touches(&prims, &rect),
            }
        })
        .collect()
}

/// Whether any primitive comes inside the rectangle (by its points, or the rectangle's
/// corners being inside a filled area, or an edge crossing it).
fn touches(prims: &[PlanPrim], rect: &Bounds) -> bool {
    let corners = [rect.min, [rect.max[0], rect.min[1]], rect.max, [rect.min[0], rect.max[1]]];
    let edges_hit = |points: &[P2], closed: bool| {
        let n = points.len();
        let segs = if closed { n } else { n.saturating_sub(1) };
        (0..segs).any(|i| segment_hits_rect(points[i], points[(i + 1) % n], rect))
    };
    prims.iter().any(|q| {
        if corners.iter().any(|c| q.distance(*c) <= 0.0) {
            return true;
        }
        match q {
            PlanPrim::Polyline { points, closed, .. } => edges_hit(points, *closed),
            PlanPrim::Polygon { points, .. } => edges_hit(points, true),
            _ => rect.intersects(&q.extent()),
        }
    })
}

fn segment_hits_rect(a: P2, b: P2, r: &Bounds) -> bool {
    // Sample-free test: clip with Liang–Barsky.
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let d = math::sub(b, a);
    for (p, q) in [(-d[0], a[0] - r.min[0]), (d[0], r.max[0] - a[0]), (-d[1], a[1] - r.min[1]), (d[1], r.max[1] - a[1])]
    {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// A truss piece under the pointer, for hanging things on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrussHit {
    pub id: Id,
    /// mm along the piece from its start (local x), clamped to the piece.
    pub along: f64,
    /// The point on the truss axis in world plan mm.
    pub point: P2,
    /// Profile height, mm, so a fixture can hang under the bottom chord.
    pub height: f64,
}

/// The nearest truss whose band (profile width plus `tolerance`) contains `p`.
pub fn truss_at(doc: &Document, ids: impl IntoIterator<Item = Id>, p: P2, tolerance: f64) -> Option<TrussHit> {
    let mut best: Option<(f64, TrussHit)> = None;
    for id in ids {
        let Some(o) = doc.objects.get(&id) else { continue };
        let ObjectKind::Truss { length, .. } = &o.kind else { continue };
        let Some(w) = stagecraft_core::geom::world(doc, id) else { continue };
        let a = crate::kinds::to_plan(&w, [0.0; 3]);
        let b = crate::kinds::to_plan(&w, [*length, 0.0, 0.0]);
        let t = math::project_t(a, b, p);
        let on = math::lerp(a, b, t);
        let d = math::dist(on, p);
        let prof = profile_of(o);
        if d <= prof.width / 2.0 + tolerance && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, TrussHit { id, along: t * length, point: on, height: prof.height }));
        }
    }
    best.map(|b| b.1)
}
