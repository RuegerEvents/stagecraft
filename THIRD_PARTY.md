# Third-party code

Crates from crates.io are listed with their licences in `Cargo.lock` / `cargo tree`. Code
pulled from elsewhere:

| What | Source | Licence | Used for |
|---|---|---|---|
| craft-libs | <https://github.com/storytold/craft-libs>, rev `52fc6b5b3d2179239664136e84543231f8576f8d` | MIT OR Apache-2.0 | `craft-dialogs` (non-blocking file and message dialogs), `craft-atomic` (crash-safe saves and exports), `craft-zip` (bounded reading of `.mvr`, `.gdtf`, `.stagecraft`), `craft-xml` (bounded MVR and GDTF XML), `craft-dxf` (DXF underlays), `craft-dwg` (DWG underlays) |
| acadrust 0.6.6 | crates.io, used unmodified behind `craft-dwg`; licence and notices in craft-libs `licenses/dwg/` | MPL-2.0 | reading DWG (native builds) |
| Roboto, Roboto Mono | `assets/fonts/` (see the README there) | Apache-2.0 | GUI text |
| Arimo | `assets/fonts/` (see the README there, licence in `Arimo-OFL.txt`) | OFL-1.1 | Sheet text (PDF, SVG, Sheets mode) |
