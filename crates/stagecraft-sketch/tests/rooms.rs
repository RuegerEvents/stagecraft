use stagecraft_sketch::*;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn near(a: Vec2, b: Vec2) -> bool {
    a.distance(b) < 1e-4
}

fn opts() -> SolveOptions {
    SolveOptions::default()
}

/// Rectilinear polygon: H/V alternating, starting with a horizontal edge.
fn rectilinear(s: &mut Sketch, pts: &[Vec2]) -> (Vec<PointId>, Vec<EdgeId>) {
    let (p, e) = s.add_polyline(pts, true);
    for i in 0..p.len() {
        let (a, b) = (p[i], p[(i + 1) % p.len()]);
        if i % 2 == 0 {
            s.add_constraint(ConstraintKind::Horizontal { a, b });
        } else {
            s.add_constraint(ConstraintKind::Vertical { a, b });
        }
    }
    (p, e)
}

#[test]
fn l_shaped_room() {
    let mut s = Sketch::new();
    // slightly off on purpose
    let (p, e) = rectilinear(
        &mut s,
        &[v(0.0, 0.0), v(9800.0, 100.0), v(10100.0, 3900.0), v(4100.0, 4100.0), v(3900.0, 9100.0), v(50.0, 8900.0)],
    );
    s.fix(p[0]);
    let width = s.add_named_dimension("width", Measure::Length { line: e[0] }, 10000.0);
    s.add_dimension(Measure::Length { line: e[5] }, 9000.0);
    s.add_dimension(Measure::Length { line: e[1] }, 4000.0);
    s.add_dimension(Measure::Length { line: e[4] }, 4000.0);
    let out = s.solve(&opts()).unwrap();
    assert_eq!(out.status.as_ref().unwrap().dof, 0);
    let s = out.sketch;
    let expect =
        [v(0.0, 0.0), v(10000.0, 0.0), v(10000.0, 4000.0), v(4000.0, 4000.0), v(4000.0, 9000.0), v(0.0, 9000.0)];
    for (id, want) in p.iter().zip(expect) {
        assert!(near(s.pos(*id), want), "{:?} vs {want:?}", s.pos(*id));
    }
    let loops = s.closed_loops();
    assert_eq!(loops.len(), 1);
    assert!((loops[0].area - 60.0e6).abs() < 1.0, "{}", loops[0].area);
    assert_eq!(loops[0].edges.len(), 6);

    let out = s.apply(&Edit::SetDimension { id: width, value: 12000.0 }, &opts()).unwrap();
    let t = &out.sketch;
    assert!(near(t.pos(p[1]), v(12000.0, 0.0)));
    assert!(near(t.pos(p[2]), v(12000.0, 4000.0)));
    assert!(near(t.pos(p[3]), v(4000.0, 4000.0)));
    assert!(near(t.pos(p[4]), v(4000.0, 9000.0)));
    assert!((t.closed_loops()[0].area - 68.0e6).abs() < 1.0);

    // a diagonal splits the L into two faces
    let mut u = t.clone();
    u.add_line(p[3], p[0]);
    let loops = u.closed_loops();
    assert_eq!(loops.len(), 2);
    let total: f64 = loops.iter().map(|l| l.area).sum();
    assert!((total - 68.0e6).abs() < 1.0);
}

#[test]
fn polygon_follows_portal_opening() {
    let mut s = Sketch::new();
    // centre line (construction), fixed
    let c0 = s.add_point(v(0.0, -2000.0));
    let c1 = s.add_point(v(0.0, 10000.0));
    let centre = s.add_line(c0, c1);
    s.edge_mut(centre).unwrap().construction = true;
    s.fix(c0);
    s.fix(c1);
    // portal opening
    let pl = s.add_point(v(-6400.0, 100.0));
    let pr = s.add_point(v(6600.0, -100.0));
    s.add_constraint(ConstraintKind::Symmetric { a: pl, b: pr, axis: centre });
    s.add_constraint(ConstraintKind::Horizontal { a: pl, b: pr });
    let portal = s.add_named_dimension("portal width", Measure::HorizontalDistance { a: pl, b: pr }, 13000.0);
    s.add_dimension(Measure::VerticalDistance { a: c0, b: pl }, 2000.0);
    // main stage hangs on the portal: back corners straight behind it
    let br = s.add_point(v(6500.0, 8400.0));
    let bl = s.add_point(v(-6500.0, 8600.0));
    let e0 = s.add_line(pl, pr);
    s.add_line(pr, br);
    s.add_line(br, bl);
    s.add_line(bl, pl);
    s.add_constraint(ConstraintKind::Vertical { a: pr, b: br });
    s.add_constraint(ConstraintKind::Vertical { a: pl, b: bl });
    s.add_constraint(ConstraintKind::Horizontal { a: bl, b: br });
    s.add_dimension(Measure::VerticalDistance { a: pr, b: br }, 8500.0);
    // a reference dimension measures the stage width
    let r = s.add_reference(Measure::Length { line: e0 });

    let out = s.solve(&opts()).unwrap();
    assert_eq!(out.status.as_ref().unwrap().dof, 0, "{:?}", out.status);
    let s = out.sketch;
    assert!(near(s.pos(pl), v(-6500.0, 0.0)));
    assert!(near(s.pos(bl), v(-6500.0, 8500.0)));
    assert!((s.dimension(r).unwrap().value - 13000.0).abs() < 1e-6);

    let out = s.apply(&Edit::SetDimension { id: portal, value: 15000.0 }, &opts()).unwrap();
    let t = &out.sketch;
    assert!(near(t.pos(pl), v(-7500.0, 0.0)));
    assert!(near(t.pos(pr), v(7500.0, 0.0)));
    assert!(near(t.pos(br), v(7500.0, 8500.0)));
    assert!(near(t.pos(bl), v(-7500.0, 8500.0)));
    assert!((t.dimension(r).unwrap().value - 15000.0).abs() < 1e-6);
    let loops = t.closed_loops();
    assert_eq!(loops.len(), 1);
    assert!((loops[0].area - 15000.0 * 8500.0).abs() < 1.0);

    // a reference dimension cannot be set
    assert_eq!(t.apply(&Edit::SetDimension { id: r, value: 1.0 }, &opts()), Err(SolveError::NotDriving(r)));
}

