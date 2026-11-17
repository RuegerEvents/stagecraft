//! Triangle meshes for the 3D view and for section cuts.
//!
//! A [`Mesh`] is in the object's local space, mm, `f32` (a venue fits easily: f32 keeps
//! 0.01 mm at 100 m). Triangles wind counter-clockwise seen from outside, and every
//! vertex carries a unit normal. Flat faces get their own vertices so normals stay
//! sharp; [`Mesh::is_watertight`] therefore compares positions, not indices.

use std::collections::HashMap;

use glam::{DAffine3, DVec3, Vec3};
use serde::{Deserialize, Serialize};

/// Level of detail. Truss is where it matters: chords and diagonals, chords only, or
/// one box. Views pick it by screen size and object count.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lod {
    /// Everything: truss chords, diagonals and end frames.
    #[default]
    Full,
    /// Truss chords only.
    Simple,
    /// Bounding boxes for truss, for far views and very many objects.
    Box,
}

/// What a surface is made of, mapped to a look by the renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialClass {
    #[default]
    Generic,
    Floor,
    Wall,
    Metal,
    Fixture,
    Lens,
    /// Curtain cloth (velour); the mesh's `tint` colours it.
    Curtain,
    /// Masking cloth (molton); the mesh's `tint` colours it.
    Masking,
    /// Platform decks.
    Deck,
}

impl MaterialClass {
    pub const ALL: [MaterialClass; 9] = [
        MaterialClass::Generic,
        MaterialClass::Floor,
        MaterialClass::Wall,
        MaterialClass::Metal,
        MaterialClass::Fixture,
        MaterialClass::Lens,
        MaterialClass::Curtain,
        MaterialClass::Masking,
        MaterialClass::Deck,
    ];

    /// Cloth takes the object's own colour (the mesh tint) when it has one.
    pub fn tintable(self) -> bool {
        matches!(self, MaterialClass::Curtain | MaterialClass::Masking)
    }

    /// Index into a renderer's colour table.
    pub fn index(self) -> u32 {
        self as u32
    }
}

/// A run of triangles of one material: `indices[start .. start + count]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub start: u32,
    pub count: u32,
    pub material: MaterialClass,
}

/// Triangles in the object's local space, mm. Counter-clockwise faces point outwards.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Three per triangle, into `positions`/`normals`.
    pub indices: Vec<u32>,
    /// Material runs covering all of `indices`, in order.
    pub groups: Vec<Group>,
    /// sRGB colour for tintable material groups (cloth), from the object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<[u8; 3]>,
}

impl Mesh {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The material of the triangle at `tri` (index / 3).
    pub fn material_of(&self, tri: usize) -> MaterialClass {
        let i = (tri * 3) as u32;
        self.groups
            .iter()
            .find(|g| i >= g.start && i < g.start + g.count)
            .map_or(MaterialClass::Generic, |g| g.material)
    }

    /// Start a run of `material`; following triangles belong to it.
    pub fn set_material(&mut self, material: MaterialClass) {
        let start = self.indices.len() as u32;
        if let Some(last) = self.groups.last_mut() {
            if last.material == material {
                return;
            }
            if last.count == 0 {
                last.material = material;
                return;
            }
        }
        self.groups.push(Group { start, count: 0, material });
    }

    fn push_vertex(&mut self, p: DVec3, n: DVec3) -> u32 {
        let i = self.positions.len() as u32;
        self.positions.push(p.as_vec3().to_array());
        self.normals.push(n.normalize_or_zero().as_vec3().to_array());
        i
    }

    fn push_tri(&mut self, a: u32, b: u32, c: u32) {
        if self.groups.is_empty() {
            self.set_material(MaterialClass::Generic);
        }
        self.indices.extend([a, b, c]);
        self.groups.last_mut().expect("a group").count += 3;
    }

    /// A flat polygon (convex, or given its own triangulation) with one normal.
    pub fn flat(&mut self, pts: &[DVec3], tris: &[[usize; 3]], normal: DVec3) {
        let base: Vec<u32> = pts.iter().map(|p| self.push_vertex(*p, normal)).collect();
        for t in tris {
            self.push_tri(base[t[0]], base[t[1]], base[t[2]]);
        }
    }

    /// A flat quad a-b-c-d, counter-clockwise seen from the side its normal points to.
    pub fn quad(&mut self, a: DVec3, b: DVec3, c: DVec3, d: DVec3) {
        let n = (b - a).cross(d - a) + (d - c).cross(b - c);
        if n.length_squared() < 1e-18 {
            return;
        }
        self.flat(&[a, b, c, d], &[[0, 1, 2], [0, 2, 3]], n);
    }

