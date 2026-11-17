//! Walls: the centre line thickened to a filled band, cut-wall style.

use glam::DAffine3;
use stagecraft_core::{Object, ObjectKind};

use super::to_plan_2d;
use crate::math::offset_polyline;
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

pub fn mesh(_obj: &Object, _lod: Lod) -> Mesh {
    // TODO(block 3): extrude the band up by the height.
    Mesh::empty(MaterialClass::Wall)
}
