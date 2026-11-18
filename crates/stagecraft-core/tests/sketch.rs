//! Layer sketches through commands: walls and floors follow, conflicts roll back,
//! undo works, free geometry can be adopted.

use serde_json::{Value, json};
use stagecraft_core::{Engine, Id, ObjectKind};

fn run(e: &mut Engine, name: &str, args: Value) -> Value {
    e.execute(name, args).unwrap_or_else(|err| panic!("{name} failed: {err}"))
}

fn id(v: &Value) -> Id {
    v["id"].as_str().expect("id").parse().expect("uuid")
}

fn kind(e: &Engine, id: Id) -> ObjectKind {
    e.doc().objects[&id].kind.clone()
}

fn xs(pts: &[[f64; 2]]) -> (f64, f64) {
    let lo = pts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
    let hi = pts.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
    (lo, hi)
}

/// A 6 × 4 m room drawn like the wall tool does: four typed lengths, Shift on, closed,
/// first corner fixed. Returns (wall, floor, width dimension id).
fn room(e: &mut Engine) -> (Id, Id, u64) {
    let seg = |len: f64| json!({ "axis_lock": true, "length": len });
    let pts = json!([[0, 0], [6000, 0], [6000, 4000], [0, 4000]]);
    let out = run(
        e,
        "sketch.draw",
        json!({ "points": pts, "closed": true, "make": "wall", "thickness": 200,
                "segments": [seg(6000.0), seg(4000.0), seg(6000.0)] }),
    );
    let wall = id(&out);
    let edges = out["edges"].clone();
    assert_eq!(edges.as_array().unwrap().len(), 4);
    let floor = id(&run(e, "sketch.make_floor", json!({ "at": [3000, 2000] })));
    let s = run(e, "sketch.get", Value::Null);
    let first_point = s["sketch"]["points"][0]["id"].as_u64().unwrap();
    let at = s["sketch"]["points"][0]["pos"].clone();
    run(e, "sketch.add_constraint", json!({ "constraint": { "type": "fixed", "point": first_point, "at": at } }));
    let dims = run(e, "dimension.list", Value::Null);
    let width = dims.as_array().unwrap()[0]["id"].as_u64().unwrap();
    (wall, floor, width)
}

#[test]
fn drawing_with_typed_lengths_gives_a_constrained_room() {
    let mut e = Engine::new();
    let (wall, floor, _) = room(&mut e);
    let st = run(&mut e, "sketch.status", Value::Null);
    // Four lines, H/V on each, three lengths + fixed corner: fully constrained? The
    // closing segment has no length, but H/V and the opposite length pin it.
    assert_eq!(st["conflicting"].as_array().unwrap().len(), 0, "{st}");
    assert_eq!(st["dof"], 0, "{st}");
    let ObjectKind::Wall { path, thickness, .. } = kind(&e, wall) else { panic!() };
    assert_eq!(thickness, 200.0);
    assert_eq!(path.len(), 5, "closed ring repeats its first point");
    assert_eq!(xs(&path), (0.0, 6000.0));
    let ObjectKind::Floor { outline, .. } = kind(&e, floor) else { panic!() };
    assert_eq!(outline.len(), 4);
    assert!(e.doc().objects[&floor].sketch.is_some());
}

#[test]
fn changing_the_width_moves_the_attached_wall_and_floor() {
    let mut e = Engine::new();
    let (wall, floor, width) = room(&mut e);
    run(&mut e, "dimension.set_value", json!({ "id": width, "value": 8500 }));
    let ObjectKind::Wall { path, .. } = kind(&e, wall) else { panic!() };
    let (lo, hi) = xs(&path);
    assert!((lo - 0.0).abs() < 1e-6 && (hi - 8500.0).abs() < 1e-6, "{path:?}");
    let ObjectKind::Floor { outline, .. } = kind(&e, floor) else { panic!() };
    let (lo, hi) = xs(&outline);
    assert!((lo - 0.0).abs() < 1e-6 && (hi - 8500.0).abs() < 1e-6, "{outline:?}");
    // Named parameters work as ids too.
    run(&mut e, "dimension.name", json!({ "id": width, "name": "room width" }));
    run(&mut e, "dimension.set_value", json!({ "id": "room width", "value": 7000 }));
    let ObjectKind::Floor { outline, .. } = kind(&e, floor) else { panic!() };
    assert!((xs(&outline).1 - 7000.0).abs() < 1e-6);
}

