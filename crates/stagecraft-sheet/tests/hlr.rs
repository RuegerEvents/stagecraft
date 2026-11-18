//! Vector views through hidden-line removal: what is hidden, cut regions, the cache,
//! tag line styles, plans from 3D, and timing on rigs with real GDTF fixtures.
//!
//!     cargo test -p stagecraft-sheet --release --test hlr -- --nocapture
//!
//! prints the timings.

use std::time::Instant;

use glam::DVec3;
use serde_json::{Value, json};
use stagecraft_core::sheet::ViewKind;
use stagecraft_core::{Engine, Id};
use stagecraft_geom::MaterialClass;
use stagecraft_geom::section::Plane;
use stagecraft_sheet::prim::{Prim, Rgb};
use stagecraft_sheet::view::{EdgeClass, ViewOptions, ViewSpec};
use stagecraft_sheet::{FallbackSource, HlrSource, Inputs, VectorViewSource, compose};

fn run(e: &mut Engine, name: &str, args: Value) -> Value {
    e.execute(name, args).unwrap_or_else(|err| panic!("{name} failed: {err}"))
}

fn id(v: &Value) -> Id {
    v["id"].as_str().unwrap().parse().unwrap()
}

fn front(scale: f64) -> ViewSpec {
    ViewSpec { dir: DVec3::Y, up: DVec3::Z, section: None, scale }
}

/// Two solid blocks (walls), a big one behind a small one, seen from the front.
fn two_platforms() -> (Engine, Id, Id) {
    let mut e = Engine::new();
    let small = id(&run(
        &mut e,
        "wall.create",
        json!({ "path": [[-2000, 1000], [-1000, 1000]], "height": 400, "thickness": 200 }),
    ));
    let big = id(&run(
        &mut e,
        "wall.create",
        json!({ "path": [[-1500, 3000], [1500, 3000]], "height": 800, "thickness": 200 }),
    ));
    (e, small, big)
}

fn length(v: &stagecraft_sheet::view::VectorView, object: Id, class: EdgeClass) -> f64 {
    v.lines
        .iter()
        .filter(|l| l.object == object && l.class == class)
        .map(|l| l.points.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum::<f64>())
        .sum()
}

#[test]
fn objects_behind_others_lose_their_hidden_edges() {
    let (e, small, big) = two_platforms();
    let ids = [small, big];
    let src = HlrSource::new();
    let v = src.view(e.doc(), &ids, &front(50.0), ViewOptions::default());
    // The big block's outline: 2 × 3000 + 2 × 800 = 7600, minus what the small one
    // covers: 400 of its left edge and 500 of its bottom edge.
    let big_visible = length(&v, big, EdgeClass::Visible);
    assert!((big_visible - 6700.0).abs() < 5.0, "big block visible {big_visible}");
    assert!((length(&v, small, EdgeClass::Visible) - 2800.0).abs() < 5.0);
    assert_eq!(length(&v, big, EdgeClass::Hidden), 0.0, "hidden lines are off by default");
    // The fallback draws the covered part too.
    let fb = FallbackSource.view(e.doc(), &ids, &front(50.0), ViewOptions::default());
    assert!(length(&fb, big, EdgeClass::Visible) > 7500.0);

    let h = src.view(e.doc(), &ids, &front(50.0), ViewOptions { hidden_lines: true });
    // The covered bottom piece lies on the small block's own bottom line and is dropped;
    // the left edge behind it comes out dashed.
    let hidden = length(&h, big, EdgeClass::Hidden);
    assert!((hidden - 400.0).abs() < 5.0, "hidden part of the big block {hidden}");
}

#[test]
fn sections_give_cut_regions_with_materials() {
    let (e, small, big) = two_platforms();
    let plane = Plane::new([0.0, 3000.0, 0.0], [0.0, -1.0, 0.0]);
    let spec = ViewSpec { section: Some(plane), ..front(50.0) };
    let v = HlrSource::new().view(e.doc(), &[small, big], &spec, ViewOptions::default());
    let cut: Vec<_> = v.cuts.iter().filter(|c| c.object == big).collect();
    assert!(!cut.is_empty(), "the big deck is cut");
    assert!(
        cut.iter().all(|c| c.material == MaterialClass::Wall),
        "{:?}",
        cut.iter().map(|c| c.material).collect::<Vec<_>>()
    );
    assert!(v.lines.iter().any(|l| l.object == big && l.class == EdgeClass::Cut));
    // The small deck lies on the removed side.
    assert!(v.lines.iter().all(|l| l.object != small));
}

