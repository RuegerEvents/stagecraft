//! Floors through the GUI: the Floor tool's height options, closing the outline, picking
//! the floor (not its sketch lines) by clicking inside, the Inspector's height fields,
//! and the floor showing in 3D even when the 3D camera was fitted to an empty document.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use glam::DVec3;
use serde_json::{Value, json};
use stagecraft_core::{Engine, Id, ObjectKind};
use stagecraft_ui::{Studio, ThemePref};

fn harness(gpu: bool) -> Harness<'static, Studio> {
    let mut studio = Studio::new(Engine::new());
    studio.ui.theme = ThemePref::Dark;
    let mut b = Harness::builder()
        .with_max_steps(8)
        .with_size(vec2(1400.0, 900.0))
        // Real frame times, so separate clicks are not double clicks.
        .with_step_dt(1.0 / 60.0);
    if gpu {
        let rs = egui_kittest::wgpu::create_render_state(
            egui_kittest::wgpu::default_wgpu_setup(),
            egui_wgpu::RendererOptions::PREDICTABLE,
        );
        stagecraft_ui::view3d::gpu::install(&rs);
        b = b.renderer(egui_kittest::wgpu::WgpuTestRenderer::from_render_state(rs));
    }
    let mut h = b.build_ui_state(|ui, s: &mut Studio| s.show(ui), studio);
    h.run();
    h
}

fn has_gpu() -> bool {
    stagecraft_ui::view3d::offscreen::Offscreen::headless().is_some()
}

/// Lets half a second pass, so the next click is not a double click.
fn pause(h: &mut Harness<'static, Studio>) {
    for _ in 0..40 {
        h.step();
    }
}

fn click_at(h: &mut Harness<'static, Studio>, pos: Pos2) {
    h.hover_at(pos);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    }
    h.run();
    pause(h);
}

fn click_world(h: &mut Harness<'static, Studio>, p: [f64; 2]) {
    let pos = h.state().ui.camera.to_screen(p);
    click_at(h, pos);
}

/// Type into a field found by its accessible name and press Enter.
fn type_into(h: &mut Harness<'static, Studio>, label: &str, text: &str) {
    h.get_by_label(label).click();
    h.run();
    h.get_by_label(label).type_text(text);
    h.run();
    h.key_press(Key::Enter);
    h.run();
}

fn the_floor(h: &Harness<'static, Studio>) -> stagecraft_core::Object {
    h.state().engine.doc().objects.values().find(|o| o.kind.type_name() == "floor").expect("a floor").clone()
}

fn hint(h: &Harness<'static, Studio>) -> String {
    stagecraft_ui::panels::hint(h.state())
}

const CORNERS: [[f64; 2]; 4] = [[-9000.0, -7000.0], [-2000.0, -7000.0], [-2000.0, -2000.0], [-9000.0, -2000.0]];

/// Floor tool with a 1 m top and 200 mm thickness, four corners, closed on the first.
fn draw_stage_floor(h: &mut Harness<'static, Studio>) {
    h.get_by_label("Tool Floor").click();
    h.run();
    type_into(h, "Option Top height", "1000");
    type_into(h, "Option Thickness", "200");
    assert!(hint(h).contains("first corner"), "{}", hint(h));
    for (i, p) in CORNERS.into_iter().enumerate() {
        click_world(h, p);
        if i == 0 {
            assert!(hint(h).contains("next corner"), "{}", hint(h));
        }
    }
    let close = hint(h);
    assert!(close.contains("Close the floor") && close.contains("Enter"), "{close}");
    click_world(h, CORNERS[0]);
}

#[test]
fn the_floor_tool_sets_top_height_and_thickness_and_remembers_them() {
    let mut h = harness(false);
    draw_stage_floor(&mut h);
    let f = the_floor(&h);
    assert_eq!(f.placement.position[2], 1000.0, "top height above the layer");
    let ObjectKind::Floor { thickness, .. } = f.kind else { unreachable!() };
    assert_eq!(thickness, 200.0);
    assert!(f.sketch.is_some(), "drawn into the sketch");
    let (status, err) = h.state().ui.status.clone().unwrap();
    assert!(!err && status.contains("Floor created") && status.contains("Inspector"), "{status}");
    // The options are kept per user.
    let tools = &h.state().prefs.get().tools;
    assert_eq!((tools.floor_z, tools.floor_thickness), (1000.0, 200.0));
}

