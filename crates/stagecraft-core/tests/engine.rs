use serde_json::{Value, json};
use stagecraft_core::{Engine, Id, ObjectKind, geom, registry};

fn run(e: &mut Engine, name: &str, args: Value) -> Value {
    e.execute(name, args).unwrap_or_else(|err| panic!("{name} failed: {err}"))
}

fn id(v: &Value) -> Id {
    v["id"].as_str().expect("id").parse().expect("uuid")
}

fn world_pos(e: &Engine, id: Id) -> [f64; 3] {
    geom::world_placement(e.doc(), id).unwrap().position
}

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
}

/// A truss at 6 m with two fixtures hung on it.
fn rig(e: &mut Engine) -> (Id, Id, Id) {
    let truss = id(&run(e, "truss.create", json!({ "start": [-4000, 2000, 6000], "end": [4000, 2000, 6000] })));
    let a = id(&run(e, "fixture.create", json!({ "parent": truss, "position": [1000, 0, -300], "fid": 101 })));
    let b = id(&run(e, "fixture.create", json!({ "parent": truss, "position": [7000, 0, -300], "fid": 102 })));
    (truss, a, b)
}

#[test]
fn every_command_has_a_schema() {
    for c in registry().entries() {
        let s = c.schema();
        assert!(s.is_object(), "{} has no schema", c.name);
    }
    assert!(registry().list().len() >= 35);
}

#[test]
fn fixtures_follow_their_truss() {
    let mut e = Engine::new();
    let (truss, a, b) = rig(&mut e);
    assert!(close(world_pos(&e, a), [-3000.0, 2000.0, 5700.0]));
    assert!(close(world_pos(&e, b), [3000.0, 2000.0, 5700.0]));

    run(&mut e, "object.move", json!({ "ids": [truss], "delta": [0, 1500, 500] }));
    assert!(close(world_pos(&e, a), [-3000.0, 3500.0, 6200.0]));

    // Turn the truss 90° about its centre: fixtures swing with it.
    run(&mut e, "object.rotate", json!({ "ids": [truss], "angle": 90, "pivot": [0, 3500, 6500] }));
    assert!(close(world_pos(&e, a), [0.0, 500.0, 6200.0]), "{:?}", world_pos(&e, a));
    assert!(close(world_pos(&e, b), [0.0, 6500.0, 6200.0]), "{:?}", world_pos(&e, b));
}

#[test]
fn moving_a_parent_and_child_together_moves_the_child_once() {
    let mut e = Engine::new();
    let (truss, a, _) = rig(&mut e);
    run(&mut e, "object.move", json!({ "ids": [a, truss], "delta": [100, 0, 0] }));
    assert!(close(world_pos(&e, a), [-2900.0, 2000.0, 5700.0]));
}

#[test]
fn layer_elevation_lifts_roots_and_their_children() {
    let mut e = Engine::new();
    run(&mut e, "layer.create", json!({ "name": "Gallery", "elevation": 4000 }));
    let t =
        id(&run(&mut e, "truss.create", json!({ "layer": "Gallery", "start": [0, 0, 1000], "end": [2000, 0, 1000] })));
    let f = id(&run(&mut e, "fixture.create", json!({ "parent": t, "position": [500, 0, 0] })));
    assert!(close(world_pos(&e, f), [500.0, 0.0, 5000.0]));
    run(&mut e, "layer.update", json!({ "layer": "Gallery", "elevation": 5000 }));
    assert!(close(world_pos(&e, f), [500.0, 0.0, 6000.0]));
}

#[test]
fn attach_and_detach_keep_world_position() {
    let mut e = Engine::new();
    let t = id(&run(&mut e, "truss.create", json!({ "start": [0, 0, 5000], "end": [0, 4000, 5000] })));
    let f = id(&run(&mut e, "fixture.create", json!({ "position": [0, 1000, 4700] })));
    run(&mut e, "object.attach", json!({ "ids": [f], "parent": t }));
    assert!(close(world_pos(&e, f), [0.0, 1000.0, 4700.0]));
    // Along the truss (which points +y) the fixture is at local x = 1000.
    let local = run(&mut e, "object.get", json!({ "id": f }))["placement"]["position"].clone();
    assert_eq!(local, json!([1000.0, 0.0, -300.0]));
    run(&mut e, "object.move", json!({ "ids": [t], "delta": [500, 0, 0] }));
    run(&mut e, "object.detach", json!({ "ids": [f] }));
    assert!(close(world_pos(&e, f), [500.0, 1000.0, 4700.0]));
}

