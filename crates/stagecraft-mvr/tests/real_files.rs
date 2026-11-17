//! Imports the real (client, git-ignored) MVR files in `testdata/private/`.
//! Skips when they are missing. Run with `--nocapture` to see the summaries.
//!
//! Set `STAGECRAFT_MVR_OUT=dir` to also write each round-trip export there
//! (used for the Perastage check; point it at `testdata/private/out/`).

use stagecraft_mvr::*;
use std::path::{Path, PathBuf};

fn private_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/private");
    let mut out = Vec::new();
    for dir in ["vectorworks", "other"] {
        let Ok(rd) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("mvr")) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn summary(name: &str, imp: &Import) -> String {
    let s = imp.mvr.scene.stats();
    let r = &imp.mvr.resources;
    format!(
        "{name}: MVR {}.{} from {:?}\n  layers {} groups {} fixtures {} trusses {} supports {} \
         scene objects {} focus points {} video screens {} projectors {}\n  symdefs {} symbols {} \
         geometry3d {} classes {} positions {}\n  files: gdtf {} 3ds {} gltf {} images {} other {}; \
         placed models {}\n  warnings {}",
        imp.mvr.header.version.major,
        imp.mvr.header.version.minor,
        imp.mvr.header.provider,
        s.layers,
        s.groups,
        s.fixtures,
        s.trusses,
        s.supports,
        s.scene_objects,
        s.focus_points,
        s.video_screens,
        s.projectors,
        s.symdefs,
        s.symbols,
        s.geometry3d,
        s.classes,
        s.positions,
        r.count_of(ResourceKind::Gdtf),
        r.count_of(ResourceKind::Model3ds),
        r.count_of(ResourceKind::ModelGltf),
        r.count_of(ResourceKind::Image),
        r.count_of(ResourceKind::Other) + r.count_of(ResourceKind::GdtfTemplate),
        imp.mvr.scene.geometry_instances().len(),
        imp.warnings.len(),
    )
}

#[test]
fn real_files_import_and_round_trip() {
    let files = private_files();
    if files.is_empty() {
        eprintln!("no private MVR files; skipping");
        return;
    }
    let out_dir = std::env::var_os("STAGECRAFT_MVR_OUT").map(PathBuf::from);
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let bytes = std::fs::read(&path).unwrap();
        let first = read_mvr(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        println!("{}", summary(&name, &first));
        for w in first.warnings.iter().take(25) {
            println!("    - {w}");
        }
        if first.warnings.len() > 25 {
            println!("    … {} more", first.warnings.len() - 25);
        }

        // Fixtures must keep their patch through the model.
        let s = first.mvr.scene.stats();
        assert!(s.layers > 0, "{name}: no layers");

        let exported = write_mvr(&first.mvr).unwrap();
        let second = read_mvr(&exported).unwrap();
        assert_eq!(second.mvr.header.provider, PROVIDER);
        assert_eq!(second.mvr.header.version, Version::CURRENT);
        assert_eq!(first.mvr.resources, second.mvr.resources, "{name}: resources differ");
        assert_scene_eq(&name, &first.mvr.scene, &second.mvr.scene);
        // Export is deterministic and stable after one round.
        assert_eq!(exported, write_mvr(&second.mvr).unwrap(), "{name}: export not stable");

        if let Some(dir) = &out_dir {
            std::fs::create_dir_all(dir).unwrap();
            let stem = path.file_stem().unwrap().to_string_lossy();
            std::fs::write(dir.join(format!("{stem}.roundtrip.mvr")), &exported).unwrap();
            // Same scene without GDTF files and GDTFSpec references: Perastage
            // stops on a modal "GDTF conflict" dialog when fixture types match
            // its library, which a headless check cannot answer.
            let mut lean = first.mvr.clone();
            lean.resources.files.retain(|n, _| ResourceKind::of(n) != ResourceKind::Gdtf);
            fn strip(objs: &mut [Object]) {
                for o in objs {
                    o.gdtf_spec = None;
                    o.gdtf_mode = None;
                    strip(&mut o.children);
                }
            }
            for l in &mut lean.scene.layers {
                strip(&mut l.children);
            }
            std::fs::write(dir.join(format!("{stem}.nogdtf.mvr")), write_mvr(&lean).unwrap()).unwrap();
        }
    }
}

fn assert_scene_eq(name: &str, a: &Scene, b: &Scene) {
    if a == b {
        return;
    }
    // Find the first differing object for a useful message.
    let ao = a.objects();
    let bo = b.objects();
    assert_eq!(ao.len(), bo.len(), "{name}: object count differs");
    for ((_, x), (_, y)) in ao.iter().zip(bo.iter()) {
        let mut x = (*x).clone();
        let mut y = (*y).clone();
        x.children.clear();
        y.children.clear();
        assert_eq!(x, y, "{name}: object differs");
    }
    assert_eq!(a.aux, b.aux, "{name}: AUXData differs");
    assert_eq!(a.user_data, b.user_data, "{name}: UserData differs");
    panic!("{name}: scenes differ outside objects");
}
