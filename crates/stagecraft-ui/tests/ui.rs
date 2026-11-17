//! The GUI driven headlessly: widgets found by accessible name, pointer events at
//! world positions on the plan, and checks on the document.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};
use stagecraft_core::{Engine, Id, ObjectKind};
use stagecraft_ui::Studio;

fn harness(engine: Engine) -> Harness<'static, Studio> {
    let mut h = Harness::builder()
        .with_size(vec2(1400.0, 900.0))
        .build_ui_state(|ui, s: &mut Studio| s.show(ui), Studio::new(engine));
    h.run();
    h
}

fn run(h: &mut Harness<'static, Studio>, name: &str, args: Value) -> Value {
    let out = h.state_mut().run(name, args).unwrap_or_else(|e| panic!("{name}: {e}"));
    h.run();
    out
}

fn id(v: &Value) -> Id {
    v["id"].as_str().unwrap().parse().unwrap()
}

fn screen(h: &Harness<'static, Studio>, p: [f64; 2]) -> Pos2 {
    h.state().ui.camera.to_screen(p)
}

fn click_world(h: &mut Harness<'static, Studio>, p: [f64; 2]) {
    let pos = screen(h, p);
    click_at(h, pos);
}

fn drag_world(h: &mut Harness<'static, Studio>, a: [f64; 2], b: [f64; 2]) {
    let (a, b) = (screen(h, a), screen(h, b));
    drag(h, a, b);
}

fn click_at(h: &mut Harness<'static, Studio>, pos: Pos2) {
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run();
}

fn objects_of(h: &Harness<'static, Studio>, ty: &str) -> Vec<stagecraft_core::Object> {
    h.state().engine.doc().objects.values().filter(|o| o.kind.type_name() == ty).cloned().collect()
}

#[test]
fn drawing_a_wall_with_a_typed_length() {
    let mut h = harness(Engine::new());
    run(&mut h, "ui.zoom", json!({ "scale": 0.05 }));
    run(&mut h, "ui.pan", json!({ "center": [2000, 0] }));
    h.get_by_label("Tool Wall").click();
    h.run();
    assert_eq!(h.state().ui.tool, stagecraft_ui::Tool::Wall);
    click_world(&mut h, [0.0, 0.0]);
    assert_eq!(h.state().ui.gesture.points, vec![[0.0, 0.0]]);
    h.event(Event::Text("4.5".into()));
    h.run();
    assert!(h.query_by_label_contains("Length: 4.5").is_some(), "the typed length shows next to the pointer");
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().ui.gesture.points.len(), 2);
    // A second Enter with nothing typed finishes the wall.
    h.key_press(Key::Enter);
    h.run();
    let walls = objects_of(&h, "wall");
    assert_eq!(walls.len(), 1);
    let ObjectKind::Wall { path, .. } = &walls[0].kind else { unreachable!() };
    assert_eq!(path, &vec![[0.0, 0.0], [4500.0, 0.0]]);
    // The new wall is selected and undo takes it away again.
    assert_eq!(h.state().engine.selection(), [walls[0].id]);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert!(objects_of(&h, "wall").is_empty());
}