#[test]
fn a_conflicting_edit_rolls_back_and_names_the_constraint() {
    let mut e = Engine::new();
    let (wall, _, width) = room(&mut e);
    let before = e.doc().clone();
    let undo_len = run(&mut e, "history.list", Value::Null)["undo"].as_array().unwrap().len();
    // The bottom edge is 6 m; a second driving dimension of 5 m on the same line
    // contradicts it.
    let s = run(&mut e, "sketch.get", Value::Null);
    let line = s["sketch"]["edges"][0]["id"].as_u64().unwrap();
    let err = e
        .execute(
            "dimension.add",
            json!({ "measure": { "type": "length", "line": line }, "value": 5000, "driving": true }),
        )
        .unwrap_err();
    assert_eq!(err.code, "conflict");
    assert!(err.message.contains("length 5000 mm"), "{}", err.message);
    assert!(err.message.contains(&format!("dimension {width}")), "names the other one too: {}", err.message);
    assert_eq!(e.doc(), &before, "rolled back");
    assert_eq!(run(&mut e, "history.list", Value::Null)["undo"].as_array().unwrap().len(), undo_len);
    // A horizontal constraint on a vertical edge conflicts too.
    let pts = &s["sketch"]["edges"][1];
    let (a, b) = (pts["geom"]["a"].clone(), pts["geom"]["b"].clone());
    let err = e
        .execute("sketch.add_constraint", json!({ "constraint": { "type": "horizontal", "a": a, "b": b } }))
        .unwrap_err();
    assert_eq!(err.code, "conflict");
    assert!(err.message.contains("(horizontal)"), "{}", err.message);
    assert_eq!(e.doc(), &before);
    let _ = wall;
}

#[test]
fn auto_driving_dimension_becomes_reference_when_redundant() {
    let mut e = Engine::new();
    room(&mut e);
    let s = run(&mut e, "sketch.get", Value::Null);
    let line = s["sketch"]["edges"][0]["id"].as_u64().unwrap();
    let out = run(&mut e, "dimension.add", json!({ "measure": { "type": "length", "line": line } }));
    assert_eq!(out["driving"], false, "fully constrained already: {out}");
    assert!((out["value"].as_f64().unwrap() - 6000.0).abs() < 1e-6);
}

#[test]
fn undo_and_redo_a_dimension_change() {
    let mut e = Engine::new();
    let (wall, _, width) = room(&mut e);
    let path_of = |e: &Engine| match kind(e, wall) {
        ObjectKind::Wall { path, .. } => path,
        _ => unreachable!(),
    };
    let before = path_of(&e);
    run(&mut e, "dimension.set_value", json!({ "id": width, "value": 9000 }));
    let after = path_of(&e);
    assert_ne!(before, after);
    run(&mut e, "history.undo", Value::Null);
    assert_eq!(path_of(&e), before);
    let v = run(&mut e, "dimension.list", Value::Null)[0]["value"].as_f64().unwrap();
    assert!((v - 6000.0).abs() < 1e-6);
    run(&mut e, "history.redo", Value::Null);
    assert_eq!(path_of(&e), after);
}

#[test]
fn moving_a_linked_wall_drags_its_points() {
    let mut e = Engine::new();
    let out = run(&mut e, "sketch.draw", json!({ "points": [[0, 0], [3000, 0]], "make": "wall" }));
    let wall = id(&out);
    run(&mut e, "object.move", json!({ "ids": [wall], "delta": [0, 1000, 0] }));
    let ObjectKind::Wall { path, .. } = kind(&e, wall) else { panic!() };
    assert!((path[0][1] - 1000.0).abs() < 1e-6 && (path[1][1] - 1000.0).abs() < 1e-6, "{path:?}");
    let p = &e.doc().objects[&wall].placement;
    assert_eq!((p.position[0], p.position[1]), (0.0, 0.0), "placement stays at the origin");
    // set_placement keeps only the height.
    run(&mut e, "object.set_placement", json!({ "id": wall, "position": [500, 500, 250] }));
    assert_eq!(e.doc().objects[&wall].placement.position, [0.0, 0.0, 250.0]);
}

#[test]
fn moving_a_linked_floor_up_changes_its_height_only() {
    let mut e = Engine::new();
    let pts = json!([[0, 0], [4000, 0], [4000, 3000], [0, 3000]]);
    let floor = id(&run(&mut e, "sketch.draw", json!({ "points": pts, "make": "floor", "thickness": 200 })));
    let before = kind(&e, floor);
    // What the inspector's height fields send: a vertical move.
    run(&mut e, "object.move", json!({ "ids": [floor], "delta": [0, 0, 1000] }));
    assert_eq!(e.doc().objects[&floor].placement.position, [0.0, 0.0, 1000.0]);
    assert_eq!(kind(&e, floor), before, "the outline stays");
    // A diagonal move does both: points dragged, height raised.
    run(&mut e, "object.move", json!({ "ids": [floor], "delta": [500, 0, -200] }));
    assert_eq!(e.doc().objects[&floor].placement.position[2], 800.0);
    let ObjectKind::Floor { outline, .. } = kind(&e, floor) else { panic!() };
    assert!((xs(&outline).0 - 500.0).abs() < 1e-6, "{outline:?}");
}

