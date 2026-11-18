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

// ---- meshes -------------------------------------------------------------------

use stagecraft_geom::section::{Plane, cut};
use stagecraft_geom::{Lod, MaterialClass, Mesh, mesh, world_mesh};

fn mesh_of(e: &Engine, id: Id, lod: Lod) -> Mesh {
    mesh(e.doc(), &e.doc().objects[&id], lod)
}

fn assert_unit_normals(m: &Mesh) {
    assert_eq!(m.normals.len(), m.positions.len());
    for n in &m.normals {
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!((l - 1.0).abs() < 1e-4, "normal {n:?}");
    }
    assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
    let covered: u32 = m.groups.iter().map(|g| g.count).sum();
    assert_eq!(covered as usize, m.indices.len(), "groups cover every index");
}

fn near3(a: [f32; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] as f64 - b[i]).abs() < 1e-3)
}

/// Bounds inside `lo`..`hi` and no more than `tol` mm short of them.
fn within(m: &Mesh, lo: [f64; 3], hi: [f64; 3], tol: f64) -> bool {
    let (a, b) = m.bounds().unwrap();
    let ok = (0..3).all(|i| {
        let (a, b) = (a[i] as f64, b[i] as f64);
        a >= lo[i] - 1e-3 && a <= lo[i] + tol && b <= hi[i] + 1e-3 && b >= hi[i] - tol
    });
    if !ok {
        eprintln!("bounds {a:?} {b:?}, wanted {lo:?} {hi:?} ± {tol}");
    }
    ok
}

#[test]
fn floor_is_the_outline_extruded_down() {
    let mut e = Engine::new();
    // An L-shaped outline, clockwise on purpose.
    let outline = json!([[0, 0], [0, 4000], [2000, 4000], [2000, 2000], [5000, 2000], [5000, 0]]);
    let f = id(&run(&mut e, "floor.create", json!({ "outline": outline, "thickness": 100 })));
    let m = mesh_of(&e, f, Lod::Full);
    assert_unit_normals(&m);
    assert!(m.is_watertight());
    // Top and bottom fans of 6 points, 6 side quads.
    assert_eq!(m.positions.len(), 6 + 6 + 6 * 4);
    assert_eq!(m.triangle_count(), 4 + 4 + 12);
    let (lo, hi) = m.bounds().unwrap();
    assert!(near3(lo, [0.0, 0.0, -100.0]) && near3(hi, [5000.0, 4000.0, 0.0]), "{lo:?} {hi:?}");
    let area = 5000.0 * 2000.0 + 2000.0 * 2000.0;
    assert!((m.volume() - area * 100.0).abs() < area * 1e-4, "{}", m.volume());
    assert_eq!(m.groups[0].material, MaterialClass::Floor);
}

#[test]
fn wall_is_mitred_and_closed() {
    let mut e = Engine::new();
    let w = id(&run(
        &mut e,
        "wall.create",
        json!({ "path": [[0, 0], [4000, 0], [4000, 3000]], "height": 2500, "thickness": 200 }),
    ));
    let m = mesh_of(&e, w, Lod::Full);
    assert_unit_normals(&m);
    assert!(m.is_watertight());
    // Two stretches of four faces, two end caps.
    assert_eq!(m.triangle_count(), (2 * 4 + 2) * 2);
    let (lo, hi) = m.bounds().unwrap();
    assert!(near3(lo, [-0.0, -100.0, 0.0]) && near3(hi, [4100.0, 3000.0, 2500.0]), "{lo:?} {hi:?}");
    // The outer corner is a sharp mitre at (4100, -100).
    assert!(m.positions.iter().any(|p| near3(*p, [4100.0, -100.0, 2500.0])));
    // Centre-line length × thickness × height.
    let vol = 7000.0 * 200.0 * 2500.0;
    assert!((m.volume() - vol).abs() < vol * 1e-6, "{} vs {vol}", m.volume());
}

