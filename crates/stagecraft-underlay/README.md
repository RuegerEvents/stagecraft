# stagecraft-underlay

Reads drawings that go under a StageCraft plan. Pure Rust, no StageCraft
dependencies, builds for `wasm32-unknown-unknown`.

| Format | How | Source unit |
|---|---|---|
| PNG, JPEG | shown as they are; only the pixel size is read (`image`) | pixel = 1/96 in |
| SVG | rendered with resvg on white, longest side ≤ 4096 px; Roboto stands in for every font; `var(--x, fallback)` / `light-dark()` resolved and `<foreignObject>` dropped first (draw.io) | SVG unit = 1/96 in |
| PDF | one page rendered with hayro on white, longest side ≤ 4096 px | point = 1/72 in |
| DXF | `dxf` crate → `Linework` (polylines, texts): lines, (lw)polylines with bulges, arcs, circles, ellipses, splines (fit or control points), TEXT/MTEXT, block inserts (8 levels, arrays) | drawing unit from `$INSUNITS`, mm when unset |

`convert(format, bytes, page) -> Converted { raster, linework, extent, unit_mm, pages }`.
Everything is in source units with y up; rasters span `[0, 0]`–`[w, h]`.

```
cargo run -p stagecraft-underlay --example convert -- plan.pdf out.png [page]
```
