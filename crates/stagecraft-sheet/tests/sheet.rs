//! Composition and export of sheets.
//!
//!     STAGECRAFT_SHEETS=<dir> cargo test -p stagecraft-sheet --test sheet
//!
//! also writes the test sheets as PDF and SVG into `<dir>` to look at.

use serde_json::{Value, json};
use stagecraft_core::{Engine, Id};
use stagecraft_sheet::prim::Prim;
use stagecraft_sheet::{Inputs, compose};

fn run(e: &mut Engine, name: &str, args: Value) -> Value {
    e.execute(name, args).unwrap_or_else(|err| panic!("{name} failed: {err}"))
}

fn id(v: &Value) -> Id {
    v["id"].as_str().unwrap().parse().unwrap()
}

/// A truss with six fixtures (two of them turned, one standing turned over), a
/// platform, and a sheet with plan, front elevation, a section and a fixture list.
fn rig() -> (Engine, Id) {
    stagecraft_sheet::install();
    let mut e = Engine::new();
    run(
        &mut e,
        "doc.set_meta",
        json!({ "project": "Test show", "venue": "Hall", "drawn_by": "SC", "date": "2026-11-18", "revision": "A" }),
    );
    run(&mut e, "floor.create", json!({ "outline": [[-6000, 0], [6000, 0], [6000, 8000], [-6000, 8000]] }));
    run(&mut e, "platform.create", json!({ "position": [0, 5000, 0], "width": 4000, "depth": 2000, "height": 600 }));
    let truss = id(&run(&mut e, "truss.create", json!({ "start": [-4000, 2000, 6000], "end": [4000, 2000, 6000] })));
    for i in 0..6 {
        let rot = match i {
            2 => 90.0,
            3 => 180.0,
            _ => 0.0,
        };
        let f = id(&run(
            &mut e,
            "fixture.create",
            json!({ "parent": truss, "position": [500.0 + i as f64 * 600.0, 0, -300], "rotation": [0, 0, rot],
                    "fid": 101 + i, "address": format!("1.{:03}", 1 + i * 20), "mode": "Standard" }),
        ));
        run(
            &mut e,
            "object.set_attrs",
            json!({ "ids": [f], "attrs": { "gel": format!("L{}", 201 + i), "circuit": format!("{}", 10 + i) } }),
        );
    }
    // A floor fixture, turned over as the fixture tool does it.
    run(&mut e, "fixture.create", json!({ "position": [-3000, 6500, 0], "rotation": [180, 0, 30], "fid": 201 }));
    let lt = id(&run(&mut e, "label.create", json!({ "name": "Electrics" })));
    let s = id(&run(&mut e, "sheet.create", json!({ "name": "Lighting plan", "number": "L-1" })));
    run(
        &mut e,
        "viewport.create",
        json!({ "sheet": s, "view": "plan", "scale": 50, "position": [120, 180], "label_template": lt,
                "crop": [[-5000, 500], [5000, 7500]] }),
    );
    run(&mut e, "viewport.create", json!({ "sheet": s, "view": "front", "scale": 100, "position": [330, 230] }));
    run(
        &mut e,
        "viewport.create",
        json!({ "sheet": s, "view": { "type": "section", "origin": [0, 5000, 0], "normal": [0, -1, 0] }, "scale": 100, "position": [330, 140] }),
    );
    run(&mut e, "viewport.create", json!({ "sheet": s, "view": "fixture_list", "position": [130, 50] }));
    (e, s)
}

fn texts(prims: &[Prim], out: &mut Vec<String>) {
    for p in prims {
        match p {
            Prim::Text { text, .. } => out.push(text.clone()),
            Prim::Clip { prims, .. } => texts(prims, out),
            _ => {}
        }
    }
}

