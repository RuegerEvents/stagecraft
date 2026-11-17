//! A small 3DS reader: triangle meshes only (chunks 0x4D4D › 0x3D3D › 0x4000 › 0x4100
//! › 0x4110 vertices, 0x4120 faces). Materials, texture coordinates, smoothing
//! groups and the keyframer are skipped. Vertices in 3DS are stored in world
//! coordinates already, so the object matrix (0x4160) is not applied.

use super::RawMesh;

const MAIN: u16 = 0x4D4D;
const EDITOR: u16 = 0x3D3D;
const OBJECT: u16 = 0x4000;
const TRIMESH: u16 = 0x4100;
const VERTICES: u16 = 0x4110;
const FACES: u16 = 0x4120;

/// Reads all triangle meshes into one [`RawMesh`] (one group per object).
pub fn load(bytes: &[u8]) -> Result<RawMesh, String> {
    if bytes.len() < 6 {
        return Err("file too short".into());
    }
    let (id, len) = header(bytes, 0).ok_or("no header")?;
    if id != MAIN {
        return Err(format!("not a 3DS file (first chunk 0x{id:04X})"));
    }
    let end = (len as usize).min(bytes.len());
    let mut out = RawMesh::default();
    for (id, body) in chunks(bytes, 6, end) {
        if id == EDITOR {
            for (id, obj) in chunks(bytes, body.0, body.1) {
                if id == OBJECT {
                    read_object(bytes, obj, &mut out);
                }
            }
        }
    }
    if out.indices.is_empty() {
        return Err("no triangles".into());
    }
    Ok(out)
}

fn read_object(bytes: &[u8], (start, end): (usize, usize), out: &mut RawMesh) {
    // Name: zero-terminated string.
    let name_end = bytes[start..end].iter().position(|b| *b == 0).map(|p| start + p);
    let Some(name_end) = name_end else { return };
    let name = String::from_utf8_lossy(&bytes[start..name_end]).into_owned();
    for (id, mesh) in chunks(bytes, name_end + 1, end) {
        if id != TRIMESH {
            continue;
        }
        let mut verts: Vec<[f32; 3]> = Vec::new();
        let mut faces: Vec<[u32; 3]> = Vec::new();
        for (id, (s, e)) in chunks(bytes, mesh.0, mesh.1) {
            match id {
                VERTICES => {
                    let Some(n) = u16_at(bytes, s) else { continue };
                    let n = n as usize;
                    if s + 2 + n * 12 > e {
                        continue;
                    }
                    verts = (0..n)
                        .map(|i| {
                            let o = s + 2 + i * 12;
                            [f32_at(bytes, o), f32_at(bytes, o + 4), f32_at(bytes, o + 8)]
                        })
                        .collect();
                }
                FACES => {
                    let Some(n) = u16_at(bytes, s) else { continue };
                    let n = n as usize;
                    if s + 2 + n * 8 > e {
                        continue;
                    }
                    faces = (0..n)
                        .map(|i| {
                            let o = s + 2 + i * 8;
                            [
                                u16_at(bytes, o).unwrap_or(0) as u32,
                                u16_at(bytes, o + 2).unwrap_or(0) as u32,
                                u16_at(bytes, o + 4).unwrap_or(0) as u32,
                            ]
                        })
                        .collect();
                }
                _ => {}
            }
        }
        let base = out.positions.len() as u32;
        let start = out.indices.len() as u32;
        let nv = verts.len() as u32;
        out.positions.extend(verts.iter().map(|v| glam::Vec3::from_array(*v)));
        for f in faces {
            if f.iter().all(|i| *i < nv) && f[0] != f[1] && f[1] != f[2] && f[0] != f[2] {
                out.indices.extend(f.iter().map(|i| base + i));
            }
        }
        let count = out.indices.len() as u32 - start;
        if count > 0 {
            out.groups.push((name.clone(), start, count));
        }
    }
}

fn header(b: &[u8], at: usize) -> Option<(u16, u32)> {
    let id = u16_at(b, at)?;
    let len = u32::from_le_bytes(b.get(at + 2..at + 6)?.try_into().ok()?);
    Some((id, len))
}

/// Sub-chunks in `[start, end)`: (id, (body start, body end)).
fn chunks(b: &[u8], start: usize, end: usize) -> Vec<(u16, (usize, usize))> {
    let mut out = Vec::new();
    let mut at = start;
    let end = end.min(b.len());
    while at + 6 <= end {
        let Some((id, len)) = header(b, at) else { break };
        let len = len as usize;
        if len < 6 || at + len > end {
            break;
        }
        out.push((id, (at + 6, at + len)));
        at += len;
    }
    out
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    let v = b.get(at..at + 4).and_then(|s| s.try_into().ok()).map(f32::from_le_bytes).unwrap_or(0.0);
    if v.is_finite() { v } else { 0.0 }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A 3DS file with one object holding the given triangles.
    pub fn write(name: &str, verts: &[[f32; 3]], faces: &[[u16; 3]]) -> Vec<u8> {
        fn chunk(id: u16, body: &[u8]) -> Vec<u8> {
            let mut v = id.to_le_bytes().to_vec();
            v.extend(((body.len() + 6) as u32).to_le_bytes());
            v.extend(body);
            v
        }
        let mut vb = (verts.len() as u16).to_le_bytes().to_vec();
        for v in verts {
            for c in v {
                vb.extend(c.to_le_bytes());
            }
        }
        let mut fb = (faces.len() as u16).to_le_bytes().to_vec();
        for f in faces {
            for i in f {
                fb.extend(i.to_le_bytes());
            }
            fb.extend(0u16.to_le_bytes());
        }
        let mut tri = chunk(VERTICES, &vb);
        tri.extend(chunk(FACES, &fb));
        let mut obj = name.as_bytes().to_vec();
        obj.push(0);
        obj.extend(chunk(TRIMESH, &tri));
        let editor = chunk(EDITOR, &chunk(OBJECT, &obj));
        chunk(MAIN, &editor)
    }

    #[test]
    fn reads_a_triangle() {
        let b = write("tri", &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 2.0]], &[[0, 1, 2]]);
        let m = load(&b).unwrap();
        assert_eq!(m.positions.len(), 3);
        assert_eq!(m.indices, vec![0, 1, 2]);
        assert_eq!(m.positions[2], glam::Vec3::new(0.0, 1.0, 2.0));
        assert_eq!(m.groups[0].0, "tri");
    }

    #[test]
    fn rejects_garbage_and_truncation() {
        assert!(load(b"hello world").is_err());
        let b = write("t", &[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], &[[0, 1, 2]]);
        for cut in [7, 20, b.len() - 3] {
            let _ = load(&b[..cut]); // must not panic
        }
        // Out-of-range indices are dropped.
        let bad = write("t", &[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], &[[0, 1, 9]]);
        assert!(load(&bad).is_err());
    }
}
