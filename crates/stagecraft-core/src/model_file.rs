//! Mesh files for model objects: reading, units and bounds. OBJ, STL, PLY, glTF/GLB, FBX and
//! 3MF are read with craft-mesh (bounded, no file opened), 3DS with the GDTF crate's reader.
//! The views draw them through `stagecraft-geom`; core only needs the extents.

use glam::{DAffine3, DMat3, DMat4, DVec3, Vec3};
use stagecraft_gdtf::models::{RawMesh, gltf, three_ds};

use crate::model::{ModelPart, P3};

/// File extensions `model.import` takes.
pub const EXTENSIONS: [&str; 8] = ["glb", "gltf", "obj", "stl", "ply", "fbx", "3mf", "3ds"];

/// What a model file holds: the mesh in its own units (glTF and FBX already turned to z-up),
/// the file's own unit in mm when it says, and what the reader left out.
pub struct Read {
    pub mesh: RawMesh,
    pub unit_mm: Option<f64>,
    pub warnings: Vec<String>,
}

/// What a model file may hold. Same order as the craft-mesh defaults, with fewer triangles: a
/// stage model past 10 million triangles is not drawable anyway.
fn limits() -> craft_mesh::Limits {
    craft_mesh::Limits { max_vertices: 10_000_000, max_triangles: 10_000_000, ..craft_mesh::Limits::default() }
}

/// Reads a mesh file by extension, in its own units; glTF and FBX are turned to z-up.
pub fn read(ext: &str, bytes: &[u8]) -> Result<RawMesh, String> {
    read_file(ext, bytes).map(|r| r.mesh)
}

/// [`read`] with the file's unit and the reader's warnings.
pub fn read_file(ext: &str, bytes: &[u8]) -> Result<Read, String> {
    use craft_mesh::{fbx, gltf as cgltf, no_external, obj, ply, stl, threemf};
    let lim = limits();
    let e = |e: craft_mesh::Error| e.to_string();
    let z_up = gltf::y_up_to_z_up().as_dmat4();
    let (scene, turn, unit_mm) = match ext.to_ascii_lowercase().as_str() {
        "3ds" => return Ok(Read { mesh: three_ds::load(bytes)?, unit_mm: None, warnings: Vec::new() }),
        "glb" | "gltf" => (cgltf::read(bytes, &lim, &mut no_external).map_err(e)?, z_up, Some(1000.0)),
        "obj" => (obj::read(bytes, &lim, &mut no_external).map_err(e)?, DMat4::IDENTITY, None),
        "stl" => (stl::read(bytes, &lim).map_err(e)?, DMat4::IDENTITY, None),
        "ply" => (ply::read(bytes, &lim).map_err(e)?, DMat4::IDENTITY, None),
        "3mf" => {
            let m = threemf::read_model(bytes, &lim).map_err(e)?;
            (m.scene, DMat4::IDENTITY, Some(m.unit.metres() * 1000.0))
        }
        "fbx" => {
            let doc = fbx::read_document(bytes, &lim).map_err(e)?;
            let axes = DMat4::from_cols_array(&doc.axes_to_y_up());
            // FBX units are centimetres unless the file says otherwise.
            let unit = doc.unit_scale().unwrap_or(1.0) * 10.0;
            (doc.scene(&lim, &mut no_external).map_err(e)?, z_up * axes, Some(unit))
        }
        other => return Err(format!("unsupported model format `{other}`; use {}", EXTENSIONS.join(", "))),
    };
    let mesh = from_scene(&scene, turn, &lim)?;
    if mesh.indices.is_empty() {
        return Err("the file has no triangles".into());
    }
    Ok(Read { mesh, unit_mm, warnings: scene.warnings.clone() })
}

/// Every placed mesh of a craft-mesh scene as one triangle list, through `turn`.
fn from_scene(scene: &craft_mesh::Scene, turn: DMat4, lim: &craft_mesh::Limits) -> Result<RawMesh, String> {
    let mut out = RawMesh::default();
    let mut normals: Option<Vec<Vec3>> = Some(Vec::new());
    for (index, world) in scene.instances(lim).map_err(|e| e.to_string())? {
        let m = turn * DMat4::from_cols_array(&world);
        let nm = DMat3::from_mat4(m).inverse().transpose();
        let flip = m.determinant() < 0.0;
        let u = scene.meshes[index].unified();
        let base = u32::try_from(out.positions.len()).map_err(|_| "too many vertices".to_string())?;
        out.positions.extend(u.positions.iter().map(|p| m.transform_point3(DVec3::from_array(*p)).as_vec3()));
        match (&mut normals, u.normals.len() == u.positions.len()) {
            (Some(ns), true) => {
                ns.extend(u.normals.iter().map(|n| (nm * DVec3::from_array(*n)).normalize_or_zero().as_vec3()))
            }
            _ => normals = None,
        }
        for t in u.parts.iter().flatten() {
            let t = if flip { [t[0], t[2], t[1]] } else { *t };
            out.indices.extend(t.map(|i| base + i));
        }
    }
    if !out.indices.is_empty() {
        out.groups.push(("model".into(), 0, out.indices.len() as u32));
    }
    out.normals = normals.filter(|n| n.len() == out.positions.len());
    Ok(out)
}

/// File units to mm when the file does not say: glTF is metres by definition, FBX and 3MF say;
/// OBJ, STL, PLY and 3DS are read as mm unless the model is smaller than 50 units across, which
/// only makes sense in metres.
pub fn guess_scale(read: &Read) -> f64 {
    if let Some(u) = read.unit_mm {
        return u;
    }
    let extent = read.mesh.bounds().map_or(0.0, |(lo, hi)| (hi - lo).max_element());
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
