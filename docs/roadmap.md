# Roadmap

Estimate from 2026-11-17: 14–21 active days for the whole MVP.

| # | Block | Days | Status |
|---|---|---|---|
| 1 | Document model, command engine, undo/redo, file format, MCP + control channel, headless | 1–2 | done 2026-11-17 (41 commands) |
| 2 | 2D drawing: canvas, snapping, polylines, walls, floors, dimensions, layers/tags, palette, inspector | 3–4 | done 2026-11-17 (geom crate, GUI, 14 ui.* commands, first table) |
| 3 | 3D view (wgpu): extrusions, camera, section planes with filled cuts | 2–3 | done 2026-11-18 (meshes for all kinds, section cuts, wgpu view, plan/3D/split, `ui.camera`, `ui.section`) |
| 4 | Stage objects: portal, curtains, fly system, platforms, truss, base plates, ceiling, catalogues | 2–3 | done 2026-11-18 (11 kinds, 22 commands incl. `truss.run`, `load.report`, `batten.list`; 9 tools; fly bar list; user catalogues) |
| 5 | GDTF: parse, 3D models, modes, patch, FID/CID, GDTF Share, symbols, load sums | 3–4 | done 2026-11-18 (gdtf crate in the workspace, assets embedded in format 2, 11 commands `gdtf.*`/`patch.*`/`symbol.*`/`fixture.set_type`, GDTF meshes and symbols cached per type, 11 built-in SVG symbols, multi-break patch with conflicts, library panel with GDTF Share off the UI thread, demo on synthetic GDTFs) |
| 6 | Vector views from 3D: hidden lines, section hatching; rendered viewports | 2–3 | done 2026-11-18 (`stagecraft-hlr` in the workspace, `HlrSource` default for elevations/sections and new `plan_3d` viewports, material hatches, `hidden_lines`, per-tag line styles, cache per document revision, Sheets composed off the UI thread, rendered viewports from an offscreen wgpu pass incl. CLI export) |
| 7 | Sheets: viewports, title block, labels (three modes), PDF | 2–3 | done 2026-11-18 (`stagecraft-sheet`, 20 commands `sheet.*`/`viewport.*`/`label.*`/`titleblock.*`/`object.set_attrs`, krilla PDF + SVG, Sheets mode, plan labels; elevations/sections use a mesh-outline fallback until HLR is wired in) |
| 8 | MVR import/export, DXF/PDF/image underlays, glTF/OBJ/3DS | 1–2 | MVR and models done 2026-11-18 (`stagecraft-mvr` in the workspace, `mvr.import`/`mvr.export`/`model.import`, kinds `group` and `model`, File menu; all private MVRs import with matching counts and round trip; grandMA3 export → StageCraft matches 23/23 fixtures). DXF/PDF/image underlays still open |
| 9 | Parametric floor-plan sketch: ezpz solver, driving + reference dimensions (docs/spec/sketch.md) | 2–3 | |
| 10 | Acceptance: draw the Hall A venue as a parametric sketch (docs/spec/sketch.md) | 0.5 | |
| — | Spreadsheet panel (part of 2 and 5) | | |

Blocks 4, 5 and 7 can run in parallel worktrees once 1–3 stand.
