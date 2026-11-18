mod common;

use stagecraft_sketch::*;

#[test]
fn sketch_round_trips_through_json() {
    let (mut s, ids, _) = common::comb_room(3);
    // cover every kind of constraint, dimension and edge
    let c = s.add_point(Vec2::new(100.0, 100.0));
    let a0 = s.add_point(Vec2::new(200.0, 100.0));
    let a1 = s.add_point(Vec2::new(100.0, 200.0));
    let arc = s.add_arc(c, a0, a1);
    let l = s.add_line(ids[0], ids[1]);
    let l2 = s.add_line(ids[1], ids[2]);
    s.edge_mut(l).unwrap().tag = Some("wall:01HZX".into());
    s.edge_mut(l2).unwrap().construction = true;
    let kinds = [
        ConstraintKind::Coincident { a: ids[0], b: ids[1] },
        ConstraintKind::Parallel { a: l, b: l2 },
        ConstraintKind::Perpendicular { a: l, b: l2 },
        ConstraintKind::EqualLength { a: l, b: l2 },
        ConstraintKind::Midpoint { point: c, line: l },
        ConstraintKind::PointOnLine { point: c, line: l },
        ConstraintKind::PointOnArc { point: c, arc },
        ConstraintKind::Tangent { line: l, arc },
        ConstraintKind::Symmetric { a: a0, b: a1, axis: l },
    ];
    for k in kinds {
        s.add_constraint(k);
    }
    let measures = [
        Measure::Distance { a: ids[0], b: ids[2] },
        Measure::HorizontalDistance { a: ids[0], b: ids[2] },
        Measure::VerticalDistance { a: ids[0], b: ids[2] },
        Measure::PointLineDistance { point: c, line: l },
        Measure::Angle { a: l, b: l2 },
        Measure::Radius { arc },
    ];
    for m in measures {
        s.add_reference(m);
    }
    let d = s.add_named_dimension("portal width", Measure::Length { line: l }, 13000.0);
    s.dimension_mut(d).unwrap().label_offset = Vec2::new(0.0, -500.0);

    let json = serde_json::to_string_pretty(&s).unwrap();
    let back: Sketch = serde_json::from_str(&json).unwrap();
    assert_eq!(back, s);
    assert!(json.contains("\"type\": \"symmetric\""));
    assert!(json.contains("portal width"));

    // edits and plans are command arguments
    let edits = Edit::Batch {
        edits: vec![
            Edit::SetDimension { id: d, value: 14000.0 },
            Edit::Drag { targets: vec![DragTarget { point: c, to: Vec2::new(1.0, 2.0) }] },
            Edit::AddDimension {
                measure: Measure::Length { line: l2 },
                value: None,
                driving: false,
                name: None,
                label_offset: Vec2::ZERO,
            },
            Edit::DrawLine {
                plan: LinePlan {
                    start: Endpoint::Existing { point: c },
                    end: Endpoint::New { at: Vec2::new(5.0, 5.0), on_line: Some(l) },
                    axis: Some(Axis::Vertical),
                    length: Some(5.0),
                },
            },
            Edit::Remove { item: ItemRef::Constraint(d) },
        ],
    };
    let json = serde_json::to_string(&edits).unwrap();
    assert_eq!(serde_json::from_str::<Edit>(&json).unwrap(), edits);

    // a minimal hand-written sketch (as an AI would send it) parses with defaults
    let hand = r#"{
        "points": [{"id": 1, "pos": {"x": 0, "y": 0}}, {"id": 2, "pos": {"x": 1000, "y": 0}}],
        "edges": [{"id": 3, "geom": {"type": "line", "a": 1, "b": 2}}],
        "constraints": [{"id": 4, "kind": {"type": "horizontal", "a": 1, "b": 2}}],
        "dimensions": [{"id": 5, "measure": {"type": "length", "line": 3}, "value": 2000}]
    }"#;
    let mut h: Sketch = serde_json::from_str(hand).unwrap();
    assert!(h.dimensions[0].driving);
    let out = h.solve(&SolveOptions::default()).unwrap();
    assert!((out.sketch.pos(PointId(2)).x - out.sketch.pos(PointId(1)).x).abs() > 1999.0);
    // the stale counter is repaired on the next add
    assert_eq!(h.add_point(Vec2::ZERO), PointId(6));

    // schemas exist for the document and the command arguments
    let schema = serde_json::to_string(&schemars::schema_for!(Sketch)).unwrap();
    assert!(schema.contains("point_on_arc"));
    let schema = serde_json::to_string(&schemars::schema_for!(Edit)).unwrap();
    assert!(schema.contains("draw_line"));
    let _ = schemars::schema_for!(SolveError);
    let _ = schemars::schema_for!(Status);
}
