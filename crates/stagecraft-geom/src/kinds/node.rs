//! Truss corners and junctions: a box the size of the profile at the centre and a
//! stub of truss along each arm, one profile width from the centre.

use glam::{DAffine3, DQuat, DVec3};
use stagecraft_core::{Object, ObjectKind};

use super::to_plan_2d;
use super::truss::{mesh_piece, plan_piece, profile_of};
use crate::{LineClass, Lod, Mesh, PlanPrim, PlanStyle};

/// Arm directions as rotations of local +x.
fn arm_rotations(obj: &Object) -> Vec<DQuat> {
    let ObjectKind::TrussNode { arms, down, up, .. } = &obj.kind else { return Vec::new() };
    let mut v: Vec<DQuat> = arms.iter().map(|a| DQuat::from_rotation_z(a.to_radians())).collect();
    // Ry(+90°) turns +x to −z.
    if *down {
        v.push(DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2));
    }
    if *up {
        v.push(DQuat::from_rotation_y(-std::f64::consts::FRAC_PI_2));
    }
    v
}

pub fn plan(w: &DAffine3, obj: &Object, style: &PlanStyle) -> Vec<PlanPrim> {
    let ObjectKind::TrussNode { arms, .. } = &obj.kind else { return Vec::new() };
    let prof = profile_of(obj);
    let h = prof.width / 2.0;
    let reach = prof.node_reach();
    let mut out = vec![PlanPrim::Polyline {
        points: vec![to_plan_2d(w, [-h, -h]), to_plan_2d(w, [h, -h]), to_plan_2d(w, [h, h]), to_plan_2d(w, [-h, h])],
        closed: true,
        line: LineClass::Medium,
    }];
    for a in arms {
        let arm = *w * DAffine3::from_rotation_z(a.to_radians()) * DAffine3::from_translation(DVec3::new(h, 0.0, 0.0));
        out.extend(plan_piece(&arm, reach - h, &prof, style));
    }
    out
}

pub fn mesh(obj: &Object, lod: Lod) -> Mesh {
    let prof = profile_of(obj);
    let piece = mesh_piece(prof.node_reach(), &prof, lod);
    let mut m = Mesh::empty();
    for q in arm_rotations(obj) {
        m.append(&piece.transformed(&DAffine3::from_quat(q)));
    }
    m
}
