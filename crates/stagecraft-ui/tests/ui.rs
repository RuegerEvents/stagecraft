//! The GUI driven headlessly: widgets found by accessible name, pointer events at
//! world positions on the plan, and checks on the document.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};
use stagecraft_core::{Engine, Id, ObjectKind};
use stagecraft_ui::Studio;

fn harness(engine: Engine) -> Harness<'static, Studio> {
    let mut h = Harness::builder().with_context_setup(stagecraft_ui::framework::install_headless)
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
    h.event(Event::Text("4.5m".into()));
    h.run();
    assert!(h.query_by_label_contains("Length: 4.5m").is_some(), "the typed length shows next to the pointer");
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
    h.event(Event::Text("2000".into()));
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
    // Technical commands only show after `>`, with their titles.
    h.get_by_label("Search commands").type_text("ui.state");
    h.run();
    assert!(h.query_by_label("UI State").is_none());
    h.state_mut().ui.palette.query.clear();
    h.run();
    h.get_by_label("Search commands").type_text("> ui.state");
    h.run();
    h.get_by_label("UI State").click();
    h.run();
    assert!(!h.state().ui.palette.open);
    assert!(h.state().ui.status.as_ref().unwrap().0.starts_with("UI State"), "{:?}", h.state().ui.status);
}

fn open_palette(h: &mut Harness<'static, Studio>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::K);
    h.run();
    assert!(h.state().ui.palette.open);
}

fn type_query(h: &mut Harness<'static, Studio>, text: &str) {
    h.get_by_label("Search commands").type_text(text);
    h.run();
}

/// The first row's item.
fn first_row(h: &Harness<'static, Studio>) -> stagecraft_ui::palette::Item {
    stagecraft_ui::palette::rows(h.state())[0].item
}

fn first_title(h: &Harness<'static, Studio>) -> String {
    match first_row(h) {
        stagecraft_ui::palette::Item::Command(i) => stagecraft_ui::command_meta::META[i].title().to_owned(),
        other => format!("{other:?}"),
    }
}

#[test]
fn the_palette_opens_groups_and_goes_back() {
    use stagecraft_ui::command_meta::Group;
    let mut h = harness(stagecraft_ui::demo::engine());
    open_palette(&mut h);
    // Nothing run yet, nothing selected: the groups, File first.
    assert_eq!(first_row(&h), stagecraft_ui::palette::Item::Group(Group::File));
    assert!(h.query_by_label("Group Rigging").is_some());
    assert!(h.query_by_label("Group Advanced").is_none(), "technical commands have no group of their own");
    // → opens the highlighted group, ← goes back.
    h.key_press(Key::ArrowRight);
    h.run();
    assert_eq!(h.state().ui.palette.group, Some(Group::File));
    assert!(h.query_by_label("Export Sheets as PDF…").is_some());
    h.key_press(Key::ArrowLeft);
    h.run();
    assert_eq!(h.state().ui.palette.group, None);
    // A click opens one too; Backspace on the empty query goes back.
    h.get_by_label("Group Rigging").click();
    h.run();
    assert_eq!(h.state().ui.palette.group, Some(Group::Rigging));
    assert!(h.query_by_label("Create Truss Piece…").is_some());
    assert!(h.query_by_label("Draw Wall").is_none(), "only the group's commands");
    h.key_press(Key::Backspace);
    h.run();
    assert_eq!(h.state().ui.palette.group, None);
    // Esc closes from the top.
    h.key_press(Key::Escape);
    h.run();
    assert!(!h.state().ui.palette.open);
}