#[test]
fn the_cache_follows_document_revisions() {
    let (mut e, small, big) = two_platforms();
    let ids = [small, big];
    let src = HlrSource::new();
    let a = src.view(e.doc(), &ids, &front(50.0), ViewOptions::default());
    let b = src.view(e.doc(), &ids, &front(50.0), ViewOptions::default());
    assert!(std::sync::Arc::ptr_eq(&a, &b));
    assert_eq!(src.stats(), (1, 1));
    // Sheets and meta do not touch the objects: still cached.
    run(&mut e, "sheet.create", json!({ "name": "x" }));
    run(&mut e, "doc.set_meta", json!({ "project": "p" }));
    let c = src.view(e.doc(), &ids, &front(50.0), ViewOptions::default());
    assert!(std::sync::Arc::ptr_eq(&a, &c));
    // Moving an object recomputes.
    run(&mut e, "object.move", json!({ "ids": [small], "delta": [500, 0, 0] }));
    let d = src.view(e.doc(), &ids, &front(50.0), ViewOptions::default());
    assert!(!std::sync::Arc::ptr_eq(&a, &d));
    assert_eq!(src.stats(), (2, 2));
    // Undo brings back the old revision; equal maps count as the same.
    run(&mut e, "history.undo", json!({}));
    let f = src.view(e.doc(), &ids, &front(50.0), ViewOptions::default());
    assert_eq!(f.lines.len(), a.lines.len());
}

/// Pen widths of unfilled paths inside viewports.
fn pen_widths(prims: &[Prim], out: &mut Vec<f64>) {
    for p in prims {
        if let Prim::Clip { prims, .. } = p {
            for q in prims {
                if let Prim::Path { pen: Some(pen), fill: None, .. } = q {
                    out.push(pen.width);
                }
            }
        }
    }
}

#[test]
fn tag_line_styles_change_pens() {
    let (mut e, small, _) = two_platforms();
    run(&mut e, "object.tag", json!({ "ids": [small], "tags": ["set"] }));
    let s = id(&run(&mut e, "sheet.create", json!({ "name": "Elevation" })));
    let vp = id(&run(&mut e, "viewport.create", json!({ "sheet": s, "view": "front", "scale": 50 })));
    let rasters = Default::default();
    let src = HlrSource::new();
    let widths = |e: &Engine| {
        let page = compose(e.doc(), s, &Inputs { source: &src, rasters: &rasters }).unwrap();
        let mut w = Vec::new();
        pen_widths(&page.prims, &mut w);
        w
    };
    assert!(!widths(&e).contains(&0.7));
    run(&mut e, "viewport.update", json!({ "viewport": vp, "tag_lines": { "set": { "weight": 0.7 } } }));
    assert!(widths(&e).contains(&0.7));
    run(&mut e, "viewport.update", json!({ "viewport": vp, "tag_lines": { "set": { "class": "thick" } } }));
    assert!(widths(&e).contains(&0.5));
    assert!(
        e.execute("viewport.update", json!({ "viewport": vp, "tag_lines": { "set": { "class": "bold" } } })).is_err()
    );
    assert!(e.execute("viewport.update", json!({ "viewport": vp, "tag_lines": { "nope": {} } })).is_err());
    // Deleting the tag drops the entry.
    run(&mut e, "tag.delete", json!({ "tag": "set" }));
    let v = e.doc().sheets[&s].viewports[0].clone();
    assert!(v.tag_lines.is_empty());
}

