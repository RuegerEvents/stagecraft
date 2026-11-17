//! Intersection lines: where surfaces of different objects or of separate parts of one
//! mesh penetrate each other (a truss diagonal welded into a chord, a clamp around a
//! tube), the meeting curve is a crease of the combined surface and is drawn.

use std::collections::HashMap;

use glam::DVec3;

use crate::prep::Prepared;
use crate::{LineClass, ObjectId, Seg};

struct Tri {
    p: [DVec3; 3],
    n: DVec3,
    /// Bounding box in the frame given by `frame` (world axes): axis-aligned stage
    /// geometry keeps tight boxes whatever the view direction.
    lo: DVec3,
    hi: DVec3,
}

/// A connected component: the unit that can intersect another.
struct Comp {
    tris: Vec<Tri>,
    object: ObjectId,
    lo: DVec3,
    hi: DVec3,
}

/// Intersection segments between triangles of different meshes or different connected
/// components, in view space. `frame` maps a view-space point to the frame used for
/// bounding boxes (world axes).
pub(crate) fn intersection_lines(
    meshes: &[(&Prepared, ObjectId)],
    frame: impl Fn(DVec3) -> DVec3,
    tol: f64,
) -> Vec<Seg> {
    let mut comps: Vec<Comp> = Vec::new();
    for (m, object) in meshes {
        let base = comps.len();
        let count = m.comp.iter().map(|&c| c as usize + 1).max().unwrap_or(0);
        comps.extend((0..count).map(|_| Comp {
            tris: Vec::new(),
            object: *object,
            lo: DVec3::splat(f64::INFINITY),
            hi: DVec3::splat(f64::NEG_INFINITY),
        }));
        for (f, t) in m.tris.iter().enumerate() {
            let p = t.map(|i| m.pos[i as usize]);
            let w = p.map(&frame);
            let (lo, hi) = (w[0].min(w[1]).min(w[2]), w[0].max(w[1]).max(w[2]));
            let c = &mut comps[base + m.comp[f] as usize];
            c.lo = c.lo.min(lo);
            c.hi = c.hi.max(hi);
            c.tris.push(Tri { p, n: m.normals[f], lo, hi });
        }
    }
    comps.retain(|c| !c.tris.is_empty());
    if comps.len() < 2 {
        return Vec::new();
    }
    let eps = (tol * 0.01).max(1e-9);
    let pad = DVec3::splat(eps);

    // Sweep and prune over x for component pairs with overlapping boxes.
    let mut order: Vec<usize> = (0..comps.len()).collect();
    order.sort_by(|&a, &b| comps[a].lo.x.total_cmp(&comps[b].lo.x));
    let mut active: Vec<usize> = Vec::new();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for &i in &order {
        let c = &comps[i];
        active.retain(|&j| comps[j].hi.x + eps >= c.lo.x);
        for &j in &active {
            let d = &comps[j];
            if c.lo.y <= d.hi.y + eps && d.lo.y <= c.hi.y + eps && c.lo.z <= d.hi.z + eps && d.lo.z <= c.hi.z + eps {
                pairs.push((j.min(i), j.max(i)));
            }
        }
        active.push(i);
    }

    let run = |&(i, j): &(usize, usize)| -> Vec<Seg> {
        let (a, b) = (&comps[i], &comps[j]);
        let (lo, hi) = (a.lo.max(b.lo) - pad, a.hi.min(b.hi) + pad);
        let inside = |t: &&Tri| t.lo.cmple(hi).all() && t.hi.cmpge(lo).all();
        let ta: Vec<&Tri> = a.tris.iter().filter(inside).collect();
        let tb: Vec<&Tri> = b.tris.iter().filter(inside).collect();
        let mut out = Vec::new();
        let mut test = |x: &Tri, y: &Tri| {
            if x.lo.cmple(y.hi + pad).all()
                && y.lo.cmple(x.hi + pad).all()
                && let Some((p, q)) = tri_tri(x, y, eps)
                && p.distance(q) > tol
            {
                out.push(Seg { a: p, b: q, object: a.object, class: LineClass::Visible, occluded: true, weak: true });
            }
        };
        if ta.len() * tb.len() <= 4096 {
            for x in &ta {
                for y in &tb {
                    test(x, y);
                }
            }
            return out;
        }
        // Many candidates: hash the second set into a grid of its median size.
        let (small, large) = if ta.len() < tb.len() { (&ta, &tb) } else { (&tb, &ta) };
        let mut sizes: Vec<f64> = large.iter().map(|t| (t.hi - t.lo).max_element()).collect();
        let mid = sizes.len() / 2;
        sizes.select_nth_unstable_by(mid, |p, q| p.total_cmp(q));
        let cell = sizes[mid].max(tol * 10.0).max(1e-3);
        let key = |p: DVec3| [(p.x / cell).floor() as i64, (p.y / cell).floor() as i64, (p.z / cell).floor() as i64];
        let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
        for (k, t) in large.iter().enumerate() {
            let (l, h) = (key(t.lo.max(lo)), key(t.hi.min(hi)));
            for x in l[0]..=h[0] {
                for y in l[1]..=h[1] {
                    for z in l[2]..=h[2] {
                        grid.entry([x, y, z]).or_default().push(k as u32);
                    }
                }
            }
        }
        let mut stamp = vec![usize::MAX; large.len()];
        for (s, x) in small.iter().enumerate() {
            let (l, h) = (key(x.lo.max(lo)), key(x.hi.min(hi)));
            for gx in l[0]..=h[0] {
                for gy in l[1]..=h[1] {
                    for gz in l[2]..=h[2] {
                        let Some(list) = grid.get(&[gx, gy, gz]) else { continue };
                        for &k in list {
                            if stamp[k as usize] != s {
                                stamp[k as usize] = s;
                                test(x, large[k as usize]);
                            }
                        }
                    }
                }
            }
        }
        out
    };

    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;
        pairs.par_iter().flat_map_iter(run).collect()
    }
    #[cfg(not(feature = "parallel"))]
    {
        pairs.iter().flat_map(run).collect()
    }
}