#[test]
fn the_palette_finds_by_title_alias_and_fid() {
    let mut h = harness(stagecraft_ui::demo::engine());
    open_palette(&mut h);
    type_query(&mut h, "auto patch");
    assert_eq!(first_title(&h), "Auto-Patch Fixtures…");
    h.state_mut().ui.palette.query.clear();
    h.run();
    // German and Vectorworks names.
    for (alias, wanted) in [("Zug", "batten"), ("Traverse", "truss"), ("Podest", "platform"), ("Klasse", "tag")] {
        h.state_mut().ui.palette.query.clear();
        h.run();
        type_query(&mut h, alias);
        let stagecraft_ui::palette::Item::Command(i) = first_row(&h) else { panic!("{alias}: {:?}", first_row(&h)) };
        assert!(stagecraft_ui::command_meta::META[i].key_str().contains(wanted), "{alias}: {}", first_title(&h));
    }
    // A FID finds the fixture; Enter selects it and closes the palette.
    h.state_mut().ui.palette.query.clear();
    h.run();
    type_query(&mut h, "101");
    let doc = h.state().engine.doc().clone();
    let foh1 = doc
        .objects
        .values()
        .find(|o| matches!(o.kind, ObjectKind::Fixture { fid: Some(101), .. }))
        .expect("FID 101 in the demo")
        .id;
    assert_eq!(first_row(&h), stagecraft_ui::palette::Item::Object(foh1));
    h.key_press(Key::Enter);
    h.run();
    assert!(!h.state().ui.palette.open);
    assert_eq!(h.state().engine.selection(), [foh1]);
    // `@` searches objects only, `#` tags.
    open_palette(&mut h);
    type_query(&mut h, "#light");
    assert!(matches!(first_row(&h), stagecraft_ui::palette::Item::Tag(_)));
}

#[test]
fn technical_commands_need_the_prefix() {
    let mut h = harness(Engine::new());
    open_palette(&mut h);
    type_query(&mut h, "batch");
    assert!(h.query_by_label("Batch").is_none());
    h.state_mut().ui.palette.query.clear();
    h.run();
    type_query(&mut h, ">batch");
    assert!(h.query_by_label("Batch").is_some());
    // `>` alone lists every command of both registries.
    h.state_mut().ui.palette.query = ">".into();
    h.run();
    let n = stagecraft_ui::palette::rows(h.state()).len();
    let registered =
        stagecraft_core::registry().entries().len() + stagecraft_ui::commands::ui_registry().entries().len();
    assert!(n >= registered, "{n} rows for {registered} commands");
}

#[test]
fn the_palette_runs_a_form_inline_and_remembers_it() {
    let mut e = Engine::new();
    let b = id(&e.execute("batten.create", json!({ "name": "Zug 1", "y": 2000, "trim": 8000 })).unwrap());
    let mut h = harness(e);
    // Without a batten selected the command is greyed and says why.
    open_palette(&mut h);
    type_query(&mut h, "edit batten");
    assert_eq!(first_title(&h), "Edit Batten…");
    h.key_press(Key::Enter);
    h.run();
    assert!(h.state().ui.palette.chosen.is_none());
    assert_eq!(h.state().ui.palette.error.as_deref(), Some("Select one batten first"));
    h.key_press(Key::Escape);
    h.key_press(Key::Escape);
    h.run();
    assert!(!h.state().ui.palette.open);

    run(&mut h, "selection.set", json!({ "ids": [b] }));
    open_palette(&mut h);
    // With the batten selected it is suggested first.
    let rows = stagecraft_ui::palette::rows(h.state());
    assert!(
        rows.iter().any(|r| r.section == stagecraft_ui::palette::Section::Suggested
            && matches!(r.item, stagecraft_ui::palette::Item::Command(i) if stagecraft_ui::command_meta::META[i].id == "batten.update")),
        "batten.update suggested for a batten"
    );
    type_query(&mut h, "edit batten");
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().ui.palette.chosen.as_deref(), Some("batten.update"));
    assert_eq!(h.state().ui.palette.values.get("id"), Some(&b.to_string()), "the id comes from the selection");
    // Esc goes back to the list, Enter opens the form again.
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().ui.palette.chosen.is_none() && h.state().ui.palette.open);
    assert_eq!(h.state().ui.palette.query, "edit batten");
    h.key_press(Key::Enter);
    h.run();
    h.get_by_label("Argument trim").click();
    h.run();
    h.get_by_label("Argument trim").type_text("7500");
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert!(!h.state().ui.palette.open);
    let o = &h.state().engine.doc().objects[&b];
    assert_eq!(o.placement.position[2], 7500.0);
    assert_eq!(h.state().ui.recent.first().map(String::as_str), Some("batten.update"));
    // The next time it is under Recent, and the settings keep it.
    assert_eq!(h.state().prefs.get().palette.recent.first().map(String::as_str), Some("batten.update"));
    open_palette(&mut h);
    let rows = stagecraft_ui::palette::rows(h.state());
    assert_eq!(rows[0].section, stagecraft_ui::palette::Section::Recent);
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

