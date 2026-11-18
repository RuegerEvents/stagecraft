//! Shared fixtures for tests and the perf example.
#![allow(dead_code)]

use stagecraft_sketch::*;

/// A rectilinear room with a toothed wall: `teeth` notches along the bottom, so the
/// outline has `4·teeth + 4` points. Every edge alternates horizontal/vertical, every
/// edge but two has a driving length, one corner is fixed: fully constrained.
/// Positions are jittered so the first solve has work to do.
pub fn comb_room(teeth: usize) -> (Sketch, Vec<PointId>, Vec<ConstraintId>) {
    let mut pts = Vec::new();
    let (w, h) = (500.0, 300.0);
    let mut x = 0.0;
    pts.push(Vec2::new(0.0, 0.0));
    for _ in 0..teeth {
        // right along the bottom, up into the tooth, right, down
        x += w;
        pts.push(Vec2::new(x, 0.0));
        pts.push(Vec2::new(x, h));
        x += w;
        pts.push(Vec2::new(x, h));
        pts.push(Vec2::new(x, 0.0));
    }
    // last bottom piece, right wall, top, left wall
    x += w;
    pts.push(Vec2::new(x, 0.0));
    pts.push(Vec2::new(x, 8000.0));
    pts.push(Vec2::new(0.0, 8000.0));
    let n = pts.len();
    // jitter deterministically
    let jit: Vec<Vec2> = pts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let k = i as f64;
            *p + Vec2::new((k * 1.7).sin() * 20.0, (k * 2.3).cos() * 20.0)
        })
        .collect();
    let mut s = Sketch::new();
    let (ids, edges) = s.add_polyline(&jit, true);
    for i in 0..n {
        let (a, b) = (ids[i], ids[(i + 1) % n]);
        let horizontal = (pts[i].y - pts[(i + 1) % n].y).abs() < 1e-9;
        if horizontal {
            s.add_constraint(ConstraintKind::Horizontal { a, b });
        } else {
            s.add_constraint(ConstraintKind::Vertical { a, b });
        }
    }
    s.add_constraint(ConstraintKind::Fixed { point: ids[0], at: Vec2::ZERO });
    let mut dims = Vec::new();
    // skip the top (closes x) and the left wall (closes y)
    for i in 0..n - 2 {
        let len = pts[i].distance(pts[(i + 1) % n]);
        dims.push(s.add_dimension(Measure::Length { line: edges[i] }, len));
    }
    (s, ids, dims)
}
