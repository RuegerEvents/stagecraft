//! Sheets, viewports, label templates, title blocks: commands and undo.

use serde_json::{Value, json};
use stagecraft_core::sheet::{LabelRotation, Orientation, Paper, Side, TitleBlock, ViewKind};
use stagecraft_core::units::{DimFormat, Language, LengthFormat};
use stagecraft_core::{Engine, Id};

fn run(e: &mut Engine, name: &str, args: Value) -> Value {
    e.execute(name, args).unwrap_or_else(|err| panic!("{name} failed: {err}"))
}

fn id(v: &Value) -> Id {
    v["id"].as_str().expect("id").parse().expect("uuid")
}

#[test]
fn sheets_are_created_numbered_and_undone() {
    let mut e = Engine::new();
    let a = id(&run(&mut e, "sheet.create", json!({ "name": "Lighting plan" })));
    let b = id(&run(&mut e, "sheet.create", json!({ "paper": "a1", "orientation": "portrait", "number": "L-2" })));
    let doc = e.doc();
    assert_eq!(doc.sheets[&a].number, "1");
    assert_eq!(doc.sheets[&a].paper, Paper::A3);
    assert_eq!(doc.sheets[&a].size(), [420.0, 297.0]);
    assert_eq!(doc.sheets[&b].size(), [594.0, 841.0]);
    assert_eq!(doc.sheets[&b].orientation, Orientation::Portrait);
    assert!(e.execute("sheet.create", json!({ "number": "L-2" })).is_err(), "numbers are unique");
    assert!(e.execute("sheet.create", json!({ "paper": "custom", "width": 500 })).is_err());
    run(&mut e, "sheet.update", json!({ "sheet": "L-2", "paper": "custom", "width": 1000, "height": 700 }));
    assert_eq!(e.doc().sheets[&b].size(), [700.0, 1000.0]);
    let list = run(&mut e, "sheet.list", json!(null));
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert_eq!(list[0]["name"], "Lighting plan");
    run(&mut e, "history.undo", json!(null));
    assert_eq!(e.doc().sheets[&b].paper, Paper::A1);
    run(&mut e, "sheet.delete", json!({ "sheet": "Lighting plan" }));
    assert!(!e.doc().sheets.contains_key(&a));
    run(&mut e, "history.undo", json!(null));
    assert!(e.doc().sheets.contains_key(&a));
}

#[test]
fn viewports_take_views_scales_crops_and_tags() {
    let mut e = Engine::new();
    run(&mut e, "tag.create", json!({ "name": "house" }));
    let s = id(&run(&mut e, "sheet.create", json!({})));
    let lt = id(&run(&mut e, "label.create", json!({ "name": "Electrics" })));
    let plan = id(&run(
        &mut e,
        "viewport.create",
        json!({ "sheet": s, "view": "plan", "scale": 50, "hidden_tags": ["house"], "label_template": "Electrics",
                "crop": [[5000, 3000], [-5000, -1000]], "line_weights": { "thick": 0.7 } }),
    ));
    let front = id(&run(&mut e, "viewport.create", json!({ "sheet": s, "view": "front", "scale": 100 })));
    let cut = id(&run(
        &mut e,
        "viewport.create",
        json!({ "sheet": s, "view": { "type": "section", "origin": [0, 4000, 0], "normal": [0, -1, 0] } }),
    ));
    let doc = e.doc();
    let sh = &doc.sheets[&s];
    let v = sh.viewport(plan).unwrap();
    assert_eq!(v.view, ViewKind::Plan);
    assert_eq!(v.crop, Some([[-5000.0, -1000.0], [5000.0, 3000.0]]), "corners are normalised");
    assert_eq!(v.label_template, Some(lt));
    assert_eq!(v.hidden_tags.len(), 1);
    assert_eq!(v.line_weights["thick"], 0.7);
    assert_eq!(sh.viewport(front).unwrap().view, ViewKind::Elevation { side: Side::Front });
    assert!(matches!(sh.viewport(cut).unwrap().view, ViewKind::Section { .. }));
    assert_eq!(sh.viewport(front).unwrap().name, "Front elevation");

    assert!(e.execute("viewport.create", json!({ "sheet": s, "view": "nope" })).is_err());
    assert!(e.execute("viewport.create", json!({ "sheet": s, "view": "plan", "scale": 0 })).is_err());
    assert!(e.execute("viewport.update", json!({ "viewport": plan, "line_weights": { "fat": 1 } })).is_err());

    run(&mut e, "viewport.update", json!({ "viewport": plan, "by": [10, -5], "scale": 20, "fit": true }));
    let v = e.doc().sheets[&s].viewport(plan).unwrap().clone();
    assert_eq!(v.scale, 20.0);
    assert_eq!(v.crop, None);
    run(&mut e, "viewport.update", json!({ "viewport": plan, "to_front": true }));
    assert_eq!(e.doc().sheets[&s].viewports.last().unwrap().id, plan);

    // Deleting the tag and the template clears them from the viewport.
    run(&mut e, "tag.delete", json!({ "tag": "house" }));
    run(&mut e, "label.delete", json!({ "template": "Electrics" }));
    let v = e.doc().sheets[&s].viewport(plan).unwrap().clone();
    assert!(v.hidden_tags.is_empty() && v.label_template.is_none());
    run(&mut e, "viewport.delete", json!({ "viewport": front }));
    assert_eq!(e.doc().sheets[&s].viewports.len(), 2);
}

