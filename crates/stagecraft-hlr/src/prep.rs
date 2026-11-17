//! Mesh preparation: weld vertices, drop degenerate triangles, repair T-junctions,
//! orient triangles consistently and build the edge table.

use std::collections::HashMap;

use glam::DVec3;

/// An undirected edge `a < b` with up to two adjacent triangles.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EdgeRec {
    pub a: u32,
    pub b: u32,
    pub faces: [u32; 2],
    /// Number of adjacent triangles (1 = boundary, 2 = manifold, more = non-manifold).
    pub count: u32,
}

/// A mesh ready for edge extraction, in view space.
pub(crate) struct Prepared {
    pub pos: Vec<DVec3>,
    pub tris: Vec<[u32; 3]>,
    pub normals: Vec<DVec3>,
    pub edges: Vec<EdgeRec>,
    /// Per triangle: its connected component is closed and consistently oriented, so
    /// back faces can never be seen.
    pub solid: Vec<bool>,
}

impl Prepared {
    /// Triangles that can hide something: back faces of solid components are skipped.
    pub fn occluders(&self) -> impl Iterator<Item = [DVec3; 3]> + '_ {
        self.tris.iter().enumerate().filter_map(move |(i, t)| {
            if self.solid[i] && self.normals[i].z <= 0.0 {
                return None;
            }
            Some([self.pos[t[0] as usize], self.pos[t[1] as usize], self.pos[t[2] as usize]])
        })
    }
}

pub(crate) fn prepare(points: &[DVec3], triangles: &[[u32; 3]], tol: f64) -> Prepared {
    let (pos, remap) = weld(points, tol);
    let mut tris: Vec<[u32; 3]> = triangles
        .iter()
        .filter(|t| t.iter().all(|&i| (i as usize) < points.len()))
        .map(|t| [remap[t[0] as usize], remap[t[1] as usize], remap[t[2] as usize]])
        .filter(|t| !degenerate(&pos, *t, tol))
        .collect();
    let mut edges = edge_table(&tris);
    if edges.iter().any(|e| e.count == 1) && repair_t_junctions(&pos, &mut tris, &edges, tol) {
        edges = edge_table(&tris);
    }
    finish(pos, tris, edges)
}

fn finish(pos: Vec<DVec3>, mut tris: Vec<[u32; 3]>, edges: Vec<EdgeRec>) -> Prepared {
    // Orientation changes the winding, not the adjacency; the edge table stays valid.
    let solid = orient(&pos, &mut tris, &edges);
    let normals = tris
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| pos[i as usize]);
            (b - a).cross(c - a).normalize_or_zero()
        })
        .collect();
    Prepared { pos, tris, normals, edges, solid }
}

/// Merge points closer than `tol`. Returns the unique points and, per input point, its
/// index among them. Grid cells are `2·tol` wide, so a neighbour within `tol` lies in
/// the own cell or the adjacent one on the near side of each axis: 8 lookups.
fn weld(points: &[DVec3], tol: f64) -> (Vec<DVec3>, Vec<u32>) {
    let tol = tol.max(1e-9);
    let cell = 2.0 * tol;
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::with_capacity(points.len());
    let mut out: Vec<DVec3> = Vec::with_capacity(points.len());
    let mut remap = Vec::with_capacity(points.len());
    let tol2 = tol * tol;
    for p in points {
        let f = *p / cell;
        let base = [f.x.floor() as i64, f.y.floor() as i64, f.z.floor() as i64];
        let step = [
            if f.x - f.x.floor() < 0.5 { -1 } else { 1 },
            if f.y - f.y.floor() < 0.5 { -1 } else { 1 },
            if f.z - f.z.floor() < 0.5 { -1 } else { 1 },
        ];
        let mut found = None;
        'search: for k in 0..8 {
            let key = [
                base[0] + if k & 1 != 0 { step[0] } else { 0 },
                base[1] + if k & 2 != 0 { step[1] } else { 0 },
                base[2] + if k & 4 != 0 { step[2] } else { 0 },
            ];
            if let Some(list) = grid.get(&key) {
                for &i in list {
                    if out[i as usize].distance_squared(*p) <= tol2 {
                        found = Some(i);
                        break 'search;
                    }
                }
            }
        }
        let idx = match found {
            Some(i) => i,
            None => {
                let i = out.len() as u32;
                out.push(*p);
                grid.entry(base).or_default().push(i);
                i
            }
        };
        remap.push(idx);
    }
    (out, remap)
}

