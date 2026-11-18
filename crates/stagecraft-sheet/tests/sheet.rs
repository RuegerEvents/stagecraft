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
    // The compact block (the default): venue stands in for the missing address, the
    // date for the missing production dates.
    for want in
        ["Test show", "Hall", "Lighting plan", "Sheet L-1  ·  Rev. A", "1:50, 1:100", "2026-11-18", "Legend", "× 7"]
    {
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
    // Zoned border: 1–3 across, A–B down, each twice.
    for want in ["1", "2", "3", "A", "B"] {
        assert!(t_count(&page.prims, want) >= 2, "zone {want}");
    }
}

fn t_count(prims: &[Prim], want: &str) -> usize {
    let mut t = Vec::new();
    texts(prims, &mut t);
    t.iter().filter(|x| *x == want).count()
}

/// Title block fields, German sheet text, comma decimals; the generic block stays
/// selectable; the legend sits on the title block, as wide.
#[test]
fn compact_block_in_german_and_the_generic_one() {
    let (mut e, s) = rig();
    run(
        &mut e,
        "doc.set_meta",
        json!({ "project": "A long production name that has to wrap onto a second line", "company": "Example Lighting GmbH",
                "website": "example.org", "venue_address": "Street 1\n12345 Town", "production_dates": "01.05.2026 – 03.05.2026",
                "sheet_language": "de" }),
    );
    let rasters = Default::default();
    let inputs = Inputs { source: &stagecraft_sheet::FallbackSource, rasters: &rasters };
    let page = compose(e.doc(), s, &inputs).unwrap();
    let mut t = Vec::new();
    texts(&page.prims, &mut t);
    for want in [
        "Produktion",
        "Zeichnung",
        "Legende",
        "Example Lighting GmbH",
        "example.org",
        "Street 1",
        "12345 Town",
        "01.05.2026 – 03.05.2026",
        "Blatt L-1  ·  Rev. A  ·  2026-11-18",
        "Typ",
        "Grundriss",
        "Ansicht vorne",
    ] {
        assert!(t.iter().any(|x| x == want), "missing `{want}` in {t:?}");
    }
    // The name wrapped: no line of it runs past the left column.
    let name_lines: Vec<_> =
        text_prims(&page.prims).into_iter().filter(|p| p.2 > 2.0 && p.0[0] > 300.0 && p.0[1] < 40.0).collect();
    assert!(name_lines.len() >= 2, "{name_lines:?}");
    // Back to English renames the default view names again.
    run(&mut e, "doc.set_meta", json!({ "sheet_language": "en" }));
    let mut t = Vec::new();
    texts(&compose(e.doc(), s, &inputs).unwrap().prims, &mut t);
    assert!(t.iter().any(|x| x == "Front elevation") && t.iter().any(|x| x == "Production"), "{t:?}");

    run(&mut e, "sheet.update", json!({ "sheet": s, "title_block": "generic" }));
    let page = compose(e.doc(), s, &inputs).unwrap();
    let mut t = Vec::new();
    texts(&page.prims, &mut t);
    assert!(t.iter().any(|x| x == "Drawn by") && !t.iter().any(|x| x == "Production"), "{t:?}");
    assert_eq!(t_count(&page.prims, "A"), 0, "no zones on the generic border");
}