/// Project Settings (from the Inspector's Document link) set the sheet language, the
/// dimension format and the title block fields through `doc.set_meta`.
#[test]
fn project_settings_set_sheet_language_and_fields() {
    let mut h = harness(Engine::new());
    assert!(h.query_by_label("Inspector Venue address").is_none(), "the fields moved out of the Inspector");
    h.get_by_label("Project Settings").click();
    h.run_steps(6);
    assert!(h.state().ui.project_settings);
    h.get_by_label("Settings sheet language").click();
    h.run_steps(6);
    h.get_by_label("Deutsch").click();
    h.run_steps(6);
    assert_eq!(h.state().engine.doc().meta.sheet_language, stagecraft_core::units::Language::De);
    h.get_by_label("Settings dimension format").click();
    h.run_steps(6);
    h.get_by_label("Millimetres (6155)").click();
    h.run_steps(6);
    assert_eq!(h.state().engine.doc().meta.dimension_format, stagecraft_core::units::DimFormat::Mm);
    h.get_by_label("Settings Venue address").click();
    h.run_steps(6);
    h.get_by_label("Settings Venue address").type_text("1 Street / 12345 Town");
    h.key_press(Key::Enter);
    h.run_steps(6);
    assert_eq!(h.state().engine.doc().meta.venue_address, "1 Street\n12345 Town");
    h.get_by_label("Close Project Settings").click();
    h.run_steps(6);
    assert!(!h.state().ui.project_settings);
    // ⌘, opens it again; so does the command.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run_steps(6);
    assert!(h.state().ui.project_settings);
    run(&mut h, "ui.project_settings", json!({ "open": false }));
    assert!(!h.state().ui.project_settings);
}