#[test]
fn plan_from_3d_uses_world_xy_and_cuts() {
    let (mut e, small, _) = two_platforms();
    let s = id(&run(&mut e, "sheet.create", json!({ "name": "Plan 3D" })));
    let vp = id(&run(&mut e, "viewport.create", json!({ "sheet": s, "view": "plan_3d", "scale": 50 })));
    let vpv = e.doc().sheets[&s].viewport(vp).unwrap().clone();
    assert_eq!(vpv.view, ViewKind::PlanFrom3d { cut: None });
    let spec = stagecraft_sheet::compose::view_spec(&vpv).unwrap();
    let v = HlrSource::new().view(e.doc(), &[small], &spec, ViewOptions::default());
    let b = v.bounds().unwrap();
    assert!((b[0][0] + 2000.0).abs() < 1.0 && (b[1][1] - 1100.0).abs() < 1.0, "{b:?}");
    run(&mut e, "viewport.update", json!({ "viewport": vp, "view": { "type": "plan_3d", "cut": 200 } }));
    let vpv = e.doc().sheets[&s].viewport(vp).unwrap().clone();
    let spec = stagecraft_sheet::compose::view_spec(&vpv).unwrap();
    let v = HlrSource::new().view(e.doc(), &[small], &spec, ViewOptions::default());
    assert!(!v.cuts.is_empty(), "cut at 200 mm goes through the 400 mm block");
    let rasters = Default::default();
    let page = compose(e.doc(), s, &Inputs { source: &HlrSource::new(), rasters: &rasters }).unwrap();
    fn has_fill(p: &[Prim]) -> bool {
        p.iter().any(|p| match p {
            Prim::Clip { prims, .. } => has_fill(prims),
            Prim::Path { fill: Some(Rgb::WHITE), pen: None, .. } => true,
            _ => false,
        })
    }
    assert!(has_fill(&page.prims));
}

/// A truss with `n` fixtures of each type in `types`, a sheet with a front elevation and
/// a section through the truss.
fn gdtf_rig(types: &[std::path::PathBuf], per_type: usize) -> (Engine, Vec<Id>) {
    let mut e = Engine::new();
    let mut keys = Vec::new();
    for p in types {
        let v = run(&mut e, "gdtf.import", json!({ "path": p }));
        keys.push(v["key"].as_str().unwrap().to_owned());
    }
    let n = keys.len() * per_type;
    let len = (n as f64 * 600.0).max(2000.0);
    let truss =
        id(&run(&mut e, "truss.create", json!({ "start": [-len / 2.0, 3000, 7000], "end": [len / 2.0, 3000, 7000] })));
    for i in 0..n {
        run(
            &mut e,
            "fixture.create",
            json!({ "parent": truss, "position": [300.0 + i as f64 * 600.0, 0, -300], "gdtf": keys[i % keys.len()],
                    "fid": 1 + i }),
        );
    }
    let ids = e.doc().objects.keys().copied().collect();
    (e, ids)
}

fn private_gdtfs() -> Vec<std::path::PathBuf> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/private/gdtf");
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .map(|d| {
            d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "gdtf")).collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

#[test]
fn timing_with_many_gdtf_fixtures() {
    let types = private_gdtfs();
    if types.is_empty() {
        eprintln!("skipped: no GDTFs in testdata/private/gdtf");
        return;
    }
    let (e, ids) = gdtf_rig(&types, 12);
    let fixtures =
        ids.iter().filter(|i| matches!(e.doc().objects[*i].kind, stagecraft_core::ObjectKind::Fixture { .. })).count();
    let src = HlrSource::new();
    for (name, spec) in [
        ("front 1:50", front(50.0)),
        ("front 1:200", front(200.0)),
        ("section", ViewSpec { section: Some(Plane::new([0.0, 3000.0, 0.0], [0.0, -1.0, 0.0])), ..front(50.0) }),
        ("side", ViewSpec { dir: DVec3::X, ..front(50.0) }),
    ] {
        let t = Instant::now();
        let scene = stagecraft_sheet::hlr::scene(e.doc(), &ids, spec.scale);
        let tris: usize = scene.meshes.iter().map(|m| m.triangles.len()).sum();
        let t_scene = t.elapsed();
        let v = src.view(e.doc(), &ids, &spec, ViewOptions::default());
        let t_all = t.elapsed();
        let t2 = Instant::now();
        let _ = src.view(e.doc(), &ids, &spec, ViewOptions::default());
        println!(
            "{name}: {fixtures} fixtures, {tris} triangles, scene {:.0} ms, total {:.0} ms, {} lines; cached {:.2} ms",
            t_scene.as_secs_f64() * 1e3,
            t_all.as_secs_f64() * 1e3,
            v.lines.len(),
            t2.elapsed().as_secs_f64() * 1e3
        );
        assert!(!v.lines.is_empty());
    }
}
