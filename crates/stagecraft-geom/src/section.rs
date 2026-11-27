//! Section cuts: a plane through a mesh gives closed polygons in the plane's own 2D
//! coordinates. The 3D view fills them as cut faces; vector sections (block 6) draw
//! them with hatching.
//!
//! Convention: a [`Plane`] hides what lies on the side its normal points to
//! (`signed_distance > 0`). Seen from that hidden side, looking back along `-normal`,
//! plane coordinates run `u` to the right and `v` up.

use std::collections::HashMap;

use glam::{DAffine3, DVec3, Vec3};
use serde::{Deserialize, Serialize};

use crate::Mesh;
use crate::math::{point_in_polygon, signed_area2};

/// A cutting plane in world mm. `normal` points to the hidden side.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
}

impl Plane {
    /// A plane through `origin` with `normal` (normalised here; zero becomes +z).
    pub fn new(origin: [f64; 3], normal: [f64; 3]) -> Self {
        let n = DVec3::from_array(normal).try_normalize().unwrap_or(DVec3::Z);
        Self { origin, normal: n.to_array() }
    }

    pub fn n(&self) -> DVec3 {
        DVec3::from_array(self.normal)
    }

    pub fn o(&self) -> DVec3 {
        DVec3::from_array(self.origin)
    }

    /// Distance of `p` from the plane, positive on the hidden side.
    pub fn signed_distance(&self, p: DVec3) -> f64 {
        (p - self.o()).dot(self.n())
    }

    /// `(u, v)`: `v` is world up projected into the plane (world +y for horizontal
    /// planes), `u = v × n`, so a viewer on the hidden side sees `u` right, `v` up.
    pub fn basis(&self) -> (DVec3, DVec3) {
        let n = self.n();
        let up = if n.z.abs() > 0.999 { DVec3::Y * n.z.signum() } else { DVec3::Z };
        let v = (up - n * up.dot(n)).normalize();
        let u = v.cross(n);
        (u, v)
    }

    /// World point to plane coordinates.
    pub fn to_plane(&self, p: DVec3) -> [f64; 2] {
        let (u, v) = self.basis();
        let d = p - self.o();
        [d.dot(u), d.dot(v)]
    }

    /// Plane coordinates back to a world point on the plane.
    pub fn to_world(&self, q: [f64; 2]) -> DVec3 {
        let (u, v) = self.basis();
        self.o() + u * q[0] + v * q[1]
    }

    /// The same plane facing the other way.
    pub fn flipped(&self) -> Self {
        Self { origin: self.origin, normal: (-self.n()).to_array() }
    }
}

/// One closed cut outline in plane coordinates, without the repeated first point.
pub type Loop = Vec<[f64; 2]>;

/// A filled cut face: an outer loop (counter-clockwise in plane coordinates) and its
/// holes (clockwise).
#[derive(Clone, Debug, PartialEq)]
pub struct CutPolygon {
    pub outer: Loop,
    pub holes: Vec<Loop>,
}

impl CutPolygon {
    /// Area in mm², holes taken off.
    pub fn area(&self) -> f64 {
        let outer = signed_area2(&self.outer).abs() / 2.0;
        outer - self.holes.iter().map(|h| signed_area2(h).abs() / 2.0).sum::<f64>()
    }
}

/// Points closer than this (mm) count as the same when chaining cut segments.
const WELD: f64 = 1e-3;

fn key(p: [f64; 2]) -> (i64, i64) {
    ((p[0] / WELD).round() as i64, (p[1] / WELD).round() as i64)
}

/// The closed loops where `plane` cuts a mesh given in world space (or in any space,
/// with `to_world` mapping it there). Open surfaces give open chains, which are
/// dropped: only closed loops can be filled.
pub fn cut_loops(mesh: &Mesh, to_world: &DAffine3, plane: &Plane) -> Vec<Loop> {
    let n = plane.n();
    let o = plane.o();
    let world: Vec<DVec3> =
        mesh.positions.iter().map(|p| to_world.transform_point3(Vec3::from_array(*p).as_dvec3())).collect();
    // A vertex exactly on the plane counts as on the kept side, so every edge has a
    // clear answer and neighbouring triangles agree.
    let dist: Vec<f64> = world.iter().map(|p| (*p - o).dot(n)).collect();
    let side = |i: usize| dist[i] > 0.0;
    let mut segments: Vec<([f64; 2], [f64; 2])> = Vec::new();
    for t in mesh.indices.as_chunks::<3>().0 {
        let [a, b, c] = [t[0] as usize, t[1] as usize, t[2] as usize];
        let hidden = [side(a), side(b), side(c)];
        let count = hidden.iter().filter(|h| **h).count();
        if count == 0 || count == 3 {
            continue;
        }
        let cross = |i: usize, j: usize| {
            // The same edge seen from either triangle must give the same bits: order
            // its ends by position (faces have their own vertices, not shared indices).
            let (i, j) = if world[i].to_array() < world[j].to_array() { (i, j) } else { (j, i) };
            let s = dist[i] / (dist[i] - dist[j]);
            plane.to_plane(world[i] + (world[j] - world[i]) * s)
        };
        // Walk the edges in winding order; the segment runs from where the boundary
        // enters the hidden side to where it leaves. For outward faces that makes the
        // cut outline counter-clockwise seen from the hidden side.
        let v = [a, b, c];
        let mut enter = None;
        let mut leave = None;
        for k in 0..3 {
            let (i, j) = (v[k], v[(k + 1) % 3]);
            match (side(i), side(j)) {
                (false, true) => enter = Some(cross(i, j)),
                (true, false) => leave = Some(cross(i, j)),
                _ => {}
            }
        }
        if let (Some(p), Some(q)) = (enter, leave)
            && key(p) != key(q)
        {
            segments.push((q, p));
        }
    }
    chain(segments)
}

