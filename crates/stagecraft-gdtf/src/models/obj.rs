//! Wavefront OBJ: vertices and faces (polygons fanned into triangles); normals,
//! texture coordinates, groups and materials are ignored. Axes and units as in the
//! file; the caller decides about y-up and metres.

use glam::Vec3;

use super::RawMesh;

/// Reads an `.obj` file. Errors when nothing triangular is in it.
pub fn load(bytes: &[u8]) -> Result<RawMesh, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = RawMesh::default();
    let mut bad = 0usize;
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let c: Vec<f32> = it.take(3).filter_map(|t| t.parse().ok()).collect();
                if c.len() == 3 && c.iter().all(|x| x.is_finite()) {
                    out.positions.push(Vec3::new(c[0], c[1], c[2]));
                } else {
                    // Keep indices aligned with the file.
                    out.positions.push(Vec3::ZERO);
                    bad += 1;
                }
            }
            Some("f") => {
                let n = out.positions.len() as i64;
                let idx: Vec<Option<u32>> = it
                    .map(|t| {
                        let i: i64 = t.split('/').next()?.parse().ok()?;
                        let i = if i < 0 { n + i } else { i - 1 };
                        (0..n).contains(&i).then_some(i as u32)
                    })
                    .collect();
                if idx.len() < 3 || idx.iter().any(Option::is_none) {
                    bad += 1;
                    continue;
                }
                let idx: Vec<u32> = idx.into_iter().flatten().collect();
                for k in 1..idx.len() - 1 {
                    out.indices.extend([idx[0], idx[k], idx[k + 1]]);
                }
            }
            _ => {}
        }
    }
    if out.indices.is_empty() {
        return Err(if bad > 0 { format!("no readable faces ({bad} bad lines)") } else { "no faces".into() });
    }
    out.groups.push(("obj".into(), 0, out.indices.len() as u32));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quads_and_negative_indices() {
        let src = "# cube side\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvn 0 0 1\nf 1//1 2//1 3//1 4//1\nf -4 -2 -1\n";
        let m = load(src.as_bytes()).unwrap();
        assert_eq!(m.indices, vec![0, 1, 2, 0, 2, 3, 0, 2, 3]);
        assert!(load(b"v 0 0 0\n").is_err());
    }
}