#[test]
fn a_sheet_has_frame_title_block_viewports_labels_and_legend() {
    let (e, s) = rig();
    let rasters = Default::default();
    let page = compose(e.doc(), s, &Inputs { source: &stagecraft_sheet::FallbackSource, rasters: &rasters }).unwrap();
    assert_eq!(page.size, [420.0, 297.0]);
    assert_eq!(page.viewports.len(), 4);
    let mut t = Vec::new();
    texts(&page.prims, &mut t);
    for want in ["Test show", "Hall", "Lighting plan", "L-1", "1:50, 1:100", "2026-11-18 A", "LEGEND", "× 7"] {
        assert!(t.iter().any(|x| x == want), "missing `{want}` in {t:?}");
    }
    // Labels: FIDs, gels and addresses of the hung fixtures.
    for want in ["101", "106", "L203", "1.021", "201"] {
        assert!(t.iter().any(|x| x == want), "label `{want}` missing");
    }
    assert_eq!(page.labels.len(), 7);
    // The plan does not draw its own FIDs on top of the labels.
    assert_eq!(t.iter().filter(|x| *x == "101").count(), 2, "label and fixture list, not the symbol");
    // No two label fields overlap.
    let sheet = &e.doc().sheets[&s];
    let vp = &sheet.viewports[0];
    let m = stagecraft_sheet::compose::plan_mapping(e.doc(), vp, &stagecraft_sheet::FallbackSource).unwrap();
    let ids = stagecraft_sheet::compose::shown_ids(e.doc(), vp);
    let t = &e.doc().label_templates[&vp.label_template.unwrap()];
    let placed = stagecraft_sheet::compose::plan_labels(e.doc(), vp, t, &ids, &m);
    let boxes: Vec<_> = placed.iter().flat_map(|l| l.fields.iter().map(|f| f.aabb())).collect();
    for (i, a) in boxes.iter().enumerate() {
        for b in &boxes[i + 1..] {
            let w = a[1][0].min(b[1][0]) - a[0][0].max(b[0][0]);
            let h = a[1][1].min(b[1][1]) - a[0][1].max(b[0][1]);
            assert!(!(w > 1e-6 && h > 1e-6), "label fields overlap: {a:?} {b:?}");
        }
    }
    // Viewports stay inside the frame.
    for v in &page.viewports {
        assert!(v.min[0] >= 10.0 && v.min[1] >= 10.0 && v.max[0] <= 410.0 && v.max[1] <= 287.0, "{v:?}");
    }
}

#[test]
fn hand_dragged_labels_move() {
    let (mut e, s) = rig();
    let rasters = Default::default();
    let inputs = Inputs { source: &stagecraft_sheet::FallbackSource, rasters: &rasters };
    let before = compose(e.doc(), s, &inputs).unwrap();
    let hit = before.labels[0].clone();
    run(&mut e, "label.move", json!({ "viewport": hit.viewport, "fixture": hit.fixture, "offset": [0, 15] }));
    let after = compose(e.doc(), s, &inputs).unwrap();
    let moved = after.labels.iter().find(|l| l.fixture == hit.fixture).unwrap();
    assert!(moved.min[1] > hit.min[1] + 10.0);
}

#[test]
fn pdf_and_svg_export() {
    let (mut e, s) = rig();
    let s2 = id(&run(&mut e, "sheet.create", json!({ "name": "Overview", "paper": "a4", "orientation": "portrait" })));
    run(&mut e, "viewport.create", json!({ "sheet": s2, "view": "rendered", "scale": 100, "position": [105, 160] }));
    let dir = std::env::var("STAGECRAFT_SHEETS").ok();
    let tmp = tempdir();
    let out = dir.clone().unwrap_or_else(|| tmp.clone());
    std::fs::create_dir_all(&out).unwrap();
    let pdf = format!("{out}/sheets.pdf");
    let r = run(&mut e, "sheet.export_pdf", json!({ "path": pdf }));
    assert_eq!(r["pages"], 2);
    let bytes = std::fs::read(&pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    let doc = lopdf::Document::load_mem(&bytes).expect("lopdf parses the PDF");
    assert_eq!(doc.get_pages().len(), 2);
    // A3 landscape in points.
    let first = *doc.get_pages().values().next().unwrap();
    let media = doc.get_object(first).unwrap().as_dict().unwrap().get(b"MediaBox").unwrap().as_array().unwrap().clone();
    let w = media[2].as_float().unwrap();
    assert!((w - 420.0 * 72.0 / 25.4).abs() < 0.5, "{w}");
    // The font is embedded.
    let fonts = doc.objects.values().filter(|o| o.as_dict().is_ok_and(|d| d.has(b"FontFile2"))).count();
    assert!(fonts >= 1, "embedded font files");

    let svg = format!("{out}/lighting.svg");
    run(&mut e, "sheet.export_svg", json!({ "path": svg, "sheet": "L-1" }));
    let text = std::fs::read_to_string(&svg).unwrap();
    assert!(text.starts_with("<svg") && text.contains("Lighting plan") && text.contains("clipPath"));
    run(&mut e, "sheet.export_svg", json!({ "path": format!("{out}/overview.svg"), "sheet": s2 }));
    if dir.is_some() && std::process::Command::new("pdfinfo").arg(&pdf).output().is_ok_and(|o| o.status.success()) {
        let status =
            std::process::Command::new("pdftoppm").args(["-png", "-r", "110", &pdf, &format!("{out}/sheet")]).status();
        assert!(status.is_ok_and(|s| s.success()));
    }
    let _ = s;
}

fn tempdir() -> String {
    let d = std::env::temp_dir().join(format!("stagecraft-sheet-{}", std::process::id()));
    d.to_string_lossy().into_owned()
}
