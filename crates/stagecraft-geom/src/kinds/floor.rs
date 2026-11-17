//! Floor areas: the outline, filled.

use glam::DAffine3;
use stagecraft_core::{Object, ObjectKind};

use super::to_plan_2d;
use crate::{FillClass, LineClass, Lod, MaterialClass, Mesh, PlanPrim, PlanStyle};

pub fn plan(w: &DAffine3, obj: &Object, _style: &PlanStyle) -> Vec<PlanPrim> {
    let ObjectKind::Floor { outline, .. } = &obj.kind else { return Vec::new() };
    let points = outline.iter().map(|p| to_plan_2d(w, *p)).collect();
    vec![PlanPrim::Polygon { points, fill: FillClass::Floor, line: Some(LineClass::Medium) }]
}

pub fn mesh(_obj: &Object, _lod: Lod) -> Mesh {
    // TODO(block 3): extrude the outline down by the thickness.
    Mesh::empty(MaterialClass::Floor)
}
