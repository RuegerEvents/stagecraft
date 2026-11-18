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
        // egui 0.37 styles a widget from its previous-pass state, so a widget that appears
        // asks for one more frame; closing a modal can take six.
        .with_max_steps(8)
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
    // CID, fixture type, mode, footprint (read only, skipped), address.
    h.event(Event::Paste("A1\t\t\t\t1.001\nA2\t\t\t\t1.017\n".into()));
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
    let Studio { engine, ui, .. } = &mut s;
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

// ---- block 4: stage tools ------------------------------------------------------

#[test]
fn stage_tools_place_battens_curtains_and_stacked_platforms() {
    let mut h = harness(Engine::new());
    run(&mut h, "ui.zoom", json!({ "scale": 0.05 }));
    run(&mut h, "ui.pan", json!({ "center": [0, 2000] }));
    // A batten by click, at the trim option.
    h.get_by_label("Tool Batten").click();
    h.run();
    run(&mut h, "ui.set_tool_options", json!({ "batten_trim": 8500 }));
    click_world(&mut h, [0.0, 1000.0]);
    let battens = objects_of(&h, "batten");
    assert_eq!(battens.len(), 1);
    assert_eq!(battens[0].placement.position, [0.0, 1000.0, 8500.0]);
    // A curtain drawn along it hangs on it.
    run(&mut h, "ui.set_tool", json!({ "tool": "curtain" }));
    click_world(&mut h, [-4000.0, 1000.0]);
    click_world(&mut h, [4000.0, 1000.0]);
    let c = &objects_of(&h, "curtain")[0];
    assert_eq!(c.parent, Some(battens[0].id));
    assert!(matches!(c.kind, ObjectKind::Curtain { width: 8000.0, .. }));
    // The inspector shows the batten's load with the curtain on it.
    run(&mut h, "selection.set", json!({ "ids": [battens[0].id] }));
    assert!(h.query_by_label_contains("Attached").is_some());
    assert!(h.query_by_label("Inspector Trim").is_some());
    // Platforms: a click on a platform stacks the next one on it.
    run(&mut h, "selection.clear", Value::Null);
    h.get_by_label("Tool Platform").click();
    h.run();
    click_world(&mut h, [0.0, 4000.0]);
    click_world(&mut h, [200.0, 4100.0]);
    let mut ps = objects_of(&h, "platform");
    ps.sort_by(|a, b| a.placement.position[2].total_cmp(&b.placement.position[2]));
    assert_eq!(ps.len(), 2);
    assert_eq!(ps[1].placement.position[2], 400.0);
}

#[test]
fn the_run_tool_closes_a_ring_of_truss() {
    let mut h = harness(Engine::new());
    run(&mut h, "ui.zoom", json!({ "scale": 0.05 }));
    run(&mut h, "ui.pan", json!({ "center": [3000, 2000] }));
    run(&mut h, "ui.set_tool", json!({ "tool": "run" }));
    for p in [[0.0, 0.0], [6000.0, 0.0], [6000.0, 4000.0], [0.0, 4000.0], [0.0, 0.0]] {
        click_world(&mut h, p);
    }
    let runs = objects_of(&h, "truss_run");
    assert_eq!(runs.len(), 1);
    assert_eq!(objects_of(&h, "truss_node").len(), 4);
    assert!(objects_of(&h, "truss").iter().all(|t| t.parent == Some(runs[0].id)));
}

#[test]
fn the_fly_bar_table_lists_battens_by_number_with_loads() {
    let mut h = harness(stagecraft_ui::demo::engine());
    h.get_by_label("Show Fly bars").click();
    h.run();
    assert_eq!(h.state().ui.table.kind.as_deref(), Some("batten"));
    // Row 4 is batten 4 with LX1: 12 m of F34 (60 kg) and eight 21 kg fixtures.
    let doc = h.state().engine.doc().clone();
    let rows = stagecraft_ui::table::rows(&doc, &h.state().ui.table);
    let b4 = &doc.objects[&rows[3]];
    assert_eq!(b4.name, "Batten 4");
    assert_eq!(stagecraft_ui::table::cell_text(&doc, b4, stagecraft_ui::table::Col::Load), "228.0");
    // Trim typed into the table moves the batten and what hangs on it.
    let lx1 = doc.objects.values().find(|o| o.name == "LX1").unwrap().id;
    let before = stagecraft_core::geom::world_placement(&doc, lx1).unwrap().position[2];
    let steps = stagecraft_ui::table::edit_steps(&doc, b4, stagecraft_ui::table::Col::Trim, "8000").unwrap();
    run(&mut h, "batch", json!({ "commands": steps }));
    let after = stagecraft_core::geom::world_placement(h.state().engine.doc(), lx1).unwrap().position[2];
    assert_eq!(after - before, 500.0);
}

