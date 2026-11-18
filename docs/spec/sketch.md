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

Round 2 (2026-11-18, photos of the hall, `testdata/private/hall_a/photos/`, private):
the seating fans out round the stage, aisles widen towards the back, and the rear
walkways form a V. "0,5" are the grey tormentors, "3,20" the concrete pillars beside the
14 m opening; the stage front is a polygon; the rig is trusses. The test writes one set
of outputs (`hall_a.stagecraft`, `.pdf`, plan/3D/house/side PNGs); the 13 → 15 m
portal check runs inside it and is undone. Open: where "21 m" starts (read from the
portal line, the only reading that gives the V with 9 m + 13 m), whether the loose-chair
area rises (photos: flat), and the guessed sizes in notes A1–A11.

Round 3 (user's answers, same day): "21 m" ends at the cross aisle's rear edge; the
cross aisle has a straight middle as wide as the tribune's middle block, so "13 m to the
middle" is the leg plus half of it; the knees lie behind the middle and the hall's rear
wall is straight. Seats are real `seat_row` objects with the user's counts (535 in all);
the front rows stand on 0.25 m tiers, the stage is 1.00 m above the pit floor.

Round 4: the pit's risers and rows are parallel offsets of the stage front (each row bends
once); the tribune's blocks keep straight rows, the middle block farthest from the stage,
the side blocks turned forward. "21 m" holds; the sketch's 9 m and 13 m cannot with this
shape (5.17 m and 11.82 m now, noted in A3).
