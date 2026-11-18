# Architecture and working rules for the build-out

Written 2026-11-17 before the overnight run of blocks 2–8. Agents working on a block
read this, `CLAUDE.md`, `docs/spec/overview.md` and `docs/spec/control.md` first.

## Crates (target state)

| Crate | Holds | Depends on |
|---|---|---|
| `stagecraft-core` | document, commands, undo, files, load sums | – |
| `stagecraft-geom` | per-object representations: plan (2D) primitives and 3D meshes; triangulation; section cuts | core, gdtf, hlr |
| `stagecraft-hlr` | hidden-line removal and section cuts for orthographic views of triangle meshes | – |
| `stagecraft-gdtf` | GDTF parsing (description.xml, geometry tree, modes, footprints, weight), 3DS/GLB model loading, GDTF Share client | core types only where needed |
| `stagecraft-mvr` | MVR file model, reader and writer, GLB writer (the mapping to the document is in core) | – |
| `stagecraft-sheet` | sheets, viewports, title blocks, labels, PDF (krilla), vector views through hlr | core, geom, hlr |
| `stagecraft-ui` | egui/eframe app: plan canvas, 3D view (wgpu), sheets, palette, inspector, tables | all above |
| `stagecraft-control` | TCP JSON-RPC, discovery, MCP | core |
| `stagecraft-app` | the `stagecraft` binary; no subcommand opens the GUI | all |

Logic stays out of `stagecraft-ui`. Every crate except `stagecraft-ui`,
`stagecraft-control` and `stagecraft-app` must keep building for
`wasm32-unknown-unknown`.

## The representation contract

`stagecraft-geom` turns a document object into what views draw. One module per object
kind (`geom/src/kinds/<kind>.rs`) implements both:

- `plan(doc, obj, &PlanStyle) -> Vec<PlanPrim>`: 2D primitives in **world** mm for the
  plan view. `PlanPrim` = polyline / polygon (with fill class) / circle / arc / text,
  each with a line class (thin, medium, thick, hidden, centre) so sheets can map classes
  to line weights. Parameters and per-object style overrides drive it (hybrid model).
- `mesh(doc, obj, Lod) -> Mesh`: triangles in the object's **local** space (positions,
  normals, indices, material class). The caller applies `geom::world`.

Adding an object kind = a variant in `ObjectKind`, a create command, a module in
`geom/src/kinds/`, tests. Parallel blocks add their own kind modules, so conflicts stay
in a few registration lines.

## UI state is controllable too

The GUI owns a second, small registry of `ui.*` commands (active tool, view, zoom, pan,
camera, theme, open panel, screenshot to PNG). In GUI mode the control server answers
`commands`/`describe`/`execute` for both registries. `ui.screenshot {path}` must work
so an AI can see the app.

## UI rules

- English UI text through an i18n catalog (`t("key")`, English catalog embedded);
  command names are English anyway.
- Light and dark theme, following macOS by default, switchable (`ui.set_theme`).
  Sheets are always white paper.
- Command palette (Cmd+K) lists every command of both registries.
- Typed dimensions while drawing.
- UI tests with `egui_kittest`; widgets are found by accessible name. Screens can be
  rendered to PNG without a window (`STAGECRAFT_SCREENSHOTS=dir cargo test -p
  stagecraft-ui --test screens`), and agents look at those PNGs to check their work.

## Versions

egui/eframe/egui_kittest 0.36.x and wgpu as egui pulls it (same as grandPA), glam 0.34,
quick-xml 0.42, krilla for PDF, lyon or earcutr for triangulation, i_overlay for 2D
booleans, ureq for HTTP, keyring/security-framework for the keychain.

## Working rules for agents (overnight, no human)

- Do not ask the user anything. Where the spec is silent, choose, and add a row to
  `docs/spec/decisions.md`.
- Commit early and often, always signed (`git commit -S`); the gpg agent caches the
  key. If signing fails, stop and report; never commit unsigned.
- Before each commit: `cargo fmt`, `cargo clippy --all-targets` with no warnings,
  `cargo test` green, wasm check for the crates that must build there.
- End commit messages with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Update `docs/roadmap.md` status for your block when done.
- Real MVRs in `testdata/private/` are client data: use them for tests that skip when
  the files are missing, never commit them, never copy their content into fixtures.
  Small synthetic fixtures under `testdata/` are fine.
- GDTF Share credentials are in the macOS keychain, service `StageCraft GDTF Share`
  (`security find-generic-password -s "StageCraft GDTF Share"`). Never print or commit
  them.
- grandMA3 onPC and Perastage may be launched for checks; back up the MA show first
  (see `../maai`), close the apps afterwards.