/// Switching the project to metres in Project Settings: the Inspector, the heads-up
/// boxes, the table and the layers show and take metres; explicit units still win;
/// the document keeps mm.
#[test]
fn a_metre_project_shows_and_takes_metres_everywhere() {
    let mut e = Engine::new();
    let p = id(&e.execute("platform.create", json!({ "width": 2000, "depth": 1000, "height": 400 })).unwrap());
    let b = id(&e.execute("batten.create", json!({ "number": 1, "y": 1500, "trim": 7500 })).unwrap());
    let mut h = harness(e);
    run(&mut h, "ui.project_settings", json!({ "open": true }));
    h.get_by_label("Settings length unit").click();
    h.run_steps(6);
    h.get_by_label("Metres (m)").click();
    h.run_steps(6);
    assert_eq!(h.state().engine.doc().meta.units.length, stagecraft_core::units::LengthUnit::M);
    assert!(h.query_by_label("Settings example 6.155 m").is_some());
    h.get_by_label("Settings precision").click();
    h.run_steps(6);
    h.get_by_label("2 decimals").click();
    h.run_steps(6);
    assert_eq!(h.state().engine.doc().meta.units.precision, Some(2));
    run(&mut h, "ui.project_settings", json!({ "open": false }));

    // Inspector: labels name the unit, values are metres, typing takes metres.
    run(&mut h, "selection.set", json!({ "ids": [p] }));
    assert!(h.query_by_label("Width (m)").is_some(), "the label shows the unit");
    let width = |h: &Harness<'static, Studio>| match h.state().engine.doc().objects[&p].kind {
        ObjectKind::Platform { width, .. } => width,
        _ => unreachable!(),
    };
    h.get_by_label("Inspector Width").click();
    h.run();
    h.get_by_label("Inspector Width").type_text("2,5");
    h.key_press(Key::Enter);
    h.run_steps(4);
    assert_eq!(width(&h), 2500.0, "2,5 in a metre project");
    h.get_by_label("Inspector Width").click();
    h.run();
    h.get_by_label("Inspector Width").type_text("1500mm");
    h.key_press(Key::Enter);
    h.run_steps(4);
    assert_eq!(width(&h), 1500.0, "an explicit unit wins");
    assert!(h.query_by_label_contains("X (m)").is_some());

    // Heads-up boxes: plain numbers are metres, the live value too.
    run(&mut h, "selection.set", json!({ "ids": [] }));
    run(&mut h, "ui.zoom", json!({ "scale": 0.05 }));
    run(&mut h, "ui.pan", json!({ "center": [0, -6000] }));
    run(&mut h, "ui.set_tool", json!({ "tool": "wall" }));
    click_world(&mut h, [0.0, -6000.0]);
    h.hover_at(screen(&h, [3000.0, -6000.0]));
    h.run();
    assert!(h.query_by_label_contains("Length: 3.00 m").is_some(), "live length in m");
    h.event(Event::Text("4.5".into()));
    h.run();
    assert!(h.query_by_label_contains("Length: 4.5 m (locked, typing)").is_some());
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().ui.gesture.points[1], [4500.0, -6000.0]);
    h.key_press(Key::Escape);
    h.key_press(Key::Escape);
    h.key_press(Key::Escape);
    h.run();

    // Table: metre cells and headers; typing and pasting from Excel take metres.
    h.state_mut().ui.table.kind = Some("batten".into());
    h.run();
    let doc = h.state().engine.doc().clone();
    let col = stagecraft_ui::table::Col::Trim;
    assert_eq!(stagecraft_ui::table::cell_text(&doc, &doc.objects[&b], col), "7.5");
    assert_eq!(col.title(&doc), "Trim (m)");
    h.get_by_label("Trim 1").click();
    h.run();
    h.event(Event::Text("8,25".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().engine.doc().objects[&b].placement.position[2], 8250.0);
    h.get_by_label("Y 1").click();
    h.run();
    h.event(Event::Paste("2.5\t9\n".into()));
    h.run();
    let pos = h.state().engine.doc().objects[&b].placement.position;
    assert_eq!((pos[1], pos[2]), (2500.0, 9000.0), "pasted cells are metres");

    // Layers: the elevation in metres.
    let layer = *h.state().engine.doc().layers.keys().next().unwrap();
    let name = format!("Elevation {}", h.state().engine.doc().layers[&layer].name);
    h.get_by_label(&name).click();
    h.run();
    h.get_by_label(&name).type_text("-1");
    h.key_press(Key::Enter);
    h.run_steps(4);
    assert_eq!(h.state().engine.doc().layers[&layer].elevation, -1000.0);
}

// ---- docking ---------------------------------------------------------------------

fn floating_panels(h: &Harness<'static, Studio>) -> Vec<stagecraft_ui::state::PanelKind> {
    h.state().ui.layout.layout().floating.iter().flat_map(|w| w.panels().copied().collect::<Vec<_>>()).collect()
}

#[test]
fn panels_hide_float_move_and_reset() {
    use stagecraft_ui::state::PanelKind;
    let mut h = harness(stagecraft_ui::demo::engine());
    // Hidden and shown again, the tags panel goes back into its group.
    run(&mut h, "ui.toggle_panel", json!({ "panel": "tags", "visible": false }));
    assert_eq!(run(&mut h, "ui.state", Value::Null)["panels"]["tags"], false);
    assert!(h.query_by_role_and_label(egui::accesskit::Role::Tab, "Tags").is_none());
    run(&mut h, "ui.toggle_panel", json!({ "panel": "tags" }));
    assert!(h.query_by_role_and_label(egui::accesskit::Role::Tab, "Tags").is_some());
    // The canvas cannot be closed.
    assert!(h.state_mut().run("ui.toggle_panel", json!({ "panel": "canvas", "visible": false })).is_err());

    // Floating: the library gets a window of its own and still works there.
    run(&mut h, "ui.float_panel", json!({ "panel": "library", "x": 400, "y": 200, "width": 300, "height": 300 }));
    assert_eq!(floating_panels(&h), vec![PanelKind::Library]);
    assert!(h.query_by_label("Fixture type StageCraft Demo@Spot 300").is_some());

    // A tab dragged onto the inspector's tab strip joins that group.
    let tags = h.get_by_role_and_label(egui::accesskit::Role::Tab, "Tags").rect().center();
    let inspector = h.get_by_role_and_label(egui::accesskit::Role::Tab, "Inspector").rect();
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
    assert_eq!(h.state().ui.layout.layout(), stagecraft_ui::dock::default_workspace().layout());
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
    assert!(!h.state().ui.layout.layout().contains(&stagecraft_ui::state::PanelKind::Table));
    // Every item names a command one of the registries (or the app) runs.
    let bar = stagecraft_ui::menu::bar(h.state());
    assert!(bar.items().len() > 40);
}

