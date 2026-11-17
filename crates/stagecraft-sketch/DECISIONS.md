# stagecraft-sketch decisions

Decisions taken while building the crate unattended (2026-11-18). The user's binding
decisions are in `docs/spec/sketch.md`. Newest last.

## ezpz: what it offers (0.2.29)

The crate was published as `kcl-ezpz` up to 0.2.18 and is now `ezpz` (0.2.29, same
repo KittyCAD/ezpz, MIT). `ezpz` 0.0.1 on crates.io is an unrelated project generator;
the version requirement `0.2.29` avoids it.

- **Entities** (`datatypes::inputs`): `DatumPoint`, `DatumLineSegment`, `DatumCircle`
  (centre + radius variable), `DatumCircularArc` (centre, start, end; counter-clockwise),
  `DatumDistance` (a free scalar).
- **Constraints**: `Fixed` (one scalar), `PointsCoincident`, `Horizontal`, `Vertical`,
  `Distance`, `DistanceVar`, `HorizontalDistance` / `VerticalDistance` (signed:
  `p.x − q.x = d`), `LinesAtAngle` (`Parallel`, `Perpendicular`, `Other(angle)`; modulo
  180°), `PointsAtAngle` (oriented angle at a vertex), `LinesEqualLength`, `Midpoint`,
  `PointLineDistance` (signed, left of p0→p1 positive), `Vertical/HorizontalPointLineDistance`,
  `Symmetric`, `ScalarEqual`, `Arc` (start/end equidistant from centre), `ArcRadius`,
  `ArcLength`, `ArcAngle`, `PointArcCoincident`, `CircleRadius`, `LineTangentToCircle`,
  `CircleTangentToCircle`.
- **Solver**: Levenberg–Marquardt on a sparse Jacobian (faer 0.24), convergence when the
  largest residual ≤ 1e-8. Priorities (lower number first; a lower priority that cannot
  be met is dropped) and per-request weights. Variable ids must be `0..n` and the
  guesses given in id order (`Layout::index_of(id) = id`).
- **Analysis**: `solve_analysis` returns under-constrained variable ids (dense QR of the
  Jacobian). There is no redundancy/conflict analysis; a failed system only reports
  which constraints stayed unsatisfied.
- **WASM**: pure Rust (faer without default features, libm); builds for
  `wasm32-unknown-unknown`.

| Topic | Decision | Why |
|---|---|---|
| Solver crate | `ezpz = "0.2.29"`, no optional features. `faer 0.24` directly (same version ezpz uses, no extra build) for our own dense QR. | Current name and version of Zoo's solver; spec names it. |
| Entities | Points, lines and arcs. Circles (radius as a solver variable) left out. | Floors and walls are lines and arcs; a round stage can be two arcs. Circles would need scalar variables besides points; add later if needed. |
| Shared end points | Lines and arcs reference point ids, so a polyline shares its corners. Snapping while drawing reuses the existing point instead of adding a point plus a `Coincident` constraint. `Coincident` still exists for points created separately (imports, AI). | Fewer variables and equations; loop detection is a graph walk on point ids. Equivalent to FreeCAD's coincident for the user. |
| Ids | `PointId`, `EdgeId`, `ConstraintId` are `u32` newtypes from one counter in the sketch (`next_id`), never reused. Dimensions share the `ConstraintId` space. | Stable across edits, short in JSON for AIs; one id space means a conflict report can name constraints and dimensions alike. |
| Storage | `Vec`s of structs with an `id` field, not maps. | Readable JSON, clean JSON schema; sketches are small. |
| Dimensions vs constraints | Two lists. `Constraint` = geometric relation (only `Fixed` carries a position). `Dimension` = measure + value + `driving` flag + label offset + optional parameter name. A driving dimension becomes an equation; a reference dimension is re-measured after every solve. | Matches spec ("dimensions are driving", "reference dimensions measure live"); one toggle converts. |
| Dimension values | Always magnitudes (≥ 0). Direction for horizontal/vertical, point-line distance and angle is taken from the geometry at solve time (sign of the current difference; for angles ±value, whichever is closer modulo 180°). | Users type "13 m", not "−13 m"; the geometry never flips because a constraint holds the sign while it is satisfied. |
| Horizontal/Vertical | Defined on two points, not on a line. | Covers both "this line is horizontal" (its end points) and aligning separate corners. |
| Tangent | Line–arc tangency where they share an end point, as "line ⟂ (centre → shared point)". | ezpz's tangent works on circles with a radius variable; this form needs none and covers the floor-plan case (fillet, curved apron). |
| Units | Millimetres in the solver as in the document; no rescaling. | Residuals are lengths (angles: ezpz scales by mean line length), so the tolerances mean µm; LM converged fine on 20 m rooms. |
| Own residuals | Every constraint is also written as our own scalar equations (generic over `f64` and a forward-mode dual number) for the satisfaction check and an exact Jacobian. | ezpz does not expose its residuals or Jacobian, and some of its residuals have extra rows (point-on-arc has 2 rows of rank 1) that would look like redundancy. |
| Satisfied | |residual| ≤ 1e-6 mm (angles: mm-equivalent). | LM converges to 1e-8; 1e-6 leaves room. |
| DOF | Right null space of the Jacobian at the solution (column-pivoted QR, like ezpz `find_dof.rs`). Per point: rank of its two rows in the orthonormal null-space basis (0, 1 or 2). Edge fully constrained = all its points (arc centre included) have 0 DOF. Points used by nothing count 2. | Gives FreeCAD's green/normal per entity, and "1 DOF" (slides on a curve) for UI hints. |
| Redundant / conflicting | Left null space of the Jacobian: an equation with participation > 1e-6 is part of a dependent set. Unsatisfied after the least-squares solve = conflicting (at a least-squares optimum the residual vector lies in the left null space, so the unsatisfied rows are exactly a dependent, contradictory set). Dependent but satisfied = redundant. | ezpz has no such analysis. Redundant constraints are accepted (reported, not rejected); only contradictions reject an edit. |
| Conflict error | `SolveError::Conflict { conflicting, culprits, descriptions }`: all unsatisfied constraints, the ones the edit touched, and a text naming each (dimension names included). | Spec: "a message naming the conflicting constraint". The touched constraint is the natural one to blame. |
| Rollback | `Sketch::apply(&self, edit)` works on a copy and returns a new sketch; on error the input is untouched. `apply_in_place` replaces only on success. | Fits the engine's command/undo model; nothing to roll back. |
| Large changes | If a driving dimension's target differs from its current value, the solve is retried in 4 and then 16 steps (all dimensions interpolated together) before declaring a conflict. | LM from far away can fail or jump to a folded configuration; walking keeps the branch the user sees. |
| Drag | Dragged point gets `Fixed x/y` requests with weight 1 at the same priority. If the hard constraints are not met afterwards (target unreachable), a second solve without the drag projects back onto them. `drag_reached` tells the UI. | ezpz priorities drop the whole lower tier when it cannot be met, so a held point would not move at all; a weighted least-squares step plus projection moves it as close as the constraints allow. Weight 0.1 converged too slowly in the tangential direction (2.7 mm off after the projection), 1.0 is exact in the tests. |
| Analysis cost | `SolveOptions::analyse` (default on). `SolveOptions::drag()` switches it off for drag steps. | DOF does not change while dragging; analyse once on release. |