#[test]
fn attaching_in_a_loop_is_refused() {
    let mut e = Engine::new();
    let (truss, a, _) = rig(&mut e);
    let err = e.execute("object.attach", json!({ "ids": [truss], "parent": a })).unwrap_err();
    assert_eq!(err.code, "invalid");
}

#[test]
fn deleting_a_truss_detaches_or_deletes_fixtures() {
    let mut e = Engine::new();
    let (truss, a, b) = rig(&mut e);
    run(&mut e, "object.delete", json!({ "ids": [truss] }));
    assert!(e.doc().objects[&a].parent.is_none());
    assert!(close(world_pos(&e, b), [3000.0, 2000.0, 5700.0]));

    run(&mut e, "history.undo", Value::Null);
    run(&mut e, "object.delete", json!({ "ids": [truss], "children": "delete" }));
    assert!(e.doc().objects.is_empty());
}

#[test]
fn duplicate_copies_the_subtree() {
    let mut e = Engine::new();
    let (truss, _, _) = rig(&mut e);
    let out = run(&mut e, "object.duplicate", json!({ "ids": [truss], "offset": [0, 3000, 0] }));
    let copy: Id = out["ids"][0].as_str().unwrap().parse().unwrap();
    assert_eq!(e.doc().objects.len(), 6);
    assert_eq!(e.doc().children(copy).len(), 2);
    for kid in e.doc().children(copy) {
        assert_eq!(world_pos(&e, kid)[1], 5000.0);
    }
    assert_eq!(e.doc().children(truss).len(), 2);
}

#[test]
fn undo_and_redo() {
    let mut e = Engine::new();
    let (truss, _, _) = rig(&mut e);
    run(&mut e, "object.move", json!({ "ids": [truss], "delta": [0, 0, 1000] }));
    let moved = e.doc().clone();
    run(&mut e, "history.undo", Value::Null);
    assert_ne!(e.doc(), &moved);
    run(&mut e, "history.redo", Value::Null);
    assert_eq!(e.doc(), &moved);
    run(&mut e, "history.undo", json!({ "steps": 4 }));
    assert!(e.doc().objects.is_empty());
    assert!(e.execute("history.undo", Value::Null).is_err());
}

#[test]
fn queries_and_failed_edits_leave_no_history() {
    let mut e = Engine::new();
    run(&mut e, "object.list", Value::Null);
    run(&mut e, "doc.info", Value::Null);
    assert!(!e.can_undo());
    assert!(e.execute("fixture.create", json!({ "address": "1.999" })).is_err());
    assert!(!e.can_undo());
    assert!(e.doc().objects.is_empty());
}

#[test]
fn batch_is_one_step_and_all_or_nothing() {
    let mut e = Engine::new();
    run(
        &mut e,
        "batch",
        json!({ "commands": [
            { "command": "fixture.create", "args": { "fid": 1 } },
            { "command": "fixture.create", "args": { "fid": 2 } },
        ]}),
    );
    assert_eq!(e.doc().objects.len(), 2);
    run(&mut e, "history.undo", Value::Null);
    assert_eq!(e.doc().objects.len(), 0);

    let err = e
        .execute(
            "batch",
            json!({ "commands": [
                { "command": "fixture.create", "args": { "fid": 1 } },
                { "command": "fixture.create", "args": { "fid": 1 } },
            ]}),
        )
        .unwrap_err();
    assert!(err.message.starts_with("step 1"), "{}", err.message);
    assert_eq!(e.doc().objects.len(), 0);
}

#[test]
fn fids_are_unique_and_numbering_works() {
    let mut e = Engine::new();
    let (_, a, b) = rig(&mut e);
    assert!(e.execute("fixture.update", json!({ "id": a, "fid": 102 })).is_err());
    // Swapping inside the renumbered set is fine.
    run(&mut e, "fixture.number", json!({ "ids": [b, a], "start": 101 }));
    let fid = |e: &Engine, id: Id| {
        run(&mut Engine::with_document(e.doc().clone()), "object.get", json!({ "id": id }))["kind"]["fid"].clone()
    };
    assert_eq!(fid(&e, b), json!(101));
    assert_eq!(fid(&e, a), json!(102));
    let out = run(&mut e, "fixture.update", json!({ "id": a, "address": "2.001" }));
    assert_eq!(out["address"], "2.001");
}