#[test]
fn labels_templates_and_manual_offsets() {
    let mut e = Engine::new();
    let fx = id(&run(&mut e, "fixture.create", json!({ "position": [0, 0, 6000], "fid": 1 })));
    run(&mut e, "object.set_attrs", json!({ "ids": [fx], "attrs": { "gel": "L201", "circuit": "12" } }));
    assert_eq!(e.doc().objects[&fx].attrs["gel"], "L201");
    run(&mut e, "object.set_attrs", json!({ "ids": [fx], "attrs": { "gel": "" } }));
    assert!(!e.doc().objects[&fx].attrs.contains_key("gel"));

    let t = id(&run(
        &mut e,
        "label.create",
        json!({ "name": "Focus", "rotation": "follow",
                "fields": [{ "field": "fid", "boxed": true }, { "field": "attr:focus", "anchor": [0, 300], "size": 1.5 }] }),
    ));
    assert_eq!(e.doc().label_templates[&t].rotation, LabelRotation::Follow);
    assert_eq!(e.doc().label_templates[&t].fields[0].size, 2.0, "default size");
    assert!(e.execute("label.create", json!({ "name": "Bad", "fields": [{ "field": "colour_temp" }] })).is_err());
    assert!(e.execute("label.create", json!({ "name": "Focus" })).is_err(), "names are unique");
    run(&mut e, "label.update", json!({ "template": "Focus", "rotation": "sheet_fixed" }));
    assert_eq!(e.doc().label_templates[&t].rotation, LabelRotation::SheetFixed);
    let d = id(&run(&mut e, "label.create", json!({ "name": "Default" })));
    assert_eq!(e.doc().label_templates[&d].fields.len(), 3);

    let s = id(&run(&mut e, "sheet.create", json!({})));
    let vp = id(&run(&mut e, "viewport.create", json!({ "sheet": s, "view": "plan", "label_template": t })));
    run(&mut e, "label.move", json!({ "viewport": vp, "fixture": fx, "offset": [5, 2] }));
    run(&mut e, "label.move", json!({ "viewport": vp, "fixture": fx, "offset": [1, 1], "relative": true }));
    assert_eq!(e.doc().sheets[&s].viewport(vp).unwrap().label_offsets[&fx], [6.0, 3.0]);
    run(&mut e, "label.move", json!({ "viewport": vp, "fixture": fx }));
    assert!(e.doc().sheets[&s].viewport(vp).unwrap().label_offsets.is_empty());
    run(&mut e, "history.undo", json!(null));
    assert_eq!(e.doc().sheets[&s].viewport(vp).unwrap().label_offsets[&fx], [6.0, 3.0]);
}

