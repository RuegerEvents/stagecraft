# stagecraft-mvr decisions

Choices made while building the crate overnight (2026-11-18) where the spec or the
brief left room. Move into `docs/spec/decisions.md` when the crate joins the workspace.

| # | Decision | Why |
|---|---|---|
| 1 | One `Object` struct with `kind: ObjectKind` for all eight MVR object node types, not one struct per type. | The node types share almost all children (matrix, classing, GDTF, IDs, addresses …). One struct keeps tree walks, round trips and tolerance simple. Callers match on `kind`. |
| 2 | Import reads every known child on every kind; export writes what is set plus what the spec makes mandatory for that kind. | Files put nodes where the spec does not list them; dropping them would lose data. StageCraft only sets valid fields, so its own exports stay valid. |
| 3 | Unknown nodes are kept as raw XML (`extra`, `user_data`) and written back after the known children. | Vectorworks writes `FixtureTypeId` and `UserData/VWEntry`; future spec nodes survive a round trip. Their position among known siblings is not kept. |
| 4 | Export always writes MVR 1.6 with `provider="StageCraft"`; the file's header is kept in `Mvr::header` for information only. | The spec says provider names the exporting application. |
| 5 | Empty optional text nodes (`<GDTFSpec></GDTFSpec>`, `<Mappings/>`) become `None`/empty and are not written. FixtureID, FixtureIDNumeric and UnitNumber are plain values (default `""`/0) because the spec makes them mandatory. | Round trips compare equal at model level; no meaningless empty nodes in output. |
| 6 | Malformed UUIDs map to UUID v5 of their text (fixed namespace); nil/missing object UUIDs get a v5 from a counter; nil *references* mean "none". | References written with the same bad text stay linked, results are deterministic, no `rand` (wasm). Real files: two other exporters write nil symbol UUIDs, show d a layer without uuid. |
| 7 | DMX addresses stored absolute; import also accepts `Universe.Address`; `0` is read as "not patched" without a warning; empty `<Address/>` is skipped with a warning. | Matches the spec's integer form and the overview's "stored absolute". |
| 8 | Zip entry names: use the raw bytes when they are valid UTF-8, else the zip crate's CP437 decoding. | Vectorworks 31 writes UTF-8 names without the UTF-8 flag (`Custom@ARRI_Orbiter_15°.gdtf`); CP437 decoding would break the GDTFSpec link. |
| 9 | `Resources::resolve_gdtf` tries exact, `+".gdtf"`, then case-insensitive. | Non-VW exporters (two other exporters) write GDTFSpec without the extension. |
| 10 | Repeated warnings with the same message collapse into one (`(40×, first here)`) when there are more than three; reference checks group by message too. | Real files gave 95 identical nil-UUID warnings. |
| 11 | Export compression: JPEG/PNG/WebP/GIF stored, everything else (GDTF too) deflated; fixed 1980 timestamps; resources in name order. | Deterministic bytes. Storing GDTFs made exports ~2.5× larger because many GDTFs contain stored entries. |
| 12 | `Cargo.lock` not committed for this crate (crate-local `.gitignore`). | It is a library and will join the workspace lock. |
| 13 | Own `Matrix` type (f64, `u v w o`), no glam. | Keeps the crate dependency-light; the conversion to glam is a few lines in the integration. |
| 14 | XML written by a small hand-written writer (2-space indent, attributes in model order), parsed with quick-xml 0.42 into an owned tree first. | Deterministic output, tolerant parse that keeps what was read before a syntax error. Comments and processing instructions are dropped. |
| 15 | Fixture children with `ChildPosition` get `world = parent.world · local`; the GDTF geometry offset is not applied. | Needs the GDTF geometry tree; left to the integration with stagecraft-gdtf. |
| 16 | Perastage check uses `*.nogdtf.mvr` variants (GDTF files and GDTFSpec removed). | Perastage stops in a modal `PromptGdtfConflicts` dialog when an MVR's fixture types match its library (seen with the original Vectorworks file too). An unattended check cannot answer it. The variant still exercises layers, groups, trusses, symbols, geometry and fixture placement. |