#[test]
fn clicking_inside_selects_the_floor_and_the_inspector_edits_its_heights() {
    let mut h = harness(false);
    draw_stage_floor(&mut h);
    let id = the_floor(&h).id;
    h.key_press(Key::Escape);
    h.run();
    h.get_by_label("Tool Select").click();
    h.run();
    h.state_mut().run("selection.clear", Value::Null).unwrap();
    // Inside, away from the lines: the floor.
    click_world(&mut h, [-5500.0, -4500.0]);
    assert_eq!(h.state().engine.selection(), [id]);
    assert!(h.state().ui.sketch.picked.is_empty());
    assert!(h.query_by_label("Inspector Z").is_none(), "the floor names its height itself");
    type_into(&mut h, "Inspector Top height", "1.2m");
    assert_eq!(the_floor(&h).placement.position[2], 1200.0);
    type_into(&mut h, "Inspector Thickness", "300");
    let ObjectKind::Floor { thickness, outline } = the_floor(&h).kind else { unreachable!() };
    assert_eq!(thickness, 300.0);
    assert_eq!(outline.len(), 4, "the outline stays");
    // On a line: the sketch line, not the floor.
    let tol = 6.0 / h.state().ui.camera.scale;
    click_world(&mut h, [-2000.0 - tol * 0.2, -4500.0]);
    assert!(h.state().engine.selection().is_empty());
    assert_eq!(h.state().ui.sketch.picked.len(), 1);

    // On a lowered layer the height stays above the layer; the world height is shown too.
    type_into(&mut h, "Elevation Stage", "-1000");
    click_world(&mut h, [-5500.0, -4500.0]);
    assert_eq!(h.state().engine.selection(), [id]);
    assert_eq!(the_floor(&h).placement.position[2], 1200.0);
    assert!(h.query_by_label_contains("World z 200 mm").is_some());
}

#[test]
fn walls_take_a_base_and_ceilings_an_underside() {
    let mut h = harness(false);
    h.get_by_label("Tool Wall").click();
    h.run();
    type_into(&mut h, "Option Base", "500");
    for p in [[0.0, 0.0], [4000.0, 0.0]] {
        click_world(&mut h, p);
    }
    h.key_press(Key::Enter);
    h.run();
    let wall: Vec<Id> =
        h.state().engine.doc().objects.values().filter(|o| o.kind.type_name() == "wall").map(|o| o.id).collect();
    assert_eq!(wall.len(), 1);
    assert_eq!(h.state().engine.doc().objects[&wall[0]].placement.position[2], 500.0);
    assert_eq!(h.state().engine.selection(), wall.as_slice());
    type_into(&mut h, "Inspector Base height", "800");
    assert_eq!(h.state().engine.doc().objects[&wall[0]].placement.position[2], 800.0);
    assert!(h.query_by_label("Inspector Height").is_some());
    h.get_by_label("Tool Ceiling").click();
    h.run();
    assert!(h.query_by_label("Option Underside").is_some());
}

#[test]
fn a_new_floor_shows_in_3d_after_the_camera_was_fitted_to_nothing() {
    let gpu = has_gpu();
    let mut h = harness(gpu);
    // The 3D view seen once with an empty document: its camera fits a default box.
    h.get_by_label("View 3D").click();
    h.run();
    h.get_by_label("View Plan").click();
    h.run();
    draw_stage_floor(&mut h);
    h.key_press(Key::Escape);
    h.run();
    h.state_mut().run("selection.clear", json!(null)).unwrap();
    h.get_by_label("View 3D").click();
    h.run();
    let cam = h.state().ui.view3d.camera;
    let top = 1000.0;
    for [x, y] in CORNERS {
        for z in [top, top - 200.0] {
            let p = cam.to_screen(DVec3::new(x, y, z)).expect("in front of the camera");
            assert!(cam.rect.contains(p), "corner {x},{y},{z} at {p:?} outside {:?}", cam.rect);
        }
    }
    // Large on screen, not a speck in a corner.
    let pts: Vec<Pos2> = CORNERS.iter().filter_map(|[x, y]| cam.to_screen(DVec3::new(*x, *y, top))).collect();
    let w = pts.iter().map(|p| p.x).fold(f32::MIN, f32::max) - pts.iter().map(|p| p.x).fold(f32::MAX, f32::min);
    assert!(w > cam.rect.width() * 0.4, "floor spans {w} of {}", cam.rect.width());
    // Orbiting by hand stops the camera following the model.
    let c = cam.rect.center();
    h.drag_at(c);
    h.hover_at(c + vec2(40.0, 0.0));
    h.run();
    h.drop_at(c + vec2(40.0, 0.0));
    h.run();
    assert!(!h.state().ui.view3d.follow);

    if gpu {
        // Drawn: the top's centre is floor-coloured, not background.
        h.state_mut().run("ui.camera", json!({ "view": "iso" })).unwrap();
        h.run();
        let img = h.render().expect("render");
        let cam = h.state().ui.view3d.camera;
        let centre = cam.to_screen(DVec3::new(-5500.0, -4500.0, top)).unwrap();
        let ppp = img.width() as f32 / 1400.0;
        let px = img.get_pixel((centre.x * ppp) as u32, (centre.y * ppp) as u32).0;
        let bg = [0x1b, 0x1c, 0x1f];
        let diff: i32 = (0..3).map(|i| (px[i] as i32 - bg[i]).abs()).sum();
        assert!(diff > 60, "floor pixel {px:?} looks like the background");
        if let Ok(dir) = std::env::var("STAGECRAFT_SCREENSHOTS") {
            img.save(format!("{dir}/50-floor-3d.png")).expect("save");
        }
    }
}
