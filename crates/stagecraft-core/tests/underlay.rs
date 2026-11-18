//! Underlays and notes through commands: import of each format, calibration, align,
//! lock, visibility, save/load with assets, MVR export leaves them out.

use serde_json::{Value, json};
use stagecraft_core::{Engine, Id, ObjectKind};

fn run(e: &mut Engine, name: &str, args: Value) -> Value {
    e.execute(name, args).unwrap_or_else(|err| panic!("{name} failed: {err}"))
}

fn id(v: &Value) -> Id {
    v["id"].as_str().expect("id").parse().expect("uuid")
}

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200">
  <rect x="0" y="0" width="400" height="200" fill="#fff" stroke="#000"/>
  <line x1="100" y1="100" x2="300" y2="100" stroke="#000"/></svg>"##;

fn dxf() -> Vec<u8> {
    // A minimal R12 DXF: one 10 m line and a text, units unset (mm).
    let s = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n30\n0\n11\n10000\n21\n0\n31\n0\n\
             0\nTEXT\n8\n0\n10\n0\n20\n500\n30\n0\n40\n250\n1\nBuehne\n0\nENDSEC\n0\nEOF\n";
    s.as_bytes().to_vec()
}

fn b64(b: &[u8]) -> String {
    stagecraft_core::sheet::base64_encode(b)
}

fn underlay(e: &Engine, id: Id) -> (f64, [f64; 3], f64, bool) {
    let o = &e.doc().objects[&id];
    let ObjectKind::Underlay { scale, opacity, locked, .. } = &o.kind else { panic!("not an underlay") };
    (*scale, o.placement.position, *opacity, *locked)
}

#[test]
fn svg_underlay_imports_on_its_own_layer_and_calibrates() {
    let mut e = Engine::new();
    let out = run(&mut e, "underlay.import", json!({ "data_base64": b64(SVG.as_bytes()), "file_name": "room.svg" }));
    let u = id(&out);
    // 1:100 of 400 px at 96 dpi: 400 · 25.4 / 96 · 100 mm
    let w = 400.0 * 25.4 / 96.0 * 100.0;
    assert!((out["size"][0].as_f64().unwrap() - w).abs() < 1e-6, "{out}");
    let o = &e.doc().objects[&u];
    let layer = &e.doc().layers[&o.layer];
    assert_eq!(layer.name, "Underlay");
    assert!(e.doc().layers.values().all(|l| l.id == layer.id || l.order > layer.order));
    let ObjectKind::Underlay { raster: Some(r), source, .. } = &o.kind else { panic!() };
    assert_eq!(e.doc().assets[r].ext, "png");
    assert_eq!(e.doc().assets[source].ext, "svg");
    assert_eq!(o.name, "room");

    // the line from x 100 to 300 px is 200 px; say it is 8 m
    let (s0, ..) = underlay(&e, u);
    let p1 = [100.0 * s0, 100.0 * s0];
    let p2 = [300.0 * s0, 100.0 * s0];
    let c = run(&mut e, "underlay.calibrate", json!({ "id": u, "p1": p1, "p2": p2, "distance_mm": 8000.0 }));
    let (s1, pos, ..) = underlay(&e, u);
    assert!((s1 * 200.0 - 8000.0).abs() < 1e-6, "{c}");
    // p1 stayed where it was: origin moved towards it
    assert!((pos[0] + 100.0 * s1 - p1[0]).abs() < 1e-6 && (pos[1] + 100.0 * s1 - p1[1]).abs() < 1e-6, "{pos:?}");

    // align: the line's ends onto (0,0)–(0,8000): turned 90°, no scale change
    let a1 = [pos[0] + 100.0 * s1, pos[1] + 100.0 * s1];
    let a2 = [pos[0] + 300.0 * s1, pos[1] + 100.0 * s1];
    let out = run(&mut e, "underlay.align", json!({ "id": u, "from": [a1, a2], "to": [[0, 0], [0, 8000]] }));
    assert!(
        (out["angle"].as_f64().unwrap() - 90.0).abs() < 1e-9 && (out["factor"].as_f64().unwrap() - 1.0).abs() < 1e-9
    );
    let w = stagecraft_core::geom::world(e.doc(), u).unwrap();
    let q = w.transform_point3(glam::DVec3::new(300.0 * s1, 100.0 * s1, 0.0));
    assert!(q.x.abs() < 1e-6 && (q.y - 8000.0).abs() < 1e-6, "{q}");

    // undo the whole import in steps
    e.execute("history.undo", Value::Null).unwrap();
    e.execute("history.undo", Value::Null).unwrap();
    e.execute("history.undo", Value::Null).unwrap();
    assert!(e.doc().objects.is_empty());
}

