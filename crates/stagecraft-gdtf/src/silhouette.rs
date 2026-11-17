//! Top-view silhouette for plan symbols of fixtures without an SVG.
//!
//! The mesh is projected onto the x/y plane (looking down −z; x right, y up on the
//! sheet = away from the audience). Each part gives a convex hull, and the whole
//! fixture one more. Units mm, counter-clockwise, no repeated closing point.

use glam::{Mat4, Vec2, Vec3};

use crate::mesh::FixtureMesh;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Silhouette {
    /// Convex hull of the whole fixture.
    pub hull: Vec<[f32; 2]>,
    /// Hull per geometry part with a mesh, (part path, polygon), tree order. Drawn
    /// together they read better than the overall hull (yoke arms, lens).
    pub parts: Vec<(String, Vec<[f32; 2]>)>,
}

impl Silhouette {
    /// Area of the overall hull in mm².
    pub fn area(&self) -> f32 {
        polygon_area(&self.hull)
    }

    /// (min, max) of the hull.
    pub fn bounds(&self) -> Option<([f32; 2], [f32; 2])> {
        let mut it = self.hull.iter();
        let f = *it.next()?;
        Some(it.fold((f, f), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])])))
    }
}

/// Silhouette of the default pose.
pub fn silhouette(mesh: &FixtureMesh) -> Silhouette {
    silhouette_with(mesh, &mesh.posed_transforms(&|_, _| Mat4::IDENTITY))
}

/// Silhouette with given part transforms (from [`FixtureMesh::posed_transforms`]),
/// e.g. a moving head tilted.
pub fn silhouette_with(mesh: &FixtureMesh, transforms: &[Mat4]) -> Silhouette {
    let mut all: Vec<Vec2> = Vec::new();
    let mut parts = Vec::new();
    for (p, t) in mesh.parts.iter().zip(transforms) {
        let Some(m) = &p.mesh else { continue };
        let pts: Vec<Vec2> = m.positions.iter().map(|v| t.transform_point3(Vec3::from_array(*v)).truncate()).collect();
        let h = convex_hull(&pts);
        if h.len() >= 3 {
            all.extend(&h);
            parts.push((p.instance.path.clone(), h.iter().map(|v| v.to_array()).collect()));
        }
    }
    Silhouette { hull: convex_hull(&all).iter().map(|v| v.to_array()).collect(), parts }
}

/// Andrew's monotone chain; counter-clockwise, collinear points dropped.
pub fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
    let mut p: Vec<Vec2> = points.iter().copied().filter(|v| v.is_finite()).collect();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut hull: Vec<Vec2> = Vec::with_capacity(p.len() * 2);
    for &v in &p {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], v) <= 0.0 {
            hull.pop();
        }
        hull.push(v);
    }
    let lower = hull.len() + 1;
    for &v in p.iter().rev().skip(1) {
        while hull.len() >= lower && cross(hull[hull.len() - 2], hull[hull.len() - 1], v) <= 0.0 {
            hull.pop();
        }
        hull.push(v);
    }
    hull.pop();
    hull
}

fn polygon_area(p: &[[f32; 2]]) -> f32 {
    let n = p.len();
    (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
        * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_of_a_square_with_inner_points() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(0.5, 0.5),
            Vec2::new(0.5, 0.0),
        ];
        let h = convex_hull(&pts);
        assert_eq!(h.len(), 4);
        let a = polygon_area(&h.iter().map(|v| v.to_array()).collect::<Vec<_>>());
        assert!((a - 1.0).abs() < 1e-6, "ccw, area 1: {a}");
    }
}
