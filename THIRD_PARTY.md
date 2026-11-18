# Third-party code

Crates from crates.io are listed with their licences in `Cargo.lock` / `cargo tree`. Code
pulled from elsewhere:

| What | Source | Licence | Used for |
|---|---|---|---|
| craft-libs | <https://github.com/storytold/craft-libs>, rev `52fc6b5b3d2179239664136e84543231f8576f8d` | MIT OR Apache-2.0 | `craft-dialogs` (non-blocking file and message dialogs), `craft-atomic` (crash-safe saves and exports), `craft-zip` (bounded reading of `.mvr`, `.gdtf`, `.stagecraft`), `craft-xml` (bounded MVR and GDTF XML), `craft-dxf` (DXF underlays), `craft-dwg` (DWG underlays), `craft-mesh` (model files), `craft-mcp-server` (MCP), `craft-jsonrpc` and `craft-control` (bounded control-channel lines, token compare and generation) |
| acadrust 0.6.6 | crates.io, used unmodified behind `craft-dwg`; licence and notices in craft-libs `licenses/dwg/` | MPL-2.0 | reading DWG (native builds) |
| storytold/egui | <https://github.com/storytold/egui>, rev `6d518fc799681571963842f95b55a6b68431b15a` (egui 0.37 plus the Craft suite's patches), with storytold/winit rev `a517b3846ee8457d84ee4975110bfda6797c44dc`; root `[patch.crates-io]` | MIT OR Apache-2.0 | egui, eframe, egui-wgpu, egui-winit, egui_kittest |
| DejaVu Sans (cut) | `assets/fonts/DejaVuSans-Symbols.ttf` (see the README there, licence in `DejaVu-LICENSE.txt`) | Bitstream Vera licence, DejaVu changes public domain | Arrows and key symbols in the GUI |
| Lucide icons (lucide-static 0.469.0) | `crates/stagecraft-ui/assets/icons/lucide/` (see the README there, licence in `LICENSE`) | ISC | Tool bar, tool palette, panel tab and status bar icons |
| Roboto, Roboto Mono | `assets/fonts/` (see the README there) | Apache-2.0 | SVG underlay text, monospaced GUI text |
| Inter 4.001 | `assets/fonts/Inter-*.ttf` (see the README there, licence in `OFL-Inter.txt`), the files storytold/photocraft ships | OFL-1.1 | GUI text |
| CADCraft (storytold/cadcraft) | <https://github.com/storytold/cadcraft>, `crates/ui-egui/src/theme.rs` and `chrome.rs` | MIT OR Apache-2.0 | The chrome's design tokens (slate chrome, navy model space, the colour values), the 28 pt macOS title band with the traffic-light geometry, the tool bar, status bar and Model/sheet tab layout, the property-row layout (`stagecraft-ui/src/theme.rs`, `chrome.rs`, `widgets.rs`); adapted, not copied verbatim |
| Arimo | `assets/fonts/` (see the README there, licence in `Arimo-OFL.txt`) | OFL-1.1 | Sheet text (PDF, SVG, Sheets mode) |
