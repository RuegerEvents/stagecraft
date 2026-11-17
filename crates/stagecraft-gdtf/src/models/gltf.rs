//! GLB (glTF 2.0 binary) meshes: positions, normals and indices of every triangle
//! primitive in the default scene, node transforms applied. glTF is y-up; the result
//! is converted to GDTF's z-up frame: (x, y, z)_gltf → (x, −z, y).

use glam::{Mat4, Vec3};

use super::RawMesh;

/// glTF y-up → GDTF z-up.
pub fn y_up_to_z_up() -> Mat4 {
    Mat4::from_cols_array_2d(&[[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, -1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0]])
}

/// Loads a `.glb` (or a self-contained `.gltf` with only the GLB blob as buffer).
/// The returned mesh is in GDTF axes, in the file's units.
pub fn load(bytes: &[u8]) -> Result<RawMesh, String> {
    let g = match gltf::Gltf::from_slice(bytes) {
        Ok(g) => g,
        Err(e) => {
            // Invalid files are read anyway, guarded against panics in the reader.
            let g = gltf::Gltf::from_slice_without_validation(bytes).map_err(|_| e.to_string())?;
            return std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read(&g)))
                .map_err(|_| format!("invalid glTF ({e})"))?;
        }
    };
    read(&g)
}

fn read(g: &gltf::Gltf) -> Result<RawMesh, String> {
    let blob = g.blob.as_deref();
    let mut out = RawMesh::default();
    let mut warnings = 0usize;
    let scene = g.default_scene().or_else(|| g.scenes().next());
    let root = y_up_to_z_up();
    match scene {
        Some(s) => {
            for n in s.nodes() {
                visit(&n, root, blob, &mut out, &mut warnings, 0);
            }
        }
        None => {
            // No scene: take every mesh untransformed.
            for m in g.meshes() {
                add_mesh(&m, root, blob, &mut out, &mut warnings);
            }
        }
    }
    if out.indices.is_empty() {
        return Err(if warnings > 0 {
            "no readable triangles (external or data-URI buffers are not supported)".into()
        } else {
            "no triangles".into()
        });
    }
    Ok(out)
}

fn visit(n: &gltf::Node, parent: Mat4, blob: Option<&[u8]>, out: &mut RawMesh, warnings: &mut usize, depth: usize) {
    if depth > 64 {
        return;
    }
    let local = Mat4::from_cols_array_2d(&n.transform().matrix());
    let world = parent * local;
    if let Some(m) = n.mesh() {
        add_mesh(&m, world, blob, out, warnings);
    }
    for c in n.children() {
        visit(&c, world, blob, out, warnings, depth + 1);
    }
}

fn add_mesh(m: &gltf::Mesh, world: Mat4, blob: Option<&[u8]>, out: &mut RawMesh, warnings: &mut usize) {
    let normal_m = glam::Mat3::from_mat4(world).inverse().transpose();
    let name = m.name().unwrap_or("mesh").to_string();
    for p in m.primitives() {
        let reader = p.reader(|b| match b.source() {
            gltf::buffer::Source::Bin => blob,
            gltf::buffer::Source::Uri(_) => None,
        });
        let Some(pos) = reader.read_positions() else {
            *warnings += 1;
            continue;
        };
        let pos: Vec<Vec3> = pos.map(|p| world.transform_point3(Vec3::from_array(p))).collect();
        if pos.iter().any(|p| !p.is_finite()) {
            *warnings += 1;
            continue;
        }
        let nrm: Option<Vec<Vec3>> =
            reader.read_normals().map(|n| n.map(|n| (normal_m * Vec3::from_array(n)).normalize_or_zero()).collect());
        let idx: Vec<u32> = match reader.read_indices() {
            Some(i) => i.into_u32().collect(),
            None => (0..pos.len() as u32).collect(),
        };
        let tris: Vec<u32> =
            match p.mode() {
                gltf::mesh::Mode::Triangles => idx,
                gltf::mesh::Mode::TriangleStrip => (2..idx.len())
                    .flat_map(|i| {
                        if i % 2 == 0 { [idx[i - 2], idx[i - 1], idx[i]] } else { [idx[i - 1], idx[i - 2], idx[i]] }
                    })
                    .collect(),
                gltf::mesh::Mode::TriangleFan => (2..idx.len()).flat_map(|i| [idx[0], idx[i - 1], idx[i]]).collect(),
                _ => continue, // points and lines carry no surface
            };
        let base = out.positions.len() as u32;
        let n = pos.len() as u32;
        let start = out.indices.len() as u32;
        let has_normals = nrm.as_ref().is_some_and(|v| v.len() == pos.len());
        // Keep normals only if every primitive so far had them.
        match (&mut out.normals, has_normals) {
            (Some(all), true) => all.extend(nrm.unwrap()),
            (None, true) if out.positions.is_empty() => out.normals = nrm,
            _ => out.normals = None,
        }
        out.positions.extend(pos);
        for t in tris.chunks_exact(3) {
            if t.iter().all(|i| *i < n) {
                out.indices.extend(t.iter().map(|i| base + i));
            }
        }
        let count = out.indices.len() as u32 - start;
        if count > 0 {
            out.groups.push((name.clone(), start, count));
        }
    }
}