/// Join segments end to start into closed loops.
fn chain(segments: Vec<([f64; 2], [f64; 2])>) -> Vec<Loop> {
    let mut from: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in segments.iter().enumerate() {
        from.entry(key(s.0)).or_default().push(i);
    }
    let mut used = vec![false; segments.len()];
    let mut loops = Vec::new();
    for start in 0..segments.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut pts = vec![segments[start].0];
        let mut end = segments[start].1;
        let closed = loop {
            if key(end) == key(pts[0]) {
                break true;
            }
            let next = from.get(&key(end)).and_then(|c| c.iter().copied().find(|&i| !used[i]));
            let Some(i) = next else { break false };
            used[i] = true;
            pts.push(segments[i].0);
            end = segments[i].1;
        };
        if closed && pts.len() >= 3 {
            loops.push(simplify(pts));
        }
    }
    loops
}

/// Drop points that lie on the straight line between their neighbours (every
/// triangle edge the plane crosses leaves one).
fn simplify(mut pts: Loop) -> Loop {
    let mut i = 0;
    while pts.len() > 3 && i < pts.len() {
        let n = pts.len();
        let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        let cross = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        let len = crate::math::dist(a, c).max(1e-9);
        if (cross / len).abs() < 1e-6 {
            pts.remove(i);
        } else {
            i += 1;
        }
    }
    pts
}

/// Loops to fillable polygons: a loop inside an odd number of others is a hole of
/// the smallest loop around it. Outers come out counter-clockwise, holes clockwise.
pub fn polygons(loops: Vec<Loop>) -> Vec<CutPolygon> {
    let n = loops.len();
    let area: Vec<f64> = loops.iter().map(|l| signed_area2(l).abs()).collect();
    // For each loop: the loops containing it.
    let inside = |i: usize, j: usize| i != j && area[j] > area[i] && point_in_polygon(&loops[j], loops[i][0]);
    let mut depth = vec![0usize; n];
    let mut parent = vec![None; n];
    for i in 0..n {
        let mut best: Option<usize> = None;
        for j in 0..n {
            if inside(i, j) {
                depth[i] += 1;
                if best.is_none_or(|b| area[j] < area[b]) {
                    best = Some(j);
                }
            }
        }
        parent[i] = best;
    }
    let orient = |mut l: Loop, ccw: bool| {
        if (signed_area2(&l) > 0.0) != ccw {
            l.reverse();
        }
        l
    };
    let mut out: Vec<CutPolygon> = Vec::new();
    let mut index_of = vec![usize::MAX; n];
    for i in (0..n).filter(|i| depth[*i].is_multiple_of(2)) {
        index_of[i] = out.len();
        out.push(CutPolygon { outer: orient(loops[i].clone(), true), holes: Vec::new() });
    }
    for i in (0..n).filter(|i| !depth[*i].is_multiple_of(2)) {
        if let Some(p) = parent[i]
            && index_of[p] != usize::MAX
        {
            out[index_of[p]].holes.push(orient(loops[i].clone(), false));
        }
    }
    out
}

/// The cut faces of a mesh: [`cut_loops`] then [`polygons`].
pub fn cut(mesh: &Mesh, to_world: &DAffine3, plane: &Plane) -> Vec<CutPolygon> {
    polygons(cut_loops(mesh, to_world, plane))
}