fn paper(h: &Harness<'static, Studio>, p: [f64; 2]) -> Pos2 {
    h.state().ui.sheets.cam.to_screen(p)
}

#[test]
fn sheets_mode_moves_viewports_and_labels_by_drag() {
    let mut h = harness(stagecraft_ui::demo::engine());
    run(&mut h, "ui.sheet", json!({ "sheet": "L-101" }));
    assert_eq!(h.state().ui.view, stagecraft_ui::View::Sheets);
    h.get_by_label("Sheet canvas");
    h.get_by_label("Sheet L-102").click();
    h.run();
    let doc = h.state().engine.doc().clone();
    let l102 = doc.sheets.values().find(|s| s.number == "L-102").unwrap().id;
    assert_eq!(h.state_mut().run("ui.state", json!(null)).unwrap()["sheet"], json!(l102));

    // Drag the front elevation 20 mm to the right on the paper.
    let vp = doc.sheets[&l102].viewports[0].clone();
    let page = stagecraft_sheet::pages(&doc, &[l102]).remove(0);
    let hit = page.viewports.iter().find(|v| v.id == vp.id).unwrap();
    let c = [(hit.min[0] + hit.max[0]) / 2.0, (hit.min[1] + hit.max[1]) / 2.0];
    let (a, b) = (paper(&h, c), paper(&h, [c[0] + 20.0, c[1]]));
    drag(&mut h, a, b);
    let moved = h.state().engine.doc().sheets[&l102].viewport(vp.id).unwrap().position;
    assert!((moved[0] - vp.position[0] - 20.0).abs() < 0.5, "{moved:?} vs {:?}", vp.position);
    assert_eq!(h.state().ui.sheets.viewport, Some(vp.id));

    // Drag a label on the lighting plan: it gets a hand-placed offset.
    run(&mut h, "ui.sheet", json!({ "sheet": "L-101" }));
    let doc = h.state().engine.doc().clone();
    let l101 = doc.sheets.values().find(|s| s.number == "L-101").unwrap().id;
    let page = stagecraft_sheet::pages(&doc, &[l101]).remove(0);
    let label = page.labels.iter().find(|l| l.max[0] - l.min[0] > 3.0).unwrap().clone();
    let c = [(label.min[0] + label.max[0]) / 2.0, (label.min[1] + label.max[1]) / 2.0];
    let (a, b) = (paper(&h, c), paper(&h, [c[0], c[1] + 12.0]));
    drag(&mut h, a, b);
    let vp = h.state().engine.doc().sheets[&l101].viewport(label.viewport).unwrap().clone();
    let off = vp.label_offsets[&label.fixture];
    assert!((off[1] - 12.0).abs() < 0.5, "{off:?}");
    run(&mut h, "history.undo", json!(null));
    assert!(h.state().engine.doc().sheets[&l101].viewport(label.viewport).unwrap().label_offsets.is_empty());
}

#[test]
fn sheet_panel_buttons_create_sheets_and_viewports() {
    let mut h = harness(Engine::new());
    run(&mut h, "ui.set_view", json!({ "view": "sheets" }));
    h.get_by_label("Create sheet").click();
    h.run();
    assert_eq!(h.state().engine.doc().sheets.len(), 1);
    h.get_by_label("Add viewport plan").click();
    h.run();
    h.get_by_label("Add viewport fixture_list").click();
    h.run();
    let s = h.state().engine.doc().sheets.values().next().unwrap().clone();
    assert_eq!(s.viewports.len(), 2);
    assert_eq!(h.state().ui.sheets.viewport, Some(s.viewports[1].id));
    run(&mut h, "ui.sheet", json!({ "viewport": s.viewports[0].id, "editor": true }));
    assert!(h.state().ui.sheets.editor);
    h.get_by_label("New label template").click();
    h.run();
    assert_eq!(h.state().engine.doc().label_templates.len(), 1);
    h.get_by_label("Label rotation Follow").click();
    h.run();
    let t = h.state().engine.doc().label_templates.values().next().unwrap().clone();
    assert_eq!(t.rotation, stagecraft_core::sheet::LabelRotation::Follow);
    run(&mut h, "ui.set_label_template", json!({ "template": t.name }));
    assert_eq!(h.state().ui.plan_labels, Some(t.id));
    assert!(h.state_mut().run("ui.sheet", json!({ "sheet": "nope" })).is_err());
}

