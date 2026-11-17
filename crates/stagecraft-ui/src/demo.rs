//! A small proscenium theatre for screenshots and manual trying: stage house with a
//! 10 m portal and an iron line, a forestage, the house a metre lower under its
//! ceiling, twelve battens with the main curtain (open), three wings of black
//! masking, two lighting trusses and a lighting pipe, a black backdrop, a rostrum of
//! Bütec decks with Nivoflex steps, an FOH truss run on a hoist and two stands on the
//! forestage. Fixtures weigh 21 kg, so the load sums show real numbers.

use serde_json::{Value, json};
use stagecraft_core::Engine;

fn run(e: &mut Engine, name: &str, args: Value) -> Value {
    e.execute(name, args).unwrap_or_else(|err| panic!("demo: {name} failed: {err}"))
}

const FIXTURE_KG: f64 = 21.0;

pub fn engine() -> Engine {
    let mut e = Engine::new();
    run(&mut e, "doc.set_meta", json!({ "project": "Demo", "venue": "Studio Theatre" }));
    run(&mut e, "layer.create", json!({ "name": "House", "elevation": -1000 }));

    // Floors: stage, forestage, house.
    run(
        &mut e,
        "floor.create",
        json!({ "name": "Stage floor", "tags": ["set"], "outline": [[-9500, 0], [9500, 0], [9500, 12000], [-9500, 12000]] }),
    );
    run(
        &mut e,
        "floor.create",
        json!({
            "name": "Forestage", "tags": ["set"], "thickness": 40,
            "outline": [[-5000, -2000], [5000, -2000], [6000, 0], [-6000, 0]]
        }),
    );
    run(
        &mut e,
        "floor.create",
        json!({
            "name": "House floor", "layer": "House", "tags": ["house"],
            "outline": [[-9500, -12000], [9500, -12000], [9500, -2000], [-9500, -2000]]
        }),
    );
    // Stage house walls (shown cut at 3.5 m) and the portal.
    run(
        &mut e,
        "wall.create",
        json!({
            "name": "Stage house", "thickness": 300, "height": 3500,
            "path": [[-6500, 400], [-9650, 400], [-9650, 12150], [9650, 12150], [9650, 400], [6500, 400]]
        }),
    );
    run(
        &mut e,
        "proscenium.create",
        json!({ "name": "Portal", "tags": ["set"], "width": 10000, "height": 6000, "depth": 800, "frame": 1500, "iron": 950 }),
    );
    run(
        &mut e,
        "ceiling.create",
        json!({
            "name": "House ceiling", "layer": "House", "tags": ["house", "ceiling"], "height": 9800,
            "outline": [[-9500, -12000], [9500, -12000], [9500, -2500], [-9500, -2500]]
        }),
    );

    // Fly system: twelve battens, 14 m, from just behind the iron line upstage.
    let battens: [(f64, f64); 12] = [
        (1200.0, 8000.0),
        (1700.0, 9000.0),
        (2350.0, 8000.0),
        (3000.0, 7500.0),
        (3900.0, 9000.0),
        (4850.0, 8000.0),
        (5600.0, 7200.0),
        (6400.0, 9000.0),
        (7350.0, 8000.0),
        (8200.0, 7500.0),
        (9100.0, 9000.0),
        (10000.0, 8500.0),
    ];
    let mut ids = Vec::new();
    for (i, (y, trim)) in battens.iter().enumerate() {
        let b = run(
            &mut e,
            "batten.create",
            json!({ "number": i + 1, "y": y, "trim": trim, "length": 14000, "tags": ["rigging"] }),
        );
        ids.push(b["id"].clone());
    }
    run(
        &mut e,
        "curtain.create",
        json!({
            "name": "Main curtain", "parent": ids[0], "tags": ["set"], "width": 11000, "height": 7990,
            "fullness": 100, "opening": 80, "gather": "tab"
        }),
    );
    run(
        &mut e,
        "curtain.create",
        json!({
            "name": "Black backdrop", "parent": ids[11], "tags": ["masking"], "width": 14000, "height": 8490,
            "fullness": 50, "split": true, "color": "#161616"
        }),
    );
    // Three wings of black masking; the borders find battens 3, 6 and 9.
    run(
        &mut e,
        "wingset.create",
        json!({
            "name": "Wings", "tags": ["masking"], "y": 2500, "count": 3, "spacing": 2400, "opening": 10000,
            "leg_width": 1500, "height": 7000, "border_height": 2000, "border_trim": 6000, "fullness": 50
        }),
    );

    let mut fid = 101;
    let mut fixture = |e: &mut Engine, parent: &Value, name: &str, position: [f64; 3], rotation: [f64; 3]| {
        let addr = 1 + (fid - 101) * 16;
        run(
            e,
            "fixture.create",
            json!({
                "parent": parent, "position": position, "rotation": rotation, "fid": fid, "address": addr,
                "mode": "Standard 16ch", "tags": ["light"], "name": name, "weight_kg": FIXTURE_KG
            }),
        );
        fid += 1;
    };
    // FOH: an 8 m run on one hoist (with a bridle) over the house.
    let hoist = run(
        &mut e,
        "hoist.create",
        json!({ "name": "FOH hoist", "layer": "House", "tags": ["rigging"], "position": [0, -5000, 9200] }),
    );
    let foh = run(
        &mut e,
        "truss.run",
        json!({
            "name": "FOH", "parent": hoist["id"], "tags": ["rigging"], "profile": "F34",
            "points": [[-4000, 0, -1300], [4000, 0, -1300]]
        }),
    );
    let mut n = 0;
    for piece in foh["pieces"].as_array().unwrap().clone().iter() {
        for x in [700.0, 2100.0] {
            n += 1;
            fixture(&mut e, piece, &format!("FOH {n}"), [x, 0.0, -150.0], [0.0, 0.0, 0.0]);
        }
    }
    // LX1 and LX3: truss under battens 4 and 10; LX2 straight on batten 7.
    for (name, batten, n) in [("LX1", 3usize, 8), ("LX3", 9, 6)] {
        let t = run(
            &mut e,
            "truss.create",
            json!({
                "name": name, "parent": ids[batten], "profile": "F34", "tags": ["rigging"],
                "start": [-6000, 0, -350], "end": [6000, 0, -350]
            }),
        );
        for i in 0..n {
            let x = 12000.0 * (i as f64 + 0.5) / n as f64;
            fixture(&mut e, &t["id"], &format!("{name} {}", i + 1), [x, 0.0, -150.0], [0.0, 0.0, 180.0]);
        }
    }
    for i in 0..6 {
        let x = -5000.0 + 2000.0 * i as f64;
        fixture(&mut e, &ids[6], &format!("LX2 {}", i + 1), [x, 0.0, -60.0], [0.0, 0.0, 180.0]);
    }
    // Stands on the forestage with two fixtures each.
    for (side, x) in [("SL", 7300.0), ("SR", -7300.0)] {
        let st = run(
            &mut e,
            "support.create",
            json!({ "name": format!("Stand {side}"), "preset": "Wind-up stand", "position": [x, -1000, 0] }),
        );
        for dx in [-150.0, 150.0] {
            fixture(&mut e, &st["id"], &format!("Boom {side}"), [dx, 0.0, 3000.0], [180.0, 0.0, 0.0]);
        }
    }

    // Rostrum upstage: three Bütec 2×1 at 400, one stacked, Nivoflex steps in front.
    let mut centre = Value::Null;
    for x in [-2000.0, 0.0, 2000.0] {
        let p = run(
            &mut e,
            "platform.create",
            json!({ "name": "Rostrum", "tags": ["set"], "system": "Bütec", "preset": "2x1",
                    "height": 400, "position": [x, 8600, 0] }),
        );
        if x == 0.0 {
            centre = p["id"].clone();
        }
    }
    run(
        &mut e,
        "platform.create",
        json!({ "name": "Rostrum top", "tags": ["set"], "system": "Bütec", "preset": "2x1", "height": 400,
                "on": centre, "position": [0, 8600, 0] }),
    );
    run(
        &mut e,
        "platform.create",
        json!({ "name": "Step", "tags": ["set"], "system": "Nivoflex", "preset": "2x0.5", "height": 200,
                "position": [0, 7850, 0] }),
    );
    // A floor fixture on a base plate upstage.
    let plate = run(&mut e, "support.create", json!({ "preset": "Base plate 600", "position": [-6500, 10800, 0] }));
    fixture(&mut e, &plate["id"], "Floor wash", [0.0, 0.0, 10.0], [180.0, 0.0, 0.0]);

    // Nothing to undo in a fresh demo.
    let mut fresh = Engine::with_document(e.doc().clone());
    std::mem::swap(&mut e, &mut fresh);
    e
}
