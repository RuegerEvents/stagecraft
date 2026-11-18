# stagecraft-sketch

Parametric 2D sketch for StageCraft floor plans (block 9, `docs/spec/sketch.md`):
points, lines and arcs with geometric constraints and driving or reference dimensions,
solved with [ezpz](https://github.com/KittyCAD/ezpz) 0.2.29 by Zoo. Closed loops become
floors; edges carry an opaque tag for wall properties.

Standalone for now (excluded from the workspace, no StageCraft dependencies). Builds for
`wasm32-unknown-unknown`. Every public type is `serde` + `schemars::JsonSchema`, so the
sketch can be document data and edits can be command arguments.

```sh
cd crates/stagecraft-sketch
CARGO_TARGET_DIR=../../target-sketch cargo test
CARGO_TARGET_DIR=../../target-sketch cargo test --release --test perf -- --nocapture
CARGO_TARGET_DIR=../../target-sketch cargo check --target wasm32-unknown-unknown
```

Units: millimetres and degrees. Axes as in StageCraft (x right, y upstage).

## Model

```rust
pub struct Sketch {
    pub points: Vec<Point>,           // Point { id: PointId, pos: Vec2 }
    pub edges: Vec<Edge>,             // Edge { id, geom, construction, tag }
    pub constraints: Vec<Constraint>, // Constraint { id: ConstraintId, kind }
    pub dimensions: Vec<Dimension>,   // driving or reference
    pub next_id: u32,                 // one id counter for everything; ids never reused
}

pub enum EdgeGeom {
    Line { a: PointId, b: PointId },
    Arc { center: PointId, start: PointId, end: PointId }, // counter-clockwise
}

pub enum ConstraintKind {
    Fixed { point, at }, Coincident { a, b }, Horizontal { a, b }, Vertical { a, b },
    Parallel { a: EdgeId, b: EdgeId }, Perpendicular { a, b }, EqualLength { a, b },
    Midpoint { point, line }, PointOnLine { point, line }, PointOnArc { point, arc },
    Tangent { line, arc },            // at their shared end point
    Symmetric { a, b, axis },
}

pub struct Dimension {
    pub id: ConstraintId,
    pub measure: Measure,  // Distance, Length, HorizontalDistance, VerticalDistance,
                           // PointLineDistance, Angle, Radius
    pub value: f64,        // magnitude; direction comes from the current geometry
    pub driving: bool,     // false = reference: re-measured after every solve
    pub label_offset: Vec2,// from the measured geometry's midpoint
    pub name: Option<String>, // parameter name, e.g. "portal width"
}
```

Lines and arcs share point ids, so a polyline's corners need no coincident constraints.
Construction edges (centre lines) take part in constraints but not in loop detection.

Building without solving:

```rust
let mut s = Sketch::new();
let (p, e) = s.add_polyline(&[Vec2::new(0., 0.), Vec2::new(4000., 0.),
                              Vec2::new(4000., 3000.), Vec2::new(0., 3000.)], true);
s.fix(p[0]);
s.add_constraint(ConstraintKind::Horizontal { a: p[0], b: p[1] });
s.add_constraint(ConstraintKind::Vertical { a: p[1], b: p[2] });
s.add_constraint(ConstraintKind::Horizontal { a: p[2], b: p[3] });
let width = s.add_named_dimension("width", Measure::Length { line: e[0] }, 4000.0);
s.add_dimension(Measure::Length { line: e[1] }, 3000.0);
let diag = s.add_reference(Measure::Distance { a: p[0], b: p[2] });
```

Also: `add_point`, `add_line`, `add_arc`, `add_edge`, `add_dimension_full`,
`remove_constraint`, `remove_edge`, `remove_point` (cascading), lookups
(`point`, `pos`, `edge`, `constraint`, `dimension`, `dimension_named`, `line_ends`),
`measure(&Measure)`, `measure_anchor`, `references_to_point`, `references_to_edge`,
`lines_at`.

## Solving and edits

```rust
let opts = SolveOptions::default();          // analyse: true
let solved = s.solve(&opts)?;                // Solved { sketch, status, drag_reached, iterations, created }
let s = solved.sketch;

let solved = s.apply(&Edit::SetDimension { id: width, value: 6500.0 }, &opts)?;
// or s.apply_in_place(&edit, &opts)? — replaces `s` only on success
```

`Sketch::apply(&self, &Edit, &SolveOptions) -> Result<Solved, SolveError>` works on a
copy and solves from the current positions, so the result stays close to what the user
sees; on error `self` is unchanged (the engine's command rollback has nothing to undo).

`Edit` (serde tag `op`): `resolve`, `drag { targets }`, `set_dimension { id, value }`,
`set_driving { id, driving }`, `set_label_offset`, `set_dimension_name`,
`add_point { at }`, `add_edge { geom, construction, tag }`, `add_constraint { kind }`,
`add_dimension { measure, value?, driving, name, label_offset }`, `remove { item }`,
`set_edge_tag`, `set_construction`, `draw_line { plan }`, `batch { edits }` (solved
once). `Solved::created` lists the ids an edit created.

Large dimension changes are walked in 4 or 16 steps when a direct solve fails, so the
geometry stays on the branch the user sees.

### Status (FreeCAD colours)

With `analyse: true`, `Solved::status` is a `Status`:

- `dof`: degrees of freedom of the whole sketch (0 = fully constrained),
- `points: Vec<PointDof>`: 0, 1 or 2 per point; `point_state(id)` /
  `edge_state(&sketch, id)` → `FullyConstrained` (green) or `UnderConstrained` (normal),
- `redundant`: constraints implied by others (accepted),
- `conflicting`: constraints that contradict (red). `constraint_state(id)` gives
  `Ok` / `Redundant` / `Conflicting`.

`Sketch::analyse()` reports the status of a sketch as it is without failing on conflicts
(for showing a broken file).

### Errors

`SolveError::Conflict { conflicting, culprits, descriptions }` names every unsatisfied
constraint, the ones the edit touched first; `Display` reads e.g.
`conflicting constraints: dimension <id> "Vor Holz-Portal" (horizontal distance 14500 mm),
dimension <id> "portal width" (horizontal distance 13000 mm), …`.
Others: `Missing(ItemRef)`, `WrongEdgeKind`, `NotTouching`, `BadValue`, `NotDriving`,
`NotConverged`, `Solver`.

## Drawing helpers

```rust
let plan = infer_line(&s, &LineGesture { start, end, axis_lock: shift, length: typed },
                      &InferOptions { snap_radius: 8.0 * mm_per_px, ..Default::default() });
// plan.start / plan.end: Endpoint::Existing { point } (snapped) or New { at, on_line }
// plan.axis: Some(Horizontal | Vertical); plan.length: Some(mm) → driving dimension
let solved = s.apply(&Edit::DrawLine { plan }, &opts)?;
```

- snap to an existing point → the line uses that point (coincident),
- snap onto a line → new point with point-on-line,
- Shift (`axis_lock`) → horizontal or vertical, whichever is closer; without Shift within
  `auto_axis_deg` (default 2°),
- typed length → driving length dimension (end not snapped).

Dragging: `s.drag(point, cursor, &SolveOptions::drag())` (or `Edit::Drag`) soft-fixes the
point to the cursor; it follows as far as the constraints allow
(`Solved::drag_reached`). Use `SolveOptions::drag()` (no analysis) per mouse move and a
normal solve on release.

## Floors

`Sketch::closed_loops() -> Vec<Loop>` traces the bounded faces of the non-construction
edges (coincident points merged, dangling edges ignored), counter-clockwise, with
`edges` (`LoopEdge { edge, reversed }`), `points` and `area` (mm², arcs included).
`loop_outline(&loop, max_step_deg)` gives the polygon with arcs split. Islands are
reported as separate loops; holes are the caller's business.

## Performance

Room with 200 points and 399 constraints/dimensions (`tests/perf.rs`, Apple M-series,
release): drag step ~1.1 ms mean, 1.3 ms worst (free point), ~1.3 ms for a held point;
dimension change 0.3 ms; full solve with analysis ~17 ms (the dense QR analysis is
~12 ms of it).

## Limits

See `DECISIONS.md` for the reasoning behind each choice and the known weak spots.
