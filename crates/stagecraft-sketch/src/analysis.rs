//! Degrees of freedom and redundancy from the Jacobian at the solved position.
//!
//! The right null space of J says which point coordinates can still move (per-point
//! DOF); the left null space says which equations depend on each other (redundant, or
//! conflicting when they are also unsatisfied). Both come from a column-pivoted QR, the
//! same approach ezpz uses for its own freedom analysis (`find_dof.rs`, MIT).

use faer::linalg::solvers::{ColPivQr, Qr};
use faer::mat::{AsMatMut, AsMatRef};
use faer::perm::permute_rows;
use faer::{Mat, MatRef};

use crate::system::System;

const RANK_TOL: f64 = 1e-9;
const PARTICIPATION_TOL: f64 = 1e-6;

pub(crate) struct Analysis {
    /// Total degrees of freedom left in the system.
    pub dof: usize,
    /// DOF per system point (0, 1 or 2).
    pub point_dof: Vec<u8>,
    /// Per equation: part of a linearly dependent set.
    pub dependent: Vec<bool>,
}

pub(crate) fn analyse(sys: &System, vals: &[f64]) -> Analysis {
    let n = sys.num_vars();
    let m = sys.eqs.len();
    if m == 0 || n == 0 {
        return Analysis { dof: n, point_dof: vec![2; sys.points.len()], dependent: vec![false; m] };
    }
    let mut j = Mat::<f64>::zeros(m, n);
    for (row, eq) in sys.eqs.iter().enumerate() {
        let (_, g) = sys.residual_grad(eq, vals);
        for k in 0..eq.arity() {
            let i = eq.pts[k];
            j[(row, 2 * i)] += g[2 * k];
            j[(row, 2 * i + 1)] += g[2 * k + 1];
        }
    }

    let right = nullspace(j.as_ref());
    let k = right.ncols();
    let point_dof = (0..sys.points.len())
        .map(|i| {
            let (rx, ry) = (2 * i, 2 * i + 1);
            let mut g11 = 0.0;
            let mut g22 = 0.0;
            let mut g12 = 0.0;
            for c in 0..k {
                let (a, b) = (right[(rx, c)], right[(ry, c)]);
                g11 += a * a;
                g22 += b * b;
                g12 += a * b;
            }
            // eigenvalues of the 2×2 Gram matrix
            let h = 0.5 * (g11 + g22);
            let disc = (h * h - (g11 * g22 - g12 * g12)).max(0.0).sqrt();
            [h + disc, h - disc].iter().filter(|&&e| e > PARTICIPATION_TOL).count() as u8
        })
        .collect();

    let jt = j.transpose().to_owned();
    let left = nullspace(jt.as_ref());
    let dependent = (0..m)
        .map(|r| {
            let s: f64 = (0..left.ncols()).map(|c| left[(r, c)].powi(2)).sum();
            s > PARTICIPATION_TOL
        })
        .collect();

    Analysis { dof: k, point_dof, dependent }
}

/// Orthonormal basis of the null space of `a` (columns of the result).
fn nullspace(a: MatRef<'_, f64>) -> Mat<f64> {
    let nvars = a.ncols();
    let qr = ColPivQr::new(a);
    let r = qr.R();
    let ndiag = r.nrows().min(r.ncols());
    let largest = (0..ndiag).map(|i| r[(i, i)].abs()).fold(0.0, f64::max);
    let tol = RANK_TOL * largest.max(1.0);
    let rank = (0..ndiag).take_while(|&i| r[(i, i)].abs() > tol).count();
    let nullity = nvars - rank;
    if nullity == 0 {
        return Mat::zeros(nvars, 0);
    }

    let mut permuted = Mat::<f64>::zeros(nvars, nullity);
    for free_col in 0..nullity {
        let free_var = rank + free_col;
        permuted[(free_var, free_col)] = 1.0;
        // Solve R11 x + R12 z = 0 with one free coordinate of z set to 1.
        for i in (0..rank).rev() {
            let mut rhs = r[(i, free_var)];
            for jj in (i + 1)..rank {
                rhs += r[(i, jj)] * permuted[(jj, free_col)];
            }
            permuted[(i, free_col)] = -rhs / r[(i, i)];
        }
    }
    let mut ns = Mat::<f64>::zeros(nvars, nullity);
    permute_rows(ns.as_mat_mut(), permuted.as_mat_ref(), qr.P().inverse());
    Qr::new(ns.as_mat_ref()).compute_thin_Q()
}
