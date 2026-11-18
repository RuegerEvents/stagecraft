//! One module per [`ObjectKind`]. Adding a kind = a module here and one line in each
//! match below.

pub mod fabric;
pub mod fixture;
pub mod floor;
pub mod model;
pub mod node;
pub mod stage;
pub mod truss;
pub mod wall;

use glam::{DAffine3, DVec3};
use stagecraft_core::model::P2;
use stagecraft_core::{Document, Object, ObjectKind};

use crate::{Detail, LineClass, Lod, Mesh, PlanPrim, PlanStyle};

/// The view style with the object's own overrides applied.
pub fn effective_style(obj: &Object, style: &PlanStyle) -> PlanStyle {
    let mut s = style.clone();
    if let Some(l) = obj.style.labels {
        s.labels = l;
    }
    match obj.style.detail.as_deref() {
        Some("low") => s.detail = Detail::Low,
        Some("high") => s.detail = Detail::High,
        _ => {}
    }
    s
}

/// A line class by its name in [`stagecraft_core::model::PlanOverride`].
pub fn line_class(name: &str) -> Option<LineClass> {
    Some(match name {
        "thin" => LineClass::Thin,
        "medium" => LineClass::Medium,
        "thick" => LineClass::Thick,
        "hidden" => LineClass::Hidden,
        "centre" => LineClass::Centre,
        _ => return None,
    })
}

/// The plan of one object at world transform `w`, before per-object overrides.
pub fn plan_at(doc: &Document, w: &DAffine3, obj: &Object, style: &PlanStyle) -> Vec<PlanPrim> {
    match &obj.kind {
        ObjectKind::Floor { .. } => floor::plan(w, obj, style),
        ObjectKind::Wall { .. } => wall::plan(w, obj, style),
        ObjectKind::Truss { .. } => truss::plan(w, obj, style),
        ObjectKind::Fixture { .. } => fixture::plan(doc, w, obj, style),
        ObjectKind::Proscenium { .. } => stage::proscenium_plan(w, obj, style),
        ObjectKind::Curtain { .. } => stage::curtain_plan(w, obj, style),
        ObjectKind::Masking { .. } => stage::masking_plan(w, obj, style),
        ObjectKind::Batten { .. } => stage::batten_plan(w, obj, style),
        ObjectKind::Platform { .. } => stage::platform_plan(w, obj, style),
        ObjectKind::TrussNode { .. } => node::plan(w, obj, style),
        ObjectKind::Support { .. } => stage::support_plan(w, obj, style),
        ObjectKind::Hoist { .. } => stage::hoist_plan(w, obj, style),
        ObjectKind::Ceiling { .. } => stage::ceiling_plan(w, obj, style),
        ObjectKind::Model { .. } => model::plan(doc, w, obj, style),
        ObjectKind::WingSet { .. } | ObjectKind::TrussRun { .. } | ObjectKind::Group { .. } => Vec::new(),
    }
}

pub fn plan(doc: &Document, obj: &Object, style: &PlanStyle) -> Vec<PlanPrim> {
    if obj.style.hidden {
        return Vec::new();
    }
    let Some(w) = stagecraft_core::geom::world(doc, obj.id) else { return Vec::new() };
    let mut prims = plan_at(doc, &w, obj, &effective_style(obj, style));
    if let Some(l) = obj.style.line.as_deref().and_then(line_class) {
        for p in &mut prims {
            match p {
                PlanPrim::Polyline { line, .. } | PlanPrim::Arc { line, .. } => *line = l,
                PlanPrim::Polygon { line, .. } | PlanPrim::Circle { line, .. } => {
                    if line.is_some() {
                        *line = Some(l);
                    }
                }
                PlanPrim::Text { .. } => {}
            }
        }
    }
    prims
}

/// The object's mesh in its local space. Meshes depend on the kind's parameters and the
/// level of detail only, never on the placement, so views can cache them per object.
pub fn mesh(doc: &Document, obj: &Object, lod: Lod) -> Mesh {
    match &obj.kind {
        ObjectKind::Floor { .. } => floor::mesh(obj, lod),
        ObjectKind::Wall { .. } => wall::mesh(obj, lod),
        ObjectKind::Truss { .. } => truss::mesh(obj, lod),
        ObjectKind::Fixture { .. } => fixture::mesh(doc, obj, lod),
        ObjectKind::Proscenium { .. } => stage::proscenium_mesh(obj),
        ObjectKind::Curtain { .. } | ObjectKind::Masking { .. } => stage::cloth_mesh(obj, lod),
        ObjectKind::Batten { .. } => stage::batten_mesh(obj, lod),
        ObjectKind::Platform { .. } => stage::platform_mesh(obj, lod),
        ObjectKind::TrussNode { .. } => node::mesh(obj, lod),
        ObjectKind::Support { .. } => stage::support_mesh(obj, lod),
        ObjectKind::Hoist { .. } => stage::hoist_mesh(obj, lod),
        ObjectKind::Ceiling { .. } => stage::ceiling_mesh(obj),
        ObjectKind::Model { .. } => model::mesh(doc, obj),
        ObjectKind::WingSet { .. } | ObjectKind::TrussRun { .. } | ObjectKind::Group { .. } => Mesh::empty(),
    }
}

