//! Underlays in the GUI: the Calibrate tool scales the underlay under the first click
//! to a typed distance; locked underlays cannot be picked; notes show in the plan.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use serde_json::{Value, json};
use stagecraft_core::{Engine, Id, ObjectKind};
use stagecraft_ui::Studio;

fn harness(engine: Engine) -> Harness<'static, Studio> {
    let mut h = Harness::builder()
        // egui 0.37 styles a widget from its previous-pass state, so a widget that appears
        // asks for one more frame; closing a modal can take six.
        .with_max_steps(8)
        .with_size(vec2(1400.0, 900.0))
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

fn click_world(h: &mut Harness<'static, Studio>, p: [f64; 2]) {
    let pos: Pos2 = h.state().ui.camera.to_screen(p);
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run();
    for _ in 0..40 {
        h.step();
    }
}

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200">
  <rect width="400" height="200" fill="#fff" stroke="#000"/><line x1="100" y1="100" x2="300" y2="100" stroke="#000"/></svg>"##;

fn import(h: &mut Harness<'static, Studio>) -> Id {
    let data = stagecraft_core::sheet::base64_encode(SVG.as_bytes());
    let out = run(h, "underlay.import", json!({ "data_base64": data, "file_name": "room.svg" }));
    serde_json::from_value(out["id"].clone()).unwrap()
}

fn scale(h: &Harness<'static, Studio>, id: Id) -> f64 {
    match &h.state().engine.doc().objects[&id].kind {
        ObjectKind::Underlay { scale, .. } => *scale,
        _ => unreachable!(),
    }
}

#[test]
fn calibrate_tool_scales_the_underlay_under_the_first_click() {
    let mut h = harness(Engine::new());
    let u = import(&mut h);
    run(&mut h, "ui.zoom_to_fit", Value::Null);
    let s0 = scale(&h, u);
    run(&mut h, "ui.set_tool", json!({ "tool": "calibrate" }));
    click_world(&mut h, [100.0 * s0, 100.0 * s0]);
    click_world(&mut h, [300.0 * s0, 100.0 * s0]);
    let pts = h.state().ui.gesture.points.clone();
    assert_eq!(pts.len(), 2, "{pts:?}");
    let d = (pts[1][0] - pts[0][0]).hypot(pts[1][1] - pts[0][1]);
    h.event(Event::Text("8".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    let s1 = scale(&h, u);
    assert!((s1 / s0 - 8000.0 / d).abs() < 1e-9, "{s0} → {s1}, measured {d}");
    assert_eq!(h.state().engine.selection(), &[u]);
    assert!(h.state().ui.gesture.points.is_empty());

    // without a selected underlay, clicking where there is none says so and starts
    // nothing
    run(&mut h, "selection.clear", Value::Null);
    // Zoomed out, so the canvas corner is clear of the underlay whatever the canvas shape.
    run(&mut h, "ui.zoom", json!({ "factor": 0.25 }));
    let cam = h.state().ui.camera;
    let corner = cam.to_world(cam.rect.left_bottom() + vec2(15.0, -15.0));
    click_world(&mut h, corner);
    assert!(h.state().ui.gesture.points.is_empty());
    let (msg, err) = h.state().ui.status.clone().unwrap();
    assert!(err && msg.contains("no underlay"), "{msg}");
}

#[test]
fn locked_underlays_are_not_picked_and_notes_draw() {
    let mut h = harness(Engine::new());
    let u = import(&mut h);
    run(&mut h, "note.create", json!({ "text": "Assumed: 1 m stage", "at": [0, -2000], "frame": true }));
    run(&mut h, "ui.zoom_to_fit", Value::Null);
    let s0 = scale(&h, u);
    let inside = [200.0 * s0, 150.0 * s0];
    run(&mut h, "ui.set_tool", json!({ "tool": "select" }));
    click_world(&mut h, inside);
    assert_eq!(h.state().engine.selection(), &[u]);
    run(&mut h, "underlay.update", json!({ "id": u, "locked": true }));
    run(&mut h, "selection.clear", Value::Null);
    click_world(&mut h, inside);
    assert!(h.state().engine.selection().is_empty());
    h.state_mut().ui.view3d.fit_pending = false;
    if let Ok(dir) = std::env::var("STAGECRAFT_SCREENSHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        h.render().unwrap().save(format!("{dir}/40-underlay.png")).unwrap();
    }
}
