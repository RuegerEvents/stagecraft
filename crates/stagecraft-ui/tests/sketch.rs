//! The sketch in the GUI: walls drawn with typed lengths become a constrained room,
//! dimensions are edited by double-click, sketch points drag live, the dimension tool
//! adds dimensions, rejected edits explain themselves.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};
use stagecraft_core::{Engine, ObjectKind};
use stagecraft_geom::dims::DimSource;
use stagecraft_ui::Studio;

fn harness(engine: Engine) -> Harness<'static, Studio> {
    let mut h = Harness::builder()
        .with_size(vec2(1400.0, 900.0))
        // Real frame times, so two clicks can be a double click (the default step of
        // 0.25 s is near egui's 0.3 s double-click delay).
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, s: &mut Studio| s.show(ui), Studio::new(engine));
    h.run();
    h
}

fn run(h: &mut Harness<'static, Studio>, name: &str, args: Value) -> Value {
    let out = h.state_mut().run(name, args).unwrap_or_else(|e| panic!("{name}: {e}"));
    h.run();
    out
}

fn screen(h: &Harness<'static, Studio>, p: [f64; 2]) -> Pos2 {
    h.state().ui.camera.to_screen(p)
}

fn press(h: &mut Harness<'static, Studio>, pos: Pos2, pressed: bool) {
    h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
}

fn click_at(h: &mut Harness<'static, Studio>, pos: Pos2) {
    h.hover_at(pos);
    h.run();
    press(h, pos, true);
    press(h, pos, false);
    h.run();
}

fn click_world(h: &mut Harness<'static, Studio>, p: [f64; 2]) {
    let pos = screen(h, p);
    click_at(h, pos);
}

fn double_click_at(h: &mut Harness<'static, Studio>, pos: Pos2) {
    h.hover_at(pos);
    h.run();
    // One step per click: `run` would let egui's clock pass the double-click delay.
    for _ in 0..2 {
        press(h, pos, true);
        press(h, pos, false);
        h.step();
    }
    h.run();
}

/// Lets half a second pass, so the next click is not a double click.
fn pause(h: &mut Harness<'static, Studio>) {
    for _ in 0..40 {
        h.step();
    }
}

fn hover_world(h: &mut Harness<'static, Studio>, p: [f64; 2]) {
    let pos = screen(h, p);
    h.hover_at(pos);
    h.run();
}