/// Repeated indices, or a sliver thinner than `tol` (its height over the longest edge).
fn degenerate(pos: &[DVec3], t: [u32; 3], tol: f64) -> bool {
    if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
        return true;
    }
    let [a, b, c] = t.map(|i| pos[i as usize]);
    let longest = (b - a).length().max((c - b).length()).max((a - c).length());
    let area2 = (b - a).cross(c - a).length();
    area2 <= longest * tol * 0.1
}

pub(crate) fn edge_table(tris: &[[u32; 3]]) -> Vec<EdgeRec> {
    let mut map: HashMap<(u32, u32), u32> = HashMap::with_capacity(tris.len() * 3 / 2);
    let mut edges: Vec<EdgeRec> = Vec::with_capacity(tris.len() * 3 / 2);
    for (f, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (i, j) = (t[k], t[(k + 1) % 3]);
            let key = (i.min(j), i.max(j));
            match map.get(&key) {
                Some(&e) => {
                    let e = &mut edges[e as usize];
                    if e.count < 2 {
                        e.faces[e.count as usize] = f as u32;
                    }
                    e.count += 1;
                }
                None => {
                    map.insert(key, edges.len() as u32);
                    edges.push(EdgeRec { a: key.0, b: key.1, faces: [f as u32, u32::MAX], count: 1 });
                }
            }
        }
    }
    edges
}

/// Split triangles whose boundary edge has another boundary vertex lying on it (a
/// T-junction), so both sides share edges and flat faces show no false boundary lines.
/// Returns whether anything changed.
fn repair_t_junctions(pos: &[DVec3], tris: &mut Vec<[u32; 3]>, edges: &[EdgeRec], tol: f64) -> bool {
    let boundary: Vec<&EdgeRec> = edges.iter().filter(|e| e.count == 1).collect();
    let mut verts: Vec<u32> = boundary.iter().flat_map(|e| [e.a, e.b]).collect();
    verts.sort_unstable();
    verts.dedup();
    if verts.len() < 3 {
        return false;
    }
    let mean_len =
        boundary.iter().map(|e| pos[e.a as usize].distance(pos[e.b as usize])).sum::<f64>() / boundary.len() as f64;
    let cell = mean_len.max(tol * 4.0);
    let key = |p: DVec3| [(p.x / cell).floor() as i64, (p.y / cell).floor() as i64, (p.z / cell).floor() as i64];
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    for &v in &verts {
        grid.entry(key(pos[v as usize])).or_default().push(v);
    }
    let boundary_keys: std::collections::HashSet<(u32, u32)> = boundary.iter().map(|e| (e.a, e.b)).collect();

    // Boundary vertices strictly inside the segment a–b, sorted from a.
    let interior = |a: u32, b: u32| -> Vec<u32> {
        let (pa, pb) = (pos[a as usize], pos[b as usize]);
        let d = pb - pa;
        let len2 = d.length_squared();
        if len2 <= tol * tol {
            return Vec::new();
        }
        let lo = key(pa.min(pb) - DVec3::splat(tol));
        let hi = key(pa.max(pb) + DVec3::splat(tol));
        let ncells = ((hi[0] - lo[0] + 1) * (hi[1] - lo[1] + 1) * (hi[2] - lo[2] + 1)) as usize;
        let mut hits: Vec<(f64, u32)> = Vec::new();
        let mut test = |v: u32| {
            if v == a || v == b {
                return;
            }
            let p = pos[v as usize];
            let t = (p - pa).dot(d) / len2;
            if t <= 0.0 || t >= 1.0 || t * t * len2 <= tol * tol || (1.0 - t) * (1.0 - t) * len2 <= tol * tol {
                return;
            }
            if (pa + d * t).distance_squared(p) <= tol * tol {
                hits.push((t, v));
            }
        };
        if ncells > verts.len() {
            verts.iter().for_each(|&v| test(v));
        } else {
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    for z in lo[2]..=hi[2] {
                        if let Some(l) = grid.get(&[x, y, z]) {
                            l.iter().for_each(|&v| test(v));
                        }
                    }
                }
            }
        }
        hits.sort_by(|p, q| p.0.total_cmp(&q.0));
        hits.into_iter().map(|h| h.1).collect()
    };

    let mut changed = false;
    let mut queue: Vec<[u32; 3]> = std::mem::take(tris);
    let mut out = Vec::with_capacity(queue.len());
    let mut guard = queue.len() * 8 + 1024;
    while let Some(t) = queue.pop() {
        guard = guard.saturating_sub(1);
        let mut split = None;
        if guard > 0 {
            for k in 0..3 {
                let (i, j) = (t[k], t[(k + 1) % 3]);
                if !boundary_keys.contains(&(i.min(j), i.max(j))) {
                    continue;
                }
                let mids = interior(i, j);
                if !mids.is_empty() {
                    split = Some((k, mids));
                    break;
                }
            }
        }
        match split {
            None => out.push(t),
            Some((k, mids)) => {
                changed = true;
                let (i, j, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
                let mut chain = vec![i];
                chain.extend(mids);
                chain.push(j);
                for w in chain.windows(2) {
                    // The other two edges of the original stay intact in the first and
                    // last pieces and are checked again.
                    queue.push([w[0], w[1], c]);
                }
            }
        }
    }
    *tris = out;
    changed
}

