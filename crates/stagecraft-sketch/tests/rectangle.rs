use stagecraft_sketch::*;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

/// Rectangle 4000 × 3000 with the lower-left corner fixed, H/V on the sides.
fn rect() -> (Sketch, Vec<PointId>, Vec<EdgeId>, ConstraintId, ConstraintId) {
    let mut s = Sketch::new();
    let (p, e) = s.add_polyline(&[v(0.0, 0.0), v(4100.0, 50.0), v(4000.0, 2900.0), v(-30.0, 3000.0)], true);
    s.fix(p[0]);
    s.add_constraint(ConstraintKind::Horizontal { a: p[0], b: p[1] });
    s.add_constraint(ConstraintKind::Vertical { a: p[1], b: p[2] });
    s.add_constraint(ConstraintKind::Horizontal { a: p[2], b: p[3] });
    s.add_constraint(ConstraintKind::Vertical { a: p[3], b: p[0] });
    let w = s.add_dimension(Measure::Length { line: e[0] }, 4000.0);
    let h = s.add_dimension(Measure::Length { line: e[1] }, 3000.0);
    (s, p, e, w, h)
}

fn near(a: Vec2, b: Vec2) -> bool {
    a.distance(b) < 1e-4
}

#[test]
fn rectangle_solves_and_is_fully_constrained() {
    let (s, p, e, _, _) = rect();
    let out = s.solve(&SolveOptions::default()).unwrap();
    let t = &out.sketch;
    assert!(near(t.pos(p[0]), v(0.0, 0.0)));
    assert!(near(t.pos(p[1]), v(4000.0, 0.0)));
    assert!(near(t.pos(p[2]), v(4000.0, 3000.0)));
    assert!(near(t.pos(p[3]), v(0.0, 3000.0)));
    let st = out.status.unwrap();
    assert_eq!(st.dof, 0, "{st:?}");
    assert!(st.redundant.is_empty());
    for &edge in &e {
        assert_eq!(st.edge_state(t, edge), EntityState::FullyConstrained);
    }
}

#[test]
fn change_width_moves_right_edge_and_keeps_it_vertical() {
    let (s, p, _, w, _) = rect();
    let s = s.solve(&SolveOptions::default()).unwrap().sketch;
    let out = s.apply(&Edit::SetDimension { id: w, value: 6500.0 }, &SolveOptions::default()).unwrap();
    let t = &out.sketch;
    assert!(near(t.pos(p[0]), v(0.0, 0.0)));
    assert!(near(t.pos(p[3]), v(0.0, 3000.0)));
    assert!(near(t.pos(p[1]), v(6500.0, 0.0)), "{:?}", t.pos(p[1]));
    assert!(near(t.pos(p[2]), v(6500.0, 3000.0)), "{:?}", t.pos(p[2]));
    // input untouched
    assert!(near(s.pos(p[1]), v(4000.0, 0.0)));
}

#[test]
fn conflicting_dimension_is_rejected_and_named() {
    let (s, _, _, w, h) = rect();
    let s = s.solve(&SolveOptions::default()).unwrap().sketch;
    let before = s.clone();
    let p = s.points.iter().map(|p| p.id).collect::<Vec<_>>();
    let err = s
        .apply(
            &Edit::AddDimension {
                measure: Measure::Distance { a: p[0], b: p[2] },
                value: Some(6000.0),
                driving: true,
                name: Some("diagonal".into()),
                label_offset: Vec2::ZERO,
            },
            &SolveOptions::default(),
        )
        .unwrap_err();
    match &err {
        SolveError::Conflict { conflicting, culprits, descriptions } => {
            assert_eq!(culprits.len(), 1);
            assert!(conflicting.contains(&culprits[0]));
            assert!(conflicting.contains(&w) && conflicting.contains(&h), "{conflicting:?}");
            assert!(descriptions[0].contains("diagonal"), "{descriptions:?}");
        }
        other => panic!("expected conflict, got {other:?}"),
    }
    assert!(err.to_string().contains("diagonal"));
    assert_eq!(s, before);
}

