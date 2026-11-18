# StageCraft

Stage and lighting planning in Rust, in the spirit of Vectorworks Spotlight with better
UX. Commits are GPG-signed.

## Where behaviour is written down

- `docs/spec/overview.md`: what the product does, settled with the user. Do not change
  scope there without asking.
- `docs/spec/decisions.md`: choices made where the spec was silent. Add a row when you
  make one.
- `docs/spec/control.md`: control channel, discovery, MCP, CLI.
- `docs/roadmap.md`: the blocks and their status.

## Rules of the code

- `stagecraft-core` holds the document and every command. No UI, no sockets, no clock.
  It must keep building for `wasm32-unknown-unknown`
  (`cargo check -p stagecraft-core -p stagecraft-geom -p stagecraft-sheet -p stagecraft-gdtf -p stagecraft-hlr -p stagecraft-mvr -p stagecraft-sketch --target wasm32-unknown-unknown`).
- **Every action is a command** registered in `crates/stagecraft-core/src/commands/`.
  The GUI never mutates the document directly. A new feature without a command is not
  done.
- Argument structs are the API for AIs: their doc comments become the JSON schema. State
  units (mm, degrees) and defaults there.
- Edits change the document through `ctx.doc_mut()`; that is what marks an undo step.
- Placements are relative to the parent, or to the layer for roots. Moves and rotations
  from the user are world-space and go through `transform_world` in `object.rs`.
  Attachment behaviour has tests in `crates/stagecraft-core/tests/engine.rs`; extend
  them when you touch it.