#[test]
fn title_blocks_and_meta() {
    let mut e = Engine::new();
    let list = run(&mut e, "titleblock.list", json!(null));
    assert_eq!(list[0]["name"], "Compact");
    assert_eq!(list[1]["name"], "Generic");
    let tb = id(&run(&mut e, "titleblock.create", json!({ "name": "Tour", "height": 40 })));
    assert_eq!(e.doc().title_blocks[&tb].height, 40.0);
    assert!(!e.doc().title_blocks[&tb].items.is_empty(), "copied from the compact block");
    let s = id(&run(&mut e, "sheet.create", json!({ "title_block": "Tour" })));
    assert_eq!(e.doc().sheets[&s].title_block, Some(tb));
    run(&mut e, "sheet.update", json!({ "sheet": s, "title_block": "none" }));
    assert!(!e.doc().sheets[&s].show_title_block);
    run(&mut e, "sheet.update", json!({ "sheet": s, "title_block": "Tour" }));
    run(&mut e, "titleblock.delete", json!({ "template": "Tour" }));
    assert_eq!(e.doc().sheets[&s].title_block, None);

    run(&mut e, "doc.set_meta", json!({ "company": "Example Co", "date": "2026-11-18" }));
    assert_eq!(e.doc().meta.company, "Example Co");
    assert!(e.execute("doc.set_meta", json!({ "logo_file": "/no/such/file.png" })).is_err());

    // The built-ins: generic by name, compact as the default; neither can change.
    run(&mut e, "sheet.update", json!({ "sheet": s, "title_block": "generic" }));
    assert_eq!(e.doc().sheets[&s].title_block, Some(TitleBlock::GENERIC_ID));
    assert_eq!(TitleBlock::for_sheet(e.doc(), &e.doc().sheets[&s]).name, "Generic");
    run(&mut e, "sheet.update", json!({ "sheet": s, "title_block": "Compact" }));
    assert_eq!(e.doc().sheets[&s].title_block, None);
    assert!(TitleBlock::for_sheet(e.doc(), &e.doc().sheets[&s]).zones);
    assert!(e.execute("titleblock.update", json!({ "template": "Generic", "width": 100 })).is_err());
    assert!(e.execute("titleblock.delete", json!({ "template": "Compact" })).is_err());
    assert!(e.execute("titleblock.create", json!({ "name": "compact" })).is_err());
    let g = id(&run(&mut e, "titleblock.create", json!({ "name": "Mine", "from": "generic" })));
    assert!(!e.doc().title_blocks[&g].zones);
}

#[test]
fn sheet_language_and_dimension_format() {
    let mut e = Engine::new();
    let s = id(&run(&mut e, "sheet.create", json!({})));
    run(&mut e, "viewport.create", json!({ "sheet": s, "view": "plan" }));
    run(&mut e, "viewport.create", json!({ "sheet": s, "view": "front", "name": "My front" }));
    assert_eq!(e.doc().meta.sheet_language, Language::En);
    let r = run(
        &mut e,
        "doc.set_meta",
        json!({ "sheet_language": "de", "venue_address": "Street 1\nTown", "production_dates": "1.–3. Mai",
                "website": "example.org" }),
    );
    assert_eq!(r["sheet_language"], "de");
    let names: Vec<_> = e.doc().sheets[&s].viewports.iter().map(|v| v.name.clone()).collect();
    assert_eq!(names, ["Grundriss", "My front"], "default names follow the language, own names stay");
    let f = LengthFormat::of(&e.doc().meta);
    assert_eq!(f.length(6155.0), "6,15⁵", "German defaults to a comma");
    run(&mut e, "viewport.create", json!({ "sheet": s, "view": "section_y" }));
    assert!(e.doc().sheets[&s].viewports.iter().any(|v| v.name == "Schnitt"));
    run(&mut e, "doc.set_meta", json!({ "decimal_separator": ".", "dimension_format": "m" }));
    assert_eq!(LengthFormat::of(&e.doc().meta).length(6155.0), "6.155 m");
    assert!(e.execute("doc.set_meta", json!({ "decimal_separator": ";" })).is_err());
    assert!(e.execute("doc.set_meta", json!({ "dimension_format": "feet" })).is_err());
    run(&mut e, "history.undo", json!({}));
    assert_eq!(e.doc().meta.dimension_format, DimFormat::MSupMm);
    assert_eq!(e.doc().meta.website, "example.org");
}

#[test]
fn sheets_survive_save_and_load() {
    let mut e = Engine::new();
    let s = id(&run(&mut e, "sheet.create", json!({ "name": "A" })));
    run(&mut e, "label.create", json!({ "name": "L" }));
    run(&mut e, "viewport.create", json!({ "sheet": s, "view": "left", "label_template": "L" }));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.stagecraft");
    run(&mut e, "doc.save", json!({ "path": path }));
    let back = Engine::open(&path).unwrap();
    assert_eq!(back.doc(), e.doc());
}

#[test]
fn export_without_an_exporter_says_so() {
    let mut e = Engine::new();
    assert!(e.execute("sheet.export_pdf", json!({ "path": "/tmp/x.pdf" })).is_err(), "no sheets");
    run(&mut e, "sheet.create", json!({}));
    let err = e.execute("sheet.export_pdf", json!({ "path": "/tmp/x.pdf" })).unwrap_err();
    assert!(err.message.contains("exporter"), "{err}");
}
