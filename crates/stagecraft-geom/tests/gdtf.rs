//! Fixture geometry from GDTF: meshes (cached), plan symbol fallback chain, built-in
//! symbols, and the real files in testdata/private (skipped when missing).

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use stagecraft_core::{Engine, Id};
use stagecraft_geom::gdtf::{self, SymbolSource};
use stagecraft_geom::{Lod, MaterialClass, PlanPrim, PlanStyle};

fn run(e: &mut Engine, name: &str, args: Value) -> Value {
    e.execute(name, args).unwrap_or_else(|err| panic!("{name} failed: {err}"))
}

fn id(v: &Value) -> Id {
    v["id"].as_str().expect("id").parse().expect("uuid")
}

fn testdata() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

fn demo(e: &mut Engine) {
    for n in ["Profile 750", "Spot 300", "Pixel Bar 8"] {
        run(e, "gdtf.import", json!({ "path": testdata().join(format!("gdtf/StageCraft Demo@{n}.gdtf")) }));
    }
}

const SPOT: &str = "StageCraft Demo@Spot 300";
const BAR: &str = "StageCraft Demo@Pixel Bar 8";
const PROFILE: &str = "StageCraft Demo@Profile 750";

#[test]
fn meshes_hang_below_the_origin_and_are_cached() {
    let mut e = Engine::new();
    demo(&mut e);
    let a = id(&run(&mut e, "fixture.create", json!({ "gdtf": SPOT, "position": [0, 0, 6000] })));
    let b = id(&run(&mut e, "fixture.create", json!({ "gdtf": SPOT, "position": [1000, 0, 6000] })));
    let doc = e.doc();
    let (oa, ob) = (&doc.objects[&a], &doc.objects[&b]);
    let ma = gdtf::fixture_mesh(doc, oa, Lod::Full).unwrap();
    let mb = gdtf::fixture_mesh(doc, ob, Lod::Full).unwrap();
    assert!(Arc::ptr_eq(&ma, &mb), "one mesh per type and mode");
    let (lo, hi) = ma.bounds().unwrap();
    assert!(hi[2] <= 1.0 && lo[2] < -450.0, "hangs below the hanging point: {lo:?} {hi:?}");
    assert!((hi[0] - lo[0] - 400.0).abs() < 1.0, "base 400 mm wide: {lo:?} {hi:?}");
    assert!(ma.groups.iter().any(|g| g.material == MaterialClass::Lens && g.count > 0));
    // The world mesh follows the placement: z from 6 m down.
    let w = stagecraft_geom::world_mesh(doc, oa, Lod::Full);
    let (wlo, whi) = w.bounds().unwrap();
    assert!(whi[2] <= 6001.0 && wlo[2] < 5550.0);
    // Box level is one cuboid.
    let bx = stagecraft_geom::mesh(doc, oa, Lod::Box);
    assert_eq!(bx.triangle_count(), 12);
    assert!(bx.is_watertight());
    // A fixture turned over stands on the floor.
    let f = id(&run(&mut e, "fixture.create", json!({ "gdtf": PROFILE, "rotation": [180, 0, 0] })));
    let doc = e.doc();
    let (flo, _) = stagecraft_geom::world_mesh(doc, &doc.objects[&f], Lod::Full).bounds().unwrap();
    assert!(flo[2] >= -1.0, "{flo:?}");
}

#[test]
fn modes_change_the_mesh_only_when_the_tree_differs() {
    let mut e = Engine::new();
    demo(&mut e);
    let f = id(&run(&mut e, "fixture.create", json!({ "gdtf": BAR, "mode": "8px RGB" })));
    let m1 = gdtf::fixture_mesh(e.doc(), &e.doc().objects[&f], Lod::Full).unwrap();
    run(&mut e, "fixture.update", json!({ "id": f, "mode": "Split" }));
    let m2 = gdtf::fixture_mesh(e.doc(), &e.doc().objects[&f], Lod::Full).unwrap();
    // Both modes use the same geometry tree, so the meshes are equal (cached apart).
    assert_eq!(*m1, *m2);
    // 8 pixels + bar.
    assert!(m1.triangle_count() >= 9 * 12);
}

fn plan(e: &Engine, f: Id) -> Vec<PlanPrim> {
    stagecraft_geom::plan(e.doc(), &e.doc().objects[&f], &PlanStyle::default())
}