#[test]
fn tags_by_name() {
    let mut e = Engine::new();
    let (truss, a, b) = rig(&mut e);
    run(&mut e, "object.tag", json!({ "ids": [a, b], "tags": ["Licht", "FOH"] }));
    run(&mut e, "object.tag", json!({ "ids": [truss], "tags": ["Rigging"] }));
    assert_eq!(run(&mut e, "object.list", json!({ "tag": "Licht" })).as_array().unwrap().len(), 2);
    run(&mut e, "tag.delete", json!({ "tag": "Licht" }));
    assert_eq!(e.doc().objects[&a].tags.len(), 1);
}

#[test]
fn truss_points_from_start_to_end() {
    let mut e = Engine::new();
    let t = id(&run(&mut e, "truss.create", json!({ "start": [0, 0, 3000], "end": [3000, 4000, 3000] })));
    let f = id(&run(&mut e, "fixture.create", json!({ "parent": t, "position": [5000, 0, 0] })));
    assert!(close(world_pos(&e, f), [3000.0, 4000.0, 3000.0]));
    // A sloped truss.
    let t = id(&run(&mut e, "truss.create", json!({ "start": [0, 0, 0], "end": [0, 3000, 4000] })));
    let f = id(&run(&mut e, "fixture.create", json!({ "parent": t, "position": [5000, 0, 0] })));
    assert!(close(world_pos(&e, f), [0.0, 3000.0, 4000.0]), "{:?}", world_pos(&e, f));
}

#[test]
fn save_and_open_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plan.stagecraft");
    let mut e = Engine::new();
    rig(&mut e);
    run(&mut e, "doc.set_meta", json!({ "project": "Example Show", "venue": "Example Venue" }));
    assert!(e.execute("doc.new", Value::Null).is_err(), "unsaved changes must block doc.new");
    run(&mut e, "doc.save", json!({ "path": path }));
    assert!(!e.dirty());
    let saved = e.doc().clone();

    let mut e2 = Engine::new();
    run(&mut e2, "doc.open", json!({ "path": path }));
    assert_eq!(e2.doc(), &saved);
    assert!(!e2.can_undo());
}

#[test]
fn saving_replaces_the_file_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plan.stagecraft");
    let mut e = Engine::new();
    rig(&mut e);
    run(&mut e, "doc.save", json!({ "path": path }));
    run(&mut e, "doc.set_meta", json!({ "project": "Second" }));
    run(&mut e, "doc.save", Value::Null);
    run(&mut e, "mvr.export", json!({ "path": dir.path().join("plan.mvr") }));
    // Nothing but the two files: no staging file is left behind.
    let mut names: Vec<_> =
        std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    assert_eq!(names, ["plan.mvr", "plan.stagecraft"]);
    let mut e2 = Engine::new();
    run(&mut e2, "doc.open", json!({ "path": path }));
    assert_eq!(e2.doc().meta.project, "Second");

    // A failed save keeps the old file.
    let missing = dir.path().join("no/such/dir/plan.stagecraft");
    assert!(e.execute("doc.save", json!({ "path": missing })).is_err());
    assert!(std::fs::metadata(&path).unwrap().len() > 0);
}

#[test]
fn journal_records_changes_only() {
    let mut e = Engine::new();
    rig(&mut e);
    run(&mut e, "object.list", Value::Null);
    run(&mut e, "history.undo", Value::Null);
    let names: Vec<_> = e.journal().iter().map(|j| j.command.as_str()).collect();
    assert_eq!(names, ["truss.create", "fixture.create", "fixture.create", "history.undo"]);
}

