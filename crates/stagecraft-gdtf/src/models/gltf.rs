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

/// Buffer contents by index: the GLB blob or a base64 `data:` URI. External files are
/// not read here (callers inline them first).
fn buffers(g: &gltf::Gltf) -> Vec<Option<Vec<u8>>> {
    g.buffers()
        .map(|b| match b.source() {
            gltf::buffer::Source::Bin => g.blob.clone(),
            gltf::buffer::Source::Uri(uri) => data_uri(uri),
        })
        .collect()
}

/// The bytes of a base64 `data:` URI.
pub fn data_uri(uri: &str) -> Option<Vec<u8>> {
    let rest = uri.strip_prefix("data:")?;
    let (head, data) = rest.split_once(',')?;
    if !head.ends_with(";base64") {
        return None;
    }
    base64(data)
}

fn base64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let bytes: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for c in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, &b) in c.iter().enumerate() {
            n |= val(b)? << (18 - 6 * i);
        }
        let got = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&got[..c.len().saturating_sub(1)]);
    }
    Some(out)
}

fn read(g: &gltf::Gltf) -> Result<RawMesh, String> {
    let data = buffers(g);
    let blob = &data;
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

fn visit(
    n: &gltf::Node,
    parent: Mat4,
    blob: &[Option<Vec<u8>>],
    out: &mut RawMesh,
    warnings: &mut usize,
    depth: usize,
) {
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

fn add_mesh(m: &gltf::Mesh, world: Mat4, blob: &[Option<Vec<u8>>], out: &mut RawMesh, warnings: &mut usize) {
    let normal_m = glam::Mat3::from_mat4(world).inverse().transpose();
    let name = m.name().unwrap_or("mesh").to_string();
    for p in m.primitives() {
        let reader = p.reader(|b| blob.get(b.index()).and_then(|d| d.as_deref()));
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
        for t in tris.as_chunks::<3>().0 {
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
