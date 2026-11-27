//! Typed dimensions while drawing, through real key events: the heads-up length and
//! angle boxes next to the rubber band, typing on every segment (also after a click),
//! Shift squaring with a typed length, Tab, Esc, and the other length tools. Most run
//! in a metre project (`harness`); plain numbers follow the project unit, so the mm and
//! ft-in tests type the same keys and get other lengths.

use egui::{Event, Key, Modifiers, PointerButton, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};
use stagecraft_core::model::P2;
use stagecraft_core::{Engine, ObjectKind};
use stagecraft_ui::Studio;

/// A project in metres.
fn harness(engine: Engine) -> Harness<'static, Studio> {
    let mut h = harness_in(engine, "mm");
    run(&mut h, "doc.set_units", json!({ "length_unit": "m" }));
    h
}

fn harness_in(engine: Engine, unit: &str) -> Harness<'static, Studio> {
    let mut engine = engine;
    engine.execute("doc.set_units", json!({ "length_unit": unit })).unwrap();
    let mut h = Harness::builder().with_context_setup(stagecraft_ui::framework::install_headless)
        .with_max_steps(8)
        .with_size(vec2(1400.0, 900.0))
        // Real frame times, so separate clicks are not double clicks.
        .with_step_dt(1.0 / 60.0)
        .build_ui_state(|ui, s: &mut Studio| s.show(ui), Studio::new(engine));
    h.run();
    run(&mut h, "ui.zoom", json!({ "scale": 0.03 }));
    run(&mut h, "ui.pan", json!({ "center": [6000, 3000] }));
    h
}

fn run(h: &mut Harness<'static, Studio>, name: &str, args: Value) -> Value {
    let v = h.state_mut().run(name, args).unwrap_or_else(|e| panic!("{name}: {e}"));
    h.run();
    v
}

fn modifiers(h: &mut Harness<'static, Studio>, m: Modifiers) {
    h.event(Event::ModifiersChanged(m));
    h.run();
}

fn hover(h: &mut Harness<'static, Studio>, p: P2) {
    let pos = h.state().ui.camera.to_screen(p);
    h.hover_at(pos);
    h.run();
}

fn click(h: &mut Harness<'static, Studio>, p: P2, m: Modifiers) {
    let pos = h.state().ui.camera.to_screen(p);
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: m });
    }
    h.run();
    // Half a second, so the next click is not a double click.
    for _ in 0..40 {
        h.step();
    }
}