#[test]
fn typed_angle_after_tab() {
    let mut h = harness(Engine::new());
    run(&mut h, "ui.zoom", json!({ "scale": 0.05 }));
    run(&mut h, "ui.pan", json!({ "center": [0, 0] }));
    run(&mut h, "ui.set_tool", json!({ "tool": "wall" }));
    click_world(&mut h, [0.0, 0.0]);
    h.event(Event::Text("2".into()));
    h.run();
    h.key_press(Key::Tab);
    h.run();
    h.event(Event::Text("90".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    let p = h.state().ui.gesture.points[1];
    assert!(p[0].abs() < 1e-6 && (p[1] - 2000.0).abs() < 1e-6, "{p:?}");
}

#[test]
fn a_fixture_clicked_onto_a_truss_hangs_on_it() {
    let mut e = Engine::new();
    let truss =
        id(&e.execute("truss.create", json!({ "start": [-4000, 2000, 6000], "end": [4000, 2000, 6000] })).unwrap());
    let mut h = harness(e);
    h.get_by_label("Tool Fixture").click();
    h.run();
    // Just off the axis, inside the 290 mm band.
    click_world(&mut h, [1000.0, 2050.0]);
    let fixtures = objects_of(&h, "fixture");
    assert_eq!(fixtures.len(), 1);
    let f = &fixtures[0];
    assert_eq!(f.parent, Some(truss));
    assert!((f.placement.position[0] - 5000.0).abs() < 1e-6, "{:?}", f.placement);
    assert_eq!(f.placement.position[1], 0.0);
    assert_eq!(f.placement.position[2], -145.0);
    let w = stagecraft_core::geom::world_placement(h.state().engine.doc(), f.id).unwrap();
    assert!((w.position[0] - 1000.0).abs() < 1e-6 && (w.position[1] - 2000.0).abs() < 1e-6);

    // Off the truss it stands on the layer.
    click_world(&mut h, [1000.0, 0.0]);
    let free: Vec<_> = objects_of(&h, "fixture").into_iter().filter(|o| o.parent.is_none()).collect();
    assert_eq!(free.len(), 1);
}

#[test]
fn table_fill_series_numbers_fixtures() {
    let mut e = Engine::new();
    for x in [0, 1000, 2000] {
        e.execute("fixture.create", json!({ "position": [x, 0, 0] })).unwrap();
    }
    let mut h = harness(e);
    h.get_by_label("FID 1").click();
    h.run();
    h.event(Event::Text("101".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    let fid_of = |h: &Harness<'static, Studio>| -> Vec<Option<u32>> {
        let doc = h.state().engine.doc();
        let ids: Vec<Id> = doc.objects.keys().copied().collect();
        stagecraft_ui::table::plan_order(doc, &ids)
            .iter()
            .map(|id| match &doc.objects[id].kind {
                ObjectKind::Fixture { fid, .. } => *fid,
                _ => None,
            })
            .collect()
    };
    assert_eq!(fid_of(&h), [Some(101), None, None]);
    h.get_by_label("FID 1").click();
    h.run();
    h.get_by_label("FID 3").click_modifiers(Modifiers::SHIFT);
    h.run();
    // Selecting cells selects the fixtures in the plan.
    assert_eq!(h.state().engine.selection().len(), 3);
    h.get_by_label("Fill series").click();
    h.run();
    assert_eq!(fid_of(&h), [Some(101), Some(102), Some(103)]);
    // One undo step for the whole series.
    run(&mut h, "history.undo", Value::Null);
    assert_eq!(fid_of(&h), [Some(101), None, None]);
}

#[test]
fn table_multi_cell_edit_and_tsv_paste() {
    let mut e = Engine::new();
    for x in [0, 1000, 2000] {
        e.execute("fixture.create", json!({ "position": [x, 0, 0] })).unwrap();
    }
    let mut h = harness(e);
    h.get_by_label("Mode 1").click();
    h.run();
    h.get_by_label("Mode 3").click_modifiers(Modifiers::SHIFT);
    h.run();
    h.event(Event::Text("Spot".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    let modes: Vec<Option<String>> = objects_of(&h, "fixture")
        .iter()
        .map(|o| match &o.kind {
            ObjectKind::Fixture { mode, .. } => mode.clone(),
            _ => None,
        })
        .collect();
    assert_eq!(modes, vec![Some("Spot".to_string()); 3]);

    h.get_by_label("CID 1").click();
    h.run();
    h.event(Event::Paste("A1\t1.001\nA2\t1.017\n".into()));
    h.run();
    let doc = h.state().engine.doc();
    let ids: Vec<Id> = doc.objects.keys().copied().collect();
    let rows: Vec<(Option<String>, Option<u32>)> = stagecraft_ui::table::plan_order(doc, &ids)
        .iter()
        .map(|id| match &doc.objects[id].kind {
            ObjectKind::Fixture { cid, address, .. } => (cid.clone(), *address),
            _ => (None, None),
        })
        .collect();
    assert_eq!(rows, [(Some("A1".into()), Some(1)), (Some("A2".into()), Some(17)), (None, None)]);
}

#[test]
fn the_palette_runs_a_command_with_arguments() {
    let mut h = harness(Engine::new());
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    assert!(h.state().ui.palette.open);
    h.get_by_label("Search commands").type_text("layer create");
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().ui.palette.chosen.as_deref(), Some("layer.create"));
    h.get_by_label("Argument name").type_text("Gallery");
    h.run();
    h.get_by_label("Argument elevation").click();
    h.run();
    h.get_by_label("Argument elevation").type_text("4500");
    h.run();
    h.get_by_label("Run").click();
    h.run();
    assert!(!h.state().ui.palette.open);
    let doc = h.state().engine.doc();
    let l = doc.layers.values().find(|l| l.name == "Gallery").expect("layer created");
    assert_eq!(l.elevation, 4500.0);
}

#[test]
fn the_palette_lists_ui_commands_and_runs_them_without_a_form() {
    let mut h = harness(stagecraft_ui::demo::engine());
    h.state_mut().ui.palette.open();
    h.run();
    h.get_by_label("Search commands").type_text("ui.state");
    h.run();
    assert!(h.query_by_label("ui.state").is_some());
    h.get_by_label("ui.state").click();
    h.run();
    assert!(!h.state().ui.palette.open);
    assert!(h.state().ui.status.as_ref().unwrap().0.starts_with("ui.state"));
}

#[test]
fn selecting_by_click_and_box_and_moving_by_drag() {
    let mut e = Engine::new();
    let a = id(&e.execute("fixture.create", json!({ "position": [0, 0, 0] })).unwrap());
    let b = id(&e.execute("fixture.create", json!({ "position": [2000, 0, 0] })).unwrap());
    let mut h = harness(e);
    run(&mut h, "ui.zoom", json!({ "scale": 0.1 }));
    run(&mut h, "ui.pan", json!({ "center": [1000, 0] }));
    click_world(&mut h, [0.0, 0.0]);
    assert_eq!(h.state().engine.selection(), [a]);
    // Shift-click adds.
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    click_world(&mut h, [2000.0, 0.0]);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    assert_eq!(h.state().engine.selection(), [a, b]);
    // Click on empty space clears; a window box around both selects them.
    click_world(&mut h, [1000.0, 1200.0]);
    assert!(h.state().engine.selection().is_empty());
    drag_world(&mut h, [-1000.0, 600.0], [3000.0, -600.0]);
    let mut sel = h.state().engine.selection().to_vec();
    sel.sort();
    let mut both = vec![a, b];
    both.sort();
    assert_eq!(sel, both, "{:?}", h.state().ui.status);
    // Dragging a selected fixture moves the selection by a grid-snapped offset.
    drag_world(&mut h, [0.0, 0.0], [1010.0, 495.0]);
    let w = |h: &Harness<'static, Studio>, id| {
        stagecraft_core::geom::world_placement(h.state().engine.doc(), id).unwrap().position
    };
    assert_eq!(w(&h, a), [1000.0, 500.0, 0.0]);
    assert_eq!(w(&h, b), [3000.0, 500.0, 0.0]);
}

fn drag(h: &mut Harness<'static, Studio>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run();
    h.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run();
    for i in 1..=5 {
        let p = from + (to - from) * (i as f32 / 5.0);
        h.event(Event::PointerMoved(p));
        h.run();
    }
    h.event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

#[test]
fn inspector_edits_apply_to_every_selected_object() {
    let mut e = Engine::new();
    let a = id(&e.execute("wall.create", json!({ "path": [[0, 0], [1000, 0]] })).unwrap());
    let b = id(&e.execute("wall.create", json!({ "path": [[0, 2000], [1000, 2000]] })).unwrap());
    e.execute("selection.set", json!({ "ids": [a, b] })).unwrap();
    let mut h = harness(e);
    assert!(h.query_by_label_contains("2 Walls").is_some());
    h.get_by_label("Inspector Height").click();
    h.run();
    h.get_by_label("Inspector Height").type_text("2500");
    h.run();
    h.key_press(Key::Enter);
    h.run();
    for id in [a, b] {
        assert!(matches!(h.state().engine.doc().objects[&id].kind, ObjectKind::Wall { height: 2500.0, .. }));
    }
    // One undo step.
    run(&mut h, "history.undo", Value::Null);
    assert!(matches!(h.state().engine.doc().objects[&a].kind, ObjectKind::Wall { height: 3000.0, .. }));
}

#[test]
fn ui_commands_change_tool_theme_panels_and_tag_visibility() {
    let mut h = harness(stagecraft_ui::demo::engine());
    run(&mut h, "ui.set_tool", json!({ "tool": "truss" }));
    run(&mut h, "ui.set_theme", json!({ "theme": "dark" }));
    run(&mut h, "ui.toggle_panel", json!({ "panel": "table", "visible": false }));
    assert!(h.query_by_label("FID 1").is_none());
    run(&mut h, "ui.set_tag_visible", json!({ "tag": "light", "visible": false }));
    let state = run(&mut h, "ui.state", Value::Null);
    assert_eq!(state["tool"], "truss");
    assert_eq!(state["theme"], "dark");
    assert_eq!(state["panels"]["table"], false);
    assert_eq!(state["hidden_tags"].as_array().unwrap().len(), 1);
    let visible = stagecraft_ui::canvas::visible_ids(h.state().engine.doc(), &h.state().ui);
    assert!(visible.iter().all(|id| h.state().engine.doc().objects[id].kind.type_name() != "fixture"));
    assert_eq!(h.ctx.theme(), egui::Theme::Dark);
}

#[test]
fn control_extension_answers_ui_commands() {
    use stagecraft_control::handle_with;
    let mut s = Studio::new(Engine::new());
    let Studio { engine, ui } = &mut s;
    let mut ext = stagecraft_ui::commands::UiExtension { ui };
    let list = handle_with(engine, &mut ext, "commands", json!({ "prefix": "ui." })).unwrap();
    let names: Vec<&str> = list.as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    for n in [
        "ui.set_tool",
        "ui.zoom",
        "ui.pan",
        "ui.zoom_to_fit",
        "ui.set_view",
        "ui.set_theme",
        "ui.toggle_panel",
        "ui.screenshot",
        "ui.state",
    ] {
        assert!(names.contains(&n), "{n} missing");
    }
    handle_with(engine, &mut ext, "execute", json!({ "command": "ui.set_tool", "args": { "tool": "wall" } })).unwrap();
    assert_eq!(s.ui.tool, stagecraft_ui::Tool::Wall);
}

// ---- 3D view -------------------------------------------------------------------

fn screen3d(h: &Harness<'static, Studio>, p: [f64; 3]) -> Pos2 {
    h.state().ui.view3d.camera.to_screen(glam::DVec3::from_array(p)).expect("in front of the camera")
}

#[test]
fn the_3d_view_follows_camera_commands_and_picks_like_the_plan() {
    let mut e = Engine::new();
    let square = json!([[0, 0], [4000, 0], [4000, 4000], [0, 4000]]);
    let low = id(&e.execute("floor.create", json!({ "outline": square, "name": "low" })).unwrap());
    let high = id(&e.execute("floor.create", json!({ "outline": square, "z": 2000, "name": "high" })).unwrap());
    let mut h = harness(e);
    run(&mut h, "ui.set_view", json!({ "view": "3d" }));
    assert_eq!(h.state().ui.view, stagecraft_ui::View::ThreeD);
    assert!(h.query_by_label("3D view").is_some());
    assert!(h.query_by_label("Plan").is_none(), "the plan canvas is not shown");

    // Standard view: front, fitted.
    let cam = run(&mut h, "ui.camera", json!({ "view": "front" }));
    assert_eq!(cam["yaw"], -90.0);
    assert_eq!(cam["pitch"], 0.0);
    assert!(cam["eye"][1].as_f64().unwrap() < cam["target"][1].as_f64().unwrap(), "{cam}");
    // Eye and target, orthographic.
    let cam = run(
        &mut h,
        "ui.camera",
        json!({ "eye": [2000, 2000, 20000], "target": [2000, 2000, 0], "projection": "orthographic" }),
    );
    assert_eq!(cam["projection"], "orthographic");
    assert_eq!(cam["eye"], json!([2000.0, 2000.0, 20000.0]));
    let state = run(&mut h, "ui.state", Value::Null);
    assert_eq!(state["camera3d"]["target"], json!([2000.0, 2000.0, 0.0]));
    run(&mut h, "ui.camera", json!({ "orbit": [10, -5], "zoom": 2 }));
    let cam = &h.state().ui.view3d.camera;
    assert!((cam.distance - 10_000.0).abs() < 1e-6 && (cam.pitch - 85.0).abs() < 1e-9);
    assert!(h.state_mut().run("ui.camera", json!({ "fov": 500 })).is_err());

    // Looking down, a click on the floors selects the upper one, as in the plan.
    run(&mut h, "ui.camera", json!({ "view": "top", "projection": "perspective" }));
    let p = screen3d(&h, [1000.0, 1000.0, 2000.0]);
    click_at(&mut h, p);
    assert_eq!(h.state().engine.selection(), [high]);
    // A horizontal section at 1 m hides the upper floor: the click reaches the lower.
    run(&mut h, "ui.section", json!({ "axis": "z", "offset": 1000 }));
    let s = run(&mut h, "ui.state", Value::Null);
    assert_eq!(s["section"]["mode"], "plane");
    click_at(&mut h, p);
    assert_eq!(h.state().engine.selection(), [low]);
    // A click into empty space clears the selection.
    let outside = h.state().ui.view3d.camera.rect.left_bottom() + vec2(40.0, -40.0);
    click_at(&mut h, outside);
    assert!(h.state().engine.selection().is_empty());

    // Section box and off; bad arguments are refused.
    run(&mut h, "ui.section", json!({ "box": { "min": [0, 0, -100], "max": [2000, 2000, 3000] } }));
    assert_eq!(h.state().ui.view3d.section.planes().len(), 6);
    assert!(h.state_mut().run("ui.section", json!({ "box": { "min": [0, 0, 0], "max": [0, 1, 1] } })).is_err());
    assert!(h.state_mut().run("ui.section", json!({})).is_err());
    run(&mut h, "ui.section", json!({ "off": true }));
    assert!(h.state().ui.view3d.section.planes().is_empty());
}

#[test]
fn the_3d_scene_is_cached_and_follows_visibility() {
    let mut e = Engine::new();
    let t = id(&e
        .execute("truss.create", json!({ "start": [0, 0, 6000], "end": [6000, 0, 6000], "tags": ["rigging"] }))
        .unwrap());
    e.execute("floor.create", json!({ "outline": [[0, 0], [4000, 0], [4000, 4000]] })).unwrap();
    let mut h = harness(e);
    run(&mut h, "ui.set_view", json!({ "view": "split" }));
    assert!(h.query_by_label("3D view").is_some() && h.query_by_label("Plan").is_some());
    let builds = h.state().ui.view3d.scene.mesh_builds;
    assert!(builds >= 2);
    for _ in 0..10 {
        h.run();
    }
    assert_eq!(h.state().ui.view3d.scene.mesh_builds, builds, "no rebuild without a change");
    // Hiding a tag removes the truss from the 3D view, showing it again needs no rebuild.
    run(&mut h, "ui.set_tag_visible", json!({ "tag": "rigging", "visible": false }));
    assert!(!h.state().ui.view3d.scene.visible().contains(&t));
    run(&mut h, "ui.set_tag_visible", json!({ "tag": "rigging", "visible": true }));
    assert!(h.state().ui.view3d.scene.visible().contains(&t));
    assert_eq!(h.state().ui.view3d.scene.mesh_builds, builds);
    // Editing the truss rebuilds only the truss.
    run(&mut h, "truss.update", json!({ "id": t, "profile": "F44P" }));
    assert_eq!(h.state().ui.view3d.scene.mesh_builds, builds + 1);
    // A selection made in the plan shows in 3D (and vice versa: one selection).
    run(&mut h, "selection.set", json!({ "ids": [t] }));
    let v = &h.state().ui.view3d.scene.buffers;
    assert!(v.vertices.iter().any(|v| v.flags & 1 == 1));
    // Zoom to fit in split fits both views.
    let out = run(&mut h, "ui.zoom_to_fit", Value::Null);
    assert!(out.get("scale").is_some() && out.get("camera3d").is_some(), "{out}");
}