// ---- toolbar ---------------------------------------------------------------------

#[test]
fn tools_sit_in_the_palette_and_tool_bar_commands_that_do_not_fit_go_into_a_menu() {
    let mut h = harness(stagecraft_ui::demo::engine());
    assert!(h.query_by_label("More commands").is_none());
    h.get_by_label("Tool Ceiling").click();
    h.run();
    assert_eq!(h.state().ui.tool, stagecraft_ui::Tool::Ceiling);
    // A folded palette section hides its tools; unfolded they are back.
    h.get_by_label("Section Stage").click();
    h.run();
    assert!(h.query_by_label("Tool Ceiling").is_none());
    h.get_by_label("Section Stage").click();
    h.run();
    assert!(h.query_by_label("Tool Ceiling").is_some());
    // A narrow window: the tool bar's last groups move into the ⋯ menu; the palette stays
    // in the layout (craft-ui may fold its column to an icon).
    let mut h = Harness::builder().with_context_setup(stagecraft_ui::framework::install_headless)
        .with_max_steps(8)
        .with_size(vec2(600.0, 700.0))
        .build_ui_state(|ui, s: &mut Studio| s.show(ui), Studio::new(stagecraft_ui::demo::engine()));
    h.run();
    assert!(h.query_by_label("Toolbar Tutorial").is_none());
    assert!(h.query_by_label("Toolbar Save").is_some());
    assert!(h.state().ui.layout.layout().contains(&stagecraft_ui::state::PanelKind::Tools));
    h.get_by_label("More commands").click();
    h.run();
    h.get_by_label("Menu Tutorial").click();
    h.run();
    assert!(h.state().ui.tutorial.active);
    assert_eq!(stagecraft_ui::panels::tool_tip(stagecraft_ui::Tool::Wall), "Wall (W)");
}

#[test]
fn table_columns_resize_by_dragging_the_header() {
    let mut h = harness(stagecraft_ui::demo::engine());
    let handle = h.get_by_label("Resize Name").rect().center();
    drag(&mut h, handle, handle + vec2(60.0, 0.0));
    let w = h.state().ui.table.widths.get(&stagecraft_ui::table::Col::Name).copied().expect("dragged width");
    assert!(w > stagecraft_ui::table::Col::Name.width(), "{w}");
}

#[test]
fn the_tool_palette_folds_to_two_or_one_column() {
    let mut h = harness(stagecraft_ui::demo::engine());
    let x = |h: &Harness<'static, Studio>, tool: &str| h.get_by_label(&format!("Tool {tool}")).rect().center().x;
    let y = |h: &Harness<'static, Studio>, tool: &str| h.get_by_label(&format!("Tool {tool}")).rect().center().y;
    // Auto: Select, Move and Rotate share the first row.
    assert_eq!(y(&h, "Select"), y(&h, "Rotate"));
    // The ≡ menu's Compact entries.
    h.get_by_label("Panel menu Tools").click();
    h.run();
    h.get_by_label("Columns one").click();
    h.run();
    h.run();
    assert_eq!(h.state().ui.tool_columns, stagecraft_ui::state::ToolColumns::One);
    assert_eq!(x(&h, "Select"), x(&h, "Move"));
    assert!(y(&h, "Move") > y(&h, "Select"));
    let tools = h.get_by_label("Tool Select").rect();
    assert!(tools.right() < 60.0, "the dock narrowed to one button: {tools:?}");
    // Tooltips stay; section titles are rules but keep their names.
    assert!(h.query_by_label("Section Stage").is_some());
    run(&mut h, "ui.tool_columns", json!({ "columns": "two" }));
    h.run();
    assert_eq!(y(&h, "Select"), y(&h, "Move"));
    assert!(y(&h, "Rotate") > y(&h, "Select"));
    assert_eq!(x(&h, "Select"), x(&h, "Rotate"));
    run(&mut h, "ui.tool_columns", json!({ "columns": "auto" }));
    h.run();
    assert_eq!(y(&h, "Select"), y(&h, "Rotate"));
    // Saved per user.
    run(&mut h, "ui.tool_columns", json!({ "columns": "one" }));
    h.run();
    assert_eq!(h.state().prefs.get().tool_palette.columns, stagecraft_ui::state::ToolColumns::One);
}