#[test]
fn free_walls_and_floors_can_be_adopted() {
    let mut e = Engine::new();
    let w = id(&run(&mut e, "wall.create", json!({ "path": [[0, 0], [5000, 0], [5000, 3000]] })));
    let f = id(&run(&mut e, "floor.create", json!({ "outline": [[0, 0], [5000, 0], [5000, 3000], [0, 3000]] })));
    // A moved and turned free wall lands where it is in the world.
    let w2 = id(&run(&mut e, "wall.create", json!({ "path": [[0, 0], [1000, 0]] })));
    run(&mut e, "object.set_placement", json!({ "id": w2, "position": [0, 3000, 0], "rotation": [0, 0, 90] }));
    let out = run(&mut e, "sketch.adopt", json!({ "ids": [w, f, w2] }));
    assert_eq!(out["adopted"].as_array().unwrap().len(), 3);
    let s = run(&mut e, "sketch.get", Value::Null);
    // Wall corners (3) + floor's extra corner (0,3000) + w2's far end (0, 4000) = 5.
    assert_eq!(s["sketch"]["points"].as_array().unwrap().len(), 5, "{}", s["sketch"]);
    // The floor reuses the wall's two lines: 2 + 2 + 1 (w2).
    assert_eq!(s["sketch"]["edges"].as_array().unwrap().len(), 5);
    let ObjectKind::Wall { path, .. } = kind(&e, w2) else { panic!() };
    assert!((path[0][0]).abs() < 1e-6 && (path[0][1] - 3000.0).abs() < 1e-6, "{path:?}");
    assert!((path[1][1] - 4000.0).abs() < 1e-6, "{path:?}");
    // Dragging the shared corner moves the wall and the floor.
    let corner = s["sketch"]["points"][1]["id"].as_u64().unwrap();
    run(&mut e, "sketch.drag", json!({ "targets": [{ "point": corner, "to": { "x": 6000, "y": 0 } }] }));
    let ObjectKind::Wall { path, .. } = kind(&e, w) else { panic!() };
    let ObjectKind::Floor { outline, .. } = kind(&e, f) else { panic!() };
    assert!(path.iter().any(|p| (p[0] - 6000.0).abs() < 1e-3), "{path:?}");
    assert!(outline.iter().any(|p| (p[0] - 6000.0).abs() < 1e-3), "{outline:?}");
}

#[test]
fn removing_an_edge_unlinks_but_keeps_geometry() {
    let mut e = Engine::new();
    let out = run(&mut e, "sketch.draw", json!({ "points": [[0, 0], [3000, 0], [3000, 2000]], "make": "wall" }));
    let wall = id(&out);
    let edge = out["edges"][0].as_u64().unwrap();
    let before = kind(&e, wall);
    let r = run(&mut e, "sketch.remove", json!({ "items": [{ "type": "edge", "id": edge }] }));
    assert_eq!(r["unlinked"].as_array().unwrap().len(), 1);
    assert_eq!(kind(&e, wall), before);
    assert!(e.doc().objects[&wall].sketch.is_none());
}

#[test]
fn reference_dimensions_measure_between_objects_live() {
    let mut e = Engine::new();
    let portal = id(&run(&mut e, "proscenium.create", json!({ "position": [0, 0, 0], "width": 10000 })));
    let truss = id(&run(&mut e, "truss.create", json!({ "start": [-4000, 3000, 6000], "end": [4000, 3000, 6000] })));
    let out = run(
        &mut e,
        "dimension.add_reference",
        json!({ "from": { "type": "object", "object": portal }, "to": { "type": "object", "object": truss, "local": [4000, 0, 0] },
                "axis": "vertical", "name": "truss to portal" }),
    );
    assert!((out["value"].as_f64().unwrap() - 3000.0).abs() < 1e-6);
    run(&mut e, "object.move", json!({ "ids": [truss], "delta": [0, 1500, 0] }));
    let list = run(&mut e, "dimension.list", Value::Null);
    let d = list.as_array().unwrap().iter().find(|d| d["name"] == "truss to portal").unwrap();
    assert!((d["value"].as_f64().unwrap() - 4500.0).abs() < 1e-6, "{d}");
    assert!(e.execute("dimension.set_value", json!({ "id": "truss to portal", "value": 1 })).is_err());
    // Deleting the truss deletes the dimension.
    run(&mut e, "object.delete", json!({ "ids": [truss] }));
    assert!(run(&mut e, "dimension.list", Value::Null).as_array().unwrap().is_empty());
}

