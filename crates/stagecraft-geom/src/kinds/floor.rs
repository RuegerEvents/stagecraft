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

/// The outline extruded down by the thickness: the top surface is at local z 0.
pub fn mesh(obj: &Object, _lod: Lod) -> Mesh {
    let ObjectKind::Floor { outline, thickness } = &obj.kind else { return Mesh::empty() };
    let mut m = Mesh::empty();
    m.set_material(MaterialClass::Floor);
    m.extrude(outline, -thickness.max(1.0), 0.0);
    m
}
