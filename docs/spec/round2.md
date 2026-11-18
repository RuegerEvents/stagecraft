# Round 2: craft-libs, UI overhaul, sheets, tutorial

User decisions from 2026-11-18 (after testing the overnight build). Binding for this round.

## Libraries (storytold/craft-libs)

Pin every craft-libs dependency to the same commit:
`git = "https://github.com/storytold/craft-libs", rev = "52fc6b5b3d2179239664136e84543231f8576f8d"`
(MIT/Apache; add a row to a `THIRD_PARTY.md` or the README's licence section).

Use (no egui needed):
- `craft-dialogs`: all file and message dialogs. Fixes the crash: `rfd` was called
  synchronously inside `App::ui` (`app.rs` `file_dialogs`); dragging a file over the window
  while the modal panel ran re-entered winit ("tried to handle event while another event is
  currently being handled"). Dialogs must never block inside a frame.
- `craft-dxf` + `craft-dwg`: underlay import (DXF and now DWG) replaces the `dxf` crate.
- `craft-mesh`: `model.import` for OBJ, STL, PLY, glTF/GLB, FBX, 3MF.
- `craft-zip` + `craft-xml`: bounded reading of `.mvr`, `.gdtf` and `.stagecraft` archives.
- `craft-atomic`: saving.
- `craft-mcp-server`, `craft-jsonrpc`, `craft-control` (as far as they fit): the control
  channel and MCP; MCP `ui.screenshot` may return the image as an MCP image result.

With the egui fork (user said yes): move to storytold's egui fork, egui 0.37 plus the
suite's patches, at the revision craft-libs pins in its workspace `[patch]`. Then use
`craft-ui` (docking, icon buttons, toolbar overflow, theme helpers), `craft-menu` (native
macOS menu bar with key equivalents, through craft-keymap), `craft-window` (macOS inset
title bar), `craft-quit`. egui_kittest must come from the same fork.

## UI

- Style **A · Studio**: neutral dark grey (panels #232428, canvas #1b1c1f, separators
  #34363b, text #e6e6e8 / #8d9097), one blue accent #3d7bfd, compact grouped icon toolbar,
  panels as tabs; matching light variant. Sheets stay white paper.
- All panels dockable (layers, tags, library, inspector, table, tutorial …); layout saved
  per user, with "Reset layout".
- Toolbar: icons with tooltip (name + shortcut), grouped. Lucide icons (ISC licence) for
  generic tools, own stage icons in the same stroke style (portal, curtain, wings, batten,
  truss, run, platform, support, hoist, fixture, ceiling, floor, wall).
- Native macOS menu bar (File, Edit, View, Insert/Tools, Window, Help) wired to commands.

## Floor usability (user could not set a floor's height, and did not see it in 3D)

- Floor tool gets options like the wall tool: top height (z) and thickness.
- Clicking inside a floor that follows the sketch selects the floor; sketch lines win only
  near the line itself. Inspector labels say what is meant ("Top height", "Thickness").
- Make sure a new floor is visible in 3D (check camera/fit, layer elevation, thickness).

## Sheets

- New default title block "compact", modelled on the user's Vectorworks sheet:
  border with grid reference zones (1–n across, A–n down), compact block bottom right
  (about a quarter of the width): row 1 sheet title bold + scale right-aligned bold;
  then "Production" label + production name large bold, venue address small, dates bold;
  right column "Drawing" with company, drawer, website. Helvetica-like font, black only,
  thin lines. **The fixture legend sits directly above the title block, same width.**
  The old template stays selectable.
- Sheet language selectable per document, default **English** (German available:
  Produktion, Zeichnung, Maßstab, Legende …).
- Dimension text format **configurable** per document: metres with superscript mm
  (6,15⁵), metres (6.155 m), millimetres (6155); decimal separator configurable
  (default from the sheet language: en ".", de ","). Applies to plan and sheets.

## Tutorial

Guided tour inside the app: a dockable panel with steps that detect completion from the
document state and point at the right tool: layers, sketch the room, floor with height,
portal, main curtain, batten, truss, fixtures (GDTF), patch, sheet with viewports,
PDF export. Covered by a UI test that walks all steps through the GUI.

## Working rules

As in `docs/architecture.md` (no questions, signed commits, fmt/clippy/test/wasm before
commit, decisions in `docs/spec/decisions.md`).