#[test]
fn sketches_survive_save_and_open_exactly() {
    let mut e = Engine::new();
    let (_, _, width) = room(&mut e);
    // Non-round numbers so the float round trip matters.
    run(&mut e, "dimension.set_value", json!({ "id": width, "value": 6123.456789 }));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("room.stagecraft");
    run(&mut e, "doc.save", json!({ "path": path }));
    let saved = e.doc().clone();
    let mut e2 = Engine::new();
    run(&mut e2, "doc.open", json!({ "path": path }));
    assert_eq!(e2.doc(), &saved);
}

#[test]
fn deleting_a_layer_forgets_its_sketch() {
    let mut e = Engine::new();
    run(&mut e, "layer.create", json!({ "name": "Upper", "elevation": 3000 }));
    run(&mut e, "sketch.draw", json!({ "layer": "Upper", "points": [[0, 0], [3000, 0]], "make": "wall" }));
    run(&mut e, "layer.delete", json!({ "layer": "Upper", "move_objects_to": "Stage" }));
    assert!(e.doc().sketches.is_empty());
    assert!(e.doc().objects.values().all(|o| o.sketch.is_none()));
}

#[test]
fn documents_without_sketches_still_load() {
    let mut e = Engine::new();
    run(&mut e, "wall.create", json!({ "path": [[0, 0], [1000, 0]] }));
    let mut v = serde_json::to_value(e.doc()).unwrap();
    let m = v.as_object_mut().unwrap();
    m.remove("sketches");
    m.remove("dimensions");
    m.insert("format".into(), json!(2));
    let doc: stagecraft_core::Document = serde_json::from_value(v).unwrap();
    assert!(doc.sketches.is_empty() && doc.dimensions.is_empty());
    assert!(doc.objects.values().all(|o| o.sketch.is_none()));
}

#[test]
fn the_first_point_of_a_sketch_is_locked() {
    let mut e = Engine::new();
    let seg = json!({ "axis_lock": true, "length": 4000 });
    let out =
        run(&mut e, "sketch.draw", json!({ "points": [[1000, 0], [5000, 0]], "segments": [seg], "make": "wall" }));
    let wall = id(&out);
    let s = run(&mut e, "sketch.get", Value::Null);
    let fixed: Vec<&Value> =
        s["sketch"]["constraints"].as_array().unwrap().iter().filter(|c| c["kind"]["type"] == "fixed").collect();
    assert_eq!(fixed.len(), 1, "{s}");
    assert_eq!(fixed[0]["kind"]["point"], s["sketch"]["points"][0]["id"]);
    assert_eq!(s["status"]["fully_constrained"], true);
    // a longer wall grows away from the locked start
    let dim = s["sketch"]["dimensions"][0]["id"].as_u64().unwrap();
    run(&mut e, "dimension.set_value", json!({ "id": dim, "value": 6000 }));
    let ObjectKind::Wall { path, .. } = kind(&e, wall) else { panic!() };
    assert_eq!(xs(&path), (1000.0, 7000.0));
    // moving the wall takes its lock along
    run(&mut e, "object.move", json!({ "ids": [wall], "delta": [500, 0, 0] }));
    let ObjectKind::Wall { path, .. } = kind(&e, wall) else { panic!() };
    let (lo, hi) = xs(&path);
    assert!((lo - 1500.0).abs() < 1e-6 && (hi - 7500.0).abs() < 1e-6, "{path:?}");
    // a second drawing does not add another lock
    run(&mut e, "sketch.draw", json!({ "points": [[0, 3000], [2000, 3000]] }));
    let s = run(&mut e, "sketch.get", Value::Null);
    let n = s["sketch"]["constraints"].as_array().unwrap().iter().filter(|c| c["kind"]["type"] == "fixed").count();
    assert_eq!(n, 1);
}

