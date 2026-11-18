# stagecraft-gdtf

GDTF 1.2 fixture types for StageCraft: `description.xml`, the geometry tree, DMX
modes with footprints, 3DS/GLB models assembled into a fixture mesh, a top-view
silhouette for plan symbols, and a GDTF Share client.

No dependencies on other StageCraft crates (core depends on this one). Default
features build for `wasm32-unknown-unknown`. Decisions are rows "GDTF: …" in
`docs/spec/decisions.md`.

```
cargo test -p stagecraft-gdtf
cargo test -p stagecraft-gdtf --features keychain   # with the Share client
```

## Coordinates and units

| What | Frame | Units |
|---|---|---|
| Parsed types (`Model`, `Geometry::position`, `BeamData`, `Physical`) | GDTF | as in the XML: m, °, W, kg (field names say which) |
| `Mesh`, `GeometryInstance::{local, world}`, `Silhouette` | GDTF | mm |

GDTF axes: **x** right, **y** away from the viewer, **z** up. The fixture is
described hanging; beams emit along **−z**. This is the fixture-local frame of an
MVR fixture (MVR: x right, y upstage, z up, mm), so a fixture's mesh is placed with
its MVR matrix and nothing else. GLB files (y-up) are turned to z-up on load.

## API

```rust
use stagecraft_gdtf::{Gdtf, Lod, silhouette};

let gdtf = Gdtf::from_bytes(&bytes)?;          // Err only for a broken archive/XML
for w in &gdtf.warnings { eprintln!("{w}"); }  // everything odd but survivable

let ft = &gdtf.fixture;                        // FixtureType
ft.name; ft.manufacturer; ft.long_name; ft.fixture_type_id;
ft.physical.weight_kg;                         // Option<f32>
ft.power_w();                                  // Option<f32>
gdtf.thumbnail_png();                          // Option<&[u8]>

// DMX
let mode = gdtf.resolve_mode("Mode 1").unwrap();   // ResolvedMode
mode.footprint;            // BTreeMap<break, slots>, e.g. {1: 27}
mode.footprint_break1();   // 27
mode.channels;             // PatchedChannel { dmx_break, offsets (1-based), geometry, attribute }
mode.virtual_channels;     // no slots
gdtf.resolve_modes();      // all modes

// 3D
let mesh = gdtf.mesh(Some("Mode 1"), Lod::Default);   // FixtureMesh, None = first mode
mesh.flatten();            // Mesh: positions/normals/indices in mm, one group per part path
mesh.axes();               // part indices of Axis geometries (pan, tilt …)
mesh.beams();              // part indices of Beam geometries (BeamData in instance.kind)
mesh.posed_transforms(&|i, part| /* extra local rotation for part i */ glam::Mat4::IDENTITY);
mesh.flatten_posed(&pose); // the same, merged
mesh.parts[i].instance;    // GeometryInstance { name, path, kind, model, parent, local, world }
mesh.parts[i].mesh;        // Option<Arc<Mesh>> in the part's own frame (shared per model)

// 2D
let sil = silhouette::silhouette(&mesh);       // hull + per-part hulls, mm, CCW
gdtf.model_svg("Base", stagecraft_gdtf::SvgView::Top);  // raw SVG bytes if present
gdtf.plan_svgs(None);                          // (model, world transform mm, svg) per instance
```

Pan/tilt: rotate an `Axis` part about its local origin. By GDTF convention the
pan axis (`Yoke`) turns about z and the tilt axis (`Head`) about x; which DMX
channel drives which axis is in `DmxChannel::geometry` and the attribute
(`Pan`/`Tilt`).

Lower level: `dmx::resolve_mode(&FixtureType, &DmxMode)`, `mesh::instances`,
`models::{gltf, three_ds, obj, primitives}::…`, `Archive`. `models::gltf::load` reads
GLB and `.gltf` with base64 `data:` buffers; `models::obj::load` reads Wavefront OBJ
(positions and faces). Both are also used for model objects and MVR geometry.

### Footprints

A channel on a geometry of the mode's own tree keeps its offsets. A channel on a
template geometry is repeated for every `GeometryReference` to that template, at
`Break.DMXOffset + Offset − 1` in the reference's break (`Overwrite` → the
reference's last break). The footprint of a break is its highest used slot.

### Models

Each model mesh is loaded from `models/gltf/<File>.glb`, else `models/3ds/<File>.3ds`
(`Lod::Low/High` try `gltf_low`/`gltf_high` first), and scaled about its origin to
`Length × Width × Height` as the spec requires. Without a file the
`PrimitiveType` is generated (Cube, Cylinder, Sphere, Base, Yoke, Head, Scanner,
Conventional, Pigtail and the `1_1` variants). See the GDTF rows in `docs/spec/decisions.md` for the
exceptions (wrong dimensions, wrong up axis).

## GDTF Share (`share`, `keychain` features)

```rust
use stagecraft_gdtf::share::{self, ShareClient};
let mut c = ShareClient::new(cache_dir);       // <cache>/list.json, <cache>/files/<rid>.gdtf
c.login_from_keychain()?;                      // keychain service "StageCraft GDTF Share"
let list = c.list(Duration::from_secs(86400))?;     // Vec<ShareEntry>, cached
let hits = share::search(&list, "robe spikie");     // all words, newest first
let newest = share::latest_revisions(&list);
let bytes = c.download(hits[0].rid)?;               // cached forever per rid
```

`login(user, password)` exists for other credential sources. Credentials are
never logged or cached. Native only (blocking HTTP); call it off the UI thread.

## Tests

- `tests/synthetic.rs`: a moving head built in memory (3DS base, GLB head,
  primitives, pixel references over two breaks, `Overwrite`, virtual channel,
  messy XML, fatal errors).
- `tests/real_files.rs`: every `.gdtf` inside `testdata/private/**/*.mvr` and in
  `testdata/private/gdtf/` must parse, resolve all modes and give a mesh and
  silhouette; prints a table with `--nocapture`. `GDTF_VIEWS=dir` writes a front
  view and plan silhouette per file as SVG. Skips without data.
- `tests/share_cache.rs` (`share`): the client on a real cached list and
  downloads, offline; checks our footprints against GDTF Share's.
- `tests/share_live.rs` (`keychain`, `#[ignore]`): logs in, lists, downloads five
  common fixtures into `testdata/private/gdtf/`.