#[test]
fn locked_underlays_refuse_edits_and_hide() {
    let mut e = Engine::new();
    let u = id(&run(&mut e, "underlay.import", json!({ "data_base64": b64(SVG.as_bytes()), "file_name": "a.svg" })));
    run(&mut e, "underlay.update", json!({ "id": u, "locked": true, "opacity": 0.3 }));
    assert_eq!(underlay(&e, u).2, 0.3);
    for (cmd, args) in [
        ("object.move", json!({ "ids": [u], "delta": [100, 0, 0] })),
        ("object.rotate", json!({ "ids": [u], "angle": 10 })),
        ("object.delete", json!({ "ids": [u] })),
        ("underlay.calibrate", json!({ "id": u, "p1": [0, 0], "p2": [1, 0], "distance_mm": 5 })),
    ] {
        let err = e.execute(cmd, args).unwrap_err();
        assert!(err.message.contains("locked"), "{cmd}: {err}");
    }
    run(&mut e, "underlay.update", json!({ "id": u, "locked": false, "visible": false }));
    assert!(e.doc().objects[&u].style.hidden);
    run(&mut e, "object.move", json!({ "ids": [u], "delta": [100, 0, 0] }));
    assert_eq!(underlay(&e, u).1[0], 100.0);
}

#[test]
fn dxf_png_and_pdf_underlays() {
    let mut e = Engine::new();
    let out = run(&mut e, "underlay.import", json!({ "data_base64": b64(&dxf()), "file_name": "plan.dxf" }));
    let u = id(&out);
    let o = &e.doc().objects[&u];
    let ObjectKind::Underlay { linework: Some(l), raster: None, scale, extent, .. } = &o.kind else { panic!() };
    assert_eq!(*scale, 1.0);
    assert_eq!(extent[1][0], 10000.0);
    let lines = stagecraft_underlay::Linework::from_json(e.doc().assets[l].data.as_slice()).unwrap();
    assert_eq!(lines.texts[0].text, "Buehne");
    // a second underlay gets a second layer
    let png = {
        let mut out = Vec::new();
        let enc = image::codecs::png::PngEncoder::new(&mut out);
        image::ImageEncoder::write_image(enc, &[0u8; 4 * 6], 3, 2, image::ExtendedColorType::Rgba8).unwrap();
        out
    };
    let out =
        run(&mut e, "underlay.import", json!({ "data_base64": b64(&png), "file_name": "photo.png", "scale": 50 }));
    let p = id(&out);
    let o = &e.doc().objects[&p];
    assert_eq!(e.doc().layers[&o.layer].name, "Underlay 2");
    let ObjectKind::Underlay { raster: Some(r), source, .. } = &o.kind else { panic!() };
    assert_eq!(r, source, "PNGs are shown as they are");
    // PDF: a one-page file made by the sheet exporter would need stagecraft-sheet; a
    // hand-written page with one line does.
    let pdf = minimal_pdf();
    let out = run(&mut e, "underlay.import", json!({ "data_base64": b64(&pdf), "file_name": "p.pdf", "scale": 1 }));
    assert_eq!(out["pages"], 1);
    let w = out["size"][0].as_f64().unwrap();
    assert!((w - 200.0 * 25.4 / 72.0).abs() < 1e-6, "{out}");
    let err =
        e.execute("underlay.import", json!({ "data_base64": b64(&pdf), "file_name": "p.pdf", "page": 3 })).unwrap_err();
    assert!(err.message.contains("page 3"), "{err}");
    let err = e.execute("underlay.import", json!({ "data_base64": "AAAA", "file_name": "x.dwg" })).unwrap_err();
    assert!(err.message.contains(".dxf"), "{err}");
}

/// A valid one-page PDF, 200 × 100 pt, with one stroked line.
fn minimal_pdf() -> Vec<u8> {
    let content = "0 0 0 RG 10 50 m 190 50 l S";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).bytes());
    out
}

#[test]
fn notes_and_round_trip() {
    let mut e = Engine::new();
    let u = id(&run(&mut e, "underlay.import", json!({ "data_base64": b64(SVG.as_bytes()), "file_name": "a.svg" })));
    let n = id(&run(
        &mut e,
        "note.create",
        json!({ "text": "A1: front area 2 m deep\nassumed", "at": [1000, 2000], "frame": true, "leader": [1000, 0] }),
    ));
    let ObjectKind::Note { leader, height, .. } = &e.doc().objects[&n].kind else { panic!() };
    assert_eq!(*leader, Some([0.0, -2000.0]));
    assert_eq!(*height, 250.0);
    assert_eq!(e.doc().objects[&n].name, "A1: front area 2 m deep");
    run(&mut e, "note.update", json!({ "id": n, "text": "changed", "no_leader": true }));
    let ObjectKind::Note { leader, text, .. } = &e.doc().objects[&n].kind else { panic!() };
    assert_eq!((leader, text.as_str()), (&None, "changed"));
    assert!(e.execute("note.create", json!({ "text": " ", "at": [0, 0] })).is_err());

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("u.stagecraft");
    run(&mut e, "doc.save", json!({ "path": path }));
    let back = Engine::open(&path).unwrap();
    assert_eq!(back.doc().objects[&u], e.doc().objects[&u]);
    let ObjectKind::Underlay { raster: Some(r), .. } = &back.doc().objects[&u].kind else { panic!() };
    assert_eq!(back.doc().assets[r].data, e.doc().assets[r].data);

    // MVR has no place for annotations
    let mvr = dir.path().join("u.mvr");
    let out = run(&mut e, "mvr.export", json!({ "path": mvr }));
    assert!(!out.to_string().contains("SceneObject"), "{out}");
}
