//! MVR import/export through the commands, with meshes from this crate (the
//! exporter's hook). Real files in `testdata/private/` are client data: tests skip
//! without them and never print their content beyond counts.
//!
//!     cargo test -p stagecraft-geom --release --test mvr -- --nocapture

use std::path::{Path, PathBuf};
use std::time::Instant;

use glam::DVec3;
use serde_json::{Value, json};
use stagecraft_core::{Engine, Id, ObjectKind};

fn run(e: &mut Engine, name: &str, args: Value) -> Value {
    e.execute(name, args).unwrap_or_else(|err| panic!("{name} failed: {err}"))
}

fn id(v: &Value) -> Id {
    v["id"].as_str().unwrap().parse().unwrap()
}

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

fn private_mvrs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for d in ["vectorworks", "other"] {
        if let Ok(rd) = std::fs::read_dir(testdata().join("private").join(d)) {
            v.extend(rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "mvr")));
        }
    }
    v.sort();
    v
}

fn world(e: &Engine, id: Id) -> glam::DAffine3 {
    stagecraft_core::geom::world(e.doc(), id).unwrap()
}

fn count(e: &Engine, f: impl Fn(&ObjectKind) -> bool) -> usize {
    e.doc().objects.values().filter(|o| f(&o.kind)).count()
}

#[derive(Debug, PartialEq)]
struct Patch {
    fid: Option<u32>,
    cid: Option<String>,
    gdtf: Option<String>,
    mode: Option<String>,
    address: Option<u32>,
    extra: Vec<u32>,
}

fn patch(e: &Engine, id: Id) -> Patch {
    let ObjectKind::Fixture { fid, cid, gdtf, mode, address, extra_addresses, .. } = &e.doc().objects[&id].kind else {
        panic!("not a fixture")
    };
    Patch {
        fid: *fid,
        cid: cid.clone(),
        gdtf: gdtf.clone(),
        mode: mode.clone(),
        address: *address,
        extra: extra_addresses.clone(),
    }
}

/// Fixtures and trusses of `a` are in `b` under the same id with the same world
/// transform (mm / 1e-6) and, for fixtures, the same patch.
fn same_rig(a: &Engine, b: &Engine) {
    let mut n = 0;
    for o in a.doc().objects.values() {
        if !matches!(o.kind, ObjectKind::Fixture { .. } | ObjectKind::Truss { .. }) {
            continue;
        }
        let other = b.doc().objects.get(&o.id).unwrap_or_else(|| panic!("{} missing after the round trip", o.name));
        let (wa, wb) = (world(a, o.id), world(b, o.id));
        assert!(
            (wa.translation - wb.translation).length() < 1e-3,
            "{}: {} vs {}",
            o.name,
            wa.translation,
            wb.translation
        );
        assert!((wa.matrix3 - wb.matrix3).to_cols_array().iter().all(|v| v.abs() < 1e-6), "{} turned", o.name);
        assert_eq!(a.doc().layers[&o.layer].name, b.doc().layers[&other.layer].name);
        if matches!(o.kind, ObjectKind::Fixture { .. }) {
            // The mode is written resolved; compare resolved modes.
            let (mut pa, mut pb) = (patch(a, o.id), patch(b, o.id));
            let resolve = |e: &Engine, p: &mut Patch| {
                if let Some(t) = p.gdtf.as_ref().and_then(|k| e.doc().fixture_types.get(k)) {
                    p.mode = t.mode_or_first(p.mode.as_deref()).map(|m| m.name.clone());
                }
            };
            resolve(a, &mut pa);
            resolve(b, &mut pb);
            assert_eq!(pa, pb, "{}", o.name);
        } else {
            assert_eq!(o.kind, other.kind, "{}", o.name);
        }
        n += 1;
    }
    assert!(n > 0);
}