    /// An axis-aligned box from `min` to `max`.
    pub fn cuboid(&mut self, min: DVec3, max: DVec3) {
        let p = |x: bool, y: bool, z: bool| {
            DVec3::new(if x { max.x } else { min.x }, if y { max.y } else { min.y }, if z { max.z } else { min.z })
        };
        let (f, t) = (false, true);
        self.quad(p(f, f, f), p(f, t, f), p(t, t, f), p(t, f, f)); // bottom
        self.quad(p(f, f, t), p(t, f, t), p(t, t, t), p(f, t, t)); // top
        self.quad(p(f, f, f), p(t, f, f), p(t, f, t), p(f, f, t)); // front (-y)
        self.quad(p(t, t, f), p(f, t, f), p(f, t, t), p(t, t, t)); // back (+y)
        self.quad(p(f, t, f), p(f, f, f), p(f, f, t), p(f, t, t)); // left (-x)
        self.quad(p(t, f, f), p(t, t, f), p(t, t, t), p(t, f, t)); // right (+x)
    }

    /// A closed cylinder from `a` to `b` with `segments` sides, smooth along its side.
    pub fn tube(&mut self, a: DVec3, b: DVec3, radius: f64, segments: usize) {
        let axis = b - a;
        let len = axis.length();
        if len < 1e-9 || radius <= 0.0 || segments < 3 {
            return;
        }
        let dir = axis / len;
        let u = dir.any_orthonormal_vector();
        let v = dir.cross(u);
        let ring: Vec<DVec3> = (0..segments)
            .map(|i| {
                let t = std::f64::consts::TAU * i as f64 / segments as f64;
                u * t.cos() + v * t.sin()
            })
            .collect();
        // Side: two rings with radial normals.
        let first = self.positions.len() as u32;
        for r in &ring {
            self.push_vertex(a + *r * radius, *r);
            self.push_vertex(b + *r * radius, *r);
        }
        let n = segments as u32;
        for i in 0..n {
            let j = (i + 1) % n;
            let (a0, b0, a1, b1) = (first + 2 * i, first + 2 * i + 1, first + 2 * j, first + 2 * j + 1);
            self.push_tri(a0, a1, b1);
            self.push_tri(a0, b1, b0);
        }
        // Caps: fans facing away from the tube.
        for (centre, normal) in [(a, -dir), (b, dir)] {
            let c = self.push_vertex(centre, normal);
            let rim: Vec<u32> = ring.iter().map(|r| self.push_vertex(centre + *r * radius, normal)).collect();
            for i in 0..segments {
                let j = (i + 1) % segments;
                if normal == dir {
                    self.push_tri(c, rim[i], rim[j]);
                } else {
                    self.push_tri(c, rim[j], rim[i]);
                }
            }
        }
    }

