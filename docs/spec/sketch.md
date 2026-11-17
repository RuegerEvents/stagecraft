# Parametric floor-plan sketch (block 9)

Settled with the user on 2026-11-17 in a second design round. Built last, after blocks
2–8.

## What it is

- **One sketch per layer**, like the FreeCAD / KiCad sketcher: points, lines and (if
  the solver supports them) arcs, plus constraints. The room outline, walls, stage areas
  and the orchestra pit are edges of that sketch.
- **Floors and walls follow the sketch.** The wall and floor tools draw into the layer's
  sketch; a wall is a sketch edge with thickness and height, a floor is a closed loop.
  Free floors/walls (from MVR import or created by an AI) stay possible and can be
  adopted into the sketch with a command.
- Other objects (truss, fixtures, platforms …) are not constrained to the sketch.
  Cross-object constraints are out of scope.

## Solver

[ezpz](https://zoo.dev/blog/announcing-solver) by Zoo/KittyCAD (crate `kcl-ezpz`):
Rust, MIT, runs in WASM. Constraints it offers include fixed, horizontal, vertical,
distance, horizontal/vertical distance, coincident, parallel, perpendicular, equal
length, midpoint. Arcs if ezpz supports them, otherwise later. Not PlaneGCS (C++ with
Eigen/Boost, LGPL, hard for WASM), although KiCad adopts PlaneGCS for v11.

## Creating constraints

- Automatic while drawing: snapping to a corner = coincident, Shift = horizontal or
  vertical, a typed length = a distance dimension.
- **Dimensions are driving.** Every driving dimension is a visible, editable dimension
  object; double-click, type a new value, and everything attached follows.
- **Reference dimensions** also exist: they measure live between any points or objects
  (e.g. truss to portal) and drive nothing. Both kinds appear in plan views and on
  sheets.
- Constraints and dimensions are commands like everything else (`sketch.*`,
  `dimension.*`), so an AI can build a parametric room.

## Over- and under-constrained

- Colours as in FreeCAD: fully constrained green, free geometry in the normal colour,
  conflicting constraints red.
- An edit that cannot be solved is rejected and rolled back (it is one command, so the
  engine's rollback applies), with a message naming the conflicting constraint.

## Acceptance test: Hall A

After all blocks, draw the venue from `testdata/private/hall_a.drawio.svg` (a
sketch by the user). Yellow boxes are only dimensions; the proportions in the drawing
are wrong, the numbers count. The angled strips ("9m", "13m bis zur Mitte", rear
"Breite 2,25m … ca. 1,80m") are walkways around the audience, about 2 m wide, at stage
height. Mark every assumption in the plan. The result should be a parametric sketch,
so changing e.g. the portal width moves what hangs on it.
