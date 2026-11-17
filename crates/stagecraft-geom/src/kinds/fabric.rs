//! Cloth shared by curtains and masking: sinusoidal pleats whose depth follows from
//! the fullness (the fabric length per wave stays the same however tightly a panel
//! is gathered), and a closed slab mesh along a pleated line with a hem that may rise.

use glam::DVec3;
use stagecraft_core::model::{Gather, P2};

use crate::Mesh;
use crate::math::offset_polyline;

/// Wavelength of the pleats of a closed curtain, mm.
pub const PLEAT: f64 = 300.0;
/// Thickness of the cloth slab, mm.
pub const CLOTH: f64 = 12.0;
/// How narrow a fully opened tab panel stacks, as a share of its width.
pub const STACK: f64 = 0.14;
/// How high the leading edge of a fully gathered Italian panel rises, as a share of
/// the height.
pub const ITALIAN_RISE: f64 = 0.82;

/// Arc length of one wave of `a · sin(2πx/λ)`.
fn arc(lambda: f64, a: f64) -> f64 {
    let n = 48;
    let mut len = 0.0;
    let mut prev = [0.0, 0.0];
    for k in 1..=n {
        let x = lambda * k as f64 / n as f64;
        let p = [x, a * (std::f64::consts::TAU * x / lambda).sin()];
        len += (p[0] - prev[0]).hypot(p[1] - prev[1]);
        prev = p;
    }
    len
}

/// The amplitude that puts `cloth` mm of fabric into one wave of `lambda` mm.
pub fn amplitude(lambda: f64, cloth: f64) -> f64 {
    if lambda <= 0.0 || cloth <= lambda * 1.000_01 {
        return 0.0;
    }
    let (mut lo, mut hi) = (0.0, cloth / 2.0);
    for _ in 0..50 {
        let mid = (lo + hi) / 2.0;
        if arc(lambda, mid) < cloth { lo = mid } else { hi = mid }
    }
    (lo + hi) / 2.0
}

/// One panel: its pleated centre line in plan (local x, y) and the hem height (local
/// z, negative: below the top edge) at each point.
#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub line: Vec<P2>,
    pub hem: Vec<f64>,
    /// Peak-to-peak pleat depth, mm.
    pub depth: f64,
}

/// What a curtain or a piece of masking looks like.
#[derive(Clone, Copy, Debug)]
pub struct Drape {
    pub width: f64,
    pub height: f64,
    /// Percent extra fabric.
    pub fullness: f64,
    pub split: bool,
    /// 0 … 100.
    pub opening: f64,
    pub gather: Gather,
}

/// The panels of a drape. `per_wave` points per pleat (0 = straight lines with no
/// pleats, for the coarsest detail).
pub fn panels(d: &Drape, per_wave: usize) -> Vec<Panel> {
    let hw = d.width / 2.0;
    // (outer edge, inner edge): panels gather towards their outer edge.
    let edges: Vec<(f64, f64)> = if d.split { vec![(-hw, 0.0), (hw, 0.0)] } else { vec![(-hw, hw)] };
    let o = (d.opening / 100.0).clamp(0.0, 1.0);
    edges
        .into_iter()
        .map(|(outer, inner)| {
            let pw = (inner - outer).abs();
            let sign = (inner - outer).signum();
            let waves = ((pw / PLEAT).round() as usize).max(1);
            let cloth = pw / waves as f64 * (1.0 + d.fullness / 100.0);
            let span = match d.gather {
                Gather::Tab => pw * (1.0 - o * (1.0 - STACK)),
                Gather::Italian => pw,
            };
            let lambda = span / waves as f64;
            let a = amplitude(lambda, cloth);
            let n = if per_wave == 0 { 1 } else { waves * per_wave };
            let mut line = Vec::with_capacity(n + 1);
            let mut hem = Vec::with_capacity(n + 1);
            for k in 0..=n {
                let t = k as f64 / n as f64;
                let x = outer + sign * span * t;
                let y = if per_wave == 0 {
                    0.0
                } else {
                    a * (std::f64::consts::TAU * (k % per_wave) as f64 / per_wave as f64).sin()
                };
                line.push([x, y]);
                let rise = match d.gather {
                    Gather::Tab => 0.0,
                    // u = 0 at the outer edge, 1 at the leading (inner) edge.
                    Gather::Italian => ITALIAN_RISE * o * (span * t / pw).powf(1.6),
                };
                hem.push(-d.height * (1.0 - rise));
            }
            Panel { line, hem, depth: 2.0 * a }
        })
        .collect()
}

/// A closed slab `thickness` thick along `line`, from `top` down to `hem[i]`.
pub fn slab(m: &mut Mesh, line: &[P2], hem: &[f64], top: f64, thickness: f64) {
    let n = line.len();
    if n < 2 || hem.len() != n {
        return;
    }
    let left = offset_polyline(line, thickness / 2.0);
    let right = offset_polyline(line, -thickness / 2.0);
    let at = |p: P2, z: f64| DVec3::new(p[0], p[1], z);
    for i in 0..n - 1 {
        let j = i + 1;
        let (l0, l1, r0, r1) = (left[i], left[j], right[i], right[j]);
        let (b0, b1) = (hem[i], hem[j]);
        m.quad(at(r0, top), at(r1, top), at(l1, top), at(l0, top));
        m.quad(at(r0, b0), at(l0, b0), at(l1, b1), at(r1, b1));
        m.quad(at(l1, b1), at(l0, b0), at(l0, top), at(l1, top));
        m.quad(at(r0, b0), at(r1, b1), at(r1, top), at(r0, top));
    }
    let (l, r) = (left[0], right[0]);
    m.quad(at(l, hem[0]), at(r, hem[0]), at(r, top), at(l, top));
    let (l, r) = (left[n - 1], right[n - 1]);
    m.quad(at(r, hem[n - 1]), at(l, hem[n - 1]), at(l, top), at(r, top));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amplitude_puts_the_cloth_into_the_wave() {
        assert_eq!(amplitude(300.0, 300.0), 0.0);
        let a = amplitude(300.0, 600.0);
        assert!((arc(300.0, a) - 600.0).abs() < 0.5);
        // Gathered tighter, the same cloth folds deeper.
        assert!(amplitude(100.0, 600.0) > a);
    }

    #[test]
    fn a_tab_curtain_stacks_at_the_sides() {
        let d =
            Drape { width: 10000.0, height: 6000.0, fullness: 100.0, split: true, opening: 100.0, gather: Gather::Tab };
        let p = panels(&d, 8);
        assert_eq!(p.len(), 2);
        let xs: Vec<f64> = p[0].line.iter().map(|q| q[0]).collect();
        let span = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - xs[0];
        assert!((span - 5000.0 * STACK).abs() < 1e-6);
        let closed = panels(&Drape { opening: 0.0, ..d }, 8);
        assert!(p[0].depth > closed[0].depth);
        assert!(p[0].hem.iter().all(|h| *h == -6000.0));
    }

    #[test]
    fn an_italian_curtain_lifts_its_leading_edge() {
        let d = Drape {
            width: 10000.0,
            height: 6000.0,
            fullness: 50.0,
            split: true,
            opening: 100.0,
            gather: Gather::Italian,
        };
        let p = &panels(&d, 8)[0];
        assert_eq!(p.hem[0], -6000.0);
        let lead = *p.hem.last().unwrap();
        assert!((lead + 6000.0 * (1.0 - ITALIAN_RISE)).abs() < 1e-6, "{lead}");
    }
}