#[test]
fn drawing_gestures_infer_constraints() {
    let mut s = Sketch::new();
    let io = InferOptions::default();
    // 1: nearly horizontal → horizontal
    let plan =
        infer_line(&s, &LineGesture { start: v(0.0, 0.0), end: v(3000.0, 40.0), axis_lock: false, length: None }, &io);
    assert_eq!(plan.axis, Some(Axis::Horizontal));
    let out = s.apply(&Edit::DrawLine { plan }, &opts()).unwrap();
    s = out.sketch;
    let first = s.points[0].id;
    let second = s.points[1].id;
    assert!(near(s.pos(second), v(3000.0, 0.0)));
    s.fix(first);

    // 2: from the end (snapped), typed length, Shift → vertical with a dimension
    let plan = infer_line(
        &s,
        &LineGesture { start: v(3030.0, 20.0), end: v(2500.0, 1900.0), axis_lock: true, length: Some(2500.0) },
        &io,
    );
    assert_eq!(plan.start, Endpoint::Existing { point: second });
    assert_eq!(plan.axis, Some(Axis::Vertical));
    assert_eq!(plan.length, Some(2500.0));
    let out = s.apply(&Edit::DrawLine { plan }, &opts()).unwrap();
    s = out.sketch;
    let third = s.points[2].id;
    assert!(near(s.pos(third), v(3000.0, 2500.0)));
    assert_eq!(s.dimensions.len(), 1);

    // 3: free line, then close onto the first point (snap → shared point)
    let plan = infer_line(
        &s,
        &LineGesture { start: v(3000.0, 2500.0), end: v(800.0, 3100.0), axis_lock: false, length: None },
        &io,
    );
    assert_eq!(plan.axis, None);
    s = s.apply(&Edit::DrawLine { plan }, &opts()).unwrap().sketch;
    let fourth = s.points[3].id;
    let plan = infer_line(
        &s,
        &LineGesture { start: v(800.0, 3100.0), end: v(60.0, -50.0), axis_lock: false, length: None },
        &io,
    );
    assert_eq!(plan.start, Endpoint::Existing { point: fourth });
    assert_eq!(plan.end, Endpoint::Existing { point: first });
    s = s.apply(&Edit::DrawLine { plan }, &opts()).unwrap().sketch;
    assert_eq!(s.points.len(), 4);
    assert_eq!(s.closed_loops().len(), 1);

    // 4: start on an existing line → point on line
    let plan = infer_line(
        &s,
        &LineGesture { start: v(1500.0, 30.0), end: v(1500.0, 1500.0), axis_lock: false, length: None },
        &io,
    );
    assert!(matches!(plan.start, Endpoint::New { on_line: Some(_), .. }));
    assert_eq!(plan.axis, Some(Axis::Vertical));
    let out = s.apply(&Edit::DrawLine { plan }, &opts()).unwrap();
    let st = out.status.unwrap();
    assert!(st.conflicting.is_empty());
    let p = out.sketch.points[4].pos;
    assert!(p.y.abs() < 1e-6, "{p:?}");
}

