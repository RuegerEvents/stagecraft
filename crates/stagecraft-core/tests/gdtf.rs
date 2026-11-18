//! GDTF library, embedded assets, footprints, conflicts and auto-patch.

use std::path::PathBuf;

use serde_json::{Value, json};
use stagecraft_core::{Engine, Id, ObjectKind, patch};

fn run(e: &mut Engine, name: &str, args: Value) -> Value {
    e.execute(name, args).unwrap_or_else(|err| panic!("{name} failed: {err}"))
}

fn id(v: &Value) -> Id {
    v["id"].as_str().expect("id").parse().expect("uuid")
}

fn demo_gdtf(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/gdtf").join(format!("StageCraft Demo@{name}.gdtf"))
}

const PROFILE: &str = "StageCraft Demo@Profile 750";
const SPOT: &str = "StageCraft Demo@Spot 300";
const BAR: &str = "StageCraft Demo@Pixel Bar 8";

fn library(e: &mut Engine) {
    for n in ["Profile 750", "Spot 300", "Pixel Bar 8"] {
        run(e, "gdtf.import", json!({ "path": demo_gdtf(n) }));
    }
}

fn fixture(e: &Engine, id: Id) -> ObjectKind {
    e.doc().objects[&id].kind.clone()
}

#[test]
fn import_lists_modes_footprints_and_weight() {
    let mut e = Engine::new();
    let r = run(&mut e, "gdtf.import", json!({ "path": demo_gdtf("Spot 300") }));
    assert_eq!(r["key"], SPOT);
    // Importing the same file again changes nothing (no undo step).
    let undo_before = e.can_undo();
    let again = run(&mut e, "gdtf.import", json!({ "path": demo_gdtf("Spot 300") }));
    assert_eq!(again["unchanged"], true);
    assert_eq!(e.can_undo(), undo_before);
    run(&mut e, "gdtf.import", json!({ "path": demo_gdtf("Pixel Bar 8") }));
    let lib = run(&mut e, "gdtf.library", json!({}));
    let types = lib["types"].as_array().unwrap();
    assert_eq!(types.len(), 2);
    let spot = types.iter().find(|t| t["key"] == SPOT).unwrap();
    assert_eq!(spot["weight_kg"], 21.0);
    assert_eq!(spot["modes"][0]["name"], "Standard");
    assert_eq!(spot["modes"][0]["footprint"], "16");
    assert_eq!(spot["modes"][1]["footprint"], "6");
    assert_eq!(spot["symbol"], "silhouette");
    assert_eq!(spot["suggested_symbol"], "mh_spot");
    let size = spot["size_mm"].as_array().unwrap();
    assert!(size[2].as_f64().unwrap() > 400.0, "{size:?}");
    let bar = types.iter().find(|t| t["key"] == BAR).unwrap();
    assert_eq!(bar["modes"][0]["footprint"], "25");
    assert_eq!(bar["modes"][1]["footprint"], "2+24");
    assert_eq!(bar["symbol"], "gdtf_svg");
    assert_eq!(e.doc().assets.len(), 2);
    let bad = e.execute("gdtf.import", json!({ "path": "/nonexistent.gdtf" }));
    assert!(bad.is_err());
}

#[test]
fn fixtures_take_type_mode_and_weight() {
    let mut e = Engine::new();
    library(&mut e);
    let f = id(&run(&mut e, "fixture.create", json!({ "gdtf": SPOT, "fid": 1, "address": "1.001" })));
    let ObjectKind::Fixture { mode, weight_kg, .. } = fixture(&e, f) else { panic!() };
    assert_eq!(mode.as_deref(), Some("Standard"));
    assert_eq!(weight_kg, Some(21.0));
    assert!(e.execute("fixture.create", json!({ "gdtf": SPOT, "mode": "Nope" })).is_err());
    assert!(e.execute("fixture.create", json!({ "gdtf": "Unknown@Type" })).is_err());

    // Weights count in load sums.
    let truss = id(&run(&mut e, "truss.create", json!({ "start": [0, 0, 6000], "end": [3000, 0, 6000] })));
    run(&mut e, "fixture.create", json!({ "gdtf": PROFILE, "parent": truss, "position": [500, 0, -150] }));
    run(&mut e, "fixture.create", json!({ "gdtf": PROFILE, "parent": truss, "position": [900, 0, -150] }));
    let l = stagecraft_core::load::load_of(e.doc(), truss);
    assert!((l.attached_kg - 15.0).abs() < 1e-9, "{l:?}");
    assert_eq!(l.unknown, 0);

    // Mode by update; a wrong mode is refused.
    let r = run(&mut e, "fixture.update", json!({ "id": f, "mode": "Basic" }));
    assert_eq!(r["footprint"], json!([6]));
    assert!(e.execute("fixture.update", json!({ "id": f, "mode": "Huge" })).is_err());
}

