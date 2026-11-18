//! A minimal glTF 2.0 binary writer for MVR geometry: one mesh, one node, positions,
//! normals and 32-bit indices, no materials. Input in MVR space (mm, z up); the file
//! is in glTF space (metres, y up): (x, y, z) → (x, z, −y) / 1000. Deterministic.

/// Writes a `.glb`. `normals` may be empty. Indices must be triangles into `positions`.
pub fn write_glb(positions: &[[f32; 3]], normals: &[[f32; 3]], indices: &[u32]) -> Vec<u8> {
    let to_gltf = |p: [f32; 3], k: f32| [p[0] * k, p[2] * k, -p[1] * k];
    let pos: Vec<[f32; 3]> = positions.iter().map(|p| to_gltf(*p, 0.001)).collect();
    let nrm: Vec<[f32; 3]> =
        if normals.len() == positions.len() { normals.iter().map(|n| to_gltf(*n, 1.0)).collect() } else { Vec::new() };
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for p in &pos {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    if pos.is_empty() {
        lo = [0.0; 3];
        hi = [0.0; 3];
    }
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut push = |bytes: Vec<u8>, target: u32| {
        let offset = bin.len();
        bin.extend_from_slice(&bytes);
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        views.push(format!(
            "{{\"buffer\":0,\"byteOffset\":{offset},\"byteLength\":{},\"target\":{target}}}",
            bytes.len()
        ));
    };
    let f32s = |v: &[[f32; 3]]| v.iter().flat_map(|p| p.iter().flat_map(|x| x.to_le_bytes())).collect::<Vec<u8>>();
    push(f32s(&pos), 34962);
    if !nrm.is_empty() {
        push(f32s(&nrm), 34962);
    }
    push(indices.iter().flat_map(|i| i.to_le_bytes()).collect(), 34963);
    let num = |x: f32| {
        let s = format!("{x}");
        if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") { s } else { format!("{s}.0") }
    };
    let vec3 = |v: [f32; 3]| format!("[{},{},{}]", num(v[0]), num(v[1]), num(v[2]));
    let mut accessors = vec![format!(
        "{{\"bufferView\":0,\"componentType\":5126,\"count\":{},\"type\":\"VEC3\",\"min\":{},\"max\":{}}}",
        pos.len(),
        vec3(lo),
        vec3(hi)
    )];
    let mut attributes = "\"POSITION\":0".to_string();
    if !nrm.is_empty() {
        accessors
            .push(format!("{{\"bufferView\":1,\"componentType\":5126,\"count\":{},\"type\":\"VEC3\"}}", nrm.len()));
        attributes.push_str(",\"NORMAL\":1");
    }
    let idx_view = views.len() - 1;
    accessors.push(format!(
        "{{\"bufferView\":{idx_view},\"componentType\":5125,\"count\":{},\"type\":\"SCALAR\"}}",
        indices.len()
    ));
    let idx_acc = accessors.len() - 1;
    let json = format!(
        concat!(
            "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"StageCraft\"}},\"scene\":0,\"scenes\":[{{\"nodes\":[0]}}],",
            "\"nodes\":[{{\"mesh\":0}}],\"meshes\":[{{\"primitives\":[{{\"attributes\":{{{}}},\"indices\":{},\"mode\":4}}]}}],",
            "\"accessors\":[{}],\"bufferViews\":[{}],\"buffers\":[{{\"byteLength\":{}}}]}}"
        ),
        attributes,
        idx_acc,
        accessors.join(","),
        views.join(","),
        bin.len()
    );
    let mut json = json.into_bytes();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_chunks_are_aligned() {
        let g = write_glb(&[[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [0.0, 1000.0, 0.0]], &[], &[0, 1, 2]);
        assert_eq!(&g[0..4], b"glTF");
        assert_eq!(u32::from_le_bytes(g[8..12].try_into().unwrap()) as usize, g.len());
        let json_len = u32::from_le_bytes(g[12..16].try_into().unwrap()) as usize;
        assert_eq!(json_len % 4, 0);
        let json = std::str::from_utf8(&g[20..20 + json_len]).unwrap();
        // y (upstage) becomes −z in glTF, metres.
        assert!(json.contains("\"min\":[0.0,0.0,-1.0]"), "{json}");
        assert_eq!(
            write_glb(&[[1.0, 2.0, 3.0]; 3], &[], &[0, 1, 2]),
            write_glb(&[[1.0, 2.0, 3.0]; 3], &[], &[0, 1, 2])
        );
    }
}
