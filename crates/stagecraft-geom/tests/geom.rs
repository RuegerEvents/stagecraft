//! Plan symbols, hit tests and snapping on real documents built through commands.

use serde_json::{Value, json};
use stagecraft_core::{Engine, Id};
use stagecraft_geom::pick::{BoxMode, in_box, object_bounds, pick, truss_at};
use stagecraft_geom::snap::{Features, SnapKind, SnapSettings, snap};
use stagecraft_geom::{Bounds, Detail, LineClass, PlanPrim, PlanStyle, plan};

fn run(e: &mut Engine, name: &str, args: Value) -> Value {
    e.execute(name, args).unwrap_or_else(|err| panic!("{name} failed: {err}"))
}

fn id(v: &Value) -> Id {
    v["id"].as_str().unwrap().parse().unwrap()
}

fn all(e: &Engine) -> Vec<Id> {
    e.doc().objects.keys().copied().collect()
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6
}

#[test]
fn f34_truss_is_a_290_mm_double_line_with_diagonals() {
    let mut e = Engine::new();
    let t = id(&run(&mut e, "truss.create", json!({ "start": [0, 0, 6000], "end": [3000, 0, 6000] })));
    let prims = plan(e.doc(), &e.doc().objects[&t], &PlanStyle::default());
    let b = stagecraft_geom::pick::bounds(&prims);
    assert!(near(b.min, [0.0, -145.0]) && near(b.max, [3000.0, 145.0]), "{b:?}");
    // Outline, two chord lines, one zigzag.
    assert_eq!(prims.len(), 4);
    let PlanPrim::Polyline { points: zig, line: LineClass::Thin, .. } = &prims[3] else { panic!("{:?}", prims[3]) };
    // Chord spacing 240 mm → 3000 / 240 = 12.5 → 13 bays, alternating sides.
    assert_eq!(zig.len(), 14);
    assert!(near(zig[0], [0.0, -120.0]));
    assert!((zig[1][1] - 120.0).abs() < 1e-9);
    assert!(near(*zig.last().unwrap(), [3000.0, 120.0]));
    let low = plan(e.doc(), &e.doc().objects[&t], &PlanStyle { detail: Detail::Low, ..Default::default() });
    assert_eq!(low.len(), 1);
}

#[test]
fn truss_symbol_follows_rotation_and_profile() {
    let mut e = Engine::new();
    let t = id(&run(
        &mut e,
        "truss.create",
        json!({ "start": [1000, 1000, 0], "end": [1000, 3000, 0], "profile": "F44P" }),
    ));
    let b = object_bounds(e.doc(), &e.doc().objects[&t], &PlanStyle::default());
    assert!(near(b.min, [800.0, 1000.0]) && near(b.max, [1200.0, 3000.0]), "{b:?}");
}

#[test]
fn walls_are_filled_bands() {
    let mut e = Engine::new();
    let w = id(&run(&mut e, "wall.create", json!({ "path": [[0, 0], [4000, 0], [4000, 3000]], "thickness": 200 })));
    let b = object_bounds(e.doc(), &e.doc().objects[&w], &PlanStyle::default());
    assert!(near(b.min, [0.0, -100.0]) && near(b.max, [4100.0, 3000.0]), "{b:?}");
}

#[test]
fn pick_prefers_fixtures_over_truss_over_floors() {
    let mut e = Engine::new();
    let floor = id(&run(
        &mut e,
        "floor.create",
        json!({ "outline": [[-5000, -5000], [5000, -5000], [5000, 5000], [-5000, 5000]] }),
    ));
    let truss = id(&run(&mut e, "truss.create", json!({ "start": [-4000, 0, 6000], "end": [4000, 0, 6000] })));
    let fx = id(&run(&mut e, "fixture.create", json!({ "parent": truss, "position": [4000, 0, -300] })));
    let s = PlanStyle::default();
    assert_eq!(pick(e.doc(), all(&e), [0.0, 0.0], 20.0, &s), Some(fx));
    assert_eq!(pick(e.doc(), all(&e), [2000.0, 50.0], 20.0, &s), Some(truss));
    assert_eq!(pick(e.doc(), all(&e), [2000.0, 1000.0], 20.0, &s), Some(floor));
    assert_eq!(pick(e.doc(), all(&e), [9000.0, 0.0], 20.0, &s), None);
    // Just outside the truss outline, within tolerance.
    assert_eq!(pick(e.doc(), vec![truss], [2000.0, 160.0], 20.0, &s), Some(truss));
    assert_eq!(pick(e.doc(), vec![truss], [2000.0, 200.0], 20.0, &s), None);
}

