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
  (`cargo check -p stagecraft-core -p stagecraft-geom -p stagecraft-sheet -p stagecraft-gdtf -p stagecraft-hlr -p stagecraft-mvr -p stagecraft-sketch -p stagecraft-underlay --target wasm32-unknown-unknown`).
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
  hoist, ceiling; from block 8 group, model, underlay and note. `wing_set`, `truss_run` and `group`
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
  `Gesture` state; one command on release). Typed dimensions are Fusion-style heads-up boxes (`hud.rs`): length (and angle)
  boxes beside the rubber band show live values; a digit goes straight into the length box,
  Tab cycles length → angle, a box whose text parses is locked (lock icon) and Enter or a
  click takes the point and empties the boxes; Shift squares the pointer's direction, a
  typed length holds along it. Esc drops the typed value, a second Esc ends the drawing
  (keeps what makes an object), a third leaves the tool. Plain numbers are the project
  unit; `mm`, `cm`, `m`, feet/inches (`15'`, `6"`) win. Tests in `tests/drawing.rs` use
  real key events (most in a metre project).
- Project unit: `meta.units` (`doc.set_units {length_unit: mm|cm|m|ft_in, precision}`,
  default mm) is how the GUI shows and takes lengths; the document and all command
  arguments stay mm. `stagecraft_core::units::Units` (`Units::of(&meta)`: `field`,
  `fixed`, `with_unit`, `parse`, `label`) is the only formatter/parser; `lengths.rs` has
  the GUI helpers (`lengths::of(s)`, `label(&u, "Width")` → "Width (m)"). A new length
  field must use them, never `{} mm` or `/ 1000.0`; plain numbers (kg, counts) use
  `lengths::fmt_number`/`parse_number`. Table length columns: `Col::is_length`.
  Project Settings (`settings.rs`, File menu, ⌘,, `ui.project_settings`) is a
  non-blocking window with units, sheet text and title block fields; the Inspector's
  Document section only links to it.
- All visible text goes through `i18n::t("key")`; the catalog is
  `assets/i18n/en.json`, and a test fails on keys used but missing.
- egui 0.37 comes from storytold's fork (root `[patch.crates-io]`, same revision as
  craft-libs); widgets name themselves with `egui::Role` (kittest fails on unnamed inputs
  and on missing glyphs, so give every combo box and checkbox a name).
- Look (Craft suite, like CADCraft): `theme.rs` holds `Tokens::DARK` / `Tokens::LIGHT`
  (chrome, `plan` canvas palette, `scene` 3D palette, axis colours); every widget reads
  them (`theme::of(ui)`, `theme::tokens(dark)`), no colours elsewhere. Inter 11.5 pt
  (`theme::BODY`) plus a DejaVu cut for arrows and ⌘ ⇧ ⌥; 19 pt rows. System/Light/Dark via
  `ui.set_theme`. Sheets stay white paper. Property lists: `widgets::prop_label` (right-
  aligned label column), full-width fields, `widgets::combo_w` (chevron), `widgets::section`.
- Window layout (`chrome.rs`): on macOS the 28 pt title row (`chrome::title_row`, the band
  AppKit centres the traffic lights in, centred title "StageCraft — file •"); then the tool
  bar (file/edit/import/export commands as icons, ⋯ overflow, Plan/3D/Split/Sheets switch,
  palette search), the options row (active tool + its options), the dock area (`dock.rs`,
  craft-ui docking) and the status bar (Model and sheet tabs, coordinates,
  the active tool's prompt and the last message, grid/object/truss snap, sketch, constraint
  colours and theme toggles). No menu row on
  macOS (`menu.rs` is the native menu bar through craft-menu). Default docks: the tool
  palette (`PanelKind::Tools`, `toolsets.rs`) left; right the browsers Layers, Tags,
  Sheets, Library, GDTF Share as icon tabs (`dock::icon`, painted by `tab_icons`, with a
  title row and ≡ panel menu in the body) above the Inspector; the tutorial opens above
  them; the table under the canvas. Layout and theme are saved per user by craft-settings
  (memory-only in tests). `ui.toggle_panel`, `ui.layout`, `ui.float_panel`,
  `ui.reset_layout` drive the docks. ⌘ keys from the native menu come back as key presses
  into `app.rs`'s `shortcuts`.
