//! The project unit: `doc.set_units`, files from before it, and commands that keep
//! taking mm whatever the unit.

use serde_json::{Value, json};
use stagecraft_core::model::{Document, FORMAT_VERSION};
use stagecraft_core::units::{DimFormat, LengthFormat, LengthUnit, Units};
use stagecraft_core::{Engine, Id, ObjectKind};

fn run(e: &mut Engine, name: &str, args: Value) -> Value {
    e.execute(name, args).unwrap_or_else(|err| panic!("{name} failed: {err}"))
}

fn id(v: &Value) -> Id {
    v["id"].as_str().expect("id").parse().expect("uuid")
}

#[test]
fn new_documents_are_mm_and_sheets_follow_the_unit() {
    let e = Engine::new();
    let m = &e.doc().meta;
    assert_eq!(m.units.length, LengthUnit::Mm);
    assert_eq!(m.dimension_format, DimFormat::Project);
    assert_eq!(Units::of(m).field(6155.0), "6155");
    assert_eq!(LengthFormat::of(m).length(6155.0), "6155");
}

#[test]
fn set_units_changes_display_only_and_undoes() {
    let mut e = Engine::new();
    let r = run(&mut e, "doc.set_units", json!({ "length_unit": "m" }));
    assert_eq!(r["precision"], 3);
    assert_eq!(r["example"], "6.155 m");
    assert_eq!(LengthFormat::of(&e.doc().meta).length(6155.0), "6.155", "sheets follow");
    run(&mut e, "doc.set_units", json!({ "precision": 2 }));
    assert_eq!(Units::of(&e.doc().meta).fixed(6155.0), "6.16");
    // A new unit without a precision takes that unit's default.
    run(&mut e, "doc.set_units", json!({ "length_unit": "ft_in" }));
    assert_eq!(e.doc().meta.units.precision, None);
    assert_eq!(Units::of(&e.doc().meta).field(6155.0), "20' 2 5/16\"");
    assert!(e.execute("doc.set_units", json!({ "precision": 3 })).is_err(), "1/3 inch is no precision");
    assert!(e.execute("doc.set_units", json!({ "length_unit": "yards" })).is_err());
    run(&mut e, "history.undo", json!({}));
    run(&mut e, "history.undo", json!({}));
    assert_eq!(e.doc().meta.units.length, LengthUnit::M);
    assert_eq!(e.doc().meta.units.precision, None);

    // A document that chose a sheet format keeps it.
    run(&mut e, "doc.set_meta", json!({ "dimension_format": "m_sup_mm" }));
    assert_eq!(LengthFormat::of(&e.doc().meta).length(6155.0), "6.15⁵");
}

#[test]
fn commands_take_mm_whatever_the_project_unit() {
    for unit in ["mm", "m", "cm", "ft_in"] {
        let mut e = Engine::new();
        run(&mut e, "doc.set_units", json!({ "length_unit": unit }));
        let p = id(&run(&mut e, "platform.create", json!({ "width": 2000, "depth": 1000, "height": 400 })));
        let o = &e.doc().objects[&p];
        assert!(
            matches!(o.kind, ObjectKind::Platform { width: 2000.0, depth: 1000.0, height: 400.0, .. }),
            "{unit}: {:?}",
            o.kind
        );
        run(&mut e, "object.move", json!({ "ids": [p], "delta": [1500, 0, 0] }));
        assert_eq!(e.doc().objects[&p].placement.position[0], 1500.0, "{unit}");
        let l = run(&mut e, "layer.create", json!({ "name": "House", "elevation": -1000 }));
        let l: Id = l["id"].as_str().unwrap().parse().unwrap();
        assert_eq!(e.doc().layers[&l].elevation, -1000.0, "{unit}");
    }
}

#[test]
fn files_before_format_5_keep_their_look() {
    // A format 4 document: no `units`, and the dimension format it stored (or none).
    let doc = Document::new();
    let mut v = serde_json::to_value(&doc).unwrap();
    v["format"] = json!(4);
    v["meta"].as_object_mut().unwrap().remove("units");
    v["meta"]["dimension_format"] = json!("m");
    let old: Document = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(old.meta.units.length, LengthUnit::Mm);
    assert_eq!(old.meta.dimension_format, DimFormat::M, "an explicit format stays");
    v["meta"].as_object_mut().unwrap().remove("dimension_format");
    let older: Document = serde_json::from_value(v).unwrap();
    assert_eq!(older.meta.dimension_format, DimFormat::MSupMm, "no field: the old default");

    // Save and load keeps the unit and writes format 5.
    let mut e = Engine::new();
    run(&mut e, "doc.set_units", json!({ "length_unit": "cm", "precision": 2 }));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("u.stagecraft");
    run(&mut e, "doc.save", json!({ "path": path }));
    let mut back = Engine::new();
    run(&mut back, "doc.open", json!({ "path": path }));
    assert_eq!(back.doc().format, FORMAT_VERSION);
    assert_eq!(FORMAT_VERSION, 5);
    assert_eq!(back.doc().meta.units, e.doc().meta.units);
    assert_eq!(Units::of(&back.doc().meta).with_unit(6155.0), "615.50 cm");
}