#[test]
fn box_selection_window_and_crossing() {
    let mut e = Engine::new();
    let a = id(&run(&mut e, "fixture.create", json!({ "position": [0, 0, 0] })));
    let b = id(&run(&mut e, "fixture.create", json!({ "position": [2000, 0, 0] })));
    let w = id(&run(&mut e, "wall.create", json!({ "path": [[-3000, 1000], [3000, 1000]] })));
    let s = PlanStyle::default();
    let rect = Bounds::from_points([-500.0, -500.0], [2500.0, 1100.0]);
    let mut win = in_box(e.doc(), all(&e), rect, BoxMode::Window, &s);
    win.sort();
    let mut want = vec![a, b];
    want.sort();
    assert_eq!(win, want);
    let cross = in_box(e.doc(), all(&e), rect, BoxMode::Crossing, &s);
    assert!(cross.contains(&w) && cross.len() == 3);
    // A box inside the wall band but away from its ends still crosses it.
    let thin = Bounds::from_points([0.0, 990.0], [10.0, 1010.0]);
    assert_eq!(in_box(e.doc(), all(&e), thin, BoxMode::Crossing, &s), vec![w]);
}

#[test]
fn truss_hit_reports_distance_along() {
    let mut e = Engine::new();
    let t = id(&run(&mut e, "truss.create", json!({ "start": [1000, 2000, 6000], "end": [5000, 2000, 6000] })));
    let hit = truss_at(e.doc(), all(&e), [2500.0, 2100.0], 10.0).unwrap();
    assert_eq!(hit.id, t);
    assert!((hit.along - 1500.0).abs() < 1e-9);
    assert!(near(hit.point, [2500.0, 2000.0]));
    assert_eq!(hit.height, 290.0);
    assert!(truss_at(e.doc(), all(&e), [2500.0, 2300.0], 10.0).is_none());
}

#[test]
fn snapping_order_and_orthogonal() {
    let mut e = Engine::new();
    let w = id(&run(&mut e, "wall.create", json!({ "path": [[0, 0], [4000, 0]] })));
    let t = id(&run(&mut e, "truss.create", json!({ "start": [0, 3000, 0], "end": [6000, 3000, 0] })));
    let f = Features::collect(e.doc(), all(&e), &[]);
    let s = SnapSettings::default();
    let r = snap([4030.0, 20.0], None, false, &f, &s, 50.0);
    assert_eq!((r.kind, r.point, r.source), (SnapKind::Endpoint, [4000.0, 0.0], Some(w)));
    let r = snap([1980.0, -30.0], None, false, &f, &s, 50.0);
    assert_eq!((r.kind, r.point), (SnapKind::Midpoint, [2000.0, 0.0]));
    // Along the truss, the distance from its start rounds to the 100 mm grid.
    let r = snap([1234.0, 3040.0], None, false, &f, &s, 50.0);
    assert_eq!((r.kind, r.source), (SnapKind::Truss, Some(t)));
    assert!(near(r.point, [1200.0, 3000.0]), "{:?}", r.point);
    let r = snap([1234.0, 1666.0], None, false, &f, &s, 50.0);
    assert_eq!((r.kind, r.point), (SnapKind::Grid, [1200.0, 1700.0]));
    // Shift: locked horizontal from the previous point, length to the grid.
    let r = snap([1234.0, 1300.0], Some([0.0, 1000.0]), true, &f, &s, 50.0);
    assert_eq!((r.kind, r.point), (SnapKind::Orthogonal, [1200.0, 1000.0]));
    // …and aligned with the wall's end when close to it.
    let r = snap([3980.0, 1300.0], Some([0.0, 1000.0]), true, &f, &s, 50.0);
    assert_eq!(r.point, [4000.0, 1000.0]);
    let free = SnapSettings { grid: None, endpoints: false, midpoints: false, truss: false };
    assert_eq!(snap([1.5, 2.5], None, false, &f, &free, 50.0).kind, SnapKind::Free);
}

#[test]
fn fixture_symbol_carries_the_fid() {
    let mut e = Engine::new();
    let fx = id(&run(&mut e, "fixture.create", json!({ "position": [500, 500, 0], "fid": 101 })));
    let prims = plan(e.doc(), &e.doc().objects[&fx], &PlanStyle::default());
    assert!(prims.iter().any(|p| matches!(p, PlanPrim::Text { text, .. } if text == "101")));
}

#[test]
fn meshes_are_stubs_for_now() {
    let mut e = Engine::new();
    let t = id(&run(&mut e, "truss.create", json!({ "start": [0, 0, 0], "end": [1000, 0, 0] })));
    assert!(stagecraft_geom::mesh(e.doc(), &e.doc().objects[&t], stagecraft_geom::Lod::High).is_empty());
}