#[test]
fn set_type_keeps_patch_and_hand_weight() {
    let mut e = Engine::new();
    library(&mut e);
    let a = id(&run(&mut e, "fixture.create", json!({ "gdtf": SPOT, "fid": 1, "address": 101 })));
    let b = id(&run(&mut e, "fixture.create", json!({ "gdtf": SPOT, "fid": 2, "address": 201, "weight_kg": 30 })));
    run(&mut e, "fixture.set_type", json!({ "ids": [a, b], "gdtf_type": BAR, "mode": "Split" }));
    let ObjectKind::Fixture { gdtf, mode, address, extra_addresses, weight_kg, fid, .. } = fixture(&e, a) else {
        panic!()
    };
    assert_eq!(gdtf.as_deref(), Some(BAR));
    assert_eq!(mode.as_deref(), Some("Split"));
    assert_eq!((fid, address), (Some(1), Some(101)));
    assert!(extra_addresses.is_empty());
    assert_eq!(weight_kg, Some(4.2));
    let ObjectKind::Fixture { weight_kg, .. } = fixture(&e, b) else { panic!() };
    assert_eq!(weight_kg, Some(30.0), "a weight typed by hand stays");
    // Back to the spot: the bar's "Split" mode does not exist there, so the first mode.
    run(&mut e, "fixture.set_type", json!({ "ids": [a], "gdtf_type": SPOT }));
    let ObjectKind::Fixture { mode, .. } = fixture(&e, a) else { panic!() };
    assert_eq!(mode.as_deref(), Some("Standard"));
}

#[test]
fn footprints_and_conflicts() {
    let mut e = Engine::new();
    library(&mut e);
    let a = id(&run(&mut e, "fixture.create", json!({ "gdtf": SPOT, "fid": 1, "address": "1.001" })));
    let b = id(&run(&mut e, "fixture.create", json!({ "gdtf": SPOT, "fid": 2, "address": "1.016" })));
    let c = id(&run(&mut e, "fixture.create", json!({ "gdtf": SPOT, "fid": 3, "address": "1.032" })));
    let doc = e.doc();
    assert_eq!(patch::footprint(doc, &doc.objects[&a]), vec![16]);
    let conf = patch::conflicts(doc);
    assert_eq!(conf[&a], vec![patch::Problem::Overlap { with: b }]);
    assert_eq!(conf[&b], vec![patch::Problem::Overlap { with: a }]);
    assert!(!conf.contains_key(&c));
    // Past the end of a universe.
    run(&mut e, "fixture.update", json!({ "id": c, "address": "1.500" }));
    let conf = patch::conflicts(e.doc());
    assert_eq!(conf[&c], vec![patch::Problem::CrossesUniverse { part: 0 }]);
    let check = run(&mut e, "patch.check", json!({}));
    assert_eq!(check["ok"], false);
    assert_eq!(check["conflicts"].as_array().unwrap().len(), 3);

    // Two breaks: one address each; the second break can clash on its own.
    let bar = id(&run(
        &mut e,
        "fixture.create",
        json!({ "gdtf": BAR, "mode": "Split", "fid": 10, "address": "2.001", "extra_addresses": ["3.001"] }),
    ));
    let doc = e.doc();
    assert_eq!(patch::footprint(doc, &doc.objects[&bar]), vec![2, 24]);
    assert_eq!(patch::address_text(&doc.objects[&bar]), "2.001 / 3.001");
    let other = id(&run(&mut e, "fixture.create", json!({ "gdtf": SPOT, "fid": 11, "address": "3.020" })));
    let conf = patch::conflicts(e.doc());
    assert!(conf[&bar].contains(&patch::Problem::Overlap { with: other }));
    let check = run(&mut e, "patch.check", json!({}));
    assert!(check["partly_patched"].as_array().unwrap().is_empty());
    run(&mut e, "fixture.update", json!({ "id": bar, "extra_addresses": [] }));
    let check = run(&mut e, "patch.check", json!({}));
    assert_eq!(check["partly_patched"], json!([bar]));
}

#[test]
fn auto_patch_in_order_skipping_used_slots() {
    let mut e = Engine::new();
    library(&mut e);
    // Something already sits at 1.020–1.035.
    run(&mut e, "fixture.create", json!({ "gdtf": SPOT, "fid": 99, "address": "1.020" }));
    let ids: Vec<Id> = (0..3)
        .map(|i| id(&run(&mut e, "fixture.create", json!({ "gdtf": SPOT, "fid": i + 1, "position": [i * 500, 0, 0] }))))
        .collect();
    let bar = id(&run(&mut e, "fixture.create", json!({ "gdtf": BAR, "mode": "Split", "fid": 20 })));
    // Patch order: the order given (the selection order in the GUI).
    let order = vec![ids[2], ids[0], ids[1], bar];
    let r = run(&mut e, "patch.auto", json!({ "ids": order, "start": "1.001", "gap": 0 }));
    let got: Vec<&str> = r["patched"].as_array().unwrap().iter().map(|p| p["address"].as_str().unwrap()).collect();
    // 1–16 fits before 20; 17–32 would hit 20, so after 35: 36–51, 52–67; the bar's
    // breaks follow at 68 and 70.
    assert_eq!(got, ["1.001", "1.036", "1.052", "1.068 / 1.070"]);
    assert!(patch::conflicts(e.doc()).is_empty());
    // A gap, and no block crosses into the next universe.
    let r = run(&mut e, "patch.auto", json!({ "ids": [ids[0], ids[1]], "start": "1.490", "gap": 4 }));
    let got: Vec<&str> = r["patched"].as_array().unwrap().iter().map(|p| p["address"].as_str().unwrap()).collect();
    assert_eq!(got, ["1.490", "2.001"]);
    // With no ids it patches the selection.
    run(&mut e, "selection.set", json!({ "ids": [ids[1], ids[0]] }));
    let r = run(&mut e, "patch.auto", json!({ "start": "5.001" }));
    assert_eq!(r["patched"][0]["id"], json!(ids[1]));
    assert_eq!(r["patched"][1]["address"], "5.017");
}

