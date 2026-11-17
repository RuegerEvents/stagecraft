# stagecraft-mvr

MVR (My Virtual Rig, DIN SPEC 15801) import and export for StageCraft. Reads MVR 1.4 to
1.6 files, writes MVR 1.6. No dependency on other StageCraft crates; GDTF files are
carried as bytes. Builds for `wasm32-unknown-unknown`.

Built standalone for now (the workspace excludes it):

```
cd crates/stagecraft-mvr
CARGO_TARGET_DIR=../../target-mvr cargo test
CARGO_TARGET_DIR=../../target-mvr cargo test --release --test real_files -- --nocapture
```

## API

```rust
use stagecraft_mvr::*;

let imp: Import = read_mvr(&bytes)?;          // or read_mvr_file(path)
for w in &imp.warnings { eprintln!("{w}"); }   // everything odd about the file
let mvr: Mvr = imp.mvr;                        // header, scene, resources

for (layer, obj) in mvr.scene.objects() {      // depth first, every object
    if obj.kind == ObjectKind::Fixture {
        let gdtf = obj.gdtf_spec.as_deref().and_then(|s| mvr.resources.resolve_gdtf(s));
        let pos_mm = obj.world_position();
        let patch = obj.addresses.first().map(|a| (a.universe(), a.address()));
    }
}
let models = mvr.scene.geometry_instances();   // every placed 3DS/GLB, symbols expanded

let bytes: Vec<u8> = write_mvr(&mvr)?;         // MVR 1.6, provider="StageCraft"
```

| Item | What it is |
|---|---|
| `read_mvr(&[u8]) -> Result<Import, MvrError>` | Zip → `Mvr` + `Vec<Warning>`. Errors only for "not a zip" and "no `GeneralSceneDescription.xml`". |
| `write_mvr(&Mvr)`, `write_mvr_scene(&Scene, &Resources)` | → `.mvr` bytes. Deterministic. |
| `read_scene_xml(&str)`, `write_scene_xml(&Scene)` | The XML alone. |
| `Mvr { header, scene, resources }` | `header` is what the file said (version, provider); export writes its own. |
| `Scene { layers, aux, user_data, extra }` | `aux`: symdefs, positions, classes, mapping definitions. |
| `Layer { uuid, name, matrix, children, extra }` | |
| `Object` | One struct for all node types; `kind: ObjectKind` (SceneObject, GroupObject, FocusPoint, Fixture, Truss, Support, VideoScreen, Projector). Fields a kind does not use stay empty. |
| `Geometry::{Geometry3D, Symbol}` | `Geometries` entries and symdef contents. |
| `Resources` | `BTreeMap<String, Vec<u8>>` of every other archive file; `resolve_gdtf`, `resolve`, `names_of(ResourceKind)`. |
| `Scene::resolve_world()` | Recomputes `Object::world` from local matrices. Import calls it; call it after editing matrices. |
| `Scene::objects()`, `objects_of(kind)`, `find(uuid)`, `symdef`, `class`, `position`, `stats()` | Queries. |
| `Scene::geometry_instances()` | Model files with world transforms, symbols expanded (cycle-safe). |
| `check(&Scene, &Resources)` | Reference and file checks (also run by import). |

### Fixture fields

`uuid`, `name`, `matrix` (local), `world`, `gdtf_spec`, `gdtf_mode`, `fixture_id`
(text), `fixture_id_numeric`, `unit_number`, `custom_id`, `custom_id_type`,
`addresses: Vec<DmxAddress { dmx_break, absolute }>` (`universe()`, `address()`),
`networks`, `protocols`, `focus` (UUID of a FocusPoint), `position` (UUID of a
Position), `classing`, `color` (CIE xyY), `gobo`, `function`, `cast_shadow`,
`dmx_invert_pan/tilt`, `child_position`, `multipatch`, `alignments`,
`custom_commands`, `overwrites`, `connections`, `mappings`, `children`. Unknown child
nodes (Vectorworks' `FixtureTypeId`, future spec additions) are kept in `extra` as
raw XML and written back.

## Conventions

- Lengths in mm, angles in degrees.
- Axes as in MVR: x right as seen from the audience, y upstage, z up, right-handed.
- `Matrix { u, v, w, o }` is the MVR 4×3 matrix: a local point `p` lands at
  `o + p.x·u + p.y·v + p.z·w` in the parent. `parent.then(&local)` composes.
  `Object::matrix` is relative to the parent object or the layer; `Object::world`
  is scene space (layer matrix included).
- Children of a fixture with `child_position` are placed relative to a geometry of the
  parent's GDTF. Without the GDTF this crate composes with the parent's origin; the
  caller has to add the geometry offset.
- Symbols inside a `Symdef` are relative to the symdef origin; `geometry_instances`
  chains them.
- UUIDs are `uuid::Uuid`, written upper case. A malformed UUID becomes a UUID v5
  derived from its text (so references with the same text still link); a nil or
  missing one gets a generated replacement. Both produce a warning.
- DMX addresses are stored absolute (`(universe-1)·512 + address`); `"U.A"` text is
  accepted on import.

## Export

- `GeneralSceneDescription verMajor="1" verMinor="6" provider="StageCraft"
  providerVersion=<crate version>`.
- Child nodes in the order of the spec tables, then unknown nodes kept from import.
  Mandatory nodes are written even when empty (`Geometries`, `FixtureID`,
  `FixtureIDNumeric`, `UnitNumber` for fixtures, `Projections` for projectors).
- Floats in the shortest form that reads back to the same `f64`.
- Zip: scene file first, then resources sorted by name; fixed timestamps; JPEG/PNG
  stored, the rest deflated. Same scene → same bytes.

## Tests

- `tests/synthetic.rs`: a scene with every node type and field, round trip, world
  matrices through layer/group/truss/symbol chains, a messy exporter-style file,
  fatal errors, mutation fuzzing of the XML, symbol cycles.
- `tests/real_files.rs`: imports every `.mvr` in `testdata/private/{vectorworks,other}`
  (skips if absent), prints a summary and warnings, round-trips and checks the scene
  stays equal and export is stable. `STAGECRAFT_MVR_OUT=dir` writes the exports
  (`*.roundtrip.mvr`, plus `*.nogdtf.mvr` for viewer checks, see DECISIONS.md).

See `DECISIONS.md` for choices where the spec was silent.