fn type_enter(h: &mut Harness<'static, Studio>, text: &str) {
    h.event(Event::Text(text.into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
}

fn wall_path(h: &Harness<'static, Studio>) -> Vec<[f64; 2]> {
    let doc = h.state().engine.doc();
    let w = doc.objects.values().find(|o| o.kind.type_name() == "wall").expect("a wall");
    match &w.kind {
        ObjectKind::Wall { path, .. } => path.clone(),
        _ => unreachable!(),
    }
}

fn xmax(p: &[[f64; 2]]) -> f64 {
    p.iter().map(|q| q[0]).fold(f64::NEG_INFINITY, f64::max)
}

/// Draws a 6 × 4 m room with the wall tool: click, type 6, Enter, 4, Enter, 6,
/// Enter, click the first corner.
fn draw_room(h: &mut Harness<'static, Studio>) {
    run(h, "ui.zoom", json!({ "scale": 0.08 }));
    run(h, "ui.pan", json!({ "center": [3000, 2000] }));
    h.get_by_label("Tool Wall").click();
    h.run();
    click_world(h, [0.0, 0.0]);
    hover_world(h, [3000.0, 0.0]);
    type_enter(h, "6");
    hover_world(h, [6000.0, 2000.0]);
    type_enter(h, "4");
    hover_world(h, [3000.0, 4000.0]);
    type_enter(h, "6");
    click_world(h, [0.0, 0.0]);
}

#[test]
fn drawing_a_room_with_typed_lengths_makes_dimensions() {
    let mut h = harness(Engine::new());
    draw_room(&mut h);
    let doc = h.state().engine.doc();
    let s = doc.sketches.values().next().expect("the wall tool drew into the sketch");
    assert_eq!(s.edges.len(), 4, "{s:?}");
    assert_eq!(s.points.len(), 4, "corners are shared");
    let driving: Vec<f64> = s.dimensions.iter().filter(|d| d.driving).map(|d| d.value).collect();
    assert_eq!(driving, vec![6000.0, 4000.0], "the third typed length is implied");
    assert_eq!(s.dimensions.len(), 3);
    // Horizontal/vertical were inferred for every side.
    assert_eq!(s.constraints.len(), 4);
    let path = wall_path(&h);
    assert_eq!(path.len(), 5, "a closed ring: {path:?}");
    assert!((xmax(&path) - 6000.0).abs() < 1e-6);
    let wall = doc.objects.values().find(|o| o.kind.type_name() == "wall").unwrap();
    assert!(wall.sketch.is_some(), "the wall follows the sketch");
    // The inspector shows the sketch status when nothing is selected.
    run(&mut h, "selection.clear", Value::Null);
    assert!(h.query_by_label_contains("2 degrees of freedom").is_some(), "the room can still be moved as a whole");
}

#[test]
fn double_click_a_dimension_type_a_value_and_the_room_follows() {
    let mut h = harness(Engine::new());
    draw_room(&mut h);
    run(&mut h, "ui.set_tool", json!({ "tool": "select" }));
    pause(&mut h);
    // Lock the first corner: the room is then fully constrained and grows to the right.
    let corner = screen(&h, [0.0, 0.0]);
    double_click_at(&mut h, corner);
    assert_eq!(h.state().ui.status.as_ref().map(|s| s.0.as_str()), Some("Corner locked (fixed)"));
    run(&mut h, "selection.clear", Value::Null);
    assert!(h.query_by_label_contains("Fully constrained").is_some());
    pause(&mut h);
    let doc = h.state().engine.doc().clone();
    let (layer, s) = doc.sketches.iter().next().unwrap();
    let width = s.dimensions.iter().find(|d| d.driving && (d.value - 6000.0).abs() < 1e-6).unwrap().id;
    let src = DimSource::Sketch { layer: *layer, id: width };
    let cam = h.state().ui.camera;
    let d = stagecraft_ui::sketch_ui::plan_dims(&doc, &cam).into_iter().find(|d| d.source == src).unwrap();
    let at = stagecraft_ui::sketch_ui::label_center(&cam, &d);
    double_click_at(&mut h, at);
    assert!(h.state().ui.sketch.editing.is_some(), "the value editor opened");
    let field = h.get_by_label("Dimension value");
    field.focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run();
    type_enter(&mut h, "8.5");
    assert!(h.state().ui.sketch.editing.is_none());
    let path = wall_path(&h);
    let xmin = path.iter().map(|q| q[0]).fold(f64::INFINITY, f64::min);
    assert!(xmin.abs() < 1e-6 && (xmax(&path) - 8500.0).abs() < 1e-6, "the wall followed: {path:?}");
    let v = h.state().engine.doc().sketches[layer].dimension(width).unwrap().value;
    assert_eq!(v, 8500.0);
    // Undo brings the old width back.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert!((xmax(&wall_path(&h)) - 6000.0).abs() < 1e-6, "{:?}", wall_path(&h));
}

#[test]
fn a_rejected_edit_names_the_conflict_in_the_status_bar() {
    let mut h = harness(Engine::new());
    draw_room(&mut h);
    let doc = h.state().engine.doc().clone();
    let s = doc.sketches.values().next().unwrap();
    let bottom = s.edges[0].id;
    let err = h
        .state_mut()
        .run(
            "dimension.add",
            json!({ "measure": { "type": "length", "line": bottom.0 }, "value": 5000, "driving": true }),
        )
        .unwrap_err();
    h.run();
    assert_eq!(err.code, "conflict");
    let status = h.state().ui.status.clone().unwrap();
    assert!(status.1, "an error");
    assert!(status.0.starts_with("Rejected: conflicting constraints: dimension"), "{}", status.0);
    assert!(status.0.contains("length 5000 mm"), "{}", status.0);
    assert!(h.query_by_label_contains("Rejected: conflicting constraints").is_some());
    assert_eq!(h.state().engine.doc(), &doc, "nothing changed");
}

#[test]
fn dragging_a_sketch_corner_moves_walls_live_and_commits_once() {
    let mut h = harness(Engine::new());
    run(&mut h, "ui.zoom", json!({ "scale": 0.08 }));
    run(&mut h, "ui.pan", json!({ "center": [3000, 2000] }));
    run(&mut h, "sketch.draw", json!({ "points": [[0, 0], [6000, 0], [6000, 4000]], "make": "wall" }));
    let undo_before = h.state().engine.journal().len();
    let from = screen(&h, [6000.0, 4000.0]);
    let to = screen(&h, [7000.0, 5000.0]);
    h.hover_at(from);
    h.run();
    press(&mut h, from, true);
    h.run();
    let mut previewed = false;
    for i in 1..=6 {
        let p = from + (to - from) * (i as f32 / 6.0);
        h.event(Event::PointerMoved(p));
        h.run();
        previewed |= h.state().ui.gesture.drag.is_some();
    }
    assert!(previewed, "a sketch drag is in progress");
    assert_eq!(h.state().engine.journal().len(), undo_before, "nothing committed while dragging");
    press(&mut h, to, false);
    h.run();
    assert_eq!(h.state().engine.journal().len(), undo_before + 1, "one command on release");
    let path = wall_path(&h);
    // The far corner moved (snapped to the 100 mm grid near 7000/5000); the
    // horizontal first segment stays horizontal.
    let last = path[path.len() - 1];
    assert!((last[0] - 7000.0).abs() < 150.0 && (last[1] - 5000.0).abs() < 150.0, "{path:?}");
    assert!((path[0][1] - path[1][1]).abs() < 1e-6, "{path:?}");
}

#[test]
fn the_dimension_tool_measures_between_sketch_corners_and_objects() {
    let mut h = harness(Engine::new());
    draw_room(&mut h);
    let truss = run(&mut h, "truss.create", json!({ "start": [1000, 2000, 6000], "end": [5000, 2000, 6000] }));
    let _ = truss;
    run(&mut h, "ui.set_tool", json!({ "tool": "dimension" }));
    pause(&mut h);
    // Corner to corner in a fully constrained room: becomes a reference dimension.
    let n0 = h.state().engine.doc().sketches.values().next().unwrap().dimensions.len();
    click_world(&mut h, [0.0, 0.0]);
    click_world(&mut h, [0.0, 4000.0]);
    click_world(&mut h, [-800.0, 2000.0]);
    let s = h.state().engine.doc().sketches.values().next().unwrap().clone();
    assert_eq!(s.dimensions.len(), n0 + 1);
    let d = s.dimensions.last().unwrap();
    assert!(!d.driving, "the room is fully constrained in y already");
    assert!(matches!(
        d.measure,
        stagecraft_core::sketch::Measure::VerticalDistance { .. } | stagecraft_core::sketch::Measure::Distance { .. }
    ));
    // Truss end to the back wall corner: a reference dimension between objects.
    click_world(&mut h, [5000.0, 2000.0]);
    click_world(&mut h, [6000.0, 4000.0]);
    click_world(&mut h, [7000.0, 3000.0]);
    let refs: Vec<_> = h.state().engine.doc().dimensions.values().cloned().collect();
    assert_eq!(refs.len(), 1, "{refs:?}");
    let (_, _, v) = refs[0].measure(h.state().engine.doc()).unwrap();
    assert!((v - 2000.0).abs() < 1e-6, "vertical from the truss end to the corner: {v}");
}

/// A 200-point room: dragging a point re-solves every frame for the preview. The
/// frames must stay interactive.
#[test]
fn dragging_in_a_200_point_sketch_stays_interactive() {
    let mut e = Engine::new();
    // A 100 × 2 strip of rooms: two rows of 100 corners, horizontal/vertical lines and
    // a driving width per bay, one corner fixed.
    let n = 100;
    let mut top = Vec::new();
    let mut bottom = Vec::new();
    for i in 0..n {
        top.push(json!([i as f64 * 1000.0, 3000.0]));
        bottom.push(json!([i as f64 * 1000.0, 0.0]));
    }
    let b = e.execute("sketch.add_polyline", json!({ "points": bottom })).unwrap();
    let t = e.execute("sketch.add_polyline", json!({ "points": top })).unwrap();
    let bp: Vec<u64> = b["points"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    let tp: Vec<u64> = t["points"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    let be: Vec<u64> = b["edges"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    let mut cmds = Vec::new();
    for i in 0..n {
        cmds.push(json!({ "command": "sketch.add_line", "args": { "from": bp[i], "to": tp[i] } }));
        cmds.push(json!({ "command": "sketch.add_constraint", "args": { "constraint": { "type": "vertical", "a": bp[i], "b": tp[i] } } }));
        if i + 1 < n {
            cmds.push(json!({ "command": "sketch.add_constraint", "args": { "constraint": { "type": "horizontal", "a": bp[i], "b": bp[i + 1] } } }));
            cmds.push(json!({ "command": "dimension.add", "args": { "measure": { "type": "length", "line": be[i] }, "value": 1000, "driving": true } }));
        }
    }
    e.execute("batch", json!({ "commands": cmds })).unwrap();
    assert_eq!(e.doc().sketches.values().next().unwrap().points.len(), 200);
    let mut h = harness(e);
    run(&mut h, "ui.zoom_to_fit", Value::Null);
    run(&mut h, "ui.set_tool", json!({ "tool": "select" }));
    let from = screen(&h, [50_000.0, 3000.0]);
    h.hover_at(from);
    h.run();
    press(&mut h, from, true);
    h.run();
    h.event(Event::PointerMoved(from + vec2(12.0, 0.0)));
    h.run();
    assert!(
        matches!(h.state().ui.gesture.drag, Some(stagecraft_ui::state::Drag::SketchPoint { .. })),
        "{:?}",
        h.state().ui.gesture.drag
    );
    let journal = h.state().engine.journal().len();
    let frames = 12;
    let start = std::time::Instant::now();
    let mut worst = std::time::Duration::ZERO;
    for i in 1..=frames {
        let t0 = std::time::Instant::now();
        h.event(Event::PointerMoved(from + vec2(12.0 + i as f32 * 3.0, -(i as f32) * 2.0)));
        h.step();
        worst = worst.max(t0.elapsed());
    }
    let mean = start.elapsed() / frames;
    press(&mut h, from + vec2(48.0, -24.0), false);
    h.run();
    assert_eq!(h.state().engine.journal().len(), journal + 1, "the drag was committed");
    eprintln!("200-point drag: mean frame {mean:?}, worst {worst:?}");
    // Debug builds solve with an optimised solver (workspace profile) but draw
    // unoptimised; release is far faster. Interactive = well under a quarter second.
    let limit = if cfg!(debug_assertions) { 250 } else { 50 };
    assert!(mean.as_millis() < limit, "mean frame {mean:?}");
}