#[test]
fn symbol_fallback_chain() {
    let mut e = Engine::new();
    demo(&mut e);
    let spot = id(&run(&mut e, "fixture.create", json!({ "gdtf": SPOT, "fid": 1 })));
    let bar = id(&run(&mut e, "fixture.create", json!({ "gdtf": BAR, "fid": 2, "position": [3000, 0, 0] })));
    let generic = id(&run(&mut e, "fixture.create", json!({ "fid": 3, "position": [6000, 0, 0] })));
    let doc = e.doc();
    // No assignment, no SVG: the silhouette.
    let s = gdtf::symbol(doc, &doc.objects[&spot]).unwrap();
    assert_eq!(s.source, SymbolSource::Silhouette);
    let (lo, hi) = s.bounds;
    assert!((hi[0] - lo[0] - 400.0).abs() < 2.0, "{lo:?} {hi:?}");
    // The GDTF's own top view: 1000 × 80 mm, centred by SVGOffset.
    let b = gdtf::symbol(doc, &doc.objects[&bar]).unwrap();
    assert_eq!(b.source, SymbolSource::GdtfSvg);
    let (lo, hi) = b.bounds;
    assert!((lo[0] + 500.0).abs() < 1e-6 && (hi[0] - 500.0).abs() < 1e-6, "{lo:?} {hi:?}");
    assert!((lo[1] + 40.0).abs() < 1e-6 && (hi[1] - 40.0).abs() < 1e-6, "{lo:?} {hi:?}");
    // Without a type: the generic box.
    assert!(gdtf::symbol(doc, &doc.objects[&generic]).is_none());
    let p = plan(&e, generic);
    assert!(matches!(&p[0], PlanPrim::Polygon { points, .. } if points.len() == 4));
    // An assigned symbol wins over the GDTF's SVG.
    run(&mut e, "symbol.assign", json!({ "gdtf_type": BAR, "symbol": "led_bar" }));
    let doc = e.doc();
    let b = gdtf::symbol(doc, &doc.objects[&bar]).unwrap();
    assert_eq!(b.source, SymbolSource::Assigned);
    let (lo, hi) = b.bounds;
    assert!((hi[0] - lo[0] - 1000.0).abs() < 1.0, "fits the bar's width: {lo:?} {hi:?}");
    // The plan symbol moves with the fixture and carries the FID.
    let p = plan(&e, bar);
    let xs: Vec<f64> = p
        .iter()
        .filter_map(|q| match q {
            PlanPrim::Polygon { points, .. } => Some(points.iter().map(|p| p[0]).fold(f64::MIN, f64::max)),
            _ => None,
        })
        .collect();
    assert!(xs.iter().all(|x| *x > 2400.0 && *x <= 3501.0), "{xs:?}");
    assert!(p.iter().any(|q| matches!(q, PlanPrim::Text { text, .. } if text == "2")));
}

#[test]
fn every_builtin_symbol_draws() {
    for (name, _) in stagecraft_core::fixture_type::SYMBOLS {
        let prims = gdtf::builtin_symbol(name, [300.0, 300.0, 500.0]).unwrap_or_else(|| panic!("{name}"));
        assert!(prims.len() >= 3, "{name}");
        assert!(prims.iter().any(|p| matches!(p, gdtf::LocalPrim::Polygon { .. })), "{name} has a filled body");
        // The front (thick line) is at local +y.
        let thick_y: Vec<f64> = prims
            .iter()
            .filter_map(|p| match p {
                gdtf::LocalPrim::Polyline { points, line: stagecraft_geom::LineClass::Thick, .. } => {
                    Some(points.iter().map(|q| q[1]).sum::<f64>() / points.len() as f64)
                }
                _ => None,
            })
            .collect();
        assert!(!thick_y.is_empty() && thick_y.iter().all(|y| *y > 0.0), "{name}: {thick_y:?}");
    }
}

/// Every real GDTF in testdata/private/gdtf into one document: mesh, plan, labels.
#[test]
fn real_gdtfs_load_and_draw() {
    let dir = testdata().join("private/gdtf");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        eprintln!("skipped: {} missing", dir.display());
        return;
    };
    let mut e = Engine::new();
    let mut n = 0;
    for entry in rd.flatten() {
        let p = entry.path();
        if p.extension().and_then(|x| x.to_str()) != Some("gdtf") {
            continue;
        }
        let r = run(&mut e, "gdtf.import", json!({ "path": p }));
        let key = r["key"].as_str().unwrap().to_owned();
        let t = e.doc().fixture_types[&key].clone();
        assert!(!t.modes.is_empty(), "{key}");
        for m in t.modes.clone() {
            let f = id(&run(
                &mut e,
                "fixture.create",
                json!({ "gdtf": key, "mode": m.name, "position": [n * 800, 0, 6000] }),
            ));
            let doc = e.doc();
            let o = &doc.objects[&f];
            let mesh = stagecraft_geom::mesh(doc, o, Lod::Full);
            assert!(mesh.triangle_count() > 0, "{key} {}", m.name);
            assert!(!stagecraft_geom::plan(doc, o, &PlanStyle::default()).is_empty(), "{key}");
            n += 1;
        }
        eprintln!("{key}: weight {:?}, {} modes, symbol {:?}", t.weight_kg, t.modes.len(), t.suggested_symbol);
    }
    eprintln!("{n} fixtures");
}

/// The silhouette follows the fixture's tilt: a profile hanging straight down is a
/// short shape from above; tilted 50° its barrel shows its length.
#[test]
fn silhouettes_follow_tilt() {
    let mut e = Engine::new();
    demo(&mut e);
    let down = id(&run(&mut e, "fixture.create", json!({ "gdtf": PROFILE, "position": [0, 0, 6000] })));
    let tilted = id(&run(
        &mut e,
        "fixture.create",
        json!({ "gdtf": PROFILE, "position": [3000, 0, 6000], "rotation": [50, 0, 0] }),
    ));
    let extent_y = |f: Id| {
        let p = plan(&e, f);
        let ys: Vec<f64> = p
            .iter()
            .flat_map(|q| match q {
                PlanPrim::Polygon { points, .. } => points.iter().map(|p| p[1]).collect(),
                _ => Vec::new(),
            })
            .collect();
        ys.iter().copied().fold(f64::MIN, f64::max) - ys.iter().copied().fold(f64::MAX, f64::min)
    };
    let (a, b) = (extent_y(down), extent_y(tilted));
    assert!(a < 260.0, "{a}");
    // 650 mm barrel at 50°: about 0.77 × 650 = 500 mm in plan.
    assert!(b > 420.0, "{b}");
}