/// Triangles of a cut polygon: the points (outer, then each hole) and index triples.
pub fn triangulate(poly: &CutPolygon) -> (Vec<[f64; 2]>, Vec<[usize; 3]>) {
    let mut pts: Vec<[f64; 2]> = poly.outer.clone();
    let mut holes = Vec::with_capacity(poly.holes.len());
    for h in &poly.holes {
        holes.push(pts.len());
        pts.extend_from_slice(h);
    }
    let flat: Vec<f64> = pts.iter().flat_map(|p| [p[0], p[1]]).collect();
    let tris = match earcutr::earcut(&flat, &holes, 2) {
        Ok(idx) => idx.as_chunks::<3>().0.iter().map(|t| [t[0], t[1], t[2]]).collect(),
        Err(_) => Vec::new(),
    };
    // Keep the winding counter-clockwise in plane coordinates.
    let tris = tris
        .into_iter()
        .map(
            |t: [usize; 3]| if signed_area2(&[pts[t[0]], pts[t[1]], pts[t[2]]]) < 0.0 { [t[0], t[2], t[1]] } else { t },
        )
        .collect();
    (pts, tris)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> Mesh {
        let mut m = Mesh::empty();
        m.cuboid(DVec3::ZERO, DVec3::splat(1000.0));
        m
    }

    #[test]
    fn plane_basis_is_right_handed_and_upright() {
        for n in [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0], [1.0, 1.0, 0.3]] {
            let p = Plane::new([0.0; 3], n);
            let (u, v) = p.basis();
            assert!((u.length() - 1.0).abs() < 1e-9 && (v.length() - 1.0).abs() < 1e-9);
            assert!(u.cross(v).distance(p.n()) < 1e-9, "{n:?}");
        }
        // A front section seen from the audience: x to the right, z up.
        let (u, v) = Plane::new([0.0; 3], [0.0, -1.0, 0.0]).basis();
        assert!(u.distance(DVec3::X) < 1e-9 && v.distance(DVec3::Z) < 1e-9);
        let p = Plane::new([1.0, 2.0, 3.0], [0.3, -0.5, 0.8]);
        let w = DVec3::new(10.0, -4.0, 7.0);
        let q = p.to_plane(w);
        let back = p.to_world(q);
        assert!(p.signed_distance(back).abs() < 1e-9);
        assert!((w - back).cross(p.n()).length() < 1e-9);
    }

    #[test]
    fn a_cube_cut_in_the_middle_is_a_square() {
        for n in [[0.0, 0.0, 1.0], [0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]] {
            let plane = Plane::new([500.0, 500.0, 500.0], n);
            let polys = cut(&cube(), &DAffine3::IDENTITY, &plane);
            assert_eq!(polys.len(), 1, "{n:?}");
            let p = &polys[0];
            assert_eq!(p.outer.len(), 4, "{:?}", p.outer);
            assert!((p.area() - 1e6).abs() < 1e-3);
            assert!(signed_area2(&p.outer) > 0.0);
            for q in &p.outer {
                assert!((q[0].abs() - 500.0).abs() < 1e-6 && (q[1].abs() - 500.0).abs() < 1e-6, "{q:?}");
            }
        }
    }

    #[test]
    fn a_rotated_moved_box_cuts_where_it_stands() {
        let t = DAffine3::from_translation(DVec3::new(2000.0, 0.0, 0.0)) * DAffine3::from_rotation_z(0.5);
        let plane = Plane::new([0.0, 0.0, 300.0], [0.0, 0.0, 1.0]);
        let polys = cut(&cube(), &t, &plane);
        assert_eq!(polys.len(), 1);
        assert!((polys[0].area() - 1e6).abs() < 1e-2);
        let c = polys[0].outer.iter().fold([0.0, 0.0], |a, p| [a[0] + p[0] / 4.0, a[1] + p[1] / 4.0]);
        let expect = t.transform_point3(DVec3::new(500.0, 500.0, 0.0));
        assert!((c[0] - expect.x).abs() < 1e-6 && (c[1] - expect.y).abs() < 1e-6, "{c:?}");
    }

    #[test]
    fn misses_and_touching_planes_give_nothing() {
        let m = cube();
        assert!(cut(&m, &DAffine3::IDENTITY, &Plane::new([0.0, 0.0, 2000.0], [0.0, 0.0, 1.0])).is_empty());
        assert!(cut(&m, &DAffine3::IDENTITY, &Plane::new([0.0, 0.0, -1.0], [0.0, 0.0, 1.0])).is_empty());
    }

    #[test]
    fn a_hollow_box_gives_a_polygon_with_a_hole() {
        // Outer box with the inner box's faces turned inside out = a tube with walls.
        let mut m = cube();
        let mut inner = Mesh::empty();
        inner.cuboid(DVec3::new(200.0, 200.0, -10.0), DVec3::new(800.0, 800.0, 1010.0));
        for t in inner.indices.as_chunks_mut::<3>().0 {
            t.swap(1, 2);
        }
        m.append(&inner);
        let polys = cut(&m, &DAffine3::IDENTITY, &Plane::new([0.0, 0.0, 500.0], [0.0, 0.0, 1.0]));
        assert_eq!(polys.len(), 1);
        assert_eq!(polys[0].holes.len(), 1);
        assert!((polys[0].area() - (1e6 - 0.36e6)).abs() < 1e-2);
        let (pts, tris) = triangulate(&polys[0]);
        let area: f64 = tris.iter().map(|t| signed_area2(&[pts[t[0]], pts[t[1]], pts[t[2]]]) / 2.0).sum();
        assert!((area - 0.64e6).abs() < 1e-2, "{area}");
    }
}
