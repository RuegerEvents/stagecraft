//! A small but busy document for screenshots and manual trying: a stage floor with
//! walls, three truss runs with fixtures, a gallery layer and some tags.

use serde_json::{Value, json};
use stagecraft_core::Engine;

fn run(e: &mut Engine, name: &str, args: Value) -> Value {
    e.execute(name, args).unwrap_or_else(|err| panic!("demo: {name} failed: {err}"))
}

pub fn engine() -> Engine {
    let mut e = Engine::new();
    run(&mut e, "doc.set_meta", json!({ "project": "Demo", "venue": "Studio Theatre" }));
    run(&mut e, "layer.create", json!({ "name": "Gallery", "elevation": 4500 }));
    run(
        &mut e,
        "floor.create",
        json!({
            "name": "Stage floor", "tags": ["set"],
            "outline": [[-7000, 0], [7000, 0], [7000, 9000], [-7000, 9000]]
        }),
    );
    run(
        &mut e,
        "floor.create",
        json!({
            "name": "Apron", "tags": ["set"], "thickness": 40,
            "outline": [[-5000, -1500], [5000, -1500], [6000, 0], [-6000, 0]]
        }),
    );
    run(
        &mut e,
        "wall.create",
        json!({
            "name": "Back wall", "thickness": 240,
            "path": [[-8000, -500], [-8000, 9500], [8000, 9500], [8000, -500]]
        }),
    );
    run(
        &mut e,
        "wall.create",
        json!({
            "name": "Gallery rail", "layer": "Gallery", "thickness": 100, "height": 1100,
            "path": [[-8000, -4000], [8000, -4000]]
        }),
    );
    let trusses = [("FOH", -3000.0, "F34", 6), ("LX1", 2000.0, "F34", 8), ("LX2", 5500.0, "F44P", 6)];
    let mut fid = 101;
    for (name, y, profile, n) in trusses {
        let t = run(
            &mut e,
            "truss.create",
            json!({
                "name": name, "profile": profile, "tags": ["rigging"],
                "start": [-6000, y, 6500], "end": [6000, y, 6500]
            }),
        );
        let tid = t["id"].clone();
        for i in 0..n {
            let x = 12000.0 * (i as f64 + 0.5) / n as f64;
            let addr = 1 + (fid - 101) * 16;
            run(
                &mut e,
                "fixture.create",
                json!({
                    "parent": tid, "position": [x, 0, -150], "rotation": [0, 0, if y < 0.0 { 0 } else { 180 }],
                    "fid": fid, "address": addr, "mode": "Standard 16ch", "tags": ["light"],
                    "name": format!("{name} {}", i + 1)
                }),
            );
            fid += 1;
        }
    }
    for x in [-4500.0, 4500.0] {
        run(
            &mut e,
            "fixture.create",
            json!({
                "position": [x, 8000, 0], "rotation": [0, 0, 180], "tags": ["light", "floor"],
                "name": "Floor wash", "mode": "8ch"
            }),
        );
    }
    // Nothing to undo in a fresh demo.
    let mut fresh = Engine::with_document(e.doc().clone());
    std::mem::swap(&mut e, &mut fresh);
    e
}