    /// A prism: the counter-clockwise plan outline from `z0` up to `z1`.
    pub fn extrude(&mut self, outline: &[[f64; 2]], z0: f64, z1: f64) {
        let mut pts = outline.to_vec();
        if pts.len() > 1 && crate::math::dist(pts[0], pts[pts.len() - 1]) < 1e-9 {
            pts.pop();
        }
        if pts.len() < 3 || z1 <= z0 {
            return;
        }
        if crate::math::signed_area2(&pts) < 0.0 {
            pts.reverse();
        }
        let tris = crate::math::triangulate(&pts);
        let at = |p: [f64; 2], z: f64| DVec3::new(p[0], p[1], z);
        let top: Vec<DVec3> = pts.iter().map(|p| at(*p, z1)).collect();
        self.flat(&top, &tris, DVec3::Z);
        let bottom: Vec<DVec3> = pts.iter().map(|p| at(*p, z0)).collect();
        let flipped: Vec<[usize; 3]> = tris.iter().map(|t| [t[0], t[2], t[1]]).collect();
        self.flat(&bottom, &flipped, -DVec3::Z);
        let n = pts.len();
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            self.quad(at(a, z0), at(b, z0), at(b, z1), at(a, z1));
        }
    }

    /// Append another mesh (its groups too).
    pub fn append(&mut self, other: &Mesh) {
        if self.tint.is_none() {
            self.tint = other.tint;
        }
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        for g in &other.groups {
            self.set_material(g.material);
            for &i in &other.indices[g.start as usize..(g.start + g.count) as usize] {
                self.indices.push(i + base);
            }
            self.groups.last_mut().expect("a group").count += g.count;
        }
    }

    /// The same mesh through a transform (normals by its rotation part).
    pub fn transformed(&self, t: &DAffine3) -> Mesh {
        let p = self.positions.iter().map(|p| t.transform_point3(Vec3::from_array(*p).as_dvec3()).as_vec3().to_array());
        let n = self
            .normals
            .iter()
            .map(|n| t.transform_vector3(Vec3::from_array(*n).as_dvec3()).normalize_or_zero().as_vec3().to_array());
        Mesh {
            positions: p.collect(),
            normals: n.collect(),
            indices: self.indices.clone(),
            groups: self.groups.clone(),
            tint: self.tint,
        }
    }

    /// Axis-aligned bounds (min, max), or `None` for an empty mesh.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut it = self.positions.iter();
        let first = *it.next()?;
        let (mut lo, mut hi) = (Vec3::from_array(first), Vec3::from_array(first));
        for p in it {
            lo = lo.min(Vec3::from_array(*p));
            hi = hi.max(Vec3::from_array(*p));
        }
        Some((lo.to_array(), hi.to_array()))
    }

    /// Every edge (by position, to 1/1000 mm) is used as often in one direction as in
    /// the other: the surface is closed and consistently wound. Closed parts that
    /// touch (truss braces meeting at a node) still count as closed.
    pub fn is_watertight(&self) -> bool {
        let key = |i: u32| {
            let p = self.positions[i as usize];
            p.map(|c| (c as f64 * 1000.0).round() as i64)
        };
        let mut edges: HashMap<([i64; 3], [i64; 3]), i32> = HashMap::new();
        for t in self.indices.chunks_exact(3) {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let (ka, kb) = (key(a), key(b));
                if ka == kb {
                    continue;
                }
                *edges.entry((ka, kb)).or_default() += 1;
            }
        }
        !edges.is_empty() && edges.iter().all(|((a, b), n)| edges.get(&(*b, *a)) == Some(n))
    }

    /// Enclosed volume in mm³ (positive when faces point outwards). Only meaningful for
    /// a watertight mesh.
    pub fn volume(&self) -> f64 {
        self.indices
            .chunks_exact(3)
            .map(|t| {
                let p = |i: u32| Vec3::from_array(self.positions[i as usize]).as_dvec3();
                p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cuboid_is_closed_and_points_outwards() {
        let mut m = Mesh::empty();
        m.set_material(MaterialClass::Wall);
        m.cuboid(DVec3::ZERO, DVec3::new(1000.0, 2000.0, 3000.0));
        assert_eq!(m.positions.len(), 24);
        assert_eq!(m.triangle_count(), 12);
        assert!(m.is_watertight());
        assert!((m.volume() - 6e9).abs() < 1e3, "{}", m.volume());
        assert_eq!(m.groups, vec![Group { start: 0, count: 36, material: MaterialClass::Wall }]);
        // Every face normal agrees with its winding.
        for t in m.indices.chunks_exact(3) {
            let p = |i: u32| Vec3::from_array(m.positions[i as usize]);
            let face = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).normalize();
            assert!(face.dot(Vec3::from_array(m.normals[t[0] as usize])) > 0.99);
        }
    }

    #[test]
    fn a_tube_is_closed_with_unit_normals() {
        let mut m = Mesh::empty();
        m.tube(DVec3::ZERO, DVec3::new(0.0, 0.0, 100.0), 10.0, 8);
        assert_eq!(m.positions.len(), 16 + 2 * 9);
        assert_eq!(m.triangle_count(), 16 + 16);
        assert!(m.is_watertight());
        let ideal = std::f64::consts::PI * 100.0 * 100.0;
        // An octagon has 90 % of its circle's area.
        assert!((m.volume() / ideal - 0.9003).abs() < 1e-3, "{}", m.volume() / ideal);
        for n in &m.normals {
            assert!((Vec3::from_array(*n).length() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn append_keeps_materials_apart() {
        let mut a = Mesh::empty();
        a.set_material(MaterialClass::Fixture);
        a.cuboid(DVec3::ZERO, DVec3::ONE);
        let mut b = Mesh::empty();
        b.set_material(MaterialClass::Lens);
        b.cuboid(DVec3::ZERO, DVec3::ONE);
        a.append(&b);
        assert_eq!(a.groups.len(), 2);
        assert_eq!(a.material_of(0), MaterialClass::Fixture);
        assert_eq!(a.material_of(12), MaterialClass::Lens);
        assert_eq!(*a.indices.iter().max().unwrap(), 47);
    }
}
