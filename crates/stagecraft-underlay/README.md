# stagecraft-underlay

Reads drawings that go under a StageCraft plan. Pure Rust, no StageCraft
dependencies, builds for `wasm32-unknown-unknown` (without DWG).

| Format | How | Source unit |
|---|---|---|
| PNG, JPEG | shown as they are; only the pixel size is read (`image`) | pixel = 1/96 in |
| SVG | rendered with resvg on white, longest side ≤ 4096 px; Roboto stands in for every font; `var(--x, fallback)` / `light-dark()` resolved and `<foreignObject>` dropped first (draw.io) | SVG unit = 1/96 in |
| PDF | one page rendered with hayro on white, longest side ≤ 4096 px | point = 1/72 in |
| DXF | `craft-dxf` (ASCII and binary; a damaged file is read up to the damage, with a note) → `Linework` (polylines, texts): lines, (lw)polylines with bulges, arcs, circles, ellipses, splines (fit or control points), TEXT/MTEXT, block inserts (8 levels, arrays up to 1000 × 1000), object coordinate systems (mirrored blocks) | drawing unit from `$INSUNITS`, mm when unset |
| DWG | native only: `craft-dwg` (acadrust, MPL-2.0) converts to DXF, then as DXF. Runs in the worker process the app sets up (`craft_dwg::use_own_worker`); without one it is refused unless `craft_dwg::allow_in_process()` was called (tests, trusted input). ≤ 256 MB | as DXF |

`convert(format, bytes, page) -> Converted { raster, linework, extent, unit_mm, pages, notes }`.
Everything is in source units with y up; rasters span `[0, 0]`–`[w, h]`.

```
cargo run -p stagecraft-underlay --example convert -- plan.pdf out.png [page]
```
