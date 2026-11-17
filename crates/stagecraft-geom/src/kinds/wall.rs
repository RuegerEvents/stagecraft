//! Walls: the centre line thickened to a filled band, cut-wall style.

use glam::{DAffine3, DVec3};
use stagecraft_core::model::P2;
use stagecraft_core::{Object, ObjectKind};

use super::to_plan_2d;
use crate::math::{dist, offset_polyline};
use crate::{FillClass, LineClass, Lod, MaterialClass, Mesh, PlanPrim, PlanStyle};

pub fn plan(w: &DAffine3, obj: &Object, style: &PlanStyle) -> Vec<PlanPrim> {
    let ObjectKind::Wall { path, thickness, .. } = &obj.kind else { return Vec::new() };
    let centre: Vec<_> = path.iter().map(|p| to_plan_2d(w, *p)).collect();
    if centre.len() < 2 {
        return Vec::new();
    }
    let mut band = offset_polyline(&centre, thickness / 2.0);
    band.extend(offset_polyline(&centre, -thickness / 2.0).into_iter().rev());
    let mut out = vec![PlanPrim::Polygon { points: band, fill: FillClass::Wall, line: Some(LineClass::Thick) }];
    if style.centre_lines {
        out.push(PlanPrim::line(centre, LineClass::Centre));
    }
    out
}

/// The band extruded from local z 0 up by the height, with mitred corners. A path
/// whose last point meets its first closes into a ring, mitred at the join too.
pub fn mesh(obj: &Object, _lod: Lod) -> Mesh {
    let ObjectKind::Wall { path, height, thickness } = &obj.kind else { return Mesh::empty() };
    let mut m = Mesh::empty();
    m.set_material(MaterialClass::Wall);
    let mut pts: Vec<P2> = Vec::with_capacity(path.len());
    for p in path {
        if pts.last().is_none_or(|q| dist(*q, *p) > 1e-6) {
            pts.push(*p);
        }
    }
    let closed = pts.len() > 3 && dist(pts[0], pts[pts.len() - 1]) < 1e-6;
    if closed {
        pts.pop();
    }
    if pts.len() < 2 || *height <= 0.0 || *thickness <= 0.0 {
        return m;
    }
    let (left, right) = if closed {
        (offset_ring(&pts, thickness / 2.0), offset_ring(&pts, -thickness / 2.0))
    } else {
        (offset_polyline(&pts, thickness / 2.0), offset_polyline(&pts, -thickness / 2.0))
    };
    let n = pts.len();
    let segs = if closed { n } else { n - 1 };
    let at = |p: P2, z: f64| DVec3::new(p[0], p[1], z);
    let h = *height;
    for i in 0..segs {
        let j = (i + 1) % n;
        let (l0, l1, r0, r1) = (left[i], left[j], right[i], right[j]);
        // Top and bottom of this stretch between the mitre lines.
        m.quad(at(r0, h), at(r1, h), at(l1, h), at(l0, h));
        m.quad(at(r0, 0.0), at(l0, 0.0), at(l1, 0.0), at(r1, 0.0));
        // Left face looks to the left of travel, right face to the right.
        m.quad(at(l1, 0.0), at(l0, 0.0), at(l0, h), at(l1, h));
        m.quad(at(r0, 0.0), at(r1, 0.0), at(r1, h), at(r0, h));
    }
    if !closed {
        let (l, r) = (left[0], right[0]);
        m.quad(at(l, 0.0), at(r, 0.0), at(r, h), at(l, h));
        let (l, r) = (left[n - 1], right[n - 1]);
        m.quad(at(r, 0.0), at(l, 0.0), at(l, h), at(r, h));
    }
    m
}

/// [`offset_polyline`] for a closed ring: every corner is mitred.
fn offset_ring(pts: &[P2], d: f64) -> Vec<P2> {
    let n = pts.len();
    // Wrap one point on each side, offset, and keep the middle.
    let mut wrapped = Vec::with_capacity(n + 2);
    wrapped.push(pts[n - 1]);
    wrapped.extend_from_slice(pts);
    wrapped.push(pts[0]);
    let off = offset_polyline(&wrapped, d);
    off[1..=n].to_vec()
}