#[test]
fn the_palette_shows_command_ids_only_after_the_prefix() {
    use stagecraft_ui::palette::{Item, shown_id, technical};
    let mut h = harness(stagecraft_ui::demo::engine());
    open_palette(&mut h);
    type_query(&mut h, "dimension");
    let rows = stagecraft_ui::palette::rows(h.state());
    assert!(matches!(rows[0].item, Item::Command(_)));
    // Highlighted or not, no row shows an ID.
    assert!(!technical(h.state()));
    assert!(rows.iter().all(|r| shown_id(h.state(), &r.item).is_none()));
    // A form opened from a search shows no ID either.
    h.state_mut().ui.palette.query.clear();
    h.run();
    stagecraft_ui::palette::choose(h.state_mut(), "patch.auto");
    h.run();
    assert!(h.query_by_label("Run").is_some(), "the form is open");
    assert!(h.query_by_label("patch.auto").is_none());
    assert!(!technical(h.state()));
    h.state_mut().ui.palette.close();
    h.run();
    // `>`: every row and the form carry their IDs.
    open_palette(&mut h);
    type_query(&mut h, ">ui.set_tool");
    let rows = stagecraft_ui::palette::rows(h.state());
    assert!(technical(h.state()));
    assert!(rows.iter().any(|r| shown_id(h.state(), &r.item).as_deref() == Some("ui.set_tool tool=dimension")));
    h.state_mut().ui.palette.query = ">patch.auto".into();
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().ui.palette.chosen.as_deref(), Some("patch.auto"));
    assert!(h.query_by_label("patch.auto").is_some());
}

#[test]
fn the_status_bar_shows_the_tool_prompt_and_there_is_no_command_line() {
    let mut h = harness(stagecraft_ui::demo::engine());
    assert!(h.query_by_label("Command line").is_none());
    run(&mut h, "ui.set_tool", json!({ "tool": "wall" }));
    let prompt = stagecraft_ui::chrome::prompt(h.state());
    assert!(prompt.starts_with("Wall: "), "{prompt}");
    assert!(h.query_by_label(&prompt).is_some(), "the prompt is in the status bar");
    // A message from a command shares the bar with the prompt.
    h.state_mut().ui.info("Placed it");
    h.run();
    assert!(h.query_by_label("Placed it").is_some());
    assert!(h.query_by_label(&prompt).is_some());
}

fn frame_size(h: &Harness<'static, Studio>, sheet: Id, vp: Id) -> [f64; 2] {
    let page = stagecraft_sheet::pages(h.state().engine.doc(), &[sheet]).remove(0);
    let hit = page.viewports.iter().find(|v| v.id == vp).unwrap();
    [hit.max[0] - hit.min[0], hit.max[1] - hit.min[1]]
}

/// Picking another scale in the Inspector keeps the frame on the paper and zooms the
/// view around its centre, for a cropped and for a fitted viewport.
#[test]
fn a_new_scale_keeps_the_viewport_frame() {
    let mut h = harness(stagecraft_ui::demo::engine());
    let doc = h.state().engine.doc().clone();
    let l102 = doc.sheets.values().find(|s| s.number == "L-102").unwrap().clone();
    let front = l102.viewports[0].clone();
    let section = l102.viewports[1].clone();
    assert!(front.crop.is_some() && section.crop.is_none());
    for vp in [&front, &section] {
        run(&mut h, "ui.sheet", json!({ "sheet": "L-102", "viewport": vp.id }));
        let before = frame_size(&h, l102.id, vp.id);
        h.get_by_label("Viewport scale").click();
        h.run();
        let want = if vp.scale == 100.0 { "1:50" } else { "1:75" };
        h.get_by_label(want).click();
        h.run();
        let now = h.state().engine.doc().sheets[&l102.id].viewport(vp.id).unwrap().clone();
        assert_eq!(format!("1:{}", now.scale), want);
        let after = frame_size(&h, l102.id, vp.id);
        assert!((after[0] - before[0]).abs() < 1e-6 && (after[1] - before[1]).abs() < 1e-6, "{before:?} → {after:?}");
        assert!(now.crop.is_some(), "a fitted viewport keeps its frame through a crop");
    }
}

