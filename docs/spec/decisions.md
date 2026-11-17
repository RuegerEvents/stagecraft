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