#[test]
fn a_closed_wall_path_becomes_a_ring() {
    let mut e = Engine::new();
    let w = id(&run(
        &mut e,
        "wall.create",
        json!({ "path": [[0, 0], [4000, 0], [4000, 4000], [0, 4000], [0, 0]], "height": 1000, "thickness": 200 }),
    ));
    let m = mesh_of(&e, w, Lod::Full);
    assert!(m.is_watertight());
    assert_eq!(m.triangle_count(), 4 * 4 * 2, "no end caps");
    let vol = (4200.0f64 * 4200.0 - 3800.0 * 3800.0) * 1000.0;
    assert!((m.volume() - vol).abs() < vol * 1e-6);
    // Cut half way up: a square ring.
    let world = stagecraft_core::geom::world(e.doc(), w).unwrap();
    let polys = cut(&m, &world, &Plane::new([0.0, 0.0, 500.0], [0.0, 0.0, 1.0]));
    assert_eq!(polys.len(), 1);
    assert_eq!(polys[0].holes.len(), 1);
    assert!((polys[0].area() - (4200.0 * 4200.0 - 3800.0 * 3800.0)).abs() < 1.0);
}

#[test]
fn an_l_shaped_wall_cuts_into_an_l_and_two_rectangles() {
    let mut e = Engine::new();
    let w = id(&run(
        &mut e,
        "wall.create",
        json!({ "path": [[0, 3000], [0, 0], [5000, 0]], "height": 3000, "thickness": 200 }),
    ));
    run(&mut e, "object.move", json!({ "ids": [w], "delta": [1000, 1000, 0] }));
    let m = world_mesh(e.doc(), &e.doc().objects[&w], Lod::Full);
    let id = glam::DAffine3::IDENTITY;
    // Horizontal cut: the L band in plan.
    let polys = cut(&m, &id, &Plane::new([0.0, 0.0, 1200.0], [0.0, 0.0, 1.0]));
    assert_eq!(polys.len(), 1);
    assert_eq!(polys[0].outer.len(), 6, "{:?}", polys[0].outer);
    let area = (3000.0 + 5000.0) * 200.0;
    assert!((polys[0].area() - area).abs() < 1e-3 * area, "{}", polys[0].area());
    // Upright cut across both legs (x = 1000 + 2500): one rectangle through the long
    // leg, 200 wide and 3000 high.
    let polys = cut(&m, &id, &Plane::new([3500.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
    assert_eq!(polys.len(), 1);
    assert!((polys[0].area() - 200.0 * 3000.0).abs() < 1e-3);
    // Diagonal cut through both legs: two pieces.
    let polys = cut(&m, &id, &Plane::new([2000.0, 2000.0, 0.0], [1.0, 1.0, 0.0]));
    assert_eq!(polys.len(), 2, "{polys:?}");
}

#[test]
fn truss_detail_levels() {
    let mut e = Engine::new();
    let t = id(&run(&mut e, "truss.create", json!({ "start": [0, 0, 6000], "end": [3000, 0, 6000] })));
    let full = mesh_of(&e, t, Lod::Full);
    let simple = mesh_of(&e, t, Lod::Simple);
    let bx = mesh_of(&e, t, Lod::Box);
    for m in [&full, &simple, &bx] {
        assert_unit_normals(m);
        assert!(m.is_watertight());
        // Polygonal tubes reach the outer size only at their corners.
        assert!(within(m, [0.0, -145.0, -145.0], [3000.0, 145.0, 145.0], 5.0));
        assert_eq!(m.groups[0].material, MaterialClass::Metal);
    }
    use stagecraft_geom::kinds::truss::{BRACE_SEGMENTS, CHORD_SEGMENTS, CHORD_SEGMENTS_SIMPLE};
    let tube = |s: usize| 2 * s + 2 * (s + 1);
    assert_eq!(bx.positions.len(), 24);
    assert_eq!(simple.positions.len(), 4 * tube(CHORD_SEGMENTS_SIMPLE));
    // F34: four faces 240 mm wide → 3000 / 240 → 13 bays each, plus two end braces.
    let braces = 4 * (13 + 2);
    assert_eq!(full.positions.len(), 4 * tube(CHORD_SEGMENTS) + braces * tube(BRACE_SEGMENTS));
}

#[test]
fn truss_chords_follow_the_profile() {
    let mut e = Engine::new();
    let mut make = |profile: &str| {
        let t =
            id(&run(&mut e, "truss.create", json!({ "start": [0, 0, 0], "end": [2000, 0, 0], "profile": profile })));
        (mesh_of(&e, t, Lod::Simple), mesh_of(&e, t, Lod::Full))
    };
    use stagecraft_geom::kinds::truss::CHORD_SEGMENTS_SIMPLE as S;
    let tube = 2 * S + 2 * (S + 1);
    for (profile, chords, w, h) in [("F31", 1, 50.0, 50.0), ("F32", 2, 290.0, 50.0), ("X30D", 3, 290.0, 250.0)] {
        let (simple, full) = make(profile);
        assert_eq!(simple.positions.len(), chords * tube, "{profile}");
        assert!(full.is_watertight(), "{profile}");
        assert!(within(&full, [0.0, -w / 2.0, -h / 2.0], [2000.0, w / 2.0, h / 2.0], 2.0), "{profile}");
    }
    // A single tube has no bracing.
    let (simple, full) = make("F31");
    let c = stagecraft_geom::kinds::truss::CHORD_SEGMENTS;
    assert_eq!(simple.positions.len(), tube);
    assert_eq!(full.positions.len(), 2 * c + 2 * (c + 1));
    // The triangle stands on two chords: the apex chord is centred at the top.
    let (simple, _) = make("X30D");
    let apex = &simple.positions[2 * tube..3 * tube];
    assert!(apex.iter().all(|p| p[1].abs() <= 25.0));
}

#[test]
fn fixture_placeholder_hangs_below_its_origin() {
    let mut e = Engine::new();
    let fx = id(&run(&mut e, "fixture.create", json!({ "position": [0, 0, 6000], "fid": 1 })));
    let m = mesh_of(&e, fx, Lod::Full);
    assert_unit_normals(&m);
    let (lo, hi) = m.bounds().unwrap();
    assert!(hi[2].abs() < 1e-3 && lo[2] < -250.0, "{lo:?} {hi:?}");
    // The lens sticks out at the front (+y).
    assert!(hi[1] > 170.0);
    assert_eq!(m.groups.iter().map(|g| g.material).collect::<Vec<_>>(), [MaterialClass::Fixture, MaterialClass::Lens]);
    // World mesh follows the placement.
    let w = world_mesh(e.doc(), &e.doc().objects[&fx], Lod::Full);
    let (_, whi) = w.bounds().unwrap();
    assert!((whi[2] - 6000.0).abs() < 1e-2);
}

// ---- block 4: stage objects ----------------------------------------------------

fn obj_plan(e: &Engine, id: Id) -> Vec<PlanPrim> {
    plan(e.doc(), &e.doc().objects[&id], &PlanStyle::default())
}

#[test]
fn every_stage_mesh_is_closed_at_every_detail_level() {
    let mut e = Engine::new();
    let ids = [
        run(&mut e, "proscenium.create", json!({ "iron": 800 })),
        run(&mut e, "curtain.create", json!({ "width": 10000, "height": 6000 })),
        run(&mut e, "curtain.create", json!({ "opening": 60 })),
        run(&mut e, "curtain.create", json!({ "state": "gathered", "split": false })),
        run(&mut e, "batten.create", json!({})),
        run(&mut e, "platform.create", json!({ "system": "Bütec", "height": 600 })),
        run(&mut e, "platform.create", json!({ "system": "Nivoflex", "height": 600, "preset": "1x2" })),
        run(&mut e, "platform.create", json!({ "under": "none", "height": 50 })),
        run(&mut e, "truss.node", json!({ "preset": "tee_4way" })),
        run(&mut e, "support.create", json!({ "preset": "Wind-up stand" })),
        run(&mut e, "support.create", json!({ "preset": "Ground support base" })),
        run(&mut e, "support.create", json!({})),
        run(&mut e, "hoist.create", json!({})),
        run(&mut e, "ceiling.create", json!({ "outline": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]] })),
    ];
    for v in &ids {
        let id = id(v);
        for lod in [Lod::Full, Lod::Simple, Lod::Box] {
            let m = mesh_of(&e, id, lod);
            let ty = e.doc().objects[&id].kind.type_name();
            assert!(!m.is_empty(), "{ty} {lod:?} empty");
            assert_unit_normals(&m);
            assert!(m.is_watertight(), "{ty} {lod:?} not closed");
            assert!(m.volume() > 0.0, "{ty} {lod:?} inside out");
        }
    }
    let wings = run(&mut e, "wingset.create", json!({ "fullness": 50 }));
    let leg: Id = serde_json::from_value(wings["legs"][0].clone()).unwrap();
    assert!(mesh_of(&e, leg, Lod::Full).is_watertight());
    assert!(mesh_of(&e, id(&wings), Lod::Full).is_empty());
}

#[test]
fn proscenium_has_two_pillars_a_header_and_the_iron_line() {
    let mut e = Engine::new();
    let p = id(&run(
        &mut e,
        "proscenium.create",
        json!({ "width": 10000, "height": 6000, "depth": 600, "frame": 1000, "iron": 800 }),
    ));
    let prims = obj_plan(&e, p);
    let walls =
        prims.iter().filter(|q| matches!(q, PlanPrim::Polygon { fill: stagecraft_geom::FillClass::Wall, .. })).count();
    assert_eq!(walls, 2);
    assert!(prims.iter().any(|q| matches!(q, PlanPrim::Polyline { line: LineClass::Hidden, .. })));
    let iron = prims.iter().find(|q| matches!(q, PlanPrim::Polyline { line: LineClass::Centre, .. })).unwrap();
    let PlanPrim::Polyline { points, .. } = iron else { unreachable!() };
    assert!(near(points[0], [-6000.0, 800.0]) && near(points[1], [6000.0, 800.0]));
    let m = mesh_of(&e, p, Lod::Full);
    assert!(within(&m, [-6000.0, 0.0, 0.0], [6000.0, 600.0, 7000.0], 0.0));
}

#[test]
fn a_curtain_hangs_down_from_its_batten_with_pleats() {
    let mut e = Engine::new();
    let b = id(&run(&mut e, "batten.create", json!({ "y": 1000, "trim": 8000, "length": 12000 })));
    let c = id(&run(&mut e, "curtain.create", json!({ "parent": b, "width": 10000, "height": 7500, "fullness": 100 })));
    let wm = world_mesh(e.doc(), &e.doc().objects[&c], Lod::Full);
    let (lo, hi) = wm.bounds().unwrap();
    assert!((lo[2] - 500.0).abs() < 1e-2 && (hi[2] - 8000.0).abs() < 1e-2, "{lo:?} {hi:?}");
    assert!((lo[0] + 5006.0).abs() < 10.0 && (hi[0] - 5006.0).abs() < 10.0);
    // Pleats: the cloth swings both ways around the track.
    assert!(lo[1] < 1000.0 - 100.0 && hi[1] > 1000.0 + 100.0, "{lo:?} {hi:?}");
    assert_eq!(wm.tint, Some([0x8b, 0x1a, 0x1a]));
    assert!(wm.groups.iter().all(|g| g.material == MaterialClass::Curtain));
    // Plan: two pleated panels and the track line; low detail: straight panels.
    let prims = obj_plan(&e, c);
    assert_eq!(prims.len(), 3);
    let PlanPrim::Polyline { points, .. } = &prims[0] else { panic!() };
    assert!(points.len() > 100);
    let low = plan(e.doc(), &e.doc().objects[&c], &PlanStyle { detail: Detail::Low, ..Default::default() });
    assert!(low.iter().all(|q| matches!(q, PlanPrim::Polyline { points, .. } if points.len() == 2)));
    // Opened, both panels sit at the sides.
    run(&mut e, "curtain.update", json!({ "id": c, "opening": 100 }));
    let b = object_bounds(e.doc(), &e.doc().objects[&c], &PlanStyle { detail: Detail::Low, ..Default::default() });
    assert!(b.size()[0] > 9000.0);
}

#[test]
fn battens_platforms_and_hoists_draw_their_symbols() {
    let mut e = Engine::new();
    let b = id(&run(&mut e, "batten.create", json!({ "number": 12, "y": 3000, "length": 10000 })));
    let prims = obj_plan(&e, b);
    assert!(prims.iter().any(|q| matches!(q, PlanPrim::Text { text, .. } if text == "12")));
    let bb = stagecraft_geom::pick::bounds(&prims);
    assert!(bb.min[0] < -5000.0 && bb.max[0] >= 5000.0 && (bb.center()[1] - 3000.0).abs() < 1.0);

    run(&mut e, "layer.create", json!({ "name": "Up", "elevation": 1000 }));
    let p = id(&run(&mut e, "platform.create", json!({ "layer": "Up", "preset": "2x1", "height": 400 })));
    let prims = obj_plan(&e, p);
    assert!(prims.iter().any(|q| matches!(q, PlanPrim::Text { text, .. } if text == "+1.40")), "{prims:?}");
    // Box selection and picking work on the new symbols.
    let hits = in_box(
        e.doc(),
        all(&e),
        Bounds::from_points([-1100.0, -600.0], [1100.0, 600.0]),
        BoxMode::Window,
        &PlanStyle::default(),
    );
    assert_eq!(hits, vec![p]);
    let h = id(&run(&mut e, "hoist.create", json!({ "position": [0, 0, 9000] })));
    assert_eq!(pick(e.doc(), all(&e), [10.0, 10.0], 50.0, &PlanStyle::default()), Some(h));
}

#[test]
fn per_object_style_overrides_the_view() {
    let mut e = Engine::new();
    let c = id(&run(&mut e, "ceiling.create", json!({ "outline": [[0, 0], [4000, 0], [4000, 3000]] })));
    assert!(matches!(obj_plan(&e, c)[0], PlanPrim::Polyline { line: LineClass::Hidden, .. }));
    run(&mut e, "ceiling.update", json!({ "id": c, "show_in_plan": false }));
    assert!(obj_plan(&e, c).is_empty());
    let t = id(&run(&mut e, "truss.create", json!({ "start": [0, 0, 6000], "end": [3000, 0, 6000] })));
    run(&mut e, "object.set_style", json!({ "ids": [t], "line": "thick", "detail": "low" }));
    let prims = obj_plan(&e, t);
    assert_eq!(prims.len(), 1);
    assert!(matches!(prims[0], PlanPrim::Polyline { line: LineClass::Thick, .. }));
    run(&mut e, "object.set_style", json!({ "ids": [t], "hidden": true }));
    assert!(obj_plan(&e, t).is_empty());
}

#[test]
fn a_corner_node_reaches_one_profile_width_along_each_arm() {
    let mut e = Engine::new();
    let n = id(&run(&mut e, "truss.node", json!({ "position": [0, 0, 6000], "preset": "corner_3way" })));
    let m = world_mesh(e.doc(), &e.doc().objects[&n], Lod::Box);
    assert!(within(&m, [-145.0, -145.0, 6000.0 - 290.0], [290.0, 290.0, 6145.0], 0.5));
    let b = object_bounds(e.doc(), &e.doc().objects[&n], &PlanStyle::default());
    assert!(near(b.min, [-145.0, -145.0]) && near(b.max, [290.0, 290.0]), "{b:?}");
}

#[test]
fn levels_carry_their_sign() {
    use stagecraft_geom::kinds::stage::level_text;
    assert_eq!(level_text(1400.0), "+1.40");
    assert_eq!(level_text(-750.0), "-0.75");
    assert_eq!(level_text(0.4), "±0.00");
}