#[test]
fn portal_batten_curtain_and_truss_follow_sketch_lines() {
    let mut e = Engine::new();
    // a portal line with a driving width, symmetric about a fixed centre line
    let o = run(&mut e, "sketch.add_point", json!({ "at": [0, 0] }))["id"].as_u64().unwrap();
    let axis = run(&mut e, "sketch.add_line", json!({ "from": o, "to": [0, 5000], "construction": true }));
    let top = axis["points"][1].as_u64().unwrap();
    run(&mut e, "sketch.add_constraint", json!({ "constraint": { "type": "vertical", "a": o, "b": top } }));
    let portal = run(&mut e, "sketch.add_line", json!({ "from": [-5000, 0], "to": [5000, 0] }));
    let (pl, pr) = (portal["points"][0].as_u64().unwrap(), portal["points"][1].as_u64().unwrap());
    let ax = axis["id"].as_u64().unwrap();
    run(
        &mut e,
        "sketch.add_constraint",
        json!({ "constraint": { "type": "symmetric", "a": pl, "b": pr, "axis": ax } }),
    );
    run(&mut e, "sketch.add_constraint", json!({ "constraint": { "type": "horizontal", "a": o, "b": pl } }));
    let w = run(
        &mut e,
        "dimension.add",
        json!({ "measure": { "type": "horizontal_distance", "a": pl, "b": pr }, "name": "portal width" }),
    );
    assert!(w["id"].is_number(), "{w}");
    // a batten line 1 m wider each side, 2 m upstage
    let bat =
        run(&mut e, "sketch.add_line", json!({ "from": [-6000, 2000], "to": [6000, 2000], "construction": true }));
    let (bl, br) = (bat["points"][0].as_u64().unwrap(), bat["points"][1].as_u64().unwrap());
    for c in [
        json!({ "type": "symmetric", "a": bl, "b": br, "axis": ax }),
        json!({ "type": "horizontal", "a": bl, "b": br }),
    ] {
        run(&mut e, "sketch.add_constraint", json!({ "constraint": c }));
    }
    run(
        &mut e,
        "dimension.add",
        json!({ "measure": { "type": "horizontal_distance", "a": bl, "b": pl }, "value": 1000 }),
    );
    run(
        &mut e,
        "dimension.add",
        json!({ "measure": { "type": "vertical_distance", "a": pl, "b": bl }, "value": 2000 }),
    );

    let pro = id(&run(&mut e, "proscenium.create", json!({ "width": 3000, "height": 6000 })));
    let batten = id(&run(&mut e, "batten.create", json!({ "y": 9999, "trim": 7500 })));
    let curtain = id(&run(&mut e, "curtain.create", json!({ "position": [0, 0, 6500] })));
    let truss = id(&run(&mut e, "truss.create", json!({ "start": [0, 0, 5000], "end": [1000, 0, 5000] })));
    let pe = portal["id"].as_u64().unwrap();
    run(&mut e, "sketch.link", json!({ "ids": [pro, curtain], "edge": pe }));
    run(&mut e, "sketch.link", json!({ "ids": [batten, truss], "edge": bat["id"] }));
    let check = |e: &Engine, width: f64| {
        let ObjectKind::Proscenium { width: pw, .. } = kind(e, pro) else { panic!() };
        assert!((pw - width).abs() < 1e-6, "portal {pw}");
        let ObjectKind::Curtain { width: cw, .. } = kind(e, curtain) else { panic!() };
        assert!((cw - width).abs() < 1e-6);
        let ObjectKind::Batten { length, .. } = kind(e, batten) else { panic!() };
        assert!((length - width - 2000.0).abs() < 1e-6, "batten {length}");
        let p = e.doc().objects[&batten].placement;
        assert!(
            p.position[0].abs() < 1e-6 && (p.position[1] - 2000.0).abs() < 1e-6 && p.position[2] == 7500.0,
            "{p:?}"
        );
        let t = e.doc().objects[&truss].placement;
        assert!((t.position[0] + width / 2.0 + 1000.0).abs() < 1e-6 && t.position[2] == 5000.0, "{t:?}");
        assert_eq!(e.doc().objects[&curtain].placement.position[2], 6500.0);
    };
    check(&e, 10000.0);
    run(&mut e, "dimension.set_value", json!({ "id": "portal width", "value": 13000 }));
    check(&e, 13000.0);
    // a width typed on the portal itself does not stick: the line wins
    run(&mut e, "proscenium.update", json!({ "id": pro, "width": 4000, "height": 5000 }));
    let ObjectKind::Proscenium { width, height, .. } = kind(&e, pro) else { panic!() };
    assert_eq!((width, height), (13000.0, 5000.0));
    // only lines; only these kinds
    let wall = id(&run(&mut e, "wall.create", json!({ "path": [[0, 0], [1, 0]] })));
    assert!(e.execute("sketch.link", json!({ "ids": [wall], "edge": pe })).is_err());
    run(&mut e, "history.undo", Value::Null);
    run(&mut e, "history.undo", Value::Null);
    check(&e, 13000.0);
}
