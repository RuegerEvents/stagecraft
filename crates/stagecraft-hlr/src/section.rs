//! Section plane in view space: clipping, cut loops, cut regions and their caps.
//!
//! A vertex exactly on the plane counts as kept, so every edge has a clear answer and
//! neighbouring triangles agree (the same rule as `stagecraft-geom`'s section cut).

use std::collections::HashMap;

use glam::{DVec2, DVec3};

use crate::prep::Prepared;
use crate::{Seg, area};

pub(crate) struct ViewPlane {
    pub origin: DVec3,
    /// Points to the removed side.
    pub normal: DVec3,
}

pub(crate) struct CutRegion {
    pub outer: Vec<DVec2>,
    pub holes: Vec<Vec<DVec2>>,
    /// Triangulated cut face in view space; occludes what lies behind it.
    pub caps: Vec<[DVec3; 3]>,
}

pub(crate) struct Cut {
    /// Cut outlines in view space. Closed loops repeat their first point at the end.
    pub lines: Vec<Vec<DVec3>>,
    pub regions: Vec<CutRegion>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    V(u32),
    E(u32, u32),
}

impl ViewPlane {
    fn dist(&self, p: DVec3) -> f64 {
        (p - self.origin).dot(self.normal)
    }

    fn cross_point(&self, p: DVec3, q: DVec3, dp: f64, dq: f64) -> DVec3 {
        p + (q - p) * (dp / (dp - dq))
    }

    pub fn clip_seg(&self, mut s: Seg) -> Option<Seg> {
        let (da, db) = (self.dist(s.a), self.dist(s.b));
        match (da > 0.0, db > 0.0) {
            (false, false) => Some(s),
            (true, true) => None,
            (true, false) => {
                s.a = self.cross_point(s.a, s.b, da, db);
                Some(s)
            }
            (false, true) => {
                s.b = self.cross_point(s.a, s.b, da, db);
                Some(s)
            }
        }
    }

    /// The kept part of a triangle as up to two triangles.
    pub fn clip_tri(&self, t: [DVec3; 3]) -> impl Iterator<Item = [DVec3; 3]> {
        let d = t.map(|p| self.dist(p));
        let mut poly: [DVec3; 4] = [DVec3::ZERO; 4];
        let mut n = 0;
        for k in 0..3 {
            let (i, j) = (k, (k + 1) % 3);
            if d[i] <= 0.0 {
                poly[n] = t[i];
                n += 1;
            }
            if (d[i] > 0.0) != (d[j] > 0.0) {
                poly[n] = self.cross_point(t[i], t[j], d[i], d[j]);
                n += 1;
            }
        }
        let first = (n >= 3).then(|| [poly[0], poly[1], poly[2]]);
        let second = (n == 4).then(|| [poly[0], poly[2], poly[3]]);
        [first, second].into_iter().flatten()
    }

    /// An orthonormal basis of the plane.
    fn basis(&self) -> (DVec3, DVec3) {
        let n = self.normal;
        let a = if n.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
        let u = n.cross(a).normalize();
        (u, n.cross(u))
    }
}

