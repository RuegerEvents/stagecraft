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
