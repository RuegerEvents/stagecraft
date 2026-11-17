# Decisions

Choices made while building, where the spec did not say. Add a row when you make one.

| Date | Topic | Decision | Why |
|---|---|---|---|
| 2026-11-17 | Undo | Undo keeps whole document versions. The document uses `imbl` persistent maps, so a version costs only the changed nodes. | Simple and always correct, no inverse operation per command. |
| 2026-11-17 | Command registration | Explicit list in `registry.rs`, no `inventory`. | `inventory` is unreliable on WASM. |
| 2026-11-17 | Geometry in the API | Points are plain `[f64; 2]` / `[f64; 3]` in mm; `glam` only inside computations. | JSON schemas stay simple for AIs. |
| 2026-11-17 | MCP shape | Three tools (`commands`, `describe`, `execute`) instead of one tool per command. | Hundreds of commands would flood an MCP client's tool list. FilmCraft uses the same shape. |
| 2026-11-17 | Discovery | A running instance writes `instances/<pid>.json` (port, token, file) into the app data dir; MCP finds it there. | FilmCraft's control channel cannot be found from outside; we avoid that. |
| 2026-11-17 | Async | Control server on plain `std` threads and channels, no tokio. | Small, and the engine is single-owner anyway. |
| 2026-11-17 | Test data | Real MVRs live in `testdata/private/` and are git-ignored. | They are client projects. |
| 2026-11-17 | UI language | English UI, i18n catalog from the start (user). | User choice. |
| 2026-11-17 | Theme | Light and dark, follows macOS, switchable (user). Sheets always white. | User choice. |
| 2026-11-17 | Truss 3D | Real chords and diagonals, simpler level of detail when zoomed out; 2D double line with diagonals (user). | User choice. |
| 2026-11-17 | Overnight order | Blocks in roadmap order; parallel worktrees where independent: 2 → 3 → (4 ∥ 5 ∥ 6) → (7 ∥ 8) (user: roadmap order, parallel allowed). | User choice. |
| 2026-11-17 | Title block | Generic with placeholders, company/logo from document settings (user). | User choice. |
| 2026-11-17 | Typed dimensions | While drawing, a plain number is metres ("4.5" = 4.5 m); `mm`/`cm`/`m` suffixes work. Tab switches to the angle (degrees, default: towards the pointer). Inspector and table fields are mm (with optional `m` suffix). | The spec's example "type 4.5" means metres; data fields stay in the model's unit. |
| 2026-11-17 | Navigation | Mouse wheel (line units) zooms at the pointer; trackpad scroll (point units) pans; pinch and ⌘-scroll zoom; middle-drag or Space-drag pans. | Matches both input devices without a setting. |
| 2026-11-17 | Box selection | Left-to-right = window (fully inside), right-to-left = crossing (touching). | CAD convention, as in Vectorworks/AutoCAD. |
| 2026-11-17 | Pick priority | Under the pointer: fixtures before truss before walls before floors, then nearest. | Small things sit on big things; a click on a floor area should not hide what stands on it. |
| 2026-11-17 | Snap order | Endpoint, midpoint, truss axis (distance along rounded to the grid), grid (100 mm default). Shift locks horizontal/vertical from the last point and aligns with nearby object points. | Object snaps beat the grid; truss positions stay on round numbers. |
| 2026-11-17 | Tag visibility | Per view (UI state, `ui.set_tag_visible`), not in the document. An object is hidden if any of its tags is hidden; untagged objects always show. Layer visibility stays in the document. | Spec: visibility per view by tags. Hiding "light" should hide a fixture also tagged "front". |
| 2026-11-17 | New truss height | The truss tool draws at 6000 mm above the layer by default (tool option). Fixtures clicked onto a truss hang at the profile's half height below its axis; elsewhere they stand on the layer at z 0. | Plan drawing has no z; a hung truss is the common case. |
| 2026-11-17 | New objects | Go on the active layer (●, `ui.set_active_layer`), or the first layer. A created object is selected. | So the inspector shows it right away. |
| 2026-11-17 | Inspector multi-edit | A field shows a value when all selected agree, else "mixed"; an edit applies to all as one `batch`. Absolute position edits apply to objects whose parent is not selected (children follow). FID for several fixtures numbers them (`fixture.number`) in plan order. | FIDs must stay unique; moving parent and child both would double the move. |
| 2026-11-17 | Table order | Fixtures with a FID by FID; the rest in plan order (x, then y). Cell edits on a range apply to every cell; fill series counts from the first cell, with the step from the first two when both are filled; addresses roll over universes. | A patch list reads by FID; plan order is stable while numbering. |
| 2026-11-17 | Fixture plan symbol | Generic 260 × 340 mm box with a lens edge at local +y and the FID inside, until block 5 brings GDTF/SVG symbols. | Something readable at plan scale. |
| 2026-11-17 | Truss catalogue | Built-in JSON in `stagecraft-geom/catalog/` (F24, F31–F34, F44P, X30D/V, H30V, H40V; nominal sizes, typical kg/m). Unknown profile names draw as an F34-sized box. User catalogues come later. | Spec: catalogues are JSON; numbers checked against maker data later. |
| 2026-11-17 | Fonts and icons | Roboto vendored (Apache 2.0, copied from grandPA); arrows/crosses painted, as Roboto lacks them. | Same look on every machine and in headless PNGs. |
| 2026-11-17 | Kind updates | Added `floor.update`, `wall.update`, `truss.update` to core. | The inspector and the table need command paths for kind parameters. |
| 2026-11-17 | Mesh detail | `Lod` is `Full` (truss chords, diagonals, end frames), `Simple` (chords only), `Box` (outer box). Only truss differs; other kinds ignore it. Meshes depend on kind parameters and `Lod` only, never on placement, so views cache them per object. | User: real chords and diagonals, simpler when zoomed out. |
| 2026-11-17 | Mesh layout | Flat faces get their own vertices (sharp normals); a mesh carries material groups (`Group`) instead of one class, so a fixture has body and lens. Watertight = every edge (by position) used equally often in both directions, so touching closed parts (braces at a node) count. | Sharp edges read better on CAD geometry; GDTF models will need several materials too. |
| 2026-11-17 | Truss mesh | Axis along local x through the centre of the section; three-chord profiles stand on two chords, apex up; diagonals ≈ 40 % of the chord Ø, one bay per face width, zigzag phase alternating per face; braces end on the chord surface. | Matches the plan symbol and common 3-point truss rigging. |
| 2026-11-17 | Fixture placeholder | Until GDTF (block 5): yoke bar, body 260 × 340 × 250 mm, lens cylinder at local +y. GDTF convention: origin = hanging point, the body hangs below (−z). The fixture tool turns floor fixtures over (rotation x 180°); the demo does too. | MVR/GDTF put standing fixtures upside down the same way; block 5 then needs no conversion. |
| 2026-11-17 | Wall rings | A wall path whose last point meets the first closes into a mitred ring. | A room outline drawn as one wall should not get a seam with end caps. |
| 2026-11-17 | Section planes | A `Plane` hides the side its normal points to. Plane coordinates: `v` = world up projected into the plane (world +y for horizontal planes), `u = v × n`, so a viewer on the hidden side sees u right, v up. Cut loops: outers counter-clockwise, holes clockwise; holes by nesting. Cap triangulation with `earcutr`. | One convention for the 3D clip, cut faces now and vector sections in block 6. |
