# stagecraft-hlr

Vector views for StageCraft sheets: feature edges, hidden-line removal and section
cuts for orthographic projections of triangle meshes. Pure Rust, no other StageCraft
crate, builds for `wasm32-unknown-unknown`.

A workspace member. `stagecraft-sheet` turns document meshes into its `Scene` and
the app enables `parallel`.

```sh
cargo test -p stagecraft-hlr
cargo test -p stagecraft-hlr --release --features parallel --test bench -- --ignored --nocapture
```

## API

```rust
use stagecraft_hlr::{draw, Scene, Mesh, View, Options, LineClass, ObjectId, Material, DVec3};
use stagecraft_hlr::svg::{to_svg, SvgStyle};

let scene = Scene {
    meshes: vec![Mesh { object: ObjectId(7), material: Material(2), positions, triangles }],
    lines: vec![],                      // optional explicit 3D lines (centre lines …)
};
let drawing = draw(&scene, &View::section_front(1500.0), &Options::default());
for line in &drawing.polylines {        // 2D, view mm
    match line.class {
        LineClass::Cut => {}            // outline of a cut face: thick
        LineClass::Visible => {}        // silhouette / crease / boundary
        LineClass::Hidden => {}         // occluded part: dashed
        LineClass::Centre => {}         // from `Line3`: dash-dot
    }
}
for region in &drawing.regions {}       // filled cut faces: outer + holes, material class
let svg = to_svg(&drawing, &SvgStyle::default());
```

### Input

- `Mesh { object, material, positions: Vec<DVec3>, triangles: Vec<[u32; 3]> }` in
  **world mm**. The caller applies object transforms. Winding, vertex sharing and
  T-junctions don't matter: vertices are welded (`Options::weld_tolerance`, default
  0.01 mm), slivers dropped, T-junctions on boundaries split, and every connected
  component is oriented consistently. Closed shells get outward normals; a shell inside
  an odd number of other shells of the same mesh (the cavity of a hollow part) faces
  inward. One mesh per object is the natural granularity, but any split works.
- `Line3 { object, points, class, occluded }`: explicit 3D polylines such as truss
  axes or fixture centre lines. With `occluded`, hidden parts of a `Visible` line come
  out as `Hidden` and hidden parts of other classes are dropped.

### View

`View { dir, up, section }`: an orthographic projection. `dir` points from the eye into
the scene, `up` is projected onto the sheet's +y.

| Constructor | Looks | Sheet x / y |
|---|---|---|
| `View::front()` | from the audience (−y) upstage | world x / z |
| `View::back()` | from upstage towards the audience | −x / z |
| `View::side_right()` | from stage right (+x) | y / z |
| `View::side_left()` | from stage left (−x) | −y / z |
| `View::plan()` | down | world x / y (identical) |
| `View::iso()` | from front right above, like the 3D view's iso preset | |
| `View::section_front(y)` | front, everything downstage of `y` removed | world x / z |
| `View::section_side(x)` | side from stage right, everything at > `x` removed | y / z |
| `View::plan_cut(z)` | plan, everything above `z` removed (architectural plan) | x / y |
| `.with_section(SectionPlane::new(origin, normal))` | any plane; the normal points to the removed side | |

`View::to_view(p)` maps a world point to view space: x right, y up (both on the sheet),
z towards the eye. Anything else drawn on the sheet (labels, dimensions) can use it.

### Output

`Drawing { polylines, regions }`, all in **view mm at 1:1** with **y up**; the sheet
applies scale and flips y for PDF/SVG.

- `Polyline { object, class, points, closed }`: consecutive collinear segments are
  merged; pieces of one object and class are chained where they meet. A closed
  polyline does not repeat its first point.
- `Region { object, material, outer, holes }`: a filled cut face. Outer loop
  counter-clockwise, holes clockwise, nesting resolved per mesh (even-odd depth).
- The caller maps `LineClass` (and `object`/tag) to weights and dashes, and `material`
  to hatch patterns.

### Options

