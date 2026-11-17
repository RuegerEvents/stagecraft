# StageCraft: what it is

A stage and lighting planning tool in the spirit of Vectorworks Spotlight, with better
UX. Native macOS first; the engine is headless and the UI also builds for WASM.

Settled in the spec round of 2026-11-17. Changes to anything here go through the user.

## Principles

- **Every action is a command.** GUI clicks, the command palette, MCP and the control
  channel all call the same registry. Nothing in the UI changes the document directly.
- **One editor per document.** Other tools and AIs drive a running instance through the
  local control channel or MCP. The command journal is kept so real-time collaboration
  can be added later.
- **Control is local only** (127.0.0.1 + token). No cloud service in the MVP.

## Units and axes

- Lengths in millimetres (`f64`), angles in degrees.
- Axes as in MVR: x to the right as seen from the audience, y upstage, z up.
- Rotation is stored as `[x, y, z]` degrees and applied as Rz · Ry · Rx.

## Model

- **Hybrid 2D/3D.** Parametric objects (truss, platform, curtain, portal …) generate
  their 2D plan symbol and their 3D mesh from the same parameters. The 2D style
  (line weights, fill, detail level) can be overridden per object; free-hand deviation
  of the 2D is not supported.
- **Layers** are spatial: storeys or areas (stage, gallery, auditorium), each with an
  elevation. Every object has exactly one layer.
- **Tags** are free and many per object (light, sound, rigging, set …). Visibility and
  style per view are driven by tags. There are no Vectorworks-style classes.
- **Attachment.** Objects can have a parent: fixture on truss, truss pieces into a
  batten or a bridle, fixture on a stand or base plate, curtain on a batten. The child's
  placement is relative to the parent; moving or rotating the parent carries every
  descendant. This must be rock solid and is covered by tests.
- **Loads:** sums only. Weight per truss and per batten (own weight + GDTF weights of
  everything attached). No distribution, no deflection.

## Objects in the MVP

| Object | Notes |
|---|---|
| Floor area | polygon outline, thickness, finish. ("Bodengänge" in the brief = floor areas) |
| Wall | polyline path, height, thickness |
| Ceiling | polygon at a height, optional |
| Proscenium (portal) | opening width/height, depth, frame width |
| Main curtain | split, red, fold mesh, state open / closed / gathered (tab, Greek/Italian) |
| Legs + borders | as a "wing set" generated in one go per wing depth |
| Fly system | battens with number, position, trim height, load list |
| Platform | free size, presets 2×1, 1×1, 2×0.5 m; under-structure scissor or legs with height; Bütec / Nivoflex presets |
| Truss | profile (2/3/4 chord, tube Ø, width, kg/m), presets F34, F44P, H30V, X30D; pieces join into runs |
| Base plate | stands, ground support base |
| Fixture | native GDTF: real geometry in 3D, z respected, DMX mode, patch |

Catalogues (truss profiles, platform systems) are JSON files and user-extendable.

## Fixtures

- GDTF is the fixture model. Import from a local file or library folder, and from
  **GDTF Share inside the app** (login stored in the macOS keychain). Every GDTF used
  is embedded in the `.stagecraft` file.
- 2D symbols come from an SVG symbol library per fixture kind (profile, fresnel, PAR,
  moving head, wash, LED bar …), assignable per GDTF. Without an assignment the plan
  shows a top-view silhouette computed from the GDTF model.
- Patch in grandMA3 style: FID unique, CID optional (MVR FixtureID + CustomId), shown as
  Universe.Address (`1.001`), stored absolute. Auto-patch and auto-numbering for a
  selection in drawing order. Conflicts are flagged.

## Labels

Label templates belong to a viewport, so the electrician's plan and the focus plan show
different fields. Fields: FID, CID, Universe.Address, mode, colour/gel, circuit, focus,
note, any attribute.

How a label behaves on a rotated fixture is configurable per template, three modes:

1. **Anchored, upright** (default): field anchors follow the fixture ("in front of the
   lens"), the text stays horizontal on the sheet.
2. **Sheet-fixed**: the layout never rotates.
3. **Follow, never upside down**: rotates with the fixture, flipped by 180° past 90°.

Labels avoid each other automatically and can be dragged per viewport.

(The Vectorworks pain this fixes: one label layout per rotation, mirrored text.)

## Bulk editing

A spreadsheet panel for every object kind (fixtures, truss, platforms …), filtered by
tag and kind, selection synced with plan and 3D. Multi-cell edit, fill series
(FID 101, 102 …), copy/paste from Excel/Numbers. Patching happens here, many fixtures at
once, never one dialog per fixture.

## Interaction

- Command palette (Cmd+K) finds every command.
- A small toolbar of about ten tools.
- A context inspector showing only what fits the selection.
- Typed dimensions while drawing: click, type `4.5`, Enter.

## Sheets

- Sheets A4 to A0, portrait/landscape, with any number of viewports. A viewport has a
  view (plan, elevation, section, 3D), scale, crop, tag visibility, label template.
- Title block from a template with placeholders (project, venue, drawn by, date, scale,
  revision, logo), filled automatically.
- Per viewport choose **vector** (hidden lines removed, section hatching, line weights
  per tag) or **rendered** (raster image of the 3D view).
- Export: PDF.

## Import / export

- MVR import and export.
- DXF, PDF or image as an underlay on its own layer, scaled to a known dimension.
- 3D models: glTF/GLB, OBJ, 3DS.

## Verification

- UI tests with `egui_kittest`; snapshot tests for SVG and PDF output.
- MVR is checked against: the MVR spec sample files, grandMA3 onPC (driven through
  `../maai`), real Vectorworks exports in `testdata/private/vectorworks/`, and Perastage
  (`/Applications/Perastage.app`, opens MVR) as an independent viewer.
