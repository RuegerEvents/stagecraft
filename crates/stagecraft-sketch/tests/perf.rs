//! Interactive performance: ~200 points, ~400 constraints. The 30 ms bound is only
//! asserted in release builds (`cargo test --release --test perf -- --nocapture`).

mod common;

use stagecraft_sketch::*;
use std::time::Instant;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

#[test]
fn room_with_200_points_drags_interactively() {
    let (s, ids, dims) = common::comb_room(49);
    let n_constraints = s.constraints.len() + s.dimensions.len();
    assert_eq!(s.points.len(), 200);
    assert!(n_constraints >= 390, "{n_constraints}");

    let t = Instant::now();
    let out = s.solve(&SolveOptions::default()).unwrap();
    let first = ms(t);
    assert_eq!(out.status.as_ref().unwrap().dof, 0);
    let s = out.sketch;

    let t = Instant::now();
    let st = s.analyse().unwrap();
    let analyse = ms(t);
    assert_eq!(st.dof, 0);

    let t = Instant::now();
    let out = s.apply(&Edit::SetDimension { id: dims[0], value: 900.0 }, &SolveOptions::drag()).unwrap();
    let dim_change = ms(t);
    assert!((out.sketch.pos(ids[1]).x - 900.0).abs() < 1e-6);

    // under-constrained variant: drop every tenth dimension, drag a free corner
    let mut u = s.clone();
    for d in dims.iter().step_by(10) {
        u.remove_constraint(*d);
    }
    let u = u.solve(&SolveOptions::default()).unwrap();
    let free_dof = u.status.unwrap().dof;
    let mut u = u.sketch;
    let p = ids[100];
    let start = u.pos(p);
    let mut worst_free: f64 = 0.0;
    let mut total_free = 0.0;
    let steps = 30;
    for k in 1..=steps {
        let to = start + Vec2::new(10.0 * k as f64, 7.0 * k as f64);
        let t = Instant::now();
        let out = u.drag(p, to, &SolveOptions::drag()).unwrap();
        let dt = ms(t);
        worst_free = worst_free.max(dt);
        total_free += dt;
        u = out.sketch;
    }

    // worst case: dragging a fully constrained point (target unreachable)
    let mut worst_held: f64 = 0.0;
    let q = ids[50];
    let qs = s.pos(q);
    for k in 1..=10 {
        let t = Instant::now();
        let out = s.drag(q, qs + Vec2::new(50.0 * k as f64, 0.0), &SolveOptions::drag()).unwrap();
        worst_held = worst_held.max(ms(t));
        assert!(out.sketch.pos(q).distance(qs) < 1e-4);
    }

    println!(
        "points {}, constraints+dimensions {n_constraints}\n\
         first solve + analysis {first:.2} ms, analysis alone {analyse:.2} ms, \
         dimension change {dim_change:.2} ms\n\
         drag free point (dof {free_dof}): mean {:.2} ms, worst {worst_free:.2} ms\n\
         drag held point: worst {worst_held:.2} ms",
        s.points.len(),
        total_free / steps as f64
    );
    if !cfg!(debug_assertions) {
        assert!(worst_free < 30.0, "drag step {worst_free} ms");
        assert!(worst_held < 30.0, "held drag step {worst_held} ms");
    }
}