/// The inspector's Document section sets the sheet language, the dimension format and
/// the new title block fields through `doc.set_meta`.
#[test]
fn document_section_sets_sheet_language_and_fields() {
    let mut h = harness(Engine::new());
    h.get_by_label("Inspector sheet language").click();
    h.run_steps(6);
    h.get_by_label("Deutsch").click();
    h.run_steps(6);
    assert_eq!(h.state().engine.doc().meta.sheet_language, stagecraft_core::units::Language::De);
    h.get_by_label("Inspector dimension format").click();
    h.run_steps(6);
    h.get_by_label("Millimetres (6155)").click();
    h.run_steps(6);
    assert_eq!(h.state().engine.doc().meta.dimension_format, stagecraft_core::units::DimFormat::Mm);
    h.get_by_label("Inspector Venue address").click();
    h.run_steps(6);
    h.get_by_label("Inspector Venue address").type_text("1 Street / 12345 Town");
    h.key_press(Key::Enter);
    h.run_steps(6);
    assert_eq!(h.state().engine.doc().meta.venue_address, "1 Street\n12345 Town");
}

// ---- docking ---------------------------------------------------------------------

fn floating_panels(h: &Harness<'static, Studio>) -> Vec<stagecraft_ui::state::PanelKind> {
    h.state().ui.layout.layout.floating.iter().flat_map(|w| w.panels().copied().collect::<Vec<_>>()).collect()
}

#[test]
fn panels_hide_float_move_and_reset() {
    use stagecraft_ui::state::PanelKind;
    let mut h = harness(stagecraft_ui::demo::engine());
    // Hidden and shown again, the tags panel goes back into its group.
    run(&mut h, "ui.toggle_panel", json!({ "panel": "tags", "visible": false }));
    assert_eq!(run(&mut h, "ui.state", Value::Null)["panels"]["tags"], false);
    assert!(h.query_by_label("Tags").is_none());
    run(&mut h, "ui.toggle_panel", json!({ "panel": "tags" }));
    assert!(h.query_by_label("Tags").is_some());
    // The canvas cannot be closed.
    assert!(h.state_mut().run("ui.toggle_panel", json!({ "panel": "canvas", "visible": false })).is_err());

    // Floating: the library gets a window of its own and still works there.
    run(&mut h, "ui.float_panel", json!({ "panel": "library", "x": 400, "y": 200, "width": 300, "height": 300 }));
    assert_eq!(floating_panels(&h), vec![PanelKind::Library]);
    assert!(h.query_by_label("Fixture type StageCraft Demo@Spot 300").is_some());

    // A tab dragged onto the inspector's tab strip joins that group.
    let tags = h.get_by_label("Tags").rect().center();
    let inspector = h.get_by_label("Inspector").rect();
    let target = egui::pos2(inspector.right() + 40.0, inspector.center().y);
    drag(&mut h, tags, target);
    let ws = run(&mut h, "ui.layout", Value::Null);
    let text = ws.to_string();
    let group = |a: &str, b: &str| text.contains(&format!("\"panels\":[\"{a}\",\"{b}\"]"));
    assert!(group("inspector", "tags"), "{text}");

    // The same through ui.layout, as an AI would.
    run(
        &mut h,
        "ui.layout",
        json!({ "actions": [{ "Move": { "panel": "share", "anchor": "inspector", "placement": { "Tab": { "before": null } } } }] }),
    );
    let text = run(&mut h, "ui.layout", Value::Null).to_string();
    assert!(text.contains("\"panels\":[\"inspector\",\"tags\",\"share\"]"), "{text}");

    // View > Reset Layout.
    run(&mut h, "ui.reset_layout", Value::Null);
    assert_eq!(h.state().ui.layout.layout, stagecraft_ui::dock::default_layout());
    assert!(floating_panels(&h).is_empty());
}

// ---- menus -----------------------------------------------------------------------

#[test]
fn the_in_window_menus_run_commands() {
    let mut h = harness(stagecraft_ui::demo::engine());
    h.get_by_label("Insert").click();
    h.run();
    h.get_by_label("Batten").click();
    h.run();
    assert_eq!(h.state().ui.tool, stagecraft_ui::Tool::Batten);
    h.get_by_label("Window").click();
    h.run();
    h.get_by_role_and_label(egui::accesskit::Role::MenuItemCheckBox, "Table").click();
    h.run();
    assert!(!h.state().ui.layout.layout.contains(&stagecraft_ui::state::PanelKind::Table));
    // Every item names a command one of the registries (or the app) runs.
    let bar = stagecraft_ui::menu::bar(h.state());
    assert!(bar.items().len() > 40);
}