#[test]
fn real_files_import_with_matching_counts_and_positions() {
    stagecraft_geom::install();
    let files = private_mvrs();
    if files.is_empty() {
        eprintln!("skipped: no MVRs in testdata/private");
        return;
    }
    let out = std::env::var("STAGECRAFT_MVR_OUT").ok();
    for path in files {
        let label = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).unwrap();
        let file = stagecraft_mvr::read_mvr(&bytes).unwrap().mvr;
        let stats = file.scene.stats();
        let mut e = Engine::new();
        let t = Instant::now();
        let r = run(&mut e, "mvr.import", json!({ "path": path }));
        let took = t.elapsed();
        let fixtures = count(&e, |k| matches!(k, ObjectKind::Fixture { .. }));
        let trusses = count(&e, |k| matches!(k, ObjectKind::Truss { .. }));
        let models = count(&e, |k| matches!(k, ObjectKind::Model { .. }));
        println!(
            "{label}: {:.0} ms; fixtures {fixtures}/{}, trusses {trusses}/{}, models {models}, groups {}, types {}, warnings {}",
            took.as_secs_f64() * 1e3,
            stats.fixtures,
            stats.trusses,
            r["groups"],
            r["gdtf_types"].as_array().unwrap().len(),
            r["warnings"].as_array().unwrap().len(),
        );
        assert_eq!(fixtures, stats.fixtures, "{label}: fixtures");
        assert_eq!(trusses, stats.trusses, "{label}: trusses");
        let objects = stats.fixtures
            + stats.trusses
            + stats.scene_objects
            + stats.groups
            + stats.supports
            + stats.video_screens
            + stats.projectors;
        assert_eq!(e.doc().objects.len(), objects, "{label}: every object but focus points");
        // World positions of fixtures equal the MVR world matrices.
        let mut checked = 0;
        for (_, o) in file.scene.objects() {
            if o.kind != stagecraft_mvr::ObjectKind::Fixture || checked >= 12 {
                continue;
            }
            let Some(d) = e.doc().objects.get(&o.uuid) else { continue };
            let w = world(&e, d.id);
            let m = DVec3::from_array(o.world.o);
            assert!((w.translation - m).length() < 1e-3, "{label}: {} at {} not {m}", d.name, w.translation);
            let u = DVec3::from_array(o.world.u).normalize();
            let wz = DVec3::from_array(o.world.w).normalize();
            assert!(
                (w.matrix3.x_axis - u).length() < 1e-6 && (w.matrix3.z_axis - wz).length() < 1e-6,
                "{label}: {}",
                d.name
            );
            // Patch: first break.
            if let Some(a) = o.addresses.iter().filter(|a| a.absolute > 0).min_by_key(|a| a.dmx_break) {
                assert_eq!(patch(&e, d.id).address, Some(a.absolute), "{label}: {}", d.name);
            }
            checked += 1;
        }
        assert!(checked > 0, "{label}: no fixture kept its uuid");
        // Round trip: StageCraft → MVR → StageCraft keeps patch and positions.
        let tmp = tempfile::tempdir().unwrap();
        let mvr_path = tmp.path().join("rt.mvr");
        let t = Instant::now();
        run(&mut e, "mvr.export", json!({ "path": mvr_path }));
        let t_export = t.elapsed();
        let mut back = Engine::new();
        run(&mut back, "mvr.import", json!({ "path": mvr_path }));
        same_rig(&e, &back);
        println!("  export {:.0} ms, round trip ok", t_export.as_secs_f64() * 1e3);
        if let Some(dir) = &out {
            let stem = path.file_stem().unwrap().to_string_lossy();
            std::fs::copy(&mvr_path, Path::new(dir).join(format!("{stem}.stagecraft.mvr"))).unwrap();
            run(&mut e, "doc.save", json!({ "path": Path::new(dir).join(format!("{stem}.stagecraft")) }));
        }
    }
}

/// A small synthetic rig: two layers (one raised), a truss with fixtures of the demo
/// GDTFs (one with two breaks), a platform, a group, a model from an OBJ.
fn synthetic() -> Engine {
    let mut e = Engine::new();
    for n in ["Profile 750", "Spot 300", "Pixel Bar 8"] {
        run(&mut e, "gdtf.import", json!({ "path": testdata().join(format!("gdtf/StageCraft Demo@{n}.gdtf")) }));
    }
    run(&mut e, "layer.create", json!({ "name": "Gallery", "elevation": 4500 }));
    run(&mut e, "tag.create", json!({ "name": "light" }));
    let truss = id(&run(
        &mut e,
        "truss.create",
        json!({ "start": [-3000, 2000, 6000], "end": [3000, 2000, 6000], "profile": "F34" }),
    ));
    let types = ["StageCraft Demo@Profile 750", "StageCraft Demo@Spot 300", "StageCraft Demo@Pixel Bar 8"];
    for i in 0..6 {
        let f = id(&run(
            &mut e,
            "fixture.create",
            json!({ "parent": truss, "position": [500.0 + i as f64 * 1000.0, 0, -300], "rotation": [0, 0, 180],
                    "gdtf": types[i % 3], "fid": 101 + i, "cid": format!("{}", 1 + i), "tags": ["light"] }),
        ));
        if i % 3 == 2 {
            run(
                &mut e,
                "fixture.update",
                json!({ "id": f, "mode": "Split", "address": format!("{}.001", i), "extra_addresses": [format!("{}.001", i + 1)] }),
            );
        } else {
            run(&mut e, "fixture.update", json!({ "id": f, "address": format!("1.{:03}", 1 + i * 40) }));
        }
    }
    run(
        &mut e,
        "fixture.create",
        json!({ "layer": "Gallery", "position": [0, -8000, 0], "rotation": [-40, 0, 0], "gdtf": types[0], "fid": 201 }),
    );
    run(&mut e, "platform.create", json!({ "position": [0, 5000, 0], "width": 2000, "depth": 1000, "height": 400 }));
    e
}