/// (position, text, size) of every text.
fn text_prims(prims: &[Prim]) -> Vec<([f64; 2], String, f64)> {
    let mut out = Vec::new();
    for p in prims {
        match p {
            Prim::Text { pos, text, size, .. } => out.push((*pos, text.clone(), *size)),
            Prim::Clip { prims, .. } => out.extend(text_prims(prims)),
            _ => {}
        }
    }
    out
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

/// GDTF types: label fields type/mode/footprint, legend rows per type, fixture
/// list with channels and per-break addresses.
#[test]
fn gdtf_types_in_labels_legend_and_list() {
    let (mut e, s) = rig();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/gdtf");
    for n in ["Spot 300", "Pixel Bar 8"] {
        run(&mut e, "gdtf.import", json!({ "path": dir.join(format!("StageCraft Demo@{n}.gdtf")) }));
    }
    let fixtures: Vec<Id> = e
        .doc()
        .objects
        .values()
        .filter(|o| matches!(o.kind, stagecraft_core::ObjectKind::Fixture { fid: Some(f), .. } if f < 200))
        .map(|o| o.id)
        .collect();
    run(&mut e, "fixture.set_type", json!({ "ids": fixtures[..4], "gdtf_type": "StageCraft Demo@Spot 300" }));
    run(
        &mut e,
        "fixture.set_type",
        json!({ "ids": fixtures[4..], "gdtf_type": "StageCraft Demo@Pixel Bar 8", "mode": "Split" }),
    );
    run(&mut e, "patch.auto", json!({ "ids": fixtures, "start": "2.001" }));
    let o = &e.doc().objects[&fixtures[4]];
    assert_eq!(stagecraft_sheet::labels::field_value(e.doc(), o, "type"), "StageCraft Demo Pixel Bar 8");
    assert_eq!(stagecraft_sheet::labels::field_value(e.doc(), o, "mode"), "Split");
    assert_eq!(stagecraft_sheet::labels::field_value(e.doc(), o, "footprint"), "2+24 ch");
    assert_eq!(stagecraft_sheet::labels::field_value(e.doc(), o, "address"), "2.065 / 2.067");
    let rasters = Default::default();
    let page = compose(e.doc(), s, &Inputs { source: &stagecraft_sheet::FallbackSource, rasters: &rasters }).unwrap();
    let mut t = Vec::new();
    texts(&page.prims, &mut t);
    for want in [
        "StageCraft Demo Spot 300  (16 ch)",
        "StageCraft Demo Pixel Bar 8  (2+24 ch)",
        "Generic fixture",
        "× 4",
        "× 2",
        "2+24",
        "2.065 / 2.067",
    ] {
        assert!(t.iter().any(|x| x == want), "missing `{want}` in {t:?}");
    }
}

/// A sketched room with typed lengths and a reference dimension (truss to the back
/// wall) on a plan viewport; the dimension toggles hide them.
#[test]
fn plan_viewports_draw_sketch_and_reference_dimensions() {
    stagecraft_sheet::install();
    let mut e = Engine::new();
    let seg = |len: f64| json!({ "axis_lock": true, "length": len });
    run(
        &mut e,
        "sketch.draw",
        json!({ "points": [[-5000, 0], [5000, 0], [5000, 8000], [-5000, 8000]], "closed": true, "make": "wall",
                "segments": [seg(10000.0), seg(8000.0)] }),
    );
    run(&mut e, "sketch.make_floor", json!({ "at": [0, 4000] }));
    let truss = id(&run(&mut e, "truss.create", json!({ "start": [-3000, 3000, 6000], "end": [3000, 3000, 6000] })));
    let s = run(&mut e, "sketch.get", Value::Null);
    let back_left = s["sketch"]["points"][3]["id"].as_u64().unwrap();
    let layer = s["layer"].clone();
    run(
        &mut e,
        "dimension.add_reference",
        json!({ "from": { "type": "object", "object": truss }, "to": { "type": "sketch_point", "layer": layer, "point": back_left },
                "axis": "vertical", "label_offset": [-3500, 0] }),
    );
    let sh = id(&run(
        &mut e,
        "sheet.create",
        json!({ "name": "Room", "number": "R-1", "paper": "a4", "orientation": "landscape" }),
    ));
    let vp = id(&run(
        &mut e,
        "viewport.create",
        json!({ "sheet": sh, "view": "plan", "scale": 100, "position": [140, 110] }),
    ));
    let rasters = Default::default();
    let inputs = Inputs { source: &stagecraft_sheet::FallbackSource, rasters: &rasters };
    // New documents follow the project unit: mm, then m.
    let mut t = Vec::new();
    texts(&compose(e.doc(), sh, &inputs).unwrap().prims, &mut t);
    for want in ["10000", "8000", "(5000)"] {
        assert!(t.iter().any(|x| x == want), "missing `{want}` in {t:?}");
    }
    run(&mut e, "doc.set_units", json!({ "length_unit": "m" }));
    let mut t = Vec::new();
    texts(&compose(e.doc(), sh, &inputs).unwrap().prims, &mut t);
    for want in ["10.000", "8.000", "(5.000)"] {
        assert!(t.iter().any(|x| x == want), "missing `{want}` in {t:?}");
    }
    run(&mut e, "doc.set_units", json!({ "length_unit": "ft_in" }));
    let mut t = Vec::new();
    texts(&compose(e.doc(), sh, &inputs).unwrap().prims, &mut t);
    assert!(t.iter().any(|x| x == "32' 9 11/16\""), "{t:?}");
    run(&mut e, "doc.set_meta", json!({ "dimension_format": "m_sup_mm" }));
    let mut t = Vec::new();
    texts(&compose(e.doc(), sh, &inputs).unwrap().prims, &mut t);
    for want in ["10.00", "8.00", "(5.00)"] {
        assert!(t.iter().any(|x| x == want), "missing `{want}` in {t:?}");
    }
    // Formats: metres with raised millimetres and a comma, plain millimetres.
    let sk = run(&mut e, "sketch.get", Value::Null);
    let width = sk["sketch"]["dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["value"].as_f64() == Some(10000.0))
        .map(|d| d["id"].clone())
        .expect("the 10 m dimension");
    run(&mut e, "dimension.set_value", json!({ "id": width, "value": 10155 }));
    run(&mut e, "doc.set_meta", json!({ "decimal_separator": "," }));
    let mut t = Vec::new();
    texts(&compose(e.doc(), sh, &inputs).unwrap().prims, &mut t);
    assert!(t.iter().any(|x| x == "10,15⁵"), "{t:?}");
    run(&mut e, "doc.set_meta", json!({ "dimension_format": "mm" }));
    let mut t = Vec::new();
    texts(&compose(e.doc(), sh, &inputs).unwrap().prims, &mut t);
    assert!(t.iter().any(|x| x == "10155"), "{t:?}");
    run(&mut e, "doc.set_meta", json!({ "dimension_format": "m_sup_mm", "decimal_separator": "auto" }));
    if let Ok(dir) = std::env::var("STAGECRAFT_SHEETS") {
        std::fs::create_dir_all(&dir).unwrap();
        run(&mut e, "sheet.export_pdf", json!({ "path": format!("{dir}/room-dimensions.pdf"), "sheet": sh }));
        run(&mut e, "sheet.export_svg", json!({ "path": format!("{dir}/room-dimensions.svg"), "sheet": sh }));
    }
    run(
        &mut e,
        "viewport.update",
        json!({ "viewport": vp, "sketch_dimensions": false, "reference_dimensions": false }),
    );
    let mut t = Vec::new();
    texts(&compose(e.doc(), sh, &inputs).unwrap().prims, &mut t);
    assert!(!t.iter().any(|x| x == "10.00" || x == "(5.00)"), "{t:?}");
}

fn images(prims: &[Prim]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for p in prims {
        match p {
            Prim::Image { angle, opacity, .. } => out.push((*angle, *opacity)),
            Prim::Clip { prims, .. } => out.extend(images(prims)),
            _ => {}
        }
    }
    out
}

#[test]
fn underlays_and_notes_on_plan_viewports() {
    stagecraft_sheet::install();
    let mut e = Engine::new();
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="200"><rect width="300" height="200" fill="#ddd"/></svg>"##;
    let u = id(&run(
        &mut e,
        "underlay.import",
        json!({ "data_base64": stagecraft_core::sheet::base64_encode(svg.as_bytes()), "file_name": "u.svg", "opacity": 0.4 }),
    ));
    run(&mut e, "object.rotate", json!({ "ids": [u], "angle": 30, "pivot": [0, 0, 0] }));
    run(&mut e, "note.create", json!({ "text": "Assumption: stage 1 m high", "at": [0, -1000], "frame": true }));
    let sh = id(&run(
        &mut e,
        "sheet.create",
        json!({ "name": "U", "number": "U-1", "paper": "a4", "orientation": "landscape" }),
    ));
    let vp = id(&run(
        &mut e,
        "viewport.create",
        json!({ "sheet": sh, "view": "plan", "scale": 100, "position": [140, 110] }),
    ));
    let rasters = Default::default();
    let inputs = Inputs { source: &stagecraft_sheet::FallbackSource, rasters: &rasters };
    let page = compose(e.doc(), sh, &inputs).unwrap();
    assert!(images(&page.prims).is_empty(), "underlays are off on sheets by default");
    let mut t = Vec::new();
    texts(&page.prims, &mut t);
    assert!(t.iter().any(|x| x == "Assumption: stage 1 m high"), "{t:?}");
    run(&mut e, "viewport.update", json!({ "viewport": vp, "underlays": true }));
    let page = compose(e.doc(), sh, &inputs).unwrap();
    let imgs = images(&page.prims);
    assert_eq!(imgs.len(), 1);
    assert!((imgs[0].0 - 30.0).abs() < 1e-6 && (imgs[0].1 - 0.4).abs() < 1e-9, "{imgs:?}");
    let dir = std::env::var("STAGECRAFT_SHEETS").unwrap_or_else(|_| tempdir());
    std::fs::create_dir_all(&dir).unwrap();
    run(&mut e, "sheet.export_pdf", json!({ "path": format!("{dir}/underlay.pdf"), "sheet": sh }));
    run(&mut e, "sheet.export_svg", json!({ "path": format!("{dir}/underlay.svg"), "sheet": sh }));
    let svg_out = std::fs::read_to_string(format!("{dir}/underlay.svg")).unwrap();
    assert!(svg_out.contains("rotate(-30.0000") && svg_out.contains("opacity=\"0.400\""));
}