#[test]
fn kind_updates_change_only_their_type() {
    let mut e = Engine::new();
    let w = id(&run(&mut e, "wall.create", json!({ "path": [[0, 0], [1000, 0]] })));
    let t = id(&run(&mut e, "truss.create", json!({ "start": [0, 0, 0], "end": [2000, 0, 0] })));
    let f = id(&run(&mut e, "floor.create", json!({ "outline": [[0, 0], [1, 0], [1, 1]] })));
    run(&mut e, "wall.update", json!({ "id": w, "height": 2500, "thickness": 100 }));
    run(&mut e, "truss.update", json!({ "id": t, "profile": "F44P", "length": 3000 }));
    run(&mut e, "floor.update", json!({ "id": f, "thickness": 40 }));
    assert_eq!(
        e.doc().objects[&w].kind,
        ObjectKind::Wall { path: vec![[0.0, 0.0], [1000.0, 0.0]], height: 2500.0, thickness: 100.0 }
    );
    assert_eq!(e.doc().objects[&t].kind, ObjectKind::Truss { length: 3000.0, profile: "F44P".into() });
    assert!(matches!(e.doc().objects[&f].kind, ObjectKind::Floor { thickness: 40.0, .. }));
    let err = e.execute("wall.update", json!({ "id": t, "height": 1 })).unwrap_err();
    assert_eq!(err.code, "invalid");
    let err = e.execute("truss.update", json!({ "id": t, "length": -1 })).unwrap_err();
    assert_eq!(err.code, "invalid_args");
}

// ---------------------------------------------------------------- block 4: stage objects

fn kind(e: &Engine, id: Id) -> ObjectKind {
    e.doc().objects[&id].kind.clone()
}

#[test]
fn attached_things_follow_the_batten_trim() {
    let mut e = Engine::new();
    let b = run(&mut e, "batten.create", json!({ "y": 1200, "trim": 8000 }));
    assert_eq!(b["number"], 1);
    let b = id(&b);
    let c = id(&run(&mut e, "curtain.create", json!({ "parent": b, "width": 11000, "height": 7500 })));
    let t = id(&run(&mut e, "truss.create", json!({ "parent": b, "start": [-3000, 0, -300], "end": [3000, 0, -300] })));
    assert!(close(world_pos(&e, c), [0.0, 1200.0, 8000.0]));
    run(&mut e, "batten.update", json!({ "id": b, "trim": 9500, "y": 1500 }));
    assert!(close(world_pos(&e, c), [0.0, 1500.0, 9500.0]));
    assert!(close(world_pos(&e, t), [-3000.0, 1500.0, 9200.0]));
    // Numbers are unique and count on.
    assert_eq!(run(&mut e, "batten.create", json!({ "y": 2000 }))["number"], 2);
    assert!(e.execute("batten.create", json!({ "number": 1 })).is_err());
    let list = run(&mut e, "batten.list", json!(null));
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert_eq!(list[0]["number"], 1);
    assert_eq!(list[0]["attached"].as_array().unwrap().len(), 2);
}

#[test]
fn a_wing_set_makes_legs_and_borders_as_separate_objects() {
    let mut e = Engine::new();
    let b1 = id(&run(&mut e, "batten.create", json!({ "y": 2100, "trim": 8000 })));
    let out = run(&mut e, "wingset.create", json!({ "y": 2000, "count": 3, "spacing": 2000, "opening": 10000 }));
    let set = id(&out);
    assert_eq!(out["legs"].as_array().unwrap().len(), 6);
    assert_eq!(out["borders"].as_array().unwrap().len(), 3);
    // The first border found the batten 100 mm away; the others stay in the set.
    assert_eq!(out["hung"].as_array().unwrap().len(), 1);
    assert_eq!(e.doc().children(set).len(), 6 + 2);
    assert_eq!(e.doc().children(b1).len(), 1);
    let leg: Id = serde_json::from_value(out["legs"][1].clone()).unwrap();
    // Right leg of the first wing: centre at opening/2 + width/2, top at 7000.
    assert!(close(world_pos(&e, leg), [5750.0, 2000.0, 7000.0]), "{:?}", world_pos(&e, leg));
    // Legs stay editable one by one.
    run(&mut e, "masking.update", json!({ "id": leg, "width": 1200, "color": "#202040" }));
    assert!(matches!(kind(&e, leg), ObjectKind::Masking { width: 1200.0, .. }));
    // The border on the batten moves with it; its world height stays the trim + drop.
    let border: Id = serde_json::from_value(out["borders"][0].clone()).unwrap();
    assert!(close(world_pos(&e, border), [0.0, 2000.0, 8000.0]));
    run(&mut e, "batten.update", json!({ "id": b1, "trim": 9000 }));
    assert!(close(world_pos(&e, border), [0.0, 2000.0, 9000.0]));
}

