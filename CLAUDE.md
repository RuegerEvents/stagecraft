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
  (`cargo check -p stagecraft-core -p stagecraft-geom --target wasm32-unknown-unknown`).
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
`pick.rs` and `snap.rs` with tests. Truss profiles live in
`catalog/truss_profiles.json`. It must build for wasm too.

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

- 3D view in `src/view3d/`: `camera.rs` (orbit camera, pure maths), `scene.rs`
  (per-object world-mesh cache keyed on kind, `Lod` and world transform; flattened
  GPU buffers with versions; cut faces; ray picking), `gpu.rs` + `*.wgsl` (offscreen
  4× MSAA pass with reversed depth in the paint callback's `prepare`, blitted in
  `paint`). `gpu::install(render_state)` must run once per wgpu render state:
  `run` does it from eframe's creation context; headless tests build their own
  render state (see `tests/screens.rs`), otherwise the 3D view stays empty.
  `ui.set_view` switches plan / 3d / split (⌘1/⌘2/⌘3); `ui.camera` and
  `ui.section` drive the rest.

Look at the screens after UI changes:

```
STAGECRAFT_SCREENSHOTS=/some/dir cargo test -p stagecraft-ui --test screens
```

renders the demo document (`demo.rs`) in light and dark to PNG, including 3D iso,
front, sections, a truss close-up and split.

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
```