#[test]
fn consistent_extra_constraint_is_accepted_as_redundant() {
    let (s, _, e, _, _) = rect();
    let s = s.solve(&SolveOptions::default()).unwrap().sketch;
    let out = s
        .apply(&Edit::AddConstraint { kind: ConstraintKind::Parallel { a: e[0], b: e[2] } }, &SolveOptions::default())
        .unwrap();
    let ItemRef::Constraint(id) = out.created[0] else { panic!() };
    let st = out.status.unwrap();
    assert_eq!(st.constraint_state(id), ConstraintState::Redundant);
    assert_eq!(st.dof, 0);
}

#[test]
fn analyse_reports_conflicts_without_failing() {
    let (mut s, p, _, _, _) = rect();
    s.add_dimension(Measure::HorizontalDistance { a: p[0], b: p[1] }, 3000.0);
    let st = s.analyse().unwrap();
    assert!(!st.conflicting.is_empty());
    assert!(s.solve(&SolveOptions::default()).is_err());
}

#[test]
fn under_constrained_dof_report() {
    let (mut s, p, e, _, h) = rect();
    s.remove_constraint(h);
    let out = s.solve(&SolveOptions::default()).unwrap();
    let st = out.status.unwrap();
    assert_eq!(st.dof, 1, "{st:?}");
    assert_eq!(st.point_dof(p[0]), Some(0));
    assert_eq!(st.point_dof(p[1]), Some(0));
    assert_eq!(st.point_dof(p[2]), Some(1));
    assert_eq!(st.point_dof(p[3]), Some(1));
    assert_eq!(st.edge_state(&out.sketch, e[0]), EntityState::FullyConstrained);
    assert_eq!(st.edge_state(&out.sketch, e[2]), EntityState::UnderConstrained);
    // a free floating point has two DOF
    let mut s2 = out.sketch.clone();
    let q = s2.add_point(v(10.0, 10.0));
    let st2 = s2.analyse().unwrap();
    assert_eq!(st2.point_dof(q), Some(2));
    assert_eq!(st2.dof, 3);
}

#[test]
fn drag_follows_cursor_within_constraints() {
    let (mut s, p, _, _, h) = rect();
    s.remove_constraint(h);
    let s = s.solve(&SolveOptions::default()).unwrap().sketch;
    let out = s.drag(p[2], v(5000.0, 4500.0), &SolveOptions::drag()).unwrap();
    assert!(!out.drag_reached);
    let t = &out.sketch;
    assert!(near(t.pos(p[2]), v(4000.0, 4500.0)), "{:?}", t.pos(p[2]));
    assert!(near(t.pos(p[3]), v(0.0, 4500.0)), "{:?}", t.pos(p[3]));
    assert!(near(t.pos(p[1]), v(4000.0, 0.0)));
    assert!(out.status.is_none());

    // fully constrained point does not move
    let out = t.drag(p[1], v(4500.0, 800.0), &SolveOptions::drag()).unwrap();
    assert!(near(out.sketch.pos(p[1]), v(4000.0, 0.0)));
}

#[test]
fn drag_free_point_reaches_cursor() {
    let mut s = Sketch::new();
    let (p, _) = s.add_polyline(&[v(0.0, 0.0), v(1000.0, 0.0)], false);
    s.fix(p[0]);
    s.add_dimension(Measure::Distance { a: p[0], b: p[1] }, 1000.0);
    let out = s.drag(p[1], v(0.0, 2000.0), &SolveOptions::drag()).unwrap();
    // stays on the circle, as close to the cursor as possible
    let q = out.sketch.pos(p[1]);
    assert!(near(q, v(0.0, 1000.0)), "{q:?}");
    let out = s.drag(p[1], v(600.0, 800.0), &SolveOptions::drag()).unwrap();
    assert!(out.drag_reached);
}