#[test]
fn curtain_states_and_checks() {
    let mut e = Engine::new();
    let c = id(&run(&mut e, "curtain.create", json!({ "state": "gathered" })));
    let ObjectKind::Curtain { opening, gather, color, split, .. } = kind(&e, c) else { panic!() };
    assert_eq!((opening, gather, split), (100.0, stagecraft_core::model::Gather::Italian, true));
    assert_eq!(color, "#8b1a1a");
    run(&mut e, "curtain.update", json!({ "id": c, "opening": 40, "gather": "greek" }));
    assert!(matches!(
        kind(&e, c),
        ObjectKind::Curtain { opening: 40.0, gather: stagecraft_core::model::Gather::Tab, .. }
    ));
    assert!(e.execute("curtain.update", json!({ "id": c, "opening": 140 })).is_err());
    assert!(e.execute("curtain.create", json!({ "color": "red" })).is_err());
}

#[test]
fn platforms_presets_systems_and_stacking() {
    let mut e = Engine::new();
    let a = id(&run(&mut e, "platform.create", json!({ "preset": "2x1", "height": 400, "system": "Bütec" })));
    assert!(matches!(
        kind(&e, a),
        ObjectKind::Platform { width: 2000.0, depth: 1000.0, under: stagecraft_core::model::Understructure::Legs, .. }
    ));
    let err = e.execute("platform.create", json!({ "system": "Nivoflex", "height": 1500 })).unwrap_err();
    assert!(err.message.contains("200–1000"), "{err}");
    let n = id(&run(&mut e, "platform.create", json!({ "system": "nivoflex", "height": 600, "preset": "2x0.5" })));
    assert!(matches!(kind(&e, n), ObjectKind::Platform { under: stagecraft_core::model::Understructure::Scissor, .. }));
    let top = id(&run(&mut e, "platform.create", json!({ "preset": "1x1", "height": 200, "on": a })));
    assert!(close(world_pos(&e, top), [0.0, 0.0, 400.0]));
}

#[test]
fn a_closed_truss_run_has_corners_and_stock_pieces() {
    let mut e = Engine::new();
    let out = run(
        &mut e,
        "truss.run",
        json!({ "points": [[-4000, 0, 6000], [4000, 0, 6000], [4000, 6000, 6000], [-4000, 6000, 6000]], "closed": true }),
    );
    let r = id(&out);
    assert_eq!(out["nodes"].as_array().unwrap().len(), 4);
    let pieces: Vec<Id> = serde_json::from_value(out["pieces"].clone()).unwrap();
    // 8 m minus two 290 mm corners = 7420 → 3000 + 3000 + 1000 + 420; 6 m → 5420 → 3000 + 2000 + 420.
    let lengths: Vec<f64> = pieces
        .iter()
        .map(|p| match kind(&e, *p) {
            ObjectKind::Truss { length, .. } => length,
            _ => panic!(),
        })
        .collect();
    assert_eq!(lengths.len(), 2 * 4 + 2 * 3);
    let total: f64 = lengths.iter().sum();
    assert!((total - 2.0 * (7420.0 + 5420.0)).abs() < 1e-6, "{total}");
    // The first piece starts one reach right of the first corner.
    assert!(close(world_pos(&e, pieces[0]), [-3710.0, 0.0, 6000.0]));
    // Moving the run moves everything in it.
    run(&mut e, "object.move", json!({ "ids": [r], "delta": [0, 1000, 0] }));
    assert!(close(world_pos(&e, pieces[0]), [-3710.0, 1000.0, 6000.0]));
    // Loads: 25.68 m of F34 at 5 kg/m plus four 4.5 kg corners.
    let a =
        id(&run(&mut e, "fixture.create", json!({ "parent": pieces[0], "position": [500, 0, -150], "weight_kg": 20 })));
    run(&mut e, "fixture.create", json!({ "parent": pieces[1], "position": [500, 0, -150] }));
    let rep = run(&mut e, "load.report", json!({ "ids": [r, pieces[0]] }));
    let run_row = &rep["runs"][0];
    assert!((run_row["own_kg"].as_f64().unwrap() - (25.68 * 5.0 + 18.0)).abs() < 0.11, "{run_row}");
    assert_eq!(run_row["attached_kg"], 20.0);
    assert_eq!(run_row["unknown_weights"], 1);
    assert_eq!(rep["trusses"][0]["total_kg"], 35.0);
    let _ = a;
}

