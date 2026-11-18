# Toolbar icons

`lucide/`: icons from Lucide (lucide-static 0.469.0, <https://lucide.dev>), unmodified, ISC
licence in `lucide/LICENSE`. mouse-pointer-2 (Select), move, rotate-cw, ruler (Calibrate),
search (command palette), undo-2, redo-2, scan; for the tool bar file-plus, folder-open, save,
file-input, printer, file-output, graduation-cap, ellipsis; panel tabs layers, tags,
file-text, library, cloud-download, sliders-horizontal, table, menu (≡); status bar
grid-3x3, magnet, waypoints, spline, sun-moon; lists eye, eye-off, lock, plus,
chevron-down, chevron-right, box.

`stage/`: StageCraft's own icons for the stage tools, drawn for this project in Lucide's
style: 24 × 24 view box, `stroke="currentColor"`, `stroke-width="2"`, round caps and joins,
no fills. floor, wall, dimension, truss, run, fixture, batten, curtain, portal, wings,
platform, support, hoist, ceiling, and the sketch constraints c-horizontal, c-vertical,
c-parallel, c-perpendicular, c-equal, c-symmetric. Same licence as StageCraft.

`src/icons.rs` embeds both, rasterises them with resvg at the drawn size and tints them.
`STAGECRAFT_ICONS=<dir> cargo test -p stagecraft-ui --lib icon_sheet` writes every icon as a
96 px PNG.