/// Where `t` crosses the plane of a triangle with normal `n` through `o`: the interval
/// of crossing points along `dir`, with the points. `None` unless `t` has vertices
/// strictly on both sides.
fn crossing(t: &[DVec3; 3], n: DVec3, o: DVec3, dir: DVec3, eps: f64) -> Option<[(f64, DVec3); 2]> {
    let d = t.map(|p| (p - o).dot(n));
    let (min, max) = (d[0].min(d[1]).min(d[2]), d[0].max(d[1]).max(d[2]));
    if !(min < -eps && max > eps) {
        return None;
    }
    let mut pts: Vec<DVec3> = Vec::with_capacity(3);
    for k in 0..3 {
        let (i, j) = (k, (k + 1) % 3);
        if d[i].abs() <= eps {
            pts.push(t[i]);
        } else if (d[i] > eps && d[j] < -eps) || (d[i] < -eps && d[j] > eps) {
            pts.push(t[i] + (t[j] - t[i]) * (d[i] / (d[i] - d[j])));
        }
    }
    let mut lo = (f64::INFINITY, DVec3::ZERO);
    let mut hi = (f64::NEG_INFINITY, DVec3::ZERO);
    for p in pts {
        let s = p.dot(dir);
        if s < lo.0 {
            lo = (s, p);
        }
        if s > hi.0 {
            hi = (s, p);
        }
    }
    (lo.0 <= hi.0).then_some([lo, hi])
}

fn tri_tri(a: &Tri, b: &Tri, eps: f64) -> Option<(DVec3, DVec3)> {
    let dir = a.n.cross(b.n);
    if dir.length_squared() < 1e-12 {
        return None; // parallel or coplanar
    }
    let dir = dir.normalize();
    let [a0, a1] = crossing(&a.p, b.n, b.p[0], dir, eps)?;
    let [b0, b1] = crossing(&b.p, a.n, a.p[0], dir, eps)?;
    let (s0, s1) = (a0.0.max(b0.0), a1.0.min(b1.0));
    if s1 - s0 <= eps {
        return None;
    }
    // Both crossing segments lie on the planes' common line; take points from `a`'s.
    let at = |s: f64| {
        if (a1.0 - a0.0).abs() < 1e-12 { a0.1 } else { a0.1 + (a1.1 - a0.1) * ((s - a0.0) / (a1.0 - a0.0)) }
    };
    Some((at(s0), at(s1)))
}
