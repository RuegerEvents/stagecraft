//! Remove line pieces that lie on top of stronger ones: a hidden line under a visible
//! or cut line, a visible line under a cut line, an intersection line under any edge,
//! and duplicates within one object.
//! Front views of boxes otherwise show every back edge as a dashed line exactly under
//! the solid outline.

use glam::DVec2;

use crate::LineClass;
use crate::hlr::{Geom, Piece};

/// Lower is stronger. Intersection lines rank below every visible edge.
fn rank(p: &Piece) -> u8 {
    match (p.class, p.weak) {
        (LineClass::Cut, _) => 0,
        (LineClass::Visible, false) => 1,
        (LineClass::Visible, true) => 2,
        (LineClass::Hidden, false) => 3,
        (LineClass::Hidden, true) => 4,
        (LineClass::Centre, _) => 5,
    }
}

pub(crate) fn remove_overlaps(mut pieces: Vec<Piece>, tol: f64, min_length: f64) -> Vec<Piece> {
    let tol = tol.max(1e-6);
    pieces.sort_by_key(rank);
    let mut lo = DVec2::splat(f64::INFINITY);
    let mut hi = DVec2::splat(f64::NEG_INFINITY);
    for p in &pieces {
        lo = lo.min(p.a).min(p.b);
        hi = hi.max(p.a).max(p.b);
    }
    if pieces.is_empty() {
        return pieces;
    }
    let (lo, hi) = (lo - DVec2::splat(2.0 * tol), hi + DVec2::splat(2.0 * tol));
    let geom = Geom::new(lo, hi, pieces.len());
    let mut cells: Vec<Vec<u32>> = vec![Vec::new(); geom.len()];
    let mut accepted: Vec<Piece> = Vec::with_capacity(pieces.len());
    let mut stamp: Vec<u32> = Vec::new();
    let mut generation = 0u32;
    let mut cut: Vec<(f64, f64)> = Vec::new();

    for p in pieces {
        if p.class == LineClass::Centre {
            accepted.push(p);
            continue;
        }
        let d = p.b - p.a;
        let len = d.length();
        if len < min_length {
            continue;
        }
        generation += 1;
        stamp.resize(accepted.len(), 0);
        cut.clear();
        let r = rank(&p);
        geom.cells(&[p.a, p.b], |c| {
            for &qi in &cells[c] {
                if stamp[qi as usize] == generation {
                    continue;
                }
                stamp[qi as usize] = generation;
                let q = &accepted[qi as usize];
                let rq = rank(q);
                if !(rq < r || (rq == r && q.object == p.object)) {
                    continue;
                }
                let off = |x: DVec2| (d.perp_dot(x - p.a) / len).abs();
                if off(q.a) > tol || off(q.b) > tol {
                    continue;
                }
                let t0 = (q.a - p.a).dot(d) / (len * len);
                let t1 = (q.b - p.a).dot(d) / (len * len);
                let (t0, t1) = (t0.min(t1).max(0.0), t0.max(t1).min(1.0));
                if t1 > t0 {
                    cut.push((t0, t1));
                }
            }
        });
        cut.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut t = 0.0;
        let mut keep: Vec<(f64, f64)> = Vec::new();
        for &(c0, c1) in &cut {
            if c0 > t {
                keep.push((t, c0));
            }
            t = t.max(c1);
        }
        if t < 1.0 {
            keep.push((t, 1.0));
        }
        for (t0, t1) in keep {
            if (t1 - t0) * len < min_length {
                continue;
            }
            let piece = Piece { a: p.a.lerp(p.b, t0), b: p.a.lerp(p.b, t1), ..p };
            let idx = accepted.len() as u32;
            // Insert with the tolerance band around it, so a coincident line on the
            // other side of a cell border still finds it.
            let n = (piece.b - piece.a).normalize_or_zero().perp() * tol;
            let e = (piece.b - piece.a).normalize_or_zero() * tol;
            let quad = [piece.a - n - e, piece.b - n + e, piece.b + n + e, piece.a + n - e];
            let mut last = usize::MAX;
            for tri in [[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[3]]] {
                geom.cells(&tri, |c| {
                    if c != last && cells[c].last() != Some(&idx) {
                        cells[c].push(idx);
                    }
                    last = c;
                });
            }
            accepted.push(piece);
        }
    }
    accepted
}
