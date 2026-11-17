//! World transforms through the attachment tree.

use glam::{DAffine3, DQuat, DVec3, EulerRot};

use crate::model::{Document, Id, Placement};

pub fn placement_to_affine(p: &Placement) -> DAffine3 {
    let [rx, ry, rz] = p.rotation;
    let q = DQuat::from_euler(EulerRot::ZYX, rz.to_radians(), ry.to_radians(), rx.to_radians());
    DAffine3::from_rotation_translation(q, DVec3::from_array(p.position))
}

pub fn affine_to_placement(a: &DAffine3) -> Placement {
    let (_, q, t) = a.to_scale_rotation_translation();
    let (rz, ry, rx) = q.to_euler(EulerRot::ZYX);
    Placement { position: clean3(t.to_array()), rotation: clean3([rx.to_degrees(), ry.to_degrees(), rz.to_degrees()]) }
}

/// Rounds away float noise below a micrometre / micro-degree, so JSON stays readable.
fn clean3(v: [f64; 3]) -> [f64; 3] {
    v.map(|x| {
        let r = (x * 1e6).round() / 1e6;
        if r == 0.0 { 0.0 } else { r }
    })
}

/// The object's transform in document space: parents' placements, then the root's
/// layer elevation. `None` if the object does not exist.
pub fn world(doc: &Document, id: Id) -> Option<DAffine3> {
    let mut obj = doc.objects.get(&id)?;
    let mut acc = placement_to_affine(&obj.placement);
    // Guard against a corrupt file with a cycle.
    for _ in 0..=doc.objects.len() {
        match obj.parent.and_then(|p| doc.objects.get(&p)) {
            Some(parent) => {
                acc = placement_to_affine(&parent.placement) * acc;
                obj = parent;
            }
            None => {
                let elev = doc.layers.get(&obj.layer).map_or(0.0, |l| l.elevation);
                return Some(DAffine3::from_translation(DVec3::new(0.0, 0.0, elev)) * acc);
            }
        }
    }
    None
}

pub fn world_placement(doc: &Document, id: Id) -> Option<Placement> {
    world(doc, id).map(|a| affine_to_placement(&a))
}

/// The local placement that puts `id` at `target_world` under `parent` (or as a root on
/// `layer`).
pub fn local_for_world(doc: &Document, target_world: &DAffine3, parent: Option<Id>, layer: Id) -> Placement {
    let parent_world = match parent {
        Some(p) => world(doc, p).unwrap_or(DAffine3::IDENTITY),
        None => {
            let elev = doc.layers.get(&layer).map_or(0.0, |l| l.elevation);
            DAffine3::from_translation(DVec3::new(0.0, 0.0, elev))
        }
    };
    affine_to_placement(&(parent_world.inverse() * *target_world))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn euler_round_trips() {
        let p = Placement { position: [1.0, 2.0, 3.0], rotation: [10.0, -20.0, 30.0] };
        let back = affine_to_placement(&placement_to_affine(&p));
        assert_eq!(back, p);
    }

    #[test]
    fn rotation_order_is_z_after_x() {
        // Tilt 90° about x, then pan 90° about z: local +y ends up along +z, +x along +y.
        let p = Placement { position: [0.0; 3], rotation: [90.0, 0.0, 90.0] };
        let v = placement_to_affine(&p).transform_vector3(DVec3::Y);
        assert!((v - DVec3::Z).length() < 1e-9, "{v}");
        let v = placement_to_affine(&p).transform_vector3(DVec3::X);
        assert!((v - DVec3::Y).length() < 1e-9, "{v}");
    }
}
