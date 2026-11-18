//! Small 2D helpers on `[f64; 2]` points.

use stagecraft_core::model::P2;

pub fn dist(a: P2, b: P2) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

pub fn sub(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}

pub fn add(a: P2, b: P2) -> P2 {
    [a[0] + b[0], a[1] + b[1]]
}

pub fn scale(a: P2, s: f64) -> P2 {
    [a[0] * s, a[1] * s]
}

pub fn lerp(a: P2, b: P2, t: f64) -> P2 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

pub fn midpoint(a: P2, b: P2) -> P2 {
    lerp(a, b, 0.5)
}

pub fn length(v: P2) -> f64 {
    v[0].hypot(v[1])
}

/// Unit vector, or zero for a zero vector.
pub fn normalize(v: P2) -> P2 {
    let l = length(v);
    if l < 1e-12 { [0.0, 0.0] } else { [v[0] / l, v[1] / l] }
}

/// Left-hand perpendicular (counter-clockwise by 90°).
pub fn perp(v: P2) -> P2 {
    [-v[1], v[0]]
}

/// Direction in degrees, counter-clockwise from +x.
pub fn angle_deg(v: P2) -> f64 {
    v[1].atan2(v[0]).to_degrees()
}

/// The point at `len` mm from `from` in direction `deg`.
pub fn polar(from: P2, len: f64, deg: f64) -> P2 {
    let r = deg.to_radians();
    [from[0] + len * r.cos(), from[1] + len * r.sin()]
}

pub fn rotate_about(p: P2, pivot: P2, deg: f64) -> P2 {
    let (s, c) = deg.to_radians().sin_cos();
    let v = sub(p, pivot);
    [pivot[0] + v[0] * c - v[1] * s, pivot[1] + v[0] * s + v[1] * c]
}

/// Parameter `t` in [0, 1] of the point on segment `a`–`b` closest to `p`.
pub fn project_t(a: P2, b: P2, p: P2) -> f64 {
    let ab = sub(b, a);
    let l2 = ab[0] * ab[0] + ab[1] * ab[1];
    if l2 < 1e-12 {
        return 0.0;
    }
    (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / l2).clamp(0.0, 1.0)
}