pub(crate) fn cut(p: &Prepared, plane: &ViewPlane, tol: f64) -> Cut {
    let dist: Vec<f64> = p.pos.iter().map(|&q| plane.dist(q)).collect();
    let removed = |i: u32| dist[i as usize] > 0.0;
    let mut points: HashMap<Key, DVec3> = HashMap::new();
    let mut point = |i: u32, j: u32| -> Key {
        // i kept, j removed.
        if dist[i as usize] == 0.0 {
            points.entry(Key::V(i)).or_insert(p.pos[i as usize]);
            return Key::V(i);
        }
        let key = Key::E(i.min(j), i.max(j));
        points.entry(key).or_insert_with(|| {
            plane.cross_point(p.pos[i as usize], p.pos[j as usize], dist[i as usize], dist[j as usize])
        });
        key
    };
    let mut segs: Vec<(Key, Key)> = Vec::new();
    for t in &p.tris {
        let gone = t.map(removed);
        let count = gone.iter().filter(|g| **g).count();
        if count == 0 || count == 3 {
            continue;
        }
        let mut enter = None;
        let mut leave = None;
        for k in 0..3 {
            let (i, j) = (t[k], t[(k + 1) % 3]);
            match (removed(i), removed(j)) {
                (false, true) => enter = Some(point(i, j)),
                (true, false) => leave = Some(point(j, i)),
                _ => {}
            }
        }
        if let (Some(a), Some(b)) = (enter, leave)
            && a != b
        {
            segs.push((b, a));
        }
    }

    // Chain segments: open chains first (from starts without incoming), then cycles.
    let mut from: HashMap<Key, Vec<usize>> = HashMap::new();
    let mut incoming: HashMap<Key, usize> = HashMap::new();
    for (i, (a, b)) in segs.iter().enumerate() {
        from.entry(*a).or_default().push(i);
        *incoming.entry(*b).or_default() += 1;
    }
    let mut used = vec![false; segs.len()];
    let mut walk = |start: usize, used: &mut Vec<bool>| -> (Vec<Key>, bool) {
        let mut keys = vec![segs[start].0];
        let mut cur = start;
        loop {
            used[cur] = true;
            let to = segs[cur].1;
            keys.push(to);
            if to == keys[0] {
                return (keys, true);
            }
            let next = from.get_mut(&to).and_then(|l| {
                let pos = l.iter().position(|&s| !used[s])?;
                Some(l.swap_remove(pos))
            });
            match next {
                Some(n) => cur = n,
                None => return (keys, false),
            }
        }
    };
    let mut chains: Vec<(Vec<Key>, bool)> = Vec::new();
    for i in 0..segs.len() {
        if !used[i] && !incoming.contains_key(&segs[i].0) {
            chains.push(walk(i, &mut used));
        }
    }
    for i in 0..segs.len() {
        if !used[i] {
            chains.push(walk(i, &mut used));
        }
    }

    let mut lines = Vec::new();
    let mut loops: Vec<Vec<DVec3>> = Vec::new();
    for (keys, closed) in chains {
        let pts: Vec<DVec3> = keys.iter().map(|k| points[k]).collect();
        if closed && pts.len() >= 4 {
            loops.push(pts[..pts.len() - 1].to_vec());
        }
        lines.push(pts);
    }
    let regions = if plane.normal.z.abs() > 1e-6 { regions(&loops, plane, tol) } else { Vec::new() };
    Cut { lines, regions }
}

fn point_in_polygon(p: DVec2, poly: &[DVec2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Group loops into outers with holes by nesting depth and triangulate them.
fn regions(loops: &[Vec<DVec3>], plane: &ViewPlane, tol: f64) -> Vec<CutRegion> {
    let (u, v) = plane.basis();
    let flat: Vec<Vec<DVec2>> =
        loops.iter().map(|l| l.iter().map(|p| DVec2::new(p.dot(u), p.dot(v))).collect()).collect();
    let areas: Vec<f64> = flat.iter().map(|l| area(l).abs()).collect();
    let n = loops.len();
    let contains = |outer: usize, inner: usize| {
        outer != inner && areas[outer] > areas[inner] && point_in_polygon(flat[inner][0], &flat[outer])
    };
    let depth: Vec<usize> = (0..n).map(|i| (0..n).filter(|&j| contains(j, i)).count()).collect();
    let mut holes_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        if depth[i] % 2 == 1
            && let Some(parent) = (0..n)
                .filter(|&j| depth[j] + 1 == depth[i] && contains(j, i))
                .min_by(|&a, &b| areas[a].total_cmp(&areas[b]))
        {
            holes_of[parent].push(i);
        }
    }
    let min_area = tol * tol;
    let mut out = Vec::new();
    for i in (0..n).filter(|&i| depth[i].is_multiple_of(2) && areas[i] > min_area) {
        let to_view = |l: &Vec<DVec3>, ccw: bool| {
            let mut pts: Vec<DVec2> = l.iter().map(|p| p.truncate()).collect();
            if (area(&pts) > 0.0) != ccw {
                pts.reverse();
            }
            pts
        };
        let outer = to_view(&loops[i], true);
        let holes: Vec<Vec<DVec2>> =
            holes_of[i].iter().filter(|&&h| areas[h] > min_area).map(|&h| to_view(&loops[h], false)).collect();

        // Cap triangles, triangulated in plane coordinates.
        let mut coords: Vec<f64> = Vec::new();
        let mut verts: Vec<DVec3> = Vec::new();
        let mut hole_idx = Vec::new();
        for (k, &l) in std::iter::once(&i).chain(holes_of[i].iter()).enumerate() {
            if k > 0 {
                hole_idx.push(verts.len());
            }
            for (p3, p2) in loops[l].iter().zip(&flat[l]) {
                coords.extend([p2.x, p2.y]);
                verts.push(*p3);
            }
        }
        let caps = earcutr::earcut(&coords, &hole_idx, 2)
            .map(|idx| idx.as_chunks::<3>().0.iter().map(|c| [verts[c[0]], verts[c[1]], verts[c[2]]]).collect())
            .unwrap_or_default();
        out.push(CutRegion { outer, holes, caps });
    }
    out
}
