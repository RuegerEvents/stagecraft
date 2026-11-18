//! Mesh files for model objects (glTF/GLB, OBJ, 3DS): reading, units and bounds.
//! The views draw them through `stagecraft-geom`; core only needs the extents.

use glam::{DAffine3, DVec3};
use stagecraft_gdtf::models::{RawMesh, gltf, obj, three_ds};

use crate::model::{ModelPart, P3};

/// File extensions `model.import` takes.
pub const EXTENSIONS: [&str; 4] = ["glb", "gltf", "obj", "3ds"];

/// Reads a mesh file by extension, in its own units; glTF is turned to z-up.
pub fn read(ext: &str, bytes: &[u8]) -> Result<RawMesh, String> {
    match ext.to_ascii_lowercase().as_str() {
        "glb" | "gltf" => gltf::load(bytes),
        "3ds" => three_ds::load(bytes),
        "obj" => obj::load(bytes),
        other => Err(format!("unsupported model format `{other}`; use glb, gltf, obj or 3ds")),
    }
}

/// File units to mm when the file does not say: glTF is metres by definition; 3DS
/// and OBJ are read as mm unless the model is smaller than 50 units across, which
/// only makes sense in metres.
pub fn guess_scale(ext: &str, raw: &RawMesh) -> f64 {
    if matches!(ext.to_ascii_lowercase().as_str(), "glb" | "gltf") {
        return 1000.0;
    }
    let extent = raw.bounds().map_or(0.0, |(lo, hi)| (hi - lo).max_element());
    if extent > 0.0 && extent < 50.0 { 1000.0 } else { 1.0 }
}

/// The part's frame as an affine: matrix · scale · (y-up turn).
pub fn part_affine(p: &ModelPart) -> DAffine3 {
    let mut a = DAffine3::from_cols_array(&p.matrix) * DAffine3::from_scale(DVec3::splat(p.scale));
    if p.y_up {
        // (x, y, z) → (x, −z, y), as for glTF.
        a *= DAffine3::from_cols_array(&[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0]);
    }
    a
}

/// Bounds of a part's mesh in the object's local space, mm.
pub fn part_bounds(raw: &RawMesh, p: &ModelPart) -> Option<[P3; 2]> {
    let a = part_affine(p);
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for q in &raw.positions {
        let w = a.transform_point3(q.as_dvec3());
        lo = lo.min(w);
        hi = hi.max(w);
    }
    (lo.x <= hi.x).then(|| [lo.to_array(), hi.to_array()])
}

pub fn union(a: Option<[P3; 2]>, b: Option<[P3; 2]>) -> Option<[P3; 2]> {
    match (a, b) {
        (Some(a), Some(b)) => Some([
            [a[0][0].min(b[0][0]), a[0][1].min(b[0][1]), a[0][2].min(b[0][2])],
            [a[1][0].max(b[1][0]), a[1][1].max(b[1][1]), a[1][2].max(b[1][2])],
        ]),
        (a, None) => a,
        (None, b) => b,
    }
}