- Command palette (⌘K, `palette.rs`): what people see of a command lives in
  `command_meta.rs`, one `META` row per command of both registries (title key
  `cmd.<name>`, `Group`, icon, shortcut, `hidden`, `Need`, `applies` kinds, `direct`)
  plus presets (a command with fixed args: "3D View" = `ui.set_view {view: 3d}`, the tools)
  and the menus' `app.*` actions. A new command needs a row and a title in `en.json`, or
  `command_meta` tests fail; a visible command whose schema requires object `ids`/`id`
  must say `Need::Selection`/`One`. MCP and CLI keep the IDs. Empty query: Recent
  (`UiState.recent`, saved in `Prefs.palette`), commands for the selection, the groups
  (→/Enter opens, ←/Backspace back, breadcrumb). Typing searches titles, aliases
  (`assets/i18n/aliases.json`: German and Vectorworks words per English term), groups,
  summaries and objects (name, FID), layers, tags, sheets; inside a group only the group.
  `>` all commands incl. hidden ones with IDs, `@` objects, `#` tags. IDs and fixed
  arguments (`ui.set_tool tool=dimension`, the form's `patch.auto`) show only in `>` mode
  and in a form opened from it (`palette::technical`, `shown_id`); everywhere else titles only. Greyed rows carry the
  reason (`command_meta::available`, no command is run to find out). Arguments: an inline
  form from `schema_form::fields_for` (units, enums, pickers for objects/layers/tags/
  sheets/viewports/fixture types, x/y/z boxes, prefill from selection and defaults); Enter
  runs, Esc goes back. Palette tests: `tests/ui.rs` (`the_palette_*`), screens `70-78`.
- There is no in-canvas command line (removed: ⌘K and the tool bar search do that). The
  active tool's prompt (`chrome::prompt`, from `panels::hint`) sits in the status bar after
  the coordinates, cut with "…" (full text as tooltip, accessible name = the text); the last
  status message follows it and gets at most half the room.
- Icons in `assets/icons/` (Lucide, ISC; own stage and constraint icons in the same
  grid), rasterised by `icons.rs` at the device pixel size (size × pixels_per_point, cached
  per icon and px, so another screen density gets new textures) with grid fitting: path
  points snap to pixel centres/corners, the stroke is Lucide's 2 units rounded to whole
  pixels (`icons::stroke_px`: 1 px at 16 px, 3 px at 32). `icons::pixel_rect` puts the
  texture on the pixel grid 1:1 (nearest filter). Use 12 or 16 pt icons (20/24 for big
  ones), not 14/15/17: those leave Lucide's grid between pixels. `STAGECRAFT_ICONS=<dir> cargo
  test -p stagecraft-ui --lib icon_sheet` writes them as PNGs; the screens test's
  `80-crisp-{1x,1.5x,2x}-*` render the window at those densities.
- Give clickable things an accessible name (`widgets::name`); tests find them so.
  Table cells are named `"<Column> <row>"`, e.g. `FID 1`, header resize handles
  `Resize <Column>` (columns fill a wide panel, give way to 70 % in a narrow one, then the
  table scrolls sideways; cut cells end in "…"); tool buttons `Tool Wall`, tool
  bar buttons `Toolbar Save`, palette sections `Section Stage`, status toggles `Toggle grid
  snap`, sheet tabs `Model tab` / `Sheet tab L-101`, panel menus `Panel menu Layers`.
- Tools sit in `panels::tool_groups` (palette sections); the palette (`toolsets.rs`) fills its
  rows to the panel width, which can be dragged down to one button; `ui.tool_columns
  {columns: auto|two|one}` (also the Tools ≡ menu's Compact entries, saved in
  `Prefs.tool_palette`) caps the rows at two or one and narrows the docked column to fit
  (`toolsets::width_for`); at one per row section headers are rules with the title as tooltip; tool options (`ToolOptions`,
  `ui.set_tool_options`, saved per user in `dock::Prefs.tools`) show on the options row;
  `panels::hint` (what the gesture expects next: first corner,
  next corner, "close the floor") is the status bar's prompt. After a tool creates something the status bar says
  what to do next (`next.<tool>` catalog keys, `tools::created_message`). Heights the
  user types (tool options, Inspector) are above the object's layer elevation; floors
  show "Top height", walls "Base height", ceilings "Underside height". One-click
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
  `ui.section` drive the rest. ⌘4 is the Sheets mode. The 3D camera follows the
  model's bounds (`View3d::follow`) until the user orbits, pans or zooms.

- Tutorial (`src/tutorial.rs`): the dockable Tutorial panel (hidden by default, opens
  above the inspector; Help > Tutorial, `ui.tutorial {action: start|next|back|stop}`)
  walks "Build a basic stage" in twelve `STEPS`. Each step has catalog texts
  `tutorial.<id>.title/.text`, a key cap, a `done` check on the document/UI (it moves
  on by itself; after Back or Skip a done step waits for Next) and `targets`
  (accessible names or panel tabs). `widgets::name` and the tool buttons call
  `tutorial::mark`, so the first target on screen gets a pulsing accent outline
  (`tutorial::paint`, after the frame); a target scrolled out of its panel scrolls in
  (`tutorial::reveal`) and entering a step brings its panels to the front of their tab
  group (the Library shares one with Layers). `tests/tutorial.rs` walks all steps through
  the GUI (screens `60-67-tutorial-*` with `STAGECRAFT_SCREENSHOTS`); floors in the
  GUI are tested in `tests/floor.rs`. On launch the app offers the tour once
  (`onboarding.rs`: egui modal Start tutorial / Not now / Don't ask again, answer in
  `Prefs.onboarding`; only `run` calls `Studio::offer_tutorial`, so tests stay quiet).

- The library panel (`src/library.rs`) lists fixture types (pick one for the fixture
  tool, assign a symbol, import files, "Demo fixtures" adds the bundled demo GDTFs);
  the GDTF Share panel (`library::share_panel`)
  searches GDTF Share.
  Requests run on worker threads behind the `ShareBackend` trait; the UI polls a
  channel in `Studio::show`, so the network never blocks a frame. Tests swap in a
  mock (`ui.library.set_backend`). On this Mac LuLu holds new binaries' connections
  until a human allows them, so live Share calls cannot be tested unattended.
  `ui.share_search` / `ui.share_add` drive it. Patch problems show in the table's
  Patch column, the inspector, and as red rings in the plan.

- Sheets mode (`src/sheets.rs`, ⌘4): the page from `stagecraft_sheet::compose`,
  cached by journal length; drags become `viewport.update` / `label.move` on
  release, while dragging the viewport is drawn live from the composed page
  (`sheets::Live`, `paint_page`: moved = page prims painted with a shifted camera,
  cropped = same content re-clipped; `ViewportHit.prims` is its range) and stays so
  until the recomposed page arrives; a new scale keeps the frame (`sheets::set_scale`,
  core `viewport.update` rescales a crop around its centre); the inspector shows sheet, viewport, title block fields and label
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
maps), `render.rs` (krilla PDF with embedded Arimo, SVG). Sheet text is Arimo
(OFL, Helvetica metrics) in PDF, SVG and the Sheets mode (`theme::sheet_font`).
Title blocks: `None` = the built-in compact one (zoned border, legend on top of it,
same width), `TitleBlock::GENERIC_ID` the old one; template text takes
`{field}`, `{a|b}` and `{t:key}`. Every word StageCraft prints on a sheet comes from
`stagecraft_core::sheet::text` (en/de, `meta.sheet_language`). Lengths on drawings go
through `stagecraft_core::units::LengthFormat::of(&doc.meta)` (`project` = the project
unit and precision, default of new documents; `m_sup_mm` 6.15⁵, `m`, `mm`, `ft_in`,
`ft`; separator from the language unless set; files before format 5 without the field
read as `m_sup_mm`): `m_sup_mm` writes the mm digit
as a Unicode superscript, and every renderer (PDF, SVG, Sheets mode via
`render::text_runs`, plan canvas via `sketch_ui::dim_text_job`) draws superscript
runs smaller and raised. Vector viewports are
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
curtains, wings, truss, platforms) in light and dark to PNG, with the macOS chrome (no
menu row, window buttons painted in), including 3D iso,
front, sections, a truss close-up, the stage from the house, the fly bar list and
split, a rearranged dock layout with a floating panel (`40-dock-float-*`), the
tutorial panel (`41-43-tutorial-*`), and `50-55-craft-*` at 1200 × 750, the size of the
Craft suite's reference screenshots, and `70-78-palette-*` (groups, selection, group,
search, alias, form, objects, `>`), `80-crisp-{1x,1.5x,2x}-*` (plan and palette at those
pixel densities) `81-tools-{two,one}-*` (the folded tool palette), `90-92-hud-*` (heads-up dimension
boxes: live, typed and locked, Shift), `93-viewport-1-{100,50}-*` (same frame, other
scale), `94-viewport-live-drag-*`, `95-first-run-offer-*`, `96-project-settings-*` and
`97-inspector-m-*` / `98-inspector-ft-in-*` (a batten, fly bar list and status bar in m
and ft-in). kittest's 8 pt margin is cut off and the traffic
lights are painted where AppKit puts them (`chrome::traffic_light_centres`). The overview shots (`53-craft-3d` too) hide the `ceiling` tag, which would cover the house. With
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
edit`, `sketch.link` (portal, batten, curtain, truss piece follow one line), `dimension.add/add_reference/set_value/set_driving/move_label/name/remove/list`
(ids: number = sketch dimension, UUID = reference, or a name). An edit the solver
cannot satisfy fails with code `conflict` naming the constraints; the engine rolls
back. Tests: `crates/stagecraft-core/tests/sketch.rs`.

Drawing: `stagecraft_geom::dims` (shared by canvas and sheets; plan viewports have
`sketch_dimensions`/`reference_dimensions` toggles). GUI: `src/sketch_ui.rs` (click picks sketch points/lines, inspector constraint buttons, `ui.sketch_pick`; overlay
in FreeCAD colours with a cached analysis, dimension hit tests, inline value editor,
drag preview, inspector section, dimension tool picks); wall/floor tools run
`sketch.draw`; `ui.sketch`, `ui.dimension`. GUI tests in `tests/sketch.rs` (the
harness there uses 1/60 s steps so double clicks register), screens `30-35-sketch-*`.

## Underlays and notes

`stagecraft-underlay` (no StageCraft deps, wasm; README there) turns PNG/JPEG, PDF
(hayro), SVG (resvg, draw.io CSS fixed up), DXF (craft-dxf, flattened `Linework`) and,
native only, DWG (craft-dwg → DXF in a worker copy of the binary: `main` calls
`craft_dwg::serve_if_asked`/`use_own_worker`; tests call `allow_in_process`) into what
the plan shows. Core `commands/annotate.rs`: `underlay.import/calibrate/align/update`,
`note.create/update`; kinds `underlay` (assets: source + PNG or JSON linework) and
`note`. Plan prim `Image`; sheets draw underlays only with viewport `underlays: true`.
GUI: Calibrate tool (K), File > Import underlay…, inspector rows. Tests:
`stagecraft-core/tests/underlay.rs`, `stagecraft-ui/tests/underlay.rs`.

## Acceptance: Hall A

`cargo test -p stagecraft-ui --test hall_a` builds the venue from
`testdata/private/hall_a.drawio.svg` through commands only (skips without it)
and writes `testdata/private/hall_a/` (one document, one PDF, plan/3D/house/side
PNGs; the 13 → 15 m portal check runs and is undone inside the test). The photos of the
real hall are in `testdata/private/hall_a/photos/` (private; never commit or
describe the people in them). Assumptions A1–A11 are in the script and as notes in the
plan.

## MVR and imported models

`stagecraft-mvr` (no StageCraft deps; README there) reads and writes MVR files;
`stagecraft-core/src/mvr.rs` maps them to the document (`mvr.import {path,
layer_mapping?}`, `mvr.export {path}`), `commands/mvr.rs` also has `model.import`
(glb, gltf, obj, stl, ply, fbx, 3mf through craft-mesh; 3ds through the GDTF reader;
`model_file::read` is the one reader core and geom use). Imported objects keep their MVR UUID and land at their MVR
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