#[test]
fn join_makes_a_run_of_loose_pieces() {
    let mut e = Engine::new();
    let a = id(&run(&mut e, "truss.create", json!({ "start": [0, 0, 5000], "end": [3000, 0, 5000] })));
    let n = id(&run(&mut e, "truss.node", json!({ "position": [3290, 0, 5000], "preset": "corner_3way" })));
    let out = run(&mut e, "truss.join", json!({ "ids": [a, n] }));
    let r = id(&out);
    assert_eq!(e.doc().objects[&a].parent, Some(r));
    assert!(close(world_pos(&e, n), [3290.0, 0.0, 5000.0]));
    assert!(matches!(kind(&e, n), ObjectKind::TrussNode { down: true, .. }));
    // Joining again into a new run removes the empty old one.
    let out = run(&mut e, "truss.join", json!({ "ids": [a, n] }));
    assert_eq!(out["removed_runs"].as_array().unwrap().len(), 1);
    assert!(e.execute("truss.join", json!({ "ids": [r] })).is_err());
}

#[test]
fn hoist_and_batten_loads_and_limits() {
    let mut e = Engine::new();
    let h =
        id(&run(&mut e, "hoist.create", json!({ "position": [0, 3000, 9000], "capacity_kg": 250, "weight_kg": 30 })));
    let t = id(&run(
        &mut e,
        "truss.create",
        json!({ "parent": h, "start": [-5000, 0, -300], "end": [5000, 0, -300], "profile": "F34" }),
    ));
    for i in 0..10 {
        run(
            &mut e,
            "fixture.create",
            json!({ "parent": t, "position": [500.0 + 1000.0 * i as f64, 0, -150], "weight_kg": 21 }),
        );
    }
    let b = id(&run(&mut e, "batten.create", json!({ "y": 1000, "max_load": 100, "length": 10000 })));
    run(&mut e, "curtain.create", json!({ "parent": b, "width": 10000, "height": 8000, "fullness": 100 }));
    let rep = run(&mut e, "load.report", json!(null));
    let hr = &rep["hoists"][0];
    assert_eq!(hr["own_kg"], 30.0);
    assert_eq!(hr["attached_kg"], 260.0);
    assert_eq!(hr["over"], true);
    let br = &rep["battens"][0];
    assert_eq!(br["own_kg"], 45.0);
    // 10 m × 2 × 8 m velour at 0.55 kg/m².
    assert_eq!(br["attached_kg"], 88.0);
    assert_eq!(br["over"], false);
    assert_eq!(rep["suspended_kg"], 30.0 + 260.0 + 45.0 + 88.0);
    // object.get shows the load too.
    let g = run(&mut e, "object.get", json!({ "id": t }));
    assert_eq!(g["load"]["attached_kg"], 210.0);
}

#[test]
fn every_new_kind_survives_save_and_load() {
    let mut e = Engine::new();
    run(&mut e, "proscenium.create", json!({ "iron": 750 }));
    run(&mut e, "ceiling.create", json!({ "outline": [[0, 0], [1000, 0], [1000, 1000]], "show_in_plan": false }));
    run(&mut e, "support.create", json!({ "preset": "Wind-up stand" }));
    let p = id(&run(&mut e, "platform.create", json!({})));
    run(&mut e, "object.set_style", json!({ "ids": [p], "line": "thick", "labels": false }));
    assert!(e.execute("object.set_style", json!({ "ids": [p], "line": "bold" })).is_err());
    let doc = e.doc().clone();
    let text = serde_json::to_string(&doc).unwrap();
    let back: stagecraft_core::Document = serde_json::from_str(&text).unwrap();
    assert_eq!(back, doc);
    assert_eq!(back.objects[&p].style.line.as_deref(), Some("thick"));
}