pub fn dot(a: P2, b: P2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

/// The foot of `p` on the infinite line through `a` and `b`.
pub fn closest_on_line(a: P2, b: P2, p: P2) -> P2 {
    let d = sub(b, a);
    let l2 = dot(d, d);
    if l2 == 0.0 {
        return a;
    }
    add(a, scale(d, dot(sub(p, a), d) / l2))
}

/// Where the line through `p` along `u` meets the line through `q` along `v`; `None`
/// if they are parallel.
pub fn line_intersection(p: P2, u: P2, q: P2, v: P2) -> Option<P2> {
    let den = u[0] * v[1] - u[1] * v[0];
    if den.abs() < 1e-12 * length(u).max(1.0) * length(v).max(1.0) {
        return None;
    }
    let w = sub(q, p);
    let t = (w[0] * v[1] - w[1] * v[0]) / den;
    Some(add(p, scale(u, t)))
}

pub fn closest_on_segment(a: P2, b: P2, p: P2) -> P2 {
    lerp(a, b, project_t(a, b, p))
}

pub fn segment_distance(a: P2, b: P2, p: P2) -> f64 {
    dist(closest_on_segment(a, b, p), p)
}

pub fn polyline_distance(points: &[P2], closed: bool, p: P2) -> f64 {
    match points {
        [] => f64::INFINITY,
        [only] => dist(*only, p),
        _ => {
            let open = points.windows(2).map(|w| segment_distance(w[0], w[1], p));
            let close = closed.then(|| segment_distance(points[points.len() - 1], points[0], p));
            open.chain(close).fold(f64::INFINITY, f64::min)
        }
    }
}

/// Even-odd rule; points on the edge may go either way.
pub fn point_in_polygon(poly: &[P2], p: P2) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + n - 1) % n];
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
            if p[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// The polyline shifted sideways by `d` (positive = left of the direction of travel),
/// with mitred corners. Very sharp corners are capped at 4× the offset.
pub fn offset_polyline(points: &[P2], d: f64) -> Vec<P2> {
    let n = points.len();
    if n < 2 {
        return points.to_vec();
    }
    let normal = |i: usize| perp(normalize(sub(points[i + 1], points[i])));
    let mut out = Vec::with_capacity(n);
    out.push(add(points[0], scale(normal(0), d)));
    #[allow(clippy::needless_range_loop)] // neighbours by index read clearer
    for i in 1..n - 1 {
        let n0 = normal(i - 1);
        let n1 = normal(i);
        let m = normalize(add(n0, n1));
        let cos = m[0] * n0[0] + m[1] * n0[1];
        let len = if cos.abs() < 0.25 { 4.0 * d } else { d / cos };
        out.push(add(points[i], scale(m, len)));
    }
    out.push(add(points[n - 1], scale(normal(n - 2), d)));
    out
}

/// Twice the signed area; positive for counter-clockwise outlines.
pub fn signed_area2(poly: &[P2]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let a = poly[i];
            let b = poly[(i + 1) % n];
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// Triangles of a simple polygon by ear clipping, as index triples into `poly`.
/// Self-intersecting input still gets triangles (a fan for what is left), so a fill
/// never disappears.
pub fn triangulate(poly: &[P2]) -> Vec<[usize; 3]> {
    let n = poly.len();
    if n < 3 {
        return Vec::new();
    }
    let ccw = signed_area2(poly) >= 0.0;
    let mut idx: Vec<usize> = if ccw { (0..n).collect() } else { (0..n).rev().collect() };
    let cross = |a: P2, b: P2, c: P2| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    let mut out = Vec::with_capacity(n - 2);
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let (ia, ib, ic) = (idx[(i + m - 1) % m], idx[i], idx[(i + 1) % m]);
            let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
            if cross(a, b, c) <= 1e-12 {
                continue;
            }
            let inside = idx.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let p = poly[j];
                cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
            });
            if inside {
                continue;
            }
            out.push([ia, ib, ic]);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    // What is left (a triangle, or a degenerate rest): fan it.
    for k in 1..idx.len().saturating_sub(1) {
        out.push([idx[0], idx[k], idx[k + 1]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: P2, b: P2) -> bool {
        dist(a, b) < 1e-9
    }

    #[test]
    fn segment_distance_and_projection() {
        assert_eq!(segment_distance([0.0, 0.0], [10.0, 0.0], [5.0, 3.0]), 3.0);
        assert_eq!(segment_distance([0.0, 0.0], [10.0, 0.0], [13.0, 4.0]), 5.0);
        assert_eq!(project_t([0.0, 0.0], [10.0, 0.0], [2.5, 7.0]), 0.25);
        assert_eq!(project_t([0.0, 0.0], [10.0, 0.0], [-5.0, 0.0]), 0.0);
    }

    #[test]
    fn polygon_inside() {
        let sq = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        assert!(point_in_polygon(&sq, [5.0, 5.0]));
        assert!(!point_in_polygon(&sq, [15.0, 5.0]));
        // L shape: the notch is outside.
        let l = [[0.0, 0.0], [10.0, 0.0], [10.0, 4.0], [4.0, 4.0], [4.0, 10.0], [0.0, 10.0]];
        assert!(!point_in_polygon(&l, [7.0, 7.0]));
        assert!(point_in_polygon(&l, [2.0, 7.0]));
    }

    #[test]
    fn offsets_keep_their_distance_at_right_angles() {
        let path = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        let left = offset_polyline(&path, 1.0);
        assert!(near(left[0], [0.0, 1.0]));
        assert!(near(left[1], [9.0, 1.0]));
        assert!(near(left[2], [9.0, 10.0]));
        let right = offset_polyline(&path, -1.0);
        assert!(near(right[1], [11.0, -1.0]));
    }

    #[test]
    fn triangulation_covers_concave_outlines() {
        let l = [[0.0, 0.0], [10.0, 0.0], [10.0, 4.0], [4.0, 4.0], [4.0, 10.0], [0.0, 10.0]];
        for poly in [l.to_vec(), l.iter().rev().copied().collect()] {
            let tris = triangulate(&poly);
            assert_eq!(tris.len(), 4);
            let area: f64 = tris.iter().map(|t| signed_area2(&[poly[t[0]], poly[t[1]], poly[t[2]]]).abs() / 2.0).sum();
            assert!((area - 64.0).abs() < 1e-9, "{area}");
            // No triangle centre falls in the notch.
            for t in &tris {
                let c = [
                    (poly[t[0]][0] + poly[t[1]][0] + poly[t[2]][0]) / 3.0,
                    (poly[t[0]][1] + poly[t[1]][1] + poly[t[2]][1]) / 3.0,
                ];
                assert!(point_in_polygon(&poly, c));
            }
        }
        assert!(triangulate(&[[0.0, 0.0], [1.0, 1.0]]).is_empty());
    }

    #[test]
    fn polar_and_rotate() {
        assert!(near(polar([1.0, 1.0], 2.0, 90.0), [1.0, 3.0]));
        assert!(near(rotate_about([2.0, 0.0], [1.0, 0.0], 90.0), [1.0, 1.0]));
        assert!((angle_deg([0.0, -1.0]) + 90.0).abs() < 1e-12);
    }
}