/// The size of the finest detail a level of detail would drop, mm: truss profile
/// width, pleat depth, pipe diameter. `None` for kinds that look the same at every
/// level. Views pick the [`Lod`] from how big this is on screen.
pub fn detail_size(obj: &Object) -> Option<f64> {
    match &obj.kind {
        ObjectKind::Truss { .. } | ObjectKind::TrussNode { .. } => Some(truss::profile_of(obj).width),
        ObjectKind::Curtain { .. } | ObjectKind::Masking { .. } => {
            stage::drape(&obj.kind).map(|d| stage::pleat_depth(&d)).filter(|d| *d > 0.0)
        }
        ObjectKind::Batten { .. } => Some(stage::PIPE * 3.0),
        // GDTF models can be heavy; far away a low-poly model or a box will do.
        ObjectKind::Fixture { gdtf: Some(_), .. } => Some(120.0),
        ObjectKind::Platform { .. } | ObjectKind::Support { .. } | ObjectKind::Hoist { .. } => Some(60.0),
        _ => None,
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

fn closed_ring(pts: Vec<P2>) -> (Vec<P2>, Vec<[P2; 2]>) {
    let n = pts.len();
    let segs = (0..n).map(|i| [pts[i], pts[(i + 1) % n]]).collect();
    (pts, segs)
}

/// Points an object offers for snapping, in world plan mm: (endpoints, segments).
/// Segments are used for midpoints; truss axes also for "along".
pub fn snap_geometry(doc: &Document, obj: &Object) -> (Vec<P2>, Vec<[P2; 2]>) {
    let Some(w) = stagecraft_core::geom::world(doc, obj.id) else { return Default::default() };
    let line = |a: P2, b: P2| (vec![a, b], vec![[a, b]]);
    let p2 = |x: f64, y: f64| to_plan_2d(&w, [x, y]);
    match &obj.kind {
        ObjectKind::Floor { outline, .. } | ObjectKind::Ceiling { outline, .. } => {
            closed_ring(outline.iter().map(|p| to_plan_2d(&w, *p)).collect())
        }
        ObjectKind::Wall { path, .. } => {
            let pts: Vec<P2> = path.iter().map(|p| to_plan_2d(&w, *p)).collect();
            let segs = pts.windows(2).map(|s| [s[0], s[1]]).collect();
            (pts, segs)
        }
        ObjectKind::Truss { length, .. } => line(to_plan(&w, [0.0; 3]), to_plan(&w, [*length, 0.0, 0.0])),
        ObjectKind::Batten { length, .. } => line(p2(-length / 2.0, 0.0), p2(length / 2.0, 0.0)),
        ObjectKind::Curtain { width, .. } | ObjectKind::Masking { width, .. } => {
            line(p2(-width / 2.0, 0.0), p2(width / 2.0, 0.0))
        }
        ObjectKind::Platform { width, depth, .. } => {
            let (hw, hd) = (width / 2.0, depth / 2.0);
            closed_ring(vec![p2(-hw, -hd), p2(hw, -hd), p2(hw, hd), p2(-hw, hd)])
        }
        ObjectKind::Proscenium { width, frame, .. } => {
            let (hw, f) = (width / 2.0, *frame);
            (vec![p2(-hw - f, 0.0), p2(-hw, 0.0), p2(hw, 0.0), p2(hw + f, 0.0)], vec![[p2(-hw, 0.0), p2(hw, 0.0)]])
        }
        ObjectKind::Fixture { .. }
        | ObjectKind::TrussNode { .. }
        | ObjectKind::Support { .. }
        | ObjectKind::Hoist { .. } => (vec![to_plan(&w, [0.0; 3])], Vec::new()),
        ObjectKind::Model { bounds, .. } => {
            let [lo, hi] = bounds;
            closed_ring(vec![p2(lo[0], lo[1]), p2(hi[0], lo[1]), p2(hi[0], hi[1]), p2(lo[0], hi[1])])
        }
        ObjectKind::WingSet { .. } | ObjectKind::TrussRun { .. } | ObjectKind::Group { .. } => Default::default(),
    }
}