/// Typing as a keyboard does: per character a key press, its text and the release.
fn keys(h: &mut Harness<'static, Studio>, text: &str) {
    for c in text.chars() {
        let key = Key::from_name(&c.to_ascii_uppercase().to_string());
        let ev = |pressed| Event::Key {
            key: key.expect("checked"),
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        if key.is_some() {
            h.event(ev(true));
        }
        h.event(Event::Text(c.to_string()));
        if key.is_some() {
            h.event(ev(false));
        }
        h.run();
    }
}

fn press(h: &mut Harness<'static, Studio>, key: Key) {
    h.key_press(key);
    h.run();
}

fn points(h: &Harness<'static, Studio>) -> Vec<P2> {
    h.state().ui.gesture.points.clone()
}

fn near(a: P2, b: P2) -> bool {
    (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
}

fn has(h: &Harness<'static, Studio>, label: &str) -> bool {
    h.query_by_label_contains(label).is_some()
}

fn wall_tool(h: &mut Harness<'static, Studio>) {
    h.get_by_label("Tool Wall").click();
    h.run();
}

#[test]
fn a_length_can_be_typed_on_every_segment_also_after_a_click() {
    let mut h = harness(Engine::new());
    wall_tool(&mut h);
    click(&mut h, [0.0, 0.0], Modifiers::NONE);
    hover(&mut h, [3000.0, 0.0]);
    keys(&mut h, "15m");
    // The click takes the typed length, not the pointer (bug: the corner went to the
    // pointer and "15m" stayed in the box).
    click(&mut h, [3000.0, 0.0], Modifiers::NONE);
    assert!(near(points(&h)[1], [15000.0, 0.0]), "{:?}", points(&h));
    assert!(h.state().ui.gesture.dim.is_none(), "the boxes are empty for the next side");
    // Second side: typing again works (bug: "4" went after the stale "15m").
    hover(&mut h, [15000.0, 3000.0]);
    keys(&mut h, "4");
    assert_eq!(h.state().ui.gesture.dim.as_ref().map(|d| d.length.as_str()), Some("4"));
    press(&mut h, Key::Enter);
    assert!(near(points(&h)[2], [15000.0, 4000.0]), "{:?}", points(&h));
    // Third side with Enter as well.
    hover(&mut h, [9000.0, 4000.0]);
    keys(&mut h, "2,5m");
    press(&mut h, Key::Enter);
    assert!(near(points(&h)[3], [12500.0, 4000.0]), "{:?}", points(&h));
    // An empty Enter ends the wall; the typed sides became driving dimensions.
    press(&mut h, Key::Enter);
    let doc = h.state().engine.doc();
    let sketch = doc.sketches.values().next().expect("drawn into the sketch");
    let mut driving: Vec<f64> = sketch.dimensions.iter().filter(|d| d.driving).map(|d| d.value).collect();
    driving.sort_by(f64::total_cmp);
    assert_eq!(driving, vec![2500.0, 4000.0, 15000.0]);
}

#[test]
fn shift_squares_the_direction_and_the_typed_length_holds() {
    let mut h = harness(Engine::new());
    wall_tool(&mut h);
    click(&mut h, [0.0, 0.0], Modifiers::NONE);
    keys(&mut h, "5");
    // Shift held, pointer a little off the axis and 3 m out: squared to +x, 5 m long.
    modifiers(&mut h, Modifiers::SHIFT);
    hover(&mut h, [3000.0, 400.0]);
    click(&mut h, [3000.0, 400.0], Modifiers::SHIFT);
    assert!(near(points(&h)[1], [5000.0, 0.0]), "bug: the corner went to the pointer: {:?}", points(&h));
    // Same with Enter while Shift is held: up the y axis.
    modifiers(&mut h, Modifiers::NONE);
    keys(&mut h, "3");
    modifiers(&mut h, Modifiers::SHIFT);
    hover(&mut h, [5300.0, 1200.0]);
    h.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
    h.run();
    assert!(near(points(&h)[2], [5000.0, 3000.0]), "{:?}", points(&h));
    let segs = &h.state().ui.gesture.segs;
    assert!(segs.iter().all(|s| s.axis_lock), "Shift becomes horizontal/vertical: {segs:?}");
    assert_eq!(segs.iter().map(|s| s.length).collect::<Vec<_>>(), vec![Some(5000.0), Some(3000.0)]);
}

#[test]
fn the_boxes_show_live_values_tab_cycles_and_typed_values_lock() {
    let mut h = harness(Engine::new());
    wall_tool(&mut h);
    click(&mut h, [0.0, 0.0], Modifiers::NONE);
    hover(&mut h, [3000.0, 3000.0]);
    // Live: length and angle of the rubber band, nothing locked.
    assert!(has(&h, "Length: 4.243 m"), "live length");
    assert!(has(&h, "Angle: 45.0°"), "live angle");
    assert!(!has(&h, "locked"));
    // Tab before typing goes into the length box; Tab again into the angle box.
    press(&mut h, Key::Tab);
    assert!(has(&h, "Length: 4.243 m (typing)"), "Tab focuses the length box");
    keys(&mut h, "2");
    press(&mut h, Key::Tab);
    assert!(has(&h, "Length: 2 m (locked)"));
    keys(&mut h, "90");
    assert!(has(&h, "Angle: 90° (locked, typing)"));
    // Both locked: the point ignores the pointer.
    hover(&mut h, [-4000.0, 500.0]);
    press(&mut h, Key::Tab);
    assert!(has(&h, "Length: 2 m (locked, typing)"), "Tab cycles back to the length");
    // Tab into a box with a value: the next key replaces it.
    keys(&mut h, "3");
    assert!(has(&h, "Length: 3 m (locked, typing)"));
    press(&mut h, Key::Enter);
    assert!(near(points(&h)[1], [0.0, 3000.0]), "{:?}", points(&h));
    assert!(!has(&h, "locked"), "the next segment starts unlocked");
    // Backspace edits, Esc first drops the typed value, then ends the wall (kept).
    keys(&mut h, "12");
    press(&mut h, Key::Backspace);
    assert_eq!(h.state().ui.gesture.dim.as_ref().map(|d| d.length.as_str()), Some("1"));
    press(&mut h, Key::Escape);
    assert!(h.state().ui.gesture.dim.is_none());
    assert_eq!(points(&h).len(), 2, "still drawing");
    press(&mut h, Key::Escape);
    assert!(points(&h).is_empty());
    let walls: Vec<_> = h.state().engine.doc().objects.values().filter(|o| o.kind.type_name() == "wall").collect();
    assert_eq!(walls.len(), 1, "the drawn side stays, as in Fusion");
    assert_eq!(h.state().ui.tool, stagecraft_ui::Tool::Wall, "the tool stays; a third Esc leaves it");
    press(&mut h, Key::Escape);
    assert_eq!(h.state().ui.tool, stagecraft_ui::Tool::Select);
}

#[test]
fn a_typed_angle_alone_lets_the_pointer_choose_the_length() {
    let mut h = harness(Engine::new());
    wall_tool(&mut h);
    click(&mut h, [0.0, 0.0], Modifiers::NONE);
    press(&mut h, Key::Tab);
    press(&mut h, Key::Tab);
    keys(&mut h, "90");
    // The pointer to the side: the point slides along the 90° ray.
    click(&mut h, [800.0, 2500.0], Modifiers::NONE);
    assert!(near(points(&h)[1], [0.0, 2500.0]), "{:?}", points(&h));
}

#[test]
fn unit_letters_after_a_digit_are_not_tool_shortcuts() {
    let mut h = harness(Engine::new());
    wall_tool(&mut h);
    click(&mut h, [0.0, 0.0], Modifiers::NONE);
    hover(&mut h, [0.0, 3000.0]);
    // M is the Move tool, F the Floor tool, T the truss: here they are units.
    keys(&mut h, "10ft");
    assert_eq!(h.state().ui.tool, stagecraft_ui::Tool::Wall);
    press(&mut h, Key::Enter);
    assert!(near(points(&h)[1], [0.0, 3048.0]), "{:?}", points(&h));
    // Both arrive in one frame: still typing.
    h.event(Event::Key {
        key: Key::Num2,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.event(Event::Text("2".into()));
    h.event(Event::Key { key: Key::M, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.event(Event::Text("m".into()));
    h.run();
    assert_eq!(h.state().ui.tool, stagecraft_ui::Tool::Wall);
    assert_eq!(h.state().ui.gesture.dim.as_ref().map(|d| d.length.as_str()), Some("2m"));
}

#[test]
fn truss_move_and_rotate_take_typed_values() {
    let mut h = harness(Engine::new());
    // Truss: start, typed length, click anywhere along the direction.
    h.get_by_label("Tool Truss").click();
    h.run();
    click(&mut h, [0.0, 0.0], Modifiers::NONE);
    hover(&mut h, [1000.0, 0.0]);
    assert!(has(&h, "Length: 1.000 m"), "the truss tool shows the boxes");
    keys(&mut h, "6");
    click(&mut h, [1000.0, 0.0], Modifiers::NONE);
    let truss = h.state().engine.doc().objects.values().find(|o| o.kind.type_name() == "truss").cloned().unwrap();
    let ObjectKind::Truss { length, .. } = truss.kind else { unreachable!() };
    assert!((length - 6000.0).abs() < 1e-6, "{length}");
    // Move: base point, typed distance and angle.
    run(&mut h, "selection.set", json!({ "ids": [truss.id] }));
    h.get_by_label("Tool Move").click();
    h.run();
    click(&mut h, [0.0, 0.0], Modifiers::NONE);
    keys(&mut h, "2");
    press(&mut h, Key::Tab);
    keys(&mut h, "90");
    press(&mut h, Key::Enter);
    let moved = h.state().engine.doc().objects[&truss.id].placement.position;
    assert!(near([moved[0], moved[1]], [0.0, 2000.0]), "{moved:?}");
    // Rotate: pivot, then the angle box takes the keys.
    h.get_by_label("Tool Rotate").click();
    h.run();
    click(&mut h, [0.0, 2000.0], Modifiers::NONE);
    keys(&mut h, "90");
    assert!(has(&h, "Angle: 90° (locked, typing)"));
    press(&mut h, Key::Enter);
    let rot = h.state().engine.doc().objects[&truss.id].placement.rotation[2];
    assert!((rot - 90.0).abs() < 1e-6, "{rot}");
}

#[test]
fn plain_numbers_are_millimetres_in_a_mm_project() {
    // New documents are mm: "1500" is 1.5 m (it used to be 1500 m).
    let mut h = harness_in(Engine::new(), "mm");
    wall_tool(&mut h);
    click(&mut h, [0.0, 0.0], Modifiers::NONE);
    hover(&mut h, [3000.0, 0.0]);
    assert!(has(&h, "Length: 3000 mm"), "live length in mm");
    assert!(has(&h, "type a length (mm)"), "the prompt names the unit");
    keys(&mut h, "1500");
    assert!(has(&h, "Length: 1500 mm (locked, typing)"));
    press(&mut h, Key::Enter);
    assert!(near(points(&h)[1], [1500.0, 0.0]), "{:?}", points(&h));
    // An explicit unit wins.
    hover(&mut h, [1500.0, 3000.0]);
    keys(&mut h, "2,5m");
    press(&mut h, Key::Enter);
    assert!(near(points(&h)[2], [1500.0, 2500.0]), "{:?}", points(&h));
}

#[test]
fn feet_and_inches_projects_take_inches_and_marks() {
    let mut h = harness_in(Engine::new(), "ft_in");
    run(&mut h, "ui.set_snap", json!({ "grid": 0, "endpoints": false, "midpoints": false }));
    wall_tool(&mut h);
    click(&mut h, [0.0, 0.0], Modifiers::NONE);
    hover(&mut h, [6155.0, 0.0]);
    assert!(has(&h, "Length: 20' 2 5/16\""), "live length in ft-in");
    keys(&mut h, "15' 6\"");
    press(&mut h, Key::Enter);
    assert!(near(points(&h)[1], [15.0 * 304.8 + 6.0 * 25.4, 0.0]), "{:?}", points(&h));
    // A bare number is inches.
    hover(&mut h, [4724.4, 3000.0]);
    keys(&mut h, "6");
    assert!(has(&h, "Length: 6\" (locked, typing)"));
    press(&mut h, Key::Enter);
    assert!(near(points(&h)[2], [4724.4, 6.0 * 25.4]), "{:?}", points(&h));
}