| Field | Default | Meaning |
|---|---|---|
| `crease_angle_deg` | 50 | faces meeting at more than this draw an edge; flat and gently curved surfaces don't |
| `weld_tolerance` | 0.01 mm | vertex welding; also joining output segments |
| `hidden_lines` | true | emit `Hidden` lines; `false` also skips edges that are always hidden (faster) |
| `min_length` | 0.05 mm | drop shorter pieces |
| `intersections` | true | draw where separate parts penetrate each other |

## How it works

1. **Prepare** each mesh in view space: weld, drop slivers, split T-junctions, build
   the edge table, orient components (BFS over manifold edges, volume sign, nesting by
   ray parity).
2. **Feature edges**: boundary (one face), non-manifold (three or more), crease
   (dihedral above the threshold) and silhouette (one face towards the eye, the other
   not). Edges inside flat or smooth regions are not drawn, so triangulation diagonals
   never show.
3. **Section** (optional): clip edges and occluding triangles to the kept side; cut
   each mesh into loops (exact vertex/edge keys, no tolerance chaining), group them into
   outer/holes, triangulate the cut faces (`earcutr`) as occluders, and add the loops
   as `Cut` edges. Open meshes (curtains) give open cut lines and no region.
4. **Intersection lines**: where triangles of different meshes, or of different
   connected components of one mesh, cross each other (strictly, not just touching),
   the crossing segment is added as a weak `Visible` edge. Component pairs come from a
   sweep over bounding boxes taken in world axes, so axis-aligned rigs keep tight boxes
   in any view; within a pair, only triangles in the overlap box are tested (hashed
   into a small grid when there are many).
5. **Hidden-line removal**: occluders are all triangles except back faces of closed
   shells, minus edge-on triangles. They go into a uniform screen grid (one cell per
   occluder on average, triangles rasterised into the cells they actually touch). Per
   edge, the candidate triangles along its cells each give at most one hidden
   parameter interval (inside the projected triangle, slightly enlarged, *and* behind
   its plane by more than a depth tolerance). The union is hidden, the rest visible.
6. **Overlap removal**: a hidden piece lying on a visible or cut line is dropped, and so
   is a visible piece on a cut line, an intersection line on any edge, or a duplicate
   within one object. Without this a
   front view of a box shows its back edges dashed exactly under the outline.
7. **Chaining**: per (object, class) the pieces are joined at shared endpoints and
   collinear points are removed.

With the `parallel` feature, mesh preparation, intersection pairs and the per-edge HLR
queries run on rayon. Leave it off for wasm.

## Performance

`tests/bench.rs` (ignored by default): 64 runs of 12 m box truss (16-sided chords,
8-sided diagonals, every diagonal penetrating two chords) plus a deck, 309 k triangles,
Apple Silicon, release build, intersection lines on:

| View | single thread | `parallel` | without hidden lines, parallel |
|---|---|---|---|
| front | 749 ms | 396 ms | 105 ms |
| iso | 1177 ms | 528 ms | 170 ms |
| plan | 860 ms | 404 ms | 127 ms |
| front section | 507 ms | 224 ms | 94 ms |

Intersections cost about 0.3 s single-threaded here; `intersections: false` brings iso
down to 442 ms single-threaded.

## Limits

- Intersection lines are only found between separate parts (meshes or connected
  components). A single self-intersecting component draws none, and faces that only
  touch (a box standing on a deck) give none, which is what a drawing wants.
- Cut regions are emitted whole and assumed to be in front: right for planes facing the
  viewer (all the presets). For a strongly oblique plane, kept geometry could lie in
  front of part of a cut face. Cut lines do go through HLR.
- Coplanar overlapping faces of different objects (z-fighting in 3D) are treated as
  "not hiding each other"; both outlines are drawn.
- Visibility is decided against triangles, not merged faces. A line running exactly
  along an occluder's outline counts as covered (the inside test is inclusive), which
  is what you want for back edges under a front outline.
- The crease threshold is global. Low-poly tubes with facets wider than 50° (hexagons)
  show facet lines; lower it for detailed chamfers.
- Points closer than the depth tolerance (≈ 1e-7 × scene extent, at least 0.001 mm) in
  depth count as not hidden, so a few µm at hidden corners may come out visible and are
  then dropped by `min_length`.
