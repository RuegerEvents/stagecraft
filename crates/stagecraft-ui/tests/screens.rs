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

        // The fly bar list with loads, batten 4 (LX1) selected: inspector shows its load.
        let b4 = doc.objects.values().find(|o| o.name == "Batten 4").unwrap().id;
        h.state_mut().ui.table.kind = Some("batten".into());
        h.state_mut().run("selection.set", json!({ "ids": [b4] })).unwrap();
        h.state_mut().run("ui.zoom", json!({ "scale": 0.055 })).unwrap();
        h.state_mut().run("ui.pan", json!({ "center": [0, 5000] })).unwrap();
        save(&mut h, &dir, &format!("13-fly-bars-{tn}"));
        // A curtain selected: its inspector fields.
        let main = doc.objects.values().find(|o| o.name == "Main curtain").unwrap().id;
        h.state_mut().run("selection.set", json!({ "ids": [main] })).unwrap();
        save(&mut h, &dir, &format!("14-curtain-{tn}"));
        h.state_mut().run("selection.clear", json!(null)).unwrap();
        h.state_mut().ui.table.kind = None;

        // 3D: iso, front, a section through the stage, a section box, split.
        let run = |h: &mut Harness<'static, Studio>, name: &str, args: serde_json::Value| {
            h.state_mut().run(name, args).unwrap_or_else(|e| panic!("{name}: {e}"));
            h.run();
        };
        // The house ceiling would cover the FOH from above; hide it for the overviews.
        run(&mut h, "ui.set_tag_visible", json!({ "tag": "ceiling", "visible": false }));
        run(&mut h, "ui.set_view", json!({ "view": "3d" }));
        run(&mut h, "ui.camera", json!({ "view": "iso" }));
        save(&mut h, &dir, &format!("6-3d-iso-{tn}"));
        run(&mut h, "ui.camera", json!({ "view": "front", "projection": "orthographic" }));
        save(&mut h, &dir, &format!("7-3d-front-{tn}"));
        run(&mut h, "ui.camera", json!({ "view": "iso", "projection": "perspective" }));
        run(&mut h, "ui.section", json!({ "axis": "x", "offset": 0 }));
        run(&mut h, "selection.set", json!({ "ids": [lx1] }));
        save(&mut h, &dir, &format!("8-3d-section-{tn}"));
        run(&mut h, "ui.section", json!({ "box": { "min": [-3000, -2500, -100], "max": [9500, 12000, 6500] } }));
        run(&mut h, "selection.clear", json!(null));
        save(&mut h, &dir, &format!("9-3d-box-{tn}"));
        // Close to the FOH truss: chords, diagonals, corners of the run, fixtures.
        run(&mut h, "ui.section", json!({ "off": true }));
        run(&mut h, "ui.camera", json!({ "eye": [-7000, -8600, 6600], "target": [-3000, -5000, 6400] }));
        save(&mut h, &dir, &format!("11-3d-truss-{tn}"));
        // The stage from the house: portal, main curtain, wings, rostrum.
        run(&mut h, "ui.set_tag_visible", json!({ "tag": "ceiling", "visible": true }));
        run(&mut h, "ui.camera", json!({ "eye": [2500, -11500, 2600], "target": [0, 3000, 3600] }));
        save(&mut h, &dir, &format!("15-3d-stage-{tn}"));
        run(&mut h, "ui.set_tag_visible", json!({ "tag": "ceiling", "visible": false }));
        // From above the forestage, stage left: battens, borders, LX truss, rostrum.
        run(&mut h, "ui.camera", json!({ "eye": [-12500, -7000, 15500], "target": [0, 5500, 3000] }));
        save(&mut h, &dir, &format!("16-3d-fly-{tn}"));
        // A horizontal cut at 2 m: hatched wall tops at the back corner.
        run(&mut h, "ui.section", json!({ "axis": "z", "offset": 2000 }));
        run(&mut h, "ui.camera", json!({ "eye": [3000, 4000, 7000], "target": [7000, 9500, 500] }));
        save(&mut h, &dir, &format!("12-3d-cut-{tn}"));
        run(&mut h, "ui.section", json!({ "off": true }));
        run(&mut h, "ui.set_view", json!({ "view": "split" }));
        run(&mut h, "ui.zoom_to_fit", json!(null));
        save(&mut h, &dir, &format!("10-split-{tn}"));

        // Sheets: white paper in both themes. The lighting plan with a viewport
        // selected, then the elevation sheet with the viewport editor open.
        run(&mut h, "ui.sheet", json!({ "sheet": "L-101", "fit": true }));
        let doc = h.state().engine.doc().clone();
        let l101 = doc.sheets.values().find(|s| s.number == "L-101").unwrap();
        run(&mut h, "ui.sheet", json!({ "viewport": l101.viewports[0].id }));
        save(&mut h, &dir, &format!("17-sheet-plan-{tn}"));
        let l102 = doc.sheets.values().find(|s| s.number == "L-102").unwrap();
        run(&mut h, "ui.sheet", json!({ "sheet": "L-102", "viewport": l102.viewports[0].id, "editor": true }));
        save(&mut h, &dir, &format!("18-sheet-sections-{tn}"));
        run(&mut h, "ui.sheet", json!({ "editor": false, "viewport": null }));
        // Labels in the plan canvas, close to LX1 (fixtures turned 180°).
        run(&mut h, "ui.set_view", json!({ "view": "plan" }));
        run(&mut h, "ui.set_label_template", json!({ "template": "Electrics" }));
        run(&mut h, "ui.zoom", json!({ "scale": 0.12 }));
        run(&mut h, "ui.pan", json!({ "center": [0, 2400] }));
        save(&mut h, &dir, &format!("19-plan-labels-{tn}"));
        run(&mut h, "ui.set_label_template", json!({ "template": null }));
        if tn == "light" {
            let pdf = format!("{dir}/demo-sheets.pdf");
            h.state_mut().run("sheet.export_pdf", json!({ "path": pdf })).expect("export");
        }
    }
}
