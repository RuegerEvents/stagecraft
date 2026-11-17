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