/// Ray parity test: is `p` inside the closed shell made of triangles `comp`? The ray
/// direction is skewed to avoid hitting edges of axis-aligned geometry.
fn inside(pos: &[DVec3], tris: &[[u32; 3]], comp: &[usize], p: DVec3) -> bool {
    let dir = DVec3::new(0.5773, 0.5779, 0.5767).normalize();
    let mut hits = 0;
    for &f in comp {
        let [a, b, c] = tris[f].map(|i| pos[i as usize]);
        let (e1, e2) = (b - a, c - a);
        let h = dir.cross(e2);
        let det = e1.dot(h);
        if det.abs() < 1e-12 {
            continue;
        }
        let s = p - a;
        let u = s.dot(h) / det;
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = s.cross(e1);
        let v = dir.dot(q) / det;
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        if e2.dot(q) / det > 1e-9 {
            hits += 1;
        }
    }
    hits % 2 == 1
}

/// Make the winding consistent across manifold edges, per connected component. Closed
/// shells get outward normals (inward when nested in an odd number of other shells);
/// open sheets keep the winding most of their triangles had. Returns per triangle
/// whether its component is a closed shell.
fn orient(pos: &[DVec3], tris: &mut [[u32; 3]], edges: &[EdgeRec]) -> Vec<bool> {
    let n = tris.len();
    // Triangle adjacency through edges (3 slots per triangle, edge index).
    let mut tri_edges = vec![[u32::MAX; 3]; n];
    let mut fill = vec![0u8; n];
    for (ei, e) in edges.iter().enumerate() {
        for s in 0..(e.count.min(2) as usize) {
            let f = e.faces[s] as usize;
            if (fill[f] as usize) < 3 {
                tri_edges[f][fill[f] as usize] = ei as u32;
                fill[f] += 1;
            }
        }
    }
    let has_dir = |t: &[u32; 3], a: u32, b: u32| (0..3).any(|k| t[k] == a && t[(k + 1) % 3] == b);

    let mut flip = vec![false; n];
    let mut seen = vec![false; n];
    let mut solid = vec![false; n];
    let mut stack = Vec::new();
    let mut comps: Vec<(Vec<usize>, bool)> = Vec::new();
    for seed in 0..n {
        if seen[seed] {
            continue;
        }
        seen[seed] = true;
        stack.push(seed);
        let mut comp = Vec::new();
        let mut closed = true;
        while let Some(f) = stack.pop() {
            comp.push(f);
            for &ei in &tri_edges[f] {
                if ei == u32::MAX {
                    continue;
                }
                let e = &edges[ei as usize];
                if e.count != 2 {
                    closed = false;
                    continue;
                }
                let g = if e.faces[0] as usize == f { e.faces[1] } else { e.faces[0] } as usize;
                // Same direction in both triangles = opposite orientation.
                let same = has_dir(&tris[f], e.a, e.b) == has_dir(&tris[g], e.a, e.b);
                let want = flip[f] ^ same;
                if !seen[g] {
                    seen[g] = true;
                    flip[g] = want;
                    stack.push(g);
                } else if flip[g] != want {
                    closed = false; // not orientable
                }
            }
        }
        comps.push((comp, closed));
    }
    for (t, f) in tris.iter_mut().zip(&flip) {
        if *f {
            t.swap(1, 2);
        }
    }
    let first_flip = std::mem::replace(&mut flip, vec![false; n]);

    // Closed shells: outward normals, unless nested inside an odd number of other
    // shells (the inner wall of a hollow solid faces the cavity).
    let closed: Vec<usize> = (0..comps.len()).filter(|&c| comps[c].1).collect();
    let bbox = |c: usize| {
        let mut lo = DVec3::splat(f64::INFINITY);
        let mut hi = DVec3::splat(f64::NEG_INFINITY);
        for &f in &comps[c].0 {
            for &i in &tris[f] {
                lo = lo.min(pos[i as usize]);
                hi = hi.max(pos[i as usize]);
            }
        }
        (lo, hi)
    };
    let boxes: Vec<(DVec3, DVec3)> = closed.iter().map(|&c| bbox(c)).collect();
    for (k, &c) in closed.iter().enumerate() {
        let probe = pos[tris[comps[c].0[0]][0] as usize];
        let depth = closed
            .iter()
            .enumerate()
            .filter(|&(j, &d)| {
                j != k
                    && boxes[j].0.cmple(boxes[k].0).all()
                    && boxes[j].1.cmpge(boxes[k].1).all()
                    && inside(pos, tris, &comps[d].0, probe)
            })
            .count();
        let vol: f64 = comps[c]
            .0
            .iter()
            .map(|&f| {
                let [a, b, cc] = tris[f].map(|i| pos[i as usize]);
                a.dot(b.cross(cc))
            })
            .sum();
        if (vol < 0.0) == (depth % 2 == 0) {
            comps[c].0.iter().for_each(|&f| flip[f] = true);
        }
    }
    // Open sheets keep the winding most of their triangles had.
    for (comp, closed) in &comps {
        if !closed {
            let originally_flipped = comp.iter().filter(|&&f| first_flip[f]).count();
            if originally_flipped * 2 > comp.len() {
                comp.iter().for_each(|&f| flip[f] = true);
            }
        }
        for &f in comp {
            solid[f] = *closed;
        }
    }
    for (t, f) in tris.iter_mut().zip(&flip) {
        if *f {
            t.swap(1, 2);
        }
    }
    solid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weld_merges_close_points() {
        let pts = [DVec3::ZERO, DVec3::new(0.004, 0.0, 0.0), DVec3::new(0.011, 0.0, 0.0), DVec3::new(0.0, 0.0099, 0.0)];
        let (out, remap) = weld(&pts, 0.01);
        assert_eq!(remap, vec![0, 0, 1, 0]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn t_junction_is_repaired() {
        // A square: left half one triangle pair, right side split at the midpoint of
        // the shared edge. Without repair the shared edge shows as boundary.
        let pos = vec![
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(1.0, 2.0, 0.0),
            DVec3::new(0.0, 2.0, 0.0),
            DVec3::new(1.0, 1.0, 0.0),
            DVec3::new(2.0, 0.0, 0.0),
            DVec3::new(2.0, 2.0, 0.0),
        ];
        let tris = vec![[0, 1, 2], [0, 2, 3], [1, 5, 4], [5, 6, 4], [4, 6, 2]];
        let p = prepare(&pos, &tris, 0.001);
        let boundary = p.edges.iter().filter(|e| e.count == 1).count();
        assert_eq!(boundary, 6, "only the outline stays boundary");
    }

    #[test]
    fn orient_fixes_flipped_triangle() {
        let pos = vec![DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y];
        let tris = vec![[0, 1, 2], [0, 3, 2]];
        let p = prepare(&pos, &tris, 0.001);
        assert!(p.normals[0].dot(p.normals[1]) > 0.99);
    }
}
