//! Feature edges: boundaries, creases and silhouettes of a prepared mesh.

use crate::prep::Prepared;
use crate::{LineClass, ObjectId, Options, Seg};

/// A face counts as front-facing when its normal points towards the eye by more than
/// this; edge-on faces count as back-facing, so an edge-on strip gives one silhouette.
const FRONT_EPS: f64 = 1e-9;

pub(crate) fn feature_edges(p: &Prepared, object: ObjectId, opts: &Options) -> Vec<Seg> {
    let cos_crease = opts.crease_angle_deg.to_radians().cos();
    let mut out = Vec::new();
    for e in &p.edges {
        let draw = match e.count {
            1 => true,
            2 => {
                let (f, g) = (e.faces[0] as usize, e.faces[1] as usize);
                let (n1, n2) = (p.normals[f], p.normals[g]);
                let front1 = n1.z > FRONT_EPS;
                let front2 = n2.z > FRONT_EPS;
                if !opts.hidden_lines && p.solid[f] && !front1 && !front2 {
                    // Both faces turn away on a closed solid: always hidden.
                    false
                } else {
                    n1.dot(n2) < cos_crease || front1 != front2
                }
            }
            _ => true,
        };
        if draw {
            out.push(Seg {
                a: p.pos[e.a as usize],
                b: p.pos[e.b as usize],
                object,
                class: LineClass::Visible,
                occluded: true,
            });
        }
    }
    out
}