/// Dragging a viewport shows it where it goes (moved or re-cropped from the composed
/// page) before the drop, and keeps it there until the recomposed page arrives.
#[test]
fn a_dragged_viewport_shows_live_and_stays_after_the_drop() {
    let mut h = harness(stagecraft_ui::demo::engine());
    run(&mut h, "ui.sheet", json!({ "sheet": "L-102" }));
    let doc = h.state().engine.doc().clone();
    let l102 = doc.sheets.values().find(|s| s.number == "L-102").unwrap().id;
    let vp = doc.sheets[&l102].viewports[0].clone();
    let page = stagecraft_sheet::pages(&doc, &[l102]).remove(0);
    let hit = page.viewports.iter().find(|v| v.id == vp.id).unwrap().clone();
    assert!(matches!(page.prims[hit.prims[0]], stagecraft_sheet::prim::Prim::Clip { .. }));
    let journal = h.state().engine.journal().len();
    // Halfway through a move: the live frame is 10 mm to the right, nothing committed.
    let c = [(hit.min[0] + hit.max[0]) / 2.0, (hit.min[1] + hit.max[1]) / 2.0];
    let (a, b) = (paper(&h, c), paper(&h, [c[0] + 20.0, c[1]]));
    h.hover_at(a);
    h.run();
    h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    for i in 1..=5 {
        h.event(Event::PointerMoved(a + (b - a) * (i as f32 / 10.0)));
        h.run();
    }
    let live = h.state().ui.sheets.live.clone().expect("a live preview while dragging");
    assert!(live.moved && live.until.is_none());
    assert!((live.min[0] - hit.min[0] - 10.0).abs() < 0.5, "{live:?}");
    assert_eq!(h.state().engine.journal().len(), journal, "nothing runs before the drop");
    for i in 6..=10 {
        h.event(Event::PointerMoved(a + (b - a) * (i as f32 / 10.0)));
        h.run();
    }
    h.event(Event::PointerButton {
        pos: b,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert_eq!(h.state().engine.journal().len(), journal + 1, "one viewport.update on release");
    // Once the page with the change is composed the preview goes.
    for _ in 0..200 {
        if h.state().ui.sheets.live.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        h.run();
    }
    assert!(h.state().ui.sheets.live.is_none(), "the recomposed page replaced the preview");

    // A corner drag re-crops: same content on the paper, new clip.
    let page = stagecraft_sheet::pages(h.state().engine.doc(), &[l102]).remove(0);
    let hit = page.viewports.iter().find(|v| v.id == vp.id).unwrap().clone();
    let corner = paper(&h, hit.max);
    h.hover_at(corner);
    h.run();
    let to = paper(&h, [hit.max[0] - 30.0, hit.max[1] - 20.0]);
    h.event(Event::PointerButton {
        pos: corner,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run();
    for i in 1..=5 {
        h.event(Event::PointerMoved(corner + (to - corner) * (i as f32 / 5.0)));
        h.run();
    }
    let live = h.state().ui.sheets.live.clone().expect("live while cropping");
    assert!(!live.moved);
    assert!((live.max[0] - (hit.max[0] - 30.0)).abs() < 0.5 && (live.min[0] - hit.min[0]).abs() < 0.5, "{live:?}");
    h.event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    let crop = h.state().engine.doc().sheets[&l102].viewport(vp.id).unwrap().crop.unwrap();
    assert!((crop[1][0] - crop[0][0] - (20400.0 - 3000.0)).abs() < 50.0, "30 mm less at 1:100: {crop:?}");
}