#[test]
fn angle_dimension_keeps_orientation() {
    let mut s = Sketch::new();
    let o = s.add_point(v(0.0, 0.0));
    let a = s.add_point(v(1000.0, 0.0));
    let b = s.add_point(v(800.0, 500.0));
    let c = s.add_point(v(800.0, -500.0));
    let la = s.add_line(o, a);
    let lb = s.add_line(o, b);
    let lc = s.add_line(o, c);
    s.fix(o);
    s.fix(a);
    s.add_dimension(Measure::Length { line: lb }, 1000.0);
    s.add_dimension(Measure::Length { line: lc }, 1000.0);
    s.add_dimension(Measure::Angle { a: la, b: lb }, 60.0);
    s.add_dimension(Measure::Angle { a: la, b: lc }, 45.0);
    let out = s.solve(&SolveOptions::default()).unwrap();
    let t = &out.sketch;
    let pb = t.pos(b);
    let pc = t.pos(c);
    assert!((pb.angle().to_degrees() - 60.0).abs() < 1e-6, "{pb:?}");
    assert!((pc.angle().to_degrees() + 45.0).abs() < 1e-6, "{pc:?}");
    assert_eq!(out.status.unwrap().dof, 0);
}

#[test]
fn free_geometry_stays_where_the_user_sees_it() {
    let (mut s, p, _, w, h) = rect();
    s.remove_constraint(h);
    let s = s.solve(&SolveOptions::default()).unwrap().sketch;
    let height = s.pos(p[3]).y;
    let out = s.apply(&Edit::SetDimension { id: w, value: 5000.0 }, &SolveOptions::default()).unwrap();
    // the height is free; it must not drift
    assert!((out.sketch.pos(p[3]).y - height).abs() < 1e-6);
    assert!((out.sketch.pos(p[2]).y - height).abs() < 1e-6);
}

#[test]
fn large_dimension_changes() {
    let (s, p, _, w, h) = rect();
    let s = s.solve(&SolveOptions::default()).unwrap().sketch;
    for value in [40000.0, 10.0, 123456.0] {
        let out = s.apply(&Edit::SetDimension { id: w, value }, &SolveOptions::default()).unwrap();
        assert!(near(out.sketch.pos(p[2]), v(value, 3000.0)), "{value}: {:?}", out.sketch.pos(p[2]));
    }
    // both at once in a batch
    let out = s
        .apply(
            &Edit::Batch {
                edits: vec![Edit::SetDimension { id: w, value: 20000.0 }, Edit::SetDimension { id: h, value: 500.0 }],
            },
            &SolveOptions::default(),
        )
        .unwrap();
    assert!(near(out.sketch.pos(p[2]), v(20000.0, 500.0)));
    // negative values are refused
    assert!(matches!(
        s.apply(&Edit::SetDimension { id: w, value: -1.0 }, &SolveOptions::default()),
        Err(SolveError::BadValue { .. })
    ));
}

#[test]
fn triangle_with_large_change_keeps_its_orientation() {
    // A triangle hanging off a fixed base; growing a side by 3× must not flip the apex
    // below the base.
    let mut s = Sketch::new();
    let (p, e) = s.add_polyline(&[v(0.0, 0.0), v(4000.0, 0.0), v(1500.0, 2500.0)], true);
    s.fix(p[0]);
    s.fix(p[1]);
    let a = s.add_dimension(Measure::Length { line: e[1] }, 3000.0);
    s.add_dimension(Measure::Length { line: e[2] }, 2500.0);
    let s = s.solve(&SolveOptions::default()).unwrap().sketch;
    assert!(s.pos(p[2]).y > 0.0);
    let out = s.apply(&Edit::SetDimension { id: a, value: 6000.0 }, &SolveOptions::default()).unwrap();
    let apex = out.sketch.pos(p[2]);
    assert!(apex.y > 0.0, "{apex:?}");
    assert!((apex.distance(v(4000.0, 0.0)) - 6000.0).abs() < 1e-6);
    // impossible triangle: 1000 + 2500 < 4000
    let err = s.apply(&Edit::SetDimension { id: a, value: 1000.0 }, &SolveOptions::default());
    assert!(matches!(err, Err(SolveError::Conflict { .. })), "{err:?}");
}
