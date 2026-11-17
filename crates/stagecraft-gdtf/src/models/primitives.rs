//! Built-in shapes for `PrimitiveType`, generated to the model's dimensions (mm).
//! The spec ships reference meshes for these types without a licence, so they are
//! approximated here: same bounding box, recognisable shape, few triangles.
//! Placement follows the spec's reference meshes: GDTF 1.0 types are centred on the
//! origin, `Base1_1`, `Scanner1_1` and `Conventional1_1` hang below it (top at z = 0).

use glam::Vec3;

use super::RawMesh;
use crate::PrimitiveType;
use crate::mesh::Mesh;

const SEGMENTS: usize = 24;

/// The primitive mesh for `kind` with extents `dims` (x, y, z in mm). Zero
/// dimensions become 1 mm so nothing degenerates.
pub fn primitive(kind: &PrimitiveType, dims: Vec3) -> Mesh {
    let d = dims.max(Vec3::splat(1.0));
    let mut b = Builder::default();
    let h = d * 0.5;
    // Shapes are built hanging below z = 0, then moved: the GDTF 1.0 reference
    // meshes are centred on the origin, the `*1_1` ones hang below it.
    let hanging = matches!(kind, PrimitiveType::Base1_1 | PrimitiveType::Scanner1_1 | PrimitiveType::Conventional1_1);
    match kind {
        PrimitiveType::Cylinder | PrimitiveType::Pigtail | PrimitiveType::Head => {
            b.cylinder_z(Vec3::ZERO, h.x, h.y, -d.z, 0.0)
        }
        PrimitiveType::Sphere => b.ellipsoid(Vec3::new(0.0, 0.0, -h.z), h),
        PrimitiveType::Base | PrimitiveType::Base1_1 => b.cuboid(Vec3::new(-h.x, -h.y, -d.z), Vec3::new(h.x, h.y, 0.0)),
        // Yoke: a cross bar at the top and two arms down at ±x.
        PrimitiveType::Yoke => {
            let bar = (d.z * 0.15).max(1.0);
            let arm = (d.x * 0.12).max(1.0);
            b.cuboid(Vec3::new(-h.x, -h.y, -bar), Vec3::new(h.x, h.y, 0.0));
            b.cuboid(Vec3::new(-h.x, -h.y, -d.z), Vec3::new(-h.x + arm, h.y, -bar));
            b.cuboid(Vec3::new(h.x - arm, -h.y, -d.z), Vec3::new(h.x, h.y, -bar));
        }
        // Scanner: a box body with a smaller mirror box below.
        PrimitiveType::Scanner | PrimitiveType::Scanner1_1 => {
            let body = d.z * 0.7;
            b.cuboid(Vec3::new(-h.x, -h.y, -body), Vec3::new(h.x, h.y, 0.0));
            b.cuboid(Vec3::new(-h.x * 0.5, -h.y * 0.5, -d.z), Vec3::new(h.x * 0.5, h.y * 0.5, -body));
        }
        // Conventional 1.0: lamp body along y, as in the spec's reference mesh.
        PrimitiveType::Conventional => {
            b.cylinder_z(Vec3::ZERO, h.x, h.z, -d.y * 0.7, 0.0);
            b.cylinder_z(Vec3::ZERO, h.x * 0.8, h.z * 0.8, -d.y, -d.y * 0.7);
            let turn = glam::Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2);
            b.raw.transform(turn);
            b.raw.transform(glam::Mat4::from_translation(Vec3::new(0.0, -h.y, -h.z)));
        }
        // Conventional 1.1: lamp body along z, lens down.
        PrimitiveType::Conventional1_1 => {
            b.cylinder_z(Vec3::ZERO, h.x, h.y, -d.z * 0.7, 0.0);
            b.cylinder_z(Vec3::ZERO, h.x * 0.8, h.y * 0.8, -d.z, -d.z * 0.7);
        }
        PrimitiveType::Cube | PrimitiveType::Undefined | PrimitiveType::Other(_) => {
            b.cuboid(Vec3::new(-h.x, -h.y, -d.z), Vec3::new(h.x, h.y, 0.0))
        }
    }
    if !hanging {
        b.raw.transform(glam::Mat4::from_translation(Vec3::new(0.0, 0.0, h.z)));
    }
    b.raw.into_mesh("primitive")
}

#[derive(Default)]
struct Builder {
    raw: RawMesh,
}

impl Builder {
    fn v(&mut self, p: Vec3) -> u32 {
        self.raw.positions.push(p);
        self.raw.positions.len() as u32 - 1
    }

    fn tri(&mut self, a: u32, b: u32, c: u32) {
        self.raw.indices.extend([a, b, c]);
    }

    fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
        let [a, b, c, d] = [a, b, c, d].map(|p| self.v(p));
        self.tri(a, b, c);
        self.tri(a, c, d);
    }

    fn cuboid(&mut self, lo: Vec3, hi: Vec3) {
        let p = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
        let (a, b) = (lo, hi);
        self.quad(p(a.x, a.y, a.z), p(a.x, b.y, a.z), p(b.x, b.y, a.z), p(b.x, a.y, a.z)); // -z
        self.quad(p(a.x, a.y, b.z), p(b.x, a.y, b.z), p(b.x, b.y, b.z), p(a.x, b.y, b.z)); // +z
        self.quad(p(a.x, a.y, a.z), p(b.x, a.y, a.z), p(b.x, a.y, b.z), p(a.x, a.y, b.z)); // -y
        self.quad(p(a.x, b.y, a.z), p(a.x, b.y, b.z), p(b.x, b.y, b.z), p(b.x, b.y, a.z)); // +y
        self.quad(p(a.x, a.y, a.z), p(a.x, a.y, b.z), p(a.x, b.y, b.z), p(a.x, b.y, a.z)); // -x
        self.quad(p(b.x, a.y, a.z), p(b.x, b.y, a.z), p(b.x, b.y, b.z), p(b.x, a.y, b.z)); // +x
    }

    /// Elliptic cylinder along z from `z0` to `z1`, capped.
    fn cylinder_z(&mut self, c: Vec3, rx: f32, ry: f32, z0: f32, z1: f32) {
        let ring = |i: usize, z: f32| {
            let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            c + Vec3::new(rx * a.cos(), ry * a.sin(), z)
        };
        for i in 0..SEGMENTS {
            let j = (i + 1) % SEGMENTS;
            self.quad(ring(i, z0), ring(j, z0), ring(j, z1), ring(i, z1));
            let (bc, tc) = (self.v(c + Vec3::new(0.0, 0.0, z0)), self.v(c + Vec3::new(0.0, 0.0, z1)));
            let (b0, b1) = (self.v(ring(i, z0)), self.v(ring(j, z0)));
            self.tri(bc, b1, b0);
            let (t0, t1) = (self.v(ring(i, z1)), self.v(ring(j, z1)));
            self.tri(tc, t0, t1);
        }
    }

    fn ellipsoid(&mut self, c: Vec3, r: Vec3) {
        let rings = SEGMENTS / 2;
        let p = |i: usize, j: usize| {
            let th = i as f32 / rings as f32 * std::f32::consts::PI;
            let ph = j as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            c + r * Vec3::new(th.sin() * ph.cos(), th.sin() * ph.sin(), -th.cos())
        };
        for i in 0..rings {
            for j in 0..SEGMENTS {
                let (a, b, cc, d) = (p(i, j), p(i, j + 1), p(i + 1, j + 1), p(i + 1, j));
                if i == 0 {
                    let [a, cc, d] = [a, cc, d].map(|q| self.v(q));
                    self.tri(a, cc, d);
                } else if i == rings - 1 {
                    let [a, b, cc] = [a, b, cc].map(|q| self.v(q));
                    self.tri(a, b, cc);
                } else {
                    self.quad(a, b, cc, d);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_primitive_fills_its_box() {
        let dims = Vec3::new(300.0, 200.0, 100.0);
        for k in [
            PrimitiveType::Cube,
            PrimitiveType::Cylinder,
            PrimitiveType::Sphere,
            PrimitiveType::Base,
            PrimitiveType::Yoke,
            PrimitiveType::Head,
            PrimitiveType::Scanner,
            PrimitiveType::Conventional,
            PrimitiveType::Pigtail,
            PrimitiveType::Base1_1,
            PrimitiveType::Scanner1_1,
            PrimitiveType::Conventional1_1,
            PrimitiveType::Undefined,
        ] {
            let m = primitive(&k, dims);
            let (lo, hi) = m.bounds().unwrap();
            let size = hi - lo;
            assert!((size - dims).abs().max_element() < 1.0, "{k:?}: {size}");
            // Outward normals: the sum of (centroid − centre)·normal is positive.
            let centre = (lo + hi) * 0.5;
            let mut dot = 0.0;
            for t in m.indices.chunks_exact(3) {
                let ps: Vec<Vec3> = t.iter().map(|i| Vec3::from_array(m.positions[*i as usize])).collect();
                let n = (ps[1] - ps[0]).cross(ps[2] - ps[0]);
                dot += ((ps[0] + ps[1] + ps[2]) / 3.0 - centre).dot(n);
            }
            assert!(dot > 0.0, "{k:?} faces inward");
        }
    }
}
