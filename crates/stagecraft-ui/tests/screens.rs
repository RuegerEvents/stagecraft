//! Not a check: renders the main views of the demo document to PNG, in light and
//! dark, for a person (or an agent) to look at.
//!
//!     STAGECRAFT_SCREENSHOTS=<dir> cargo test -p stagecraft-ui --test screens

use egui::{Event, Key, Modifiers, PointerButton, vec2};
use egui_kittest::Harness;
use serde_json::json;
use stagecraft_ui::{Studio, ThemePref};

/// A headless wgpu renderer with the 3D view's pipelines installed.
fn renderer() -> egui_kittest::wgpu::WgpuTestRenderer {
    let rs = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        egui_wgpu::RendererOptions::PREDICTABLE,
    );
    stagecraft_ui::view3d::gpu::install(&rs);
    egui_kittest::wgpu::WgpuTestRenderer::from_render_state(rs)
}

fn harness(theme: ThemePref) -> Harness<'static, Studio> {
    let mut studio = Studio::new(stagecraft_ui::demo::engine());
    studio.ui.theme = theme;
    let mut h = Harness::builder()
        .with_size(vec2(1440.0, 900.0))
        .renderer(renderer())
        .build_ui_state(|ui, s: &mut Studio| s.show(ui), studio);
    h.run();
    h
}

fn save(h: &mut Harness<'static, Studio>, dir: &str, name: &str) {
    h.run();
    h.render().expect("render").save(format!("{dir}/{name}.png")).expect("save");
}

#[test]
fn screenshots() {
    let Ok(dir) = std::env::var("STAGECRAFT_SCREENSHOTS") else { return };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    for (theme, tn) in [(ThemePref::Light, "light"), (ThemePref::Dark, "dark")] {
        let mut h = harness(theme);
        save(&mut h, &dir, &format!("1-plan-{tn}"));

        // A truss selected: inspector and table follow.
        let doc = h.state().engine.doc().clone();
        let lx1 = doc.objects.values().find(|o| o.name == "LX1").unwrap().id;
        let kids: Vec<_> = doc.children(lx1);
        h.state_mut().run("selection.set", json!({ "ids": kids })).unwrap();
        save(&mut h, &dir, &format!("2-selection-{tn}"));

        // Drawing a wall with a typed length.
        h.state_mut().run("selection.clear", json!(null)).unwrap();
        h.state_mut().run("ui.set_tool", json!({ "tool": "wall" })).unwrap();
        let a = h.state().ui.camera.to_screen([-6000.0, -2000.0]);
        let b = h.state().ui.camera.to_screen([-2000.0, -2600.0]);
        h.hover_at(a);
        h.run();
        for pressed in [true, false] {
            h.event(Event::PointerButton {
                pos: a,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        h.run();
        h.hover_at(b);
        h.run();
        h.event(Event::Text("4.5".into()));
        save(&mut h, &dir, &format!("3-drawing-{tn}"));
        h.key_press(Key::Escape);
        h.key_press(Key::Escape);
        h.run();

        // The command palette.
        h.state_mut().run("ui.set_tool", json!({ "tool": "select" })).unwrap();
        h.state_mut().ui.palette.open();
        h.run();
        h.state_mut().ui.palette.query = "truss".into();
        save(&mut h, &dir, &format!("4-palette-{tn}"));
        h.state_mut().ui.palette.close();

        // Truss table, zoomed in on a corner of the stage.
        h.state_mut().ui.table.kind = Some("truss".into());
        h.state_mut().run("ui.zoom", json!({ "scale": 0.25 })).unwrap();
        h.state_mut().run("ui.pan", json!({ "center": [4500, 2000] })).unwrap();
        save(&mut h, &dir, &format!("5-zoomed-{tn}"));
        h.state_mut().ui.table.kind = None;

        // 3D: iso, front, a section through the stage, a section box, split.
        let run = |h: &mut Harness<'static, Studio>, name: &str, args: serde_json::Value| {
            h.state_mut().run(name, args).unwrap_or_else(|e| panic!("{name}: {e}"));
            h.run();
        };
        run(&mut h, "ui.set_view", json!({ "view": "3d" }));
        run(&mut h, "ui.camera", json!({ "view": "iso" }));
        save(&mut h, &dir, &format!("6-3d-iso-{tn}"));
        run(&mut h, "ui.camera", json!({ "view": "front", "projection": "orthographic" }));
        save(&mut h, &dir, &format!("7-3d-front-{tn}"));
        run(&mut h, "ui.camera", json!({ "view": "iso", "projection": "perspective" }));
        run(&mut h, "ui.section", json!({ "axis": "x", "offset": 0 }));
        run(&mut h, "selection.set", json!({ "ids": [lx1] }));
        save(&mut h, &dir, &format!("8-3d-section-{tn}"));
        run(&mut h, "ui.section", json!({ "box": { "min": [-3000, -500, -100], "max": [7950, 9500, 6500] } }));
        run(&mut h, "selection.clear", json!(null));
        save(&mut h, &dir, &format!("9-3d-box-{tn}"));
        // Close to the FOH truss: chords, diagonals, fixtures.
        run(&mut h, "ui.section", json!({ "off": true }));
        run(&mut h, "ui.camera", json!({ "eye": [-7800, -5600, 7400], "target": [-4300, -3000, 6300] }));
        save(&mut h, &dir, &format!("11-3d-truss-{tn}"));
        // A horizontal cut at 2 m: hatched wall tops at the back corner.
        run(&mut h, "ui.section", json!({ "axis": "z", "offset": 2000 }));
        run(&mut h, "ui.camera", json!({ "eye": [3000, 4000, 7000], "target": [7000, 8500, 1500] }));
        save(&mut h, &dir, &format!("12-3d-cut-{tn}"));
        run(&mut h, "ui.section", json!({ "off": true }));
        run(&mut h, "ui.set_view", json!({ "view": "split" }));
        run(&mut h, "ui.zoom_to_fit", json!(null));
        save(&mut h, &dir, &format!("10-split-{tn}"));
    }
}