#[test]
fn synthetic_round_trip_is_stable_and_deterministic() {
    stagecraft_geom::install();
    let mut e = synthetic();
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.mvr");
    let b = tmp.path().join("b.mvr");
    let r = run(&mut e, "mvr.export", json!({ "path": a }));
    assert!(r["warnings"].as_array().unwrap().is_empty(), "{r}");
    run(&mut e, "mvr.export", json!({ "path": b }));
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap(), "export is deterministic");
    // The file reads back cleanly in the MVR crate: GDTFs embedded, geometry present.
    let f = stagecraft_mvr::read_mvr(&std::fs::read(&a).unwrap()).unwrap();
    assert!(f.warnings.is_empty(), "{:?}", f.warnings);
    let s = f.mvr.scene.stats();
    assert_eq!((s.fixtures, s.trusses, s.layers, s.classes), (7, 1, 2, 1));
    assert_eq!(f.mvr.resources.names_of(stagecraft_mvr::ResourceKind::Gdtf).count(), 3);
    assert!(s.geometry3d >= 2, "truss and platform geometry");
    let bar = f.mvr.scene.objects_of(stagecraft_mvr::ObjectKind::Fixture).find(|o| o.addresses.len() == 2).unwrap();
    assert_eq!(bar.addresses[1].dmx_break, 1);
    let mut back = Engine::new();
    let r = run(&mut back, "mvr.import", json!({ "path": a }));
    assert_eq!(r["fixtures"], 7);
    same_rig(&e, &back);
    // Gallery keeps its elevation; the gallery fixture is a root on it.
    let gallery = back.doc().layers.values().find(|l| l.name == "Gallery").unwrap();
    assert_eq!(gallery.elevation, 4500.0);
    // Tags came back from the classes.
    let light = back.doc().tags.values().find(|t| t.name == "light").unwrap().id;
    assert_eq!(back.doc().objects.values().filter(|o| o.tags.contains(&light)).count(), 6);
    // Fixtures hang in the truss again.
    let truss = back.doc().objects.values().find(|o| matches!(o.kind, ObjectKind::Truss { .. })).unwrap().id;
    assert_eq!(back.doc().children(truss).len(), 6);
    // The platform comes back as a model with the same extent.
    let m = back.doc().objects.values().find(|o| matches!(o.kind, ObjectKind::Model { .. })).unwrap();
    let ObjectKind::Model { bounds, .. } = &m.kind else { unreachable!() };
    assert!((bounds[1][0] - bounds[0][0] - 2000.0).abs() < 1.0 && (bounds[1][2] - bounds[0][2] - 400.0).abs() < 1.0);
    assert!(!stagecraft_geom::mesh(back.doc(), m, stagecraft_geom::Lod::Full).is_empty());
}

#[test]
fn model_import_reads_obj_and_glb() {
    let tmp = tempfile::tempdir().unwrap();
    // A 2 × 1 × 0.5 m box in metres, y-up as OBJ files usually are.
    let obj = "v 0 0 0\nv 2 0 0\nv 2 0.5 0\nv 0 0.5 0\nv 0 0 1\nv 2 0 1\nv 2 0.5 1\nv 0 0.5 1\n\
               f 1 4 3 2\nf 5 6 7 8\nf 1 2 6 5\nf 2 3 7 6\nf 3 4 8 7\nf 4 1 5 8\n";
    let p = tmp.path().join("crate.obj");
    std::fs::write(&p, obj).unwrap();
    let mut e = Engine::new();
    let r = run(&mut e, "model.import", json!({ "path": p, "position": [1000, 2000, 0] }));
    let size: Vec<f64> = r["size"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    // y-up in the file: 0.5 m up becomes z, the file's z (1 m) becomes −y.
    assert_eq!(size, vec![2000.0, 1000.0, 500.0], "{r}");
    let m = id(&r);
    let o = e.doc().objects[&m].clone();
    assert_eq!(o.name, "crate");
    let mesh = stagecraft_geom::mesh(e.doc(), &o, stagecraft_geom::Lod::Full);
    assert_eq!(mesh.triangle_count(), 12);
    let plan = stagecraft_geom::plan(e.doc(), &o, &Default::default());
    let b = stagecraft_geom::pick::bounds(&plan);
    assert!((b.min[0] - 1000.0).abs() < 1e-3 && (b.max[0] - 3000.0).abs() < 1e-3, "{b:?}");
    // A GLB written by the exporter imports at the same size.
    let glb = stagecraft_mvr::glb::write_glb(&mesh.positions, &mesh.normals, &mesh.indices);
    let g = tmp.path().join("crate.glb");
    std::fs::write(&g, glb).unwrap();
    let r = run(&mut e, "model.import", json!({ "path": g }));
    let size: Vec<f64> =
        r["size"].as_array().unwrap().iter().map(|v| (v.as_f64().unwrap() * 10.0).round() / 10.0).collect();
    assert_eq!(size, vec![2000.0, 1000.0, 500.0]);
    assert!(e.execute("model.import", json!({ "path": tmp.path().join("x.stl") })).is_err());
    // Saved and loaded with its asset.
    let file = tmp.path().join("m.stagecraft");
    run(&mut e, "doc.save", json!({ "path": file }));
    let e2 = Engine::open(&file).unwrap();
    let o2 = &e2.doc().objects[&m];
    assert_eq!(stagecraft_geom::mesh(e2.doc(), o2, stagecraft_geom::Lod::Full).triangle_count(), 12);
}