#[test]
fn curved_apron_with_arcs_and_remaining_constraints() {
    // Apron: straight sides, a semicircular front tangent to them, symmetric about the
    // centre line.
    let mut s = Sketch::new();
    let o = s.add_point(v(0.0, 0.0));
    let up = s.add_point(v(0.0, 5000.0));
    let centre = s.add_line(o, up);
    s.edge_mut(centre).unwrap().construction = true;
    s.fix(o);
    s.fix(up);
    let l0 = s.add_point(v(-5100.0, 50.0));
    let l1 = s.add_point(v(-4900.0, -2100.0));
    let r0 = s.add_point(v(4900.0, 0.0));
    let r1 = s.add_point(v(5050.0, -1900.0));
    let c = s.add_point(v(100.0, -2000.0));
    let left = s.add_line(l0, l1);
    let right = s.add_line(r1, r0);
    let top = s.add_line(r0, l0);
    let arc = s.add_arc(c, l1, r1);
    s.add_constraint(ConstraintKind::Symmetric { a: l0, b: r0, axis: centre });
    s.add_constraint(ConstraintKind::Horizontal { a: o, b: l0 });
    let width = s.add_dimension(Measure::HorizontalDistance { a: l0, b: r0 }, 10000.0);
    s.add_constraint(ConstraintKind::Vertical { a: l0, b: l1 });
    s.add_constraint(ConstraintKind::Parallel { a: right, b: centre });
    s.add_constraint(ConstraintKind::EqualLength { a: left, b: right });
    s.add_dimension(Measure::Length { line: left }, 2000.0);
    s.add_constraint(ConstraintKind::Tangent { line: left, arc });
    let m = s.add_point(v(10.0, 20.0));
    s.add_constraint(ConstraintKind::Midpoint { point: m, line: top });
    let q = s.add_point(v(300.0, -6900.0));
    s.add_constraint(ConstraintKind::PointOnArc { point: q, arc });
    s.add_constraint(ConstraintKind::PointOnLine { point: q, line: centre });
    let radius = s.add_reference(Measure::Radius { arc });

    let out = s.solve(&opts()).unwrap();
    let st = out.status.unwrap();
    assert_eq!(st.dof, 0, "{st:?}");
    assert!(st.redundant.is_empty(), "{st:?}");
    let t = out.sketch;
    assert!(near(t.pos(c), v(0.0, -2000.0)), "{:?}", t.pos(c));
    assert!(near(t.pos(r1), v(5000.0, -2000.0)));
    assert!(near(t.pos(q), v(0.0, -7000.0)), "{:?}", t.pos(q));
    assert!(near(t.pos(m), v(0.0, 0.0)));
    assert!((t.dimension(radius).unwrap().value - 5000.0).abs() < 1e-6);
    let loops = t.closed_loops();
    assert_eq!(loops.len(), 1);
    let want = 10000.0 * 2000.0 + std::f64::consts::PI * 5000.0f64.powi(2) / 2.0;
    // arcs are split into 5° pieces for the area
    assert!((loops[0].area - want).abs() / want < 2e-3, "{} vs {want}", loops[0].area);
    let outline = t.loop_outline(&loops[0], 1.0);
    assert!(outline.len() > 180);

    // the right tangent is implied by symmetry: accepted, reported as redundant
    let out = t.apply(&Edit::AddConstraint { kind: ConstraintKind::Tangent { line: right, arc } }, &opts()).unwrap();
    let ItemRef::Constraint(tangent) = out.created[0] else { panic!() };
    assert_eq!(out.status.unwrap().constraint_state(tangent), ConstraintState::Redundant);

    // wider apron: radius follows
    let out = t.apply(&Edit::SetDimension { id: width, value: 12000.0 }, &opts()).unwrap();
    let u = out.sketch;
    assert!((u.dimension(radius).unwrap().value - 6000.0).abs() < 1e-6);
    assert!(near(u.pos(q), v(0.0, -8000.0)), "{:?}", u.pos(q));
}

#[test]
fn bad_references_are_errors() {
    let mut s = Sketch::new();
    let a = s.add_point(v(0.0, 0.0));
    let b = s.add_point(v(1.0, 0.0));
    let c = s.add_point(v(1.0, 1.0));
    let l = s.add_line(a, b);
    let arc = s.add_arc(a, b, c);
    let e = s.apply(&Edit::AddConstraint { kind: ConstraintKind::Parallel { a: l, b: arc } }, &opts()).unwrap_err();
    assert!(matches!(e, SolveError::WrongEdgeKind { .. }), "{e:?}");
    let e =
        s.apply(&Edit::AddConstraint { kind: ConstraintKind::Coincident { a, b: PointId(999) } }, &opts()).unwrap_err();
    assert_eq!(e, SolveError::Missing(ItemRef::Point(PointId(999))));
    let e = s.apply(&Edit::Remove { item: ItemRef::Edge(EdgeId(999)) }, &opts()).unwrap_err();
    assert_eq!(e, SolveError::Missing(ItemRef::Edge(EdgeId(999))));
    // removing a point removes its edges and constraints
    s.add_constraint(ConstraintKind::Horizontal { a, b });
    let out = s.apply(&Edit::Remove { item: ItemRef::Point(a) }, &opts()).unwrap();
    assert!(out.sketch.edges.is_empty());
    assert!(out.sketch.constraints.is_empty());
}