#[test]
fn embedded_types_survive_save_load_and_undo_shares_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plan.stagecraft");
    let mut e = Engine::new();
    library(&mut e);
    let f = id(&run(
        &mut e,
        "fixture.create",
        json!({ "gdtf": BAR, "mode": "Split", "address": 1, "extra_addresses": [513] }),
    ));
    run(&mut e, "symbol.assign", json!({ "gdtf_type": SPOT, "symbol": "mh_wash" }));
    // Undo copies of the document share the asset bytes.
    let before = e.doc().clone();
    run(&mut e, "object.rename", json!({ "id": f, "name": "Bar A" }));
    for (k, a) in &e.doc().assets {
        assert!(a.data.shares(&before.assets[k].data));
    }
    run(&mut e, "doc.save", json!({ "path": path }));

    // The ZIP holds document.json and one entry per asset.
    let zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
    assert_eq!(names.iter().filter(|n| n.starts_with("assets/") && n.ends_with(".gdtf")).count(), 3, "{names:?}");

    let mut back = Engine::open(&path).unwrap();
    assert_eq!(back.doc().fixture_types, e.doc().fixture_types);
    assert_eq!(back.doc().assets, e.doc().assets);
    assert_eq!(back.doc().objects[&f], e.doc().objects[&f]);
    let lib = run(&mut back, "gdtf.library", json!({}));
    let spot = lib["types"].as_array().unwrap().iter().find(|t| t["key"] == SPOT).unwrap().clone();
    assert_eq!(spot["symbol"], "assigned:mh_wash");
    // The embedded bytes still parse.
    for a in back.doc().assets.values() {
        stagecraft_gdtf::Gdtf::from_bytes(a.data.as_slice()).unwrap();
    }
}

#[test]
fn format_1_files_still_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.stagecraft");
    let mut e = Engine::new();
    let f = id(&run(&mut e, "fixture.create", json!({ "fid": 5, "address": "1.001" })));
    let mut v = serde_json::to_value(e.doc()).unwrap();
    let m = v.as_object_mut().unwrap();
    m.remove("assets");
    m.remove("fixture_types");
    m.insert("format".into(), json!(1));
    let mut w = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    w.start_file("document.json", zip::write::SimpleFileOptions::default()).unwrap();
    std::io::Write::write_all(&mut w, serde_json::to_string(&v).unwrap().as_bytes()).unwrap();
    w.finish().unwrap();
    let back = Engine::open(&path).unwrap();
    assert_eq!(back.doc().format, stagecraft_core::model::FORMAT_VERSION);
    assert!(back.doc().assets.is_empty());
    assert_eq!(back.doc().objects[&f].kind, e.doc().objects[&f].kind);
}

#[test]
fn symbols_and_library_edits() {
    let mut e = Engine::new();
    library(&mut e);
    assert!(e.execute("symbol.assign", json!({ "gdtf_type": SPOT, "symbol": "teapot" })).is_err());
    run(&mut e, "symbol.assign", json!({ "gdtf_type": PROFILE, "symbol": "suggested" }));
    assert_eq!(e.doc().fixture_types[PROFILE].symbol.as_deref(), Some("profile"));
    run(&mut e, "symbol.assign", json!({ "gdtf_type": PROFILE, "symbol": "" }));
    assert_eq!(e.doc().fixture_types[PROFILE].symbol, None);
    let list = run(&mut e, "symbol.list", json!({}));
    assert_eq!(list["symbols"].as_array().unwrap().len(), 11);

    let f = id(&run(&mut e, "fixture.create", json!({ "gdtf": PROFILE })));
    assert!(e.execute("gdtf.remove", json!({ "type": PROFILE })).is_err());
    run(&mut e, "gdtf.remove", json!({ "type": PROFILE, "force": true }));
    let ObjectKind::Fixture { gdtf, weight_kg, .. } = fixture(&e, f) else { panic!() };
    assert_eq!((gdtf, weight_kg), (None, None));
    assert_eq!(e.doc().assets.len(), 2);
    run(&mut e, "history.undo", json!({}));
    assert_eq!(e.doc().assets.len(), 3);
}