- Units mm and degrees, axes as in MVR: x right from the audience, y upstage, z up.
  y = 0 is the setting line (the proscenium's downstage face in the demo).
- Object kinds (`model.rs`): floor, wall, truss, fixture, and from block 4 proscenium,
  curtain, wing_set + masking, batten, platform, truss_node + truss_run, support,
  hoist, ceiling; from block 8 group and model. `wing_set`, `truss_run` and `group`
  are groups (children, no geometry).
  Battens keep position and trim in their placement (y, z); things hang on them by
  attachment. Stage commands are in `commands/stage.rs`, truss runs/nodes, load
  report and per-object plan style in `commands/rigging.rs`.
- Catalogues (truss profiles, platform systems, supports) are JSON in
  `crates/stagecraft-core/catalog/`, embedded; `catalog::extend`/`load_dir` merge
  user files (the app reads `<data dir>/StageCraft/catalog/*.json`). Load sums
  (`load.rs`, `load.report`) need them, which is why they live in core.

## Fixtures and GDTF

`stagecraft-gdtf` (workspace member, no StageCraft deps; see its README) parses
GDTF; core depends on it. The document holds `assets` (embedded files by SHA-256,
bytes in a shared `Arc` so undo never copies them; `asset.rs`) and `fixture_types`
(library entries keyed `Manufacturer@Name` with modes, footprints, weight, size,
symbol; `fixture_type.rs`). `.stagecraft` format 2 stores assets as
`assets/<sha256>.<ext>` next to `document.json`; format 1 files load unchanged.
A fixture's `gdtf` is a library key, `mode` a mode name, `address` the first
break and `extra_addresses` further breaks. `patch.rs` has footprints, ranges,
conflicts and free-range search. Commands in `commands/fixture.rs`:
`gdtf.import/library/remove`, `fixture.create/update/set_type/number`,
`symbol.assign/list`, `patch.auto/check`. Weight comes from the GDTF unless set by
hand, so load sums include it. Built-in plan symbols are SVGs in
`crates/stagecraft-core/assets/symbols/` (front of the fixture at the top).

`stagecraft-geom::gdtf` builds what views draw from the embedded GDTF and caches it
per asset hash + mode (+ `Lod`): the mesh (hanging from the origin, pan/tilt
neutral), the plan symbol (assigned built-in symbol → the GDTF's top-view SVG →
silhouette; generic box without a type) and outline points that make the
silhouette follow the fixture's tilt. `svg.rs` is a small SVG reader. The demo's
three GDTFs are synthetic (`testdata/gdtf/`, made by `make_demo_gdtf.py` there,
run with `python3 -I`); the real GDTF Share downloads in `testdata/private/gdtf/`
are used by tests that skip without them.

## Geometry

`stagecraft-geom` turns objects into what views draw: `plan()` gives 2D primitives in
world mm with line/fill classes, `mesh(doc, obj, Lod)` triangles in the object's local
space (normals, material groups), one module per kind in `src/kinds/`. Meshes depend
on kind parameters and `Lod` (Full/Simple/Box, only truss differs) only, so callers
cache them; `world_mesh` applies the transform. `Mesh` has builders (`quad`,
`cuboid`, `tube`, `extrude`) and checks (`is_watertight`, `volume`, `bounds`).
`section.rs` cuts a mesh with a `Plane` into closed polygons in plane coordinates
(holes by nesting) and triangulates them; a plane hides the side its normal points
to. Hit tests, box selection, truss hits and snapping are pure functions in
`pick.rs` and `snap.rs` with tests. It must build for wasm too.

Stage kinds are in `kinds/stage.rs` (plus `fabric.rs`: pleats from fullness and the
closed cloth slab, `node.rs`: truss corners from truss stubs). `kinds::plan` applies
the object's `style` overrides; `kinds::plan_at` draws a kind at any transform
(tool ghosts use it). `kinds::detail_size` tells views which `Lod` matters (truss,
pleats, pipes, legs). Cloth meshes carry a `tint` (the object's colour). Every mesh
must stay watertight; `tests/geom.rs` checks all kinds at all levels.

## UI (`stagecraft-ui`)

- `Studio` = engine + `UiState`; `Studio::show(ui)` draws the whole window, so tests
  drive it with `egui_kittest`. `Studio::run(name, args)` runs a command of either
  registry; the GUI uses nothing else to change the document.
- UI state has its own `ui.*` registry in `src/commands.rs`. In GUI mode the control
  server lists/describes/runs both (`UiExtension`); calls are answered in
  `StageApp::logic`, and `ui.screenshot` is answered after the frame arrives.
- Canvas drawing in `canvas.rs`, tool behaviour in `tools.rs` (drags are transient
  `Gesture` state; one command on release). Typed dimensions: plain numbers are
  metres, Tab switches to the angle, Enter takes it, an empty Enter finishes.
- All visible text goes through `i18n::t("key")`; the catalog is
  `assets/i18n/en.json`, and a test fails on keys used but missing.
- Theme: `theme.rs` holds both palettes; System/Light/Dark via `ui.set_theme`.
  Roboto is vendored; it has no arrows or crosses, so use `widgets::icon_button`.
- Give clickable things an accessible name (`widgets::name`); tests find them so.
  Table cells are named `"<Column> <row>"`, e.g. `FID 1`.
- Tools are `Tool::DRAW` then `Tool::STAGE` in the toolbar; tool options
  (`ToolOptions`, `ui.set_tool_options`) show on the toolbar's second row. One-click
  stage tools go through `place_stage` in `tools.rs`, with ghosts from `stage_ghost`.
  Inspector fields for stage kinds are in `inspector::stage_fields` (helpers `num`,
  `choice`, `flag`, `colour`), loads in `load_section`. The table's "Fly bars" view
  (`kind = batten`) is the Zug-Liste.

- 3D view in `src/view3d/`: `camera.rs` (orbit camera, pure maths), `scene.rs`
  (per-object world-mesh cache keyed on kind, `Lod` and world transform; flattened
  GPU buffers with versions; cut faces; ray picking), `gpu.rs` + `*.wgsl` (offscreen
  4× MSAA pass with reversed depth in the paint callback's `prepare`, blitted in
  `paint`). `gpu::install(render_state)` must run once per wgpu render state:
  `run` does it from eframe's creation context; headless tests build their own
  render state (see `tests/screens.rs`), otherwise the 3D view stays empty.
  `ui.set_view` switches plan / 3d / split (⌘1/⌘2/⌘3); `ui.camera` and
  `ui.section` drive the rest. ⌘4 is the Sheets mode.

- The library panel (`src/library.rs`, left side) lists fixture types (pick one for
  the fixture tool, assign a symbol, import files) and searches GDTF Share.
  Requests run on worker threads behind the `ShareBackend` trait; the UI polls a
  channel in `Studio::show`, so the network never blocks a frame. Tests swap in a
  mock (`ui.library.set_backend`). On this Mac LuLu holds new binaries' connections
  until a human allows them, so live Share calls cannot be tested unattended.
  `ui.share_search` / `ui.share_add` drive it. Patch problems show in the table's
  Patch column, the inspector, and as red rings in the plan.

- Sheets mode (`src/sheets.rs`, ⌘4): the page from `stagecraft_sheet::compose`,
  cached by journal length; drags become `viewport.update` / `label.move` on
  release; the inspector shows sheet, viewport, title block fields and label
  templates. `ui.sheet` and `ui.set_label_template` drive it; the plan canvas draws
  labels with the same placement code (`canvas::draw_labels`).

## Sheets (`stagecraft-sheet`)

The document holds `sheets`, `label_templates`, `title_blocks` (model in
`stagecraft-core/src/sheet.rs`, commands in `commands/sheet.rs`); object attributes
(`gel`, `circuit`, `focus`, `note` …) are `Object.attrs`. `stagecraft-sheet` composes
a sheet into `Prim`s in paper mm (origin lower left, y up): `compose.rs` (frame,
viewports, titles + scale bars, legend, title block placeholders, fixture list),
`labels.rs` (pure label layout: three rotation modes, per-field greedy collision
avoidance, leaders), `view.rs` (`VectorViewSource`, `ViewSpec`; `FallbackSource`
draws mesh outlines without hidden-line removal, kept for tests), `hlr.rs`
(`HlrSource`, the default: meshes per object and material run into
`stagecraft-hlr`, cached per view and document revision = identity of the `imbl`
maps), `render.rs` (krilla PDF with embedded Roboto, SVG). Vector viewports are
`elevation`, `section` (cut faces hatched by material) and `plan_3d` (top view from
meshes, optional cut height, labels as in `plan`); `hidden_lines` adds dashed
hidden edges, `tag_lines` restyles objects by tag. Core cannot depend on it, so
`sheet.export_pdf`/`export_svg` call an exporter that `stagecraft_sheet::install()`
registers (the app's `main` and `Studio::new` do; it also runs
`stagecraft_geom::install()` for the MVR mesher). Rendered viewports take images
from `stagecraft_sheet::set_raster`: the GUI's `view3d::offscreen::Offscreen` renders
them with the 3D pipelines before composing and before `sheet.export_*`; `stagecraft
exec sheet.export_* --file` makes a windowless wgpu device (`STAGECRAFT_NO_GPU`
skips it); without a GPU they print a placeholder. The Sheets mode composes on a
worker thread and shows the previous page meanwhile. Fonts live in `assets/fonts/`
for GUI and PDF.

```
STAGECRAFT_SHEETS=/some/dir cargo test -p stagecraft-sheet --test sheet
```

writes the test sheets as PDF/SVG/PNG; the screens test also writes
`demo-sheets.pdf` (the demo's two sheets).

Look at the screens after UI changes:

```
STAGECRAFT_SCREENSHOTS=/some/dir cargo test -p stagecraft-ui --test screens
```

renders the demo document (`demo.rs`, a small proscenium theatre with battens,
curtains, wings, truss, platforms) in light and dark to PNG, including 3D iso,
front, sections, a truss close-up, the stage from the house, the fly bar list and
split. The overview shots hide the `ceiling` tag, which would cover the house. With
`testdata/private/vectorworks/sample_show.mvr` present it also renders that
import in plan and 3D (`26-mvr-*`, `27-mvr-*`).

## Parametric sketch (block 9, `docs/spec/sketch.md`)

`stagecraft-sketch` (workspace member, no StageCraft deps; README there) is the
constraint sketch solved with ezpz: points, lines, arcs, constraints, driving and
reference dimensions, `Edit`s, loops, drawing inference. The workspace root optimises
it, faer and ezpz in dev builds (the analysis is ~500× slower unoptimised).

Core (`sketch.rs`, commands in `commands/sketch.rs`): `Document.sketches` holds one
sketch per layer (layer plan mm). Walls and floors with `Object.sketch =
Some(SketchLink { edges })` follow it: `sketch::store`/`sync_layer` rewrite their
`path`/`outline` after every edit, so geom, sheets, HLR and MVR need no sketch
knowledge. Linked objects are roots at x/y/rotation 0 (only z is theirs);
`object.move`/`rotate` drag their sketch points. `Document.dimensions` are reference
dimensions between anchors (object + local point, sketch point, world point).
Commands: `sketch.get/status/add_point/add_line/add_arc/add_polyline/add_constraint/
remove_constraint/remove/drag/draw_line/draw/make_wall/make_floor/adopt/unlink/solve/
edit`, `dimension.add/add_reference/set_value/set_driving/move_label/name/remove/list`
(ids: number = sketch dimension, UUID = reference, or a name). An edit the solver
cannot satisfy fails with code `conflict` naming the constraints; the engine rolls
back. Tests: `crates/stagecraft-core/tests/sketch.rs`.

Drawing: `stagecraft_geom::dims` (shared by canvas and sheets; plan viewports have
`sketch_dimensions`/`reference_dimensions` toggles). GUI: `src/sketch_ui.rs` (overlay
in FreeCAD colours with a cached analysis, dimension hit tests, inline value editor,
drag preview, inspector section, dimension tool picks); wall/floor tools run
`sketch.draw`; `ui.sketch`, `ui.dimension`. GUI tests in `tests/sketch.rs` (the
harness there uses 1/60 s steps so double clicks register), screens `30-35-sketch-*`.

## MVR and imported models

`stagecraft-mvr` (no StageCraft deps; README there) reads and writes MVR files;
`stagecraft-core/src/mvr.rs` maps them to the document (`mvr.import {path,
layer_mapping?}`, `mvr.export {path}`), `commands/mvr.rs` also has `model.import`
(glb, gltf, obj, 3ds). Imported objects keep their MVR UUID and land at their MVR
world matrix; fixtures get their GDTF into the library, mode, FID/CID and
multi-break patch; trusses become truss pieces (catalogue profile by name, else the
closest cross-section); objects with geometry become `model` objects whose parts
are mesh files in `assets` (`model_file.rs` reads them; geom's `kinds/model.rs`
caches meshes and draws a hidden-line top view). Export writes MVR 1.6 with GLBs of
geom meshes through `mvr::install_mesher` (set by `stagecraft_geom::install`).
Tests: `cargo test -p stagecraft-geom --release --test mvr -- --nocapture` imports
every private MVR, checks counts against the file, fixture world matrices and
patch, and round trips. grandMA3 has no MVR import outside its PSR GUI.

## Test data

`testdata/private/` holds real MVR files from client projects and is git-ignored.
`vectorworks/` are Vectorworks 31 exports; `other/` comes from other tools (one from
the-pult). Never commit them or paste their contents anywhere outside this machine.

Perastage (`/Applications/Perastage.app`) opens MVR files and serves as an independent
viewer: `open -a Perastage file.mvr`.

## Trying it

```
cargo test
cargo run -p stagecraft-app                      # the GUI
cargo run -p stagecraft-app -- exec ui.screenshot '{"path":"/tmp/s.png"}'
cargo run -p stagecraft-app -- commands
cargo run -p stagecraft-app -- serve /tmp/plan.stagecraft
cargo run -p stagecraft-app -- exec object.list
cargo run -p stagecraft-app -- exec sheet.export_pdf '{"path":"/tmp/s.pdf"}' --file plan.stagecraft
```
