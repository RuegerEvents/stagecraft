//! Fixtures. With a GDTF type the plan draws its symbol (see [`crate::gdtf`]) and 3D
//! its model; without one a generic body box 260 × 340 mm with a thick lens edge at
//! the front (local +y). The FID sits in the middle either way.

use glam::{DAffine3, DVec3};
use stagecraft_core::model::P2;
use stagecraft_core::{Document, Object, ObjectKind};

use super::to_plan;
use crate::gdtf::LocalPrim;
use crate::{FillClass, LineClass, Lod, MaterialClass, Mesh, PlanPrim, PlanStyle, TextAnchor};

pub const BODY_WIDTH: f64 = 260.0;
pub const BODY_DEPTH: f64 = 340.0;
pub const LABEL_HEIGHT: f64 = 90.0;

/// The plan frame of a fixture: local (x, y) → plan. A fixture tilted so far that its
/// local x/y plane is seen edge-on keeps only its heading, so the symbol never
/// collapses to a line.
pub fn plan_frame(w: &DAffine3) -> impl Fn(P2) -> P2 + use<> {
    let o = to_plan(w, [0.0; 3]);
    let ax = to_plan(w, [1.0, 0.0, 0.0]);
    let ay = to_plan(w, [0.0, 1.0, 0.0]);
    let mut ex = [ax[0] - o[0], ax[1] - o[1]];
    let mut ey = [ay[0] - o[0], ay[1] - o[1]];
    if (ex[0] * ey[1] - ex[1] * ey[0]).abs() < 0.2 {
        let lx = ex[0].hypot(ex[1]);
        if lx > 0.2 {
            ex = [ex[0] / lx, ex[1] / lx];
            ey = [-ex[1], ex[0]];
        } else {
            let ly = ey[0].hypot(ey[1]).max(1e-9);
            ey = [ey[0] / ly, ey[1] / ly];
            ex = [ey[1], -ey[0]];
        }
    }
    move |p: P2| [o[0] + ex[0] * p[0] + ey[0] * p[1], o[1] + ex[1] * p[0] + ey[1] * p[1]]
}

pub fn plan(doc: &Document, w: &DAffine3, obj: &Object, style: &PlanStyle) -> Vec<PlanPrim> {
    let ObjectKind::Fixture { fid, .. } = &obj.kind else { return Vec::new() };
    let frame = plan_frame(w);
    let p = |x: f64, y: f64| frame([x, y]);
    let mut out = Vec::new();
    let mut label_at = [0.0, -20.0];
    match crate::gdtf::symbol(doc, obj) {
        Some(sym)
            if sym.source == crate::gdtf::SymbolSource::Silhouette
                && let Some(parts) = crate::gdtf::outline_points(doc, obj) =>
        {
            // The true top view of the turned and tilted model.
            out.extend(crate::gdtf::world_silhouette(&parts, w));
            let (lo, hi) = sym.bounds;
            label_at = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        }
        Some(sym) => {
            for lp in &sym.prims {
                out.push(match lp {
                    LocalPrim::Polygon { points, fill, line } => PlanPrim::Polygon {
                        points: points.iter().map(|q| frame(*q)).collect(),
                        fill: *fill,
                        line: *line,
                    },
                    LocalPrim::Polyline { points, closed, line } => PlanPrim::Polyline {
                        points: points.iter().map(|q| frame(*q)).collect(),
                        closed: *closed,
                        line: *line,
                    },
                });
            }
            let (lo, hi) = sym.bounds;
            label_at = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
        }
        None => {
            let (hw, hd) = (BODY_WIDTH / 2.0, BODY_DEPTH / 2.0);
            let body = vec![p(-hw, -hd), p(hw, -hd), p(hw, hd), p(-hw, hd)];
            out.push(PlanPrim::Polygon { points: body, fill: FillClass::Body, line: Some(LineClass::Medium) });
            out.push(PlanPrim::line(vec![p(-hw, hd - 40.0), p(hw, hd - 40.0)], LineClass::Thick));
        }
    }
    if style.labels
        && let Some(f) = fid
    {
        out.push(PlanPrim::Text {
            pos: p(label_at[0], label_at[1]),
            text: f.to_string(),
            height: LABEL_HEIGHT,
            rotation: 0.0,
            anchor: TextAnchor::Center,
        });
    }
    out
}

/// Height of the yoke bar under the hanging point.
pub const YOKE_HEIGHT: f64 = 60.0;
/// Height of the body below the yoke.
pub const BODY_HEIGHT: f64 = 250.0;
pub const LENS_DIAMETER: f64 = 180.0;
pub const LENS_LENGTH: f64 = 50.0;

/// The GDTF model (cached per type, mode and level of detail), else a placeholder.
/// GDTF convention: the origin is the hanging point and the fixture hangs down
/// (-z); floor fixtures are turned over (rotation x 180°).
pub fn mesh(doc: &Document, obj: &Object, lod: Lod) -> Mesh {
    match crate::gdtf::fixture_mesh(doc, obj, lod) {
        Some(m) if !m.is_empty() => (*m).clone(),
        _ => placeholder_mesh(),
    }
}

/// A yoke bar, the body box under it, and a lens cylinder on the front face (+y), as
/// in the generic plan symbol.
pub fn placeholder_mesh() -> Mesh {
    let (hw, hd) = (BODY_WIDTH / 2.0, BODY_DEPTH / 2.0);
    let body_top = -YOKE_HEIGHT;
    let body_bottom = body_top - BODY_HEIGHT;
    let mut m = Mesh::empty();
    m.set_material(MaterialClass::Fixture);
    m.cuboid(DVec3::new(-hw * 0.8, -40.0, body_top), DVec3::new(hw * 0.8, 40.0, 0.0));
    m.cuboid(DVec3::new(-hw, -hd, body_bottom), DVec3::new(hw, hd, body_top));
    m.set_material(MaterialClass::Lens);
    let zc = (body_top + body_bottom) / 2.0;
    m.tube(DVec3::new(0.0, hd, zc), DVec3::new(0.0, hd + LENS_LENGTH, zc), LENS_DIAMETER / 2.0, 20);
    m
}
