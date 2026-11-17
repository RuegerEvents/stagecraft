//! One module per [`ObjectKind`]. Adding a kind = a module here and one line in each
//! match below.

pub mod fixture;
pub mod floor;
pub mod truss;
pub mod wall;

use glam::{DAffine3, DVec3};
use stagecraft_core::model::P2;
use stagecraft_core::{Document, Object, ObjectKind};

use crate::{Lod, Mesh, PlanPrim, PlanStyle};

pub fn plan(doc: &Document, obj: &Object, style: &PlanStyle) -> Vec<PlanPrim> {
    let Some(w) = stagecraft_core::geom::world(doc, obj.id) else { return Vec::new() };
    match &obj.kind {
        ObjectKind::Floor { .. } => floor::plan(&w, obj, style),
        ObjectKind::Wall { .. } => wall::plan(&w, obj, style),
        ObjectKind::Truss { .. } => truss::plan(&w, obj, style),
        ObjectKind::Fixture { .. } => fixture::plan(&w, obj, style),
    }
}

pub fn mesh(_doc: &Document, obj: &Object, lod: Lod) -> Mesh {
    match &obj.kind {
        ObjectKind::Floor { .. } => floor::mesh(obj, lod),
        ObjectKind::Wall { .. } => wall::mesh(obj, lod),
        ObjectKind::Truss { .. } => truss::mesh(obj, lod),
        ObjectKind::Fixture { .. } => fixture::mesh(obj, lod),
    }
}

/// A local point (mm) through the world transform, seen from above.
pub(crate) fn to_plan(w: &DAffine3, local: [f64; 3]) -> P2 {
    let p = w.transform_point3(DVec3::from_array(local));
    [p.x, p.y]
}

pub(crate) fn to_plan_2d(w: &DAffine3, local: P2) -> P2 {
    to_plan(w, [local[0], local[1], 0.0])
}

/// Points an object offers for snapping, in world plan mm: (endpoints, segments).
/// Segments are used for midpoints; truss axes also for "along".
pub fn snap_geometry(doc: &Document, obj: &Object) -> (Vec<P2>, Vec<[P2; 2]>) {
    let Some(w) = stagecraft_core::geom::world(doc, obj.id) else { return Default::default() };
    match &obj.kind {
        ObjectKind::Floor { outline, .. } => {
            let pts: Vec<P2> = outline.iter().map(|p| to_plan_2d(&w, *p)).collect();
            let n = pts.len();
            let segs = (0..n).map(|i| [pts[i], pts[(i + 1) % n]]).collect();
            (pts, segs)
        }
        ObjectKind::Wall { path, .. } => {
            let pts: Vec<P2> = path.iter().map(|p| to_plan_2d(&w, *p)).collect();
            let segs = pts.windows(2).map(|s| [s[0], s[1]]).collect();
            (pts, segs)
        }
        ObjectKind::Truss { length, .. } => {
            let a = to_plan(&w, [0.0; 3]);
            let b = to_plan(&w, [*length, 0.0, 0.0]);
            (vec![a, b], vec![[a, b]])
        }
        ObjectKind::Fixture { .. } => (vec![to_plan(&w, [0.0; 3])], Vec::new()),
    }
}
