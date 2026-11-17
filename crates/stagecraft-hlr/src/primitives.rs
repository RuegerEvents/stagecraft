//! Small mesh builders for tests, demos and placeholders (world mm).

use glam::DVec3;

use crate::{Material, Mesh, ObjectId};

impl Mesh {
    /// An empty mesh for `object` with `material`.
    pub fn new(object: ObjectId, material: Material) -> Self {
        Self { object, material, positions: Vec::new(), triangles: Vec::new() }
    }

    /// Append another mesh's triangles (its object and material are ignored).
    pub fn append(&mut self, other: &Mesh) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.triangles.extend(other.triangles.iter().map(|t| t.map(|i| i + base)));
    }

    /// The same triangles with three own vertices each, as many file formats store
    /// flat-shaded meshes. Useful to test welding.
    pub fn unshared(&self) -> Mesh {
        let mut m = Mesh::new(self.object, self.material);
        for t in &self.triangles {
            let base = m.positions.len() as u32;
            m.positions.extend(t.map(|i| self.positions[i as usize]));
            m.triangles.push([base, base + 1, base + 2]);
        }
        m
    }

    /// Reverse the winding of every triangle.
    pub fn flipped(mut self) -> Mesh {
        for t in &mut self.triangles {
            t.swap(1, 2);
        }
        self
    }
}

/// An axis-aligned box from `min` to `max`, outward winding.
pub fn cuboid(object: ObjectId, material: Material, min: DVec3, max: DVec3) -> Mesh {
    let p = |x: bool, y: bool, z: bool| {
        DVec3::new(if x { max.x } else { min.x }, if y { max.y } else { min.y }, if z { max.z } else { min.z })
    };
    let positions = (0..8).map(|i| p(i & 1 != 0, i & 2 != 0, i & 4 != 0)).collect();
    // Quads as (a, b, c, d) counter-clockwise seen from outside.
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    let triangles = quads.iter().flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]]).collect();
    Mesh { object, material, positions, triangles }
}

/// Two unit vectors perpendicular to `axis` and each other.
fn perpendiculars(axis: DVec3) -> (DVec3, DVec3) {
    let a = axis.normalize();
    let helper = if a.z.abs() < 0.9 { DVec3::Z } else { DVec3::X };
    let u = a.cross(helper).normalize();
    (u, a.cross(u))
}

/// A closed prism from `a` to `b` whose cross-section is `profile` (2D points in the
/// plane perpendicular to the axis, any winding, may be concave).
pub fn prism(object: ObjectId, material: Material, a: DVec3, b: DVec3, profile: &[[f64; 2]]) -> Mesh {
    let (u, v) = perpendiculars(b - a);
    let n = profile.len() as u32;
    let mut m = Mesh::new(object, material);
    for end in [a, b] {
        for p in profile {
            m.positions.push(end + u * p[0] + v * p[1]);
        }
    }
    for i in 0..n {
        let j = (i + 1) % n;
        m.triangles.push([i, j, n + j]);
        m.triangles.push([i, n + j, n + i]);
    }
    let flat: Vec<f64> = profile.iter().flat_map(|p| [p[0], p[1]]).collect();
    for t in earcutr::earcut(&flat, &[], 2).unwrap_or_default().chunks_exact(3) {
        let [i, j, k] = [t[0] as u32, t[1] as u32, t[2] as u32];
        m.triangles.push([i, k, j]);
        m.triangles.push([n + i, n + j, n + k]);
    }
    m
}

/// A capped cylinder from `a` to `b` with `segments` facets.
pub fn cylinder(object: ObjectId, material: Material, a: DVec3, b: DVec3, radius: f64, segments: usize) -> Mesh {
    let profile: Vec<[f64; 2]> = (0..segments.max(3))
        .map(|i| {
            let t = i as f64 / segments.max(3) as f64 * std::f64::consts::TAU;
            [radius * t.cos(), radius * t.sin()]
        })
        .collect();
    prism(object, material, a, b, &profile)
}

/// A square bar of `width` from `a` to `b`.
pub fn bar(object: ObjectId, material: Material, a: DVec3, b: DVec3, width: f64) -> Mesh {
    let h = width / 2.0;
    prism(object, material, a, b, &[[-h, -h], [h, -h], [h, h], [-h, h]])
}

/// A box with walls of thickness `wall` around a closed cavity: an outer and an inner
/// shell in one mesh.
pub fn hollow_box(object: ObjectId, material: Material, min: DVec3, max: DVec3, wall: f64) -> Mesh {
    let mut m = cuboid(object, material, min, max);
    let inner = cuboid(object, material, min + DVec3::splat(wall), max - DVec3::splat(wall)).flipped();
    m.append(&inner);
    m
}

/// A box truss run along x from `start`, `length` long, `size` square (centre to
/// centre of the chords), chord tubes of radius `r` with `segments` facets, diagonals
/// on all four sides every `size` along the run.
pub fn box_truss(
    object: ObjectId,
    material: Material,
    start: DVec3,
    length: f64,
    size: f64,
    r: f64,
    segments: usize,
) -> Mesh {
    let mut m = Mesh::new(object, material);
    let h = size / 2.0;
    let corners = [[-h, -h], [h, -h], [h, h], [-h, h]];
    let at = |x: f64, c: [f64; 2]| start + DVec3::new(x, c[0], c[1]);
    for c in corners {
        m.append(&cylinder(object, material, at(0.0, c), at(length, c), r, segments));
    }
    let bays = (length / size).floor().max(1.0) as usize;
    let step = length / bays as f64;
    for i in 0..bays {
        let (x0, x1) = (i as f64 * step, (i + 1) as f64 * step);
        for k in 0..4 {
            let (c0, c1) = (corners[k], corners[(k + 1) % 4]);
            let (p, q) = if i % 2 == 0 { (at(x0, c0), at(x1, c1)) } else { (at(x1, c0), at(x0, c1)) };
            m.append(&cylinder(object, material, p, q, r * 0.6, segments.min(8)));
        }
    }
    m
}
