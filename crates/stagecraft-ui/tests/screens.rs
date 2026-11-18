//! Not a check: renders the main views of the demo document to PNG, in light and
//! dark, for a person (or an agent) to look at.
//!
//!     STAGECRAFT_SCREENSHOTS=<dir> cargo test -p stagecraft-ui --test screens

use egui::{Event, Key, Modifiers, PointerButton, vec2};
use egui_kittest::Harness;
use serde_json::json;
use stagecraft_ui::{Studio, ThemePref};

/// A headless wgpu renderer with the 3D view's pipelines installed, and an offscreen
/// renderer for sheet images on the same device.
fn renderer() -> (egui_kittest::wgpu::WgpuTestRenderer, stagecraft_ui::view3d::offscreen::Offscreen) {
    let rs = egui_kittest::wgpu::create_render_state(
        egui_kittest::wgpu::default_wgpu_setup(),
        egui_wgpu::RendererOptions::PREDICTABLE,
    );
    stagecraft_ui::view3d::gpu::install(&rs);
    let off = stagecraft_ui::view3d::offscreen::Offscreen::from_render_state(&rs);
    (egui_kittest::wgpu::WgpuTestRenderer::from_render_state(rs), off)
}

fn harness(theme: ThemePref) -> Harness<'static, Studio> {
    harness_with(theme, stagecraft_ui::demo::engine())
}

fn harness_with(theme: ThemePref, engine: stagecraft_core::Engine) -> Harness<'static, Studio> {
    harness_sized(theme, engine, vec2(1440.0, 900.0))
}

/// kittest puts an 8 pt margin around the UI; the harness is that much larger and [`save`]
/// cuts it off, so the screens are the window at `size`.
const MARGIN: f32 = 8.0;

fn harness_sized(theme: ThemePref, engine: stagecraft_core::Engine, size: egui::Vec2) -> Harness<'static, Studio> {
    harness_ppp(theme, engine, size, 1.0)
}

/// [`harness_sized`] at a pixel density: 1.0 is a plain external monitor, 2.0 Retina.
fn harness_ppp(
    theme: ThemePref,
    engine: stagecraft_core::Engine,
    size: egui::Vec2,
    ppp: f32,
) -> Harness<'static, Studio> {
    let mut studio = Studio::new(engine);
    studio.ui.theme = theme;
    // As on macOS: the menus are the system's, the tool bar is the title bar.
    studio.ui.native_menu = true;
    studio.ui.inset_title_bar = true;
    let (renderer, off) = renderer();
    studio.offscreen = Some(off);
    let mut h = Harness::builder()
        // egui 0.37 styles a widget from its previous-pass state, so a widget that appears
        // asks for one more frame; closing a modal can take six.
        .with_max_steps(8)
        .with_pixels_per_point(ppp)
        .with_size(size + vec2(2.0 * MARGIN, 2.0 * MARGIN))
        .renderer(renderer)
        .build_ui_state(|ui, s: &mut Studio| s.show(ui), studio);
    h.run();
    h
}

fn save(h: &mut Harness<'static, Studio>, dir: &str, name: &str) {
    h.run();
    let full = h.render().expect("render");
    let m = (MARGIN * h.ctx.pixels_per_point()) as u32;
    let mut img = image::imageops::crop_imm(&full, m, m, full.width() - 2 * m, full.height() - 2 * m).to_image();
    window_buttons(&mut img);
    img.save(format!("{dir}/{name}.png")).expect("save");
}

/// The macOS window buttons AppKit draws over the inset title bar, so the screens look like
/// the window (they are not part of egui's frame).
fn window_buttons(img: &mut image::RgbaImage) {
    let lights = stagecraft_ui::chrome::traffic_light_centres();
    for (i, rgb) in [[0xff, 0x5f, 0x57], [0xfe, 0xbc, 0x2e], [0x28, 0xc8, 0x40]].into_iter().enumerate() {
        let r = stagecraft_ui::chrome::MAC_LIGHT / 2.0;
        let (cx, cy) = (lights[i].x, lights[i].y);
        for y in (cy - r - 1.0) as u32..=(cy + r + 1.0) as u32 {
            for x in (cx - r - 1.0) as u32..=(cx + r + 1.0) as u32 {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                let a = (r + 0.5 - d).clamp(0.0, 1.0);
                if a > 0.0 && x < img.width() && y < img.height() {
                    let p = img.get_pixel_mut(x, y);
                    for (ch, want) in p.0.iter_mut().zip(rgb) {
                        *ch = (*ch as f32 * (1.0 - a) + want as f32 * a) as u8;
                    }
                }
            }
        }
    }
}

/// Icons and text at 1× (a plain external monitor) and 2× (Retina), and at 1.5×: the tool
/// palette, the tool bar, the palette with the Draw group. `80-crisp-<ppp>x-*`.
#[test]
fn crisp_screens() {
    let Ok(dir) = std::env::var("STAGECRAFT_SCREENSHOTS") else { return };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    for (theme, tn) in [(ThemePref::Light, "light"), (ThemePref::Dark, "dark")] {
        for (ppp, pn) in [(1.0, "1x"), (1.5, "1.5x"), (2.0, "2x")] {
            let mut h = harness_ppp(theme, stagecraft_ui::demo::engine(), vec2(1200.0, 750.0), ppp);
            h.state_mut().run("ui.set_tool", json!({ "tool": "dimension" })).unwrap();
            save(&mut h, &dir, &format!("80-crisp-{pn}-plan-{tn}"));
            h.state_mut().ui.palette.open();
            h.state_mut().ui.palette.group = Some(stagecraft_ui::command_meta::Group::Draw);
            h.run();
            save(&mut h, &dir, &format!("80-crisp-{pn}-palette-{tn}"));
            h.state_mut().ui.palette.close();
            if pn == "1.5x" {
                continue;
            }
            // The tool palette folded to two and to one column.
            for cols in ["two", "one"] {
                h.state_mut().run("ui.tool_columns", json!({ "columns": cols })).unwrap();
                save(&mut h, &dir, &format!("81-tools-{cols}-{pn}-{tn}"));
            }
        }
    }
}

/// The command palette: groups with recents, suggestions for a selection, a group, flat
/// search, aliases, the inline form, object search and `>` mode.
#[test]
fn palette_screens() {
    let Ok(dir) = std::env::var("STAGECRAFT_SCREENSHOTS") else { return };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    for (theme, tn) in [(ThemePref::Light, "light"), (ThemePref::Dark, "dark")] {
        let mut h = harness(theme);
        let shot = |h: &mut Harness<'static, Studio>, query: &str, name: &str| {
            h.state_mut().ui.palette.open();
            h.run();
            h.state_mut().ui.palette.query = query.into();
            save(h, &dir, &format!("{name}-{tn}"));
        };
        h.state_mut().ui.recent =
            vec!["patch.auto".into(), "ui.set_view {\"view\":\"3d\"}".into(), "app.export_pdf".into()];
        shot(&mut h, "", "70-palette-groups");
        let doc = h.state().engine.doc().clone();
        let foh: Vec<_> = doc
            .objects
            .values()
            .filter(|o| o.name.starts_with("FOH") && o.kind.type_name() == "fixture")
            .map(|o| o.id)
            .collect();
        h.state_mut().run("selection.set", json!({ "ids": foh })).unwrap();
        shot(&mut h, "", "71-palette-selection");
        h.state_mut().ui.palette.open();
        h.state_mut().ui.palette.group = Some(stagecraft_ui::command_meta::Group::Rigging);
        h.run();
        save(&mut h, &dir, &format!("72-palette-group-{tn}"));
        shot(&mut h, "truss", "73-palette-search");
        shot(&mut h, "zug", "74-palette-alias");
        shot(&mut h, "101", "76-palette-objects");
        shot(&mut h, "> ui.", "77-palette-commands");
        h.state_mut().ui.palette.open();
        h.run();
        stagecraft_ui::palette::choose(h.state_mut(), "patch.auto");
        h.run();
        save(&mut h, &dir, &format!("75-palette-form-{tn}"));
        h.state_mut().ui.palette.close();
        h.state_mut().run("selection.clear", json!(null)).unwrap();
        h.state_mut().ui.palette.open();
        h.run();
        stagecraft_ui::palette::choose(h.state_mut(), "truss.create");
        h.run();
        save(&mut h, &dir, &format!("78-palette-form-truss-{tn}"));
        h.state_mut().ui.palette.close();
    }
}

/// The window at the size of the Craft suite's reference screenshots (1200 × 750).
#[test]
fn craft_size() {
    let Ok(dir) = std::env::var("STAGECRAFT_SCREENSHOTS") else { return };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    for (theme, tn) in [(ThemePref::Light, "light"), (ThemePref::Dark, "dark")] {
        let mut h = harness_sized(theme, stagecraft_ui::demo::engine(), vec2(1200.0, 750.0));
        save(&mut h, &dir, &format!("50-craft-plan-{tn}"));
        let doc = h.state().engine.doc().clone();
        let lx1 = doc.objects.values().find(|o| o.name == "LX1").unwrap().id;
        h.state_mut().run("selection.set", json!({ "ids": doc.children(lx1) })).unwrap();
        save(&mut h, &dir, &format!("51-craft-selection-{tn}"));
        h.state_mut().run("selection.clear", json!(null)).unwrap();
        h.state_mut().run("ui.set_tool", json!({ "tool": "wall" })).unwrap();
        save(&mut h, &dir, &format!("52-craft-wall-tool-{tn}"));
        h.state_mut().run("ui.set_tool", json!({ "tool": "select" })).unwrap();
        // As in the other overviews: the house ceiling would lie over the stage in the iso view.
        h.state_mut().run("ui.set_tag_visible", json!({ "tag": "ceiling", "visible": false })).unwrap();
        h.state_mut().run("ui.set_view", json!({ "view": "3d" })).unwrap();
        h.state_mut().run("ui.camera", json!({ "view": "iso" })).unwrap();
        save(&mut h, &dir, &format!("53-craft-3d-{tn}"));
        h.state_mut().run("ui.set_tag_visible", json!({ "tag": "ceiling", "visible": true })).unwrap();
        h.state_mut().run("ui.set_view", json!({ "view": "sheets" })).unwrap();
        for _ in 0..30 {
            h.step();
        }
        save(&mut h, &dir, &format!("54-craft-sheets-{tn}"));
        h.state_mut().run("ui.set_view", json!({ "view": "plan" })).unwrap();
        h.state_mut().run("ui.tutorial", json!({ "action": "start" })).unwrap();
        save(&mut h, &dir, &format!("55-craft-tutorial-{tn}"));
    }
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
        // A rearranged layout: tags as a tab beside the inspector, the library floating
        // over the plan, the table closed.
        run(&mut h, "ui.zoom_to_fit", json!(null));
        run(
            &mut h,
            "ui.layout",
            json!({ "actions": [{ "Move": { "panel": "tags", "anchor": "inspector", "placement": { "Tab": { "before": null } } } }] }),
        );
        run(&mut h, "ui.toggle_panel", json!({ "panel": "table", "visible": false }));
        run(&mut h, "ui.float_panel", json!({ "panel": "library", "x": 900, "y": 420, "width": 300, "height": 300 }));
        save(&mut h, &dir, &format!("40-dock-float-{tn}"));
        run(&mut h, "ui.reset_layout", json!(null));
        gdtf_shots(&mut h, &dir, tn);
        if tn == "light" {
            let pdf = format!("{dir}/demo-sheets.pdf");
            h.state_mut().run("sheet.export_pdf", json!({ "path": pdf })).expect("export");
        }
        tutorial_shots(theme, &dir, tn);
    }
}

/// The tutorial panel on an empty document: the first step pointing at "Add layer", the
/// floor step pointing at the Top height option, and the end.
fn tutorial_shots(theme: ThemePref, dir: &str, tn: &str) {
    let mut h = harness_with(theme, stagecraft_core::Engine::new());
    h.state_mut().run("ui.tutorial", json!({ "action": "start" })).unwrap();
    save(&mut h, dir, &format!("41-tutorial-start-{tn}"));
    for _ in 0..2 {
        h.state_mut().run("ui.tutorial", json!({ "action": "next" })).unwrap();
    }
    h.state_mut().run("ui.set_tool", json!({ "tool": "floor" })).unwrap();
    save(&mut h, dir, &format!("42-tutorial-floor-{tn}"));
    for _ in 0..10 {
        h.state_mut().run("ui.tutorial", json!({ "action": "next" })).unwrap();
    }
    save(&mut h, dir, &format!("43-tutorial-end-{tn}"));
}

/// GDTF Share without a network: three entries, downloads from the demo GDTFs.
struct MockShare;

impl stagecraft_ui::library::ShareBackend for MockShare {
    fn list(&mut self, _: bool) -> Result<Vec<stagecraft_gdtf::share::ShareEntry>, String> {
        let entry = |rid: u64, m: &str, f: &str, modes: &[(&str, u32)]| stagecraft_gdtf::share::ShareEntry {
            rid,
            manufacturer: m.into(),
            fixture: f.into(),
            revision: "demo".into(),
            modes: modes
                .iter()
                .map(|(n, c)| stagecraft_gdtf::share::ShareMode { name: (*n).into(), dmx_footprint: *c })
                .collect(),
            ..Default::default()
        };
        Ok(vec![
            entry(1, "StageCraft Demo", "Spot 300", &[("Standard", 16), ("Basic", 6)]),
            entry(2, "StageCraft Demo", "Profile 750", &[("Dimmer", 1), ("Dimmer 16 bit", 2)]),
            entry(3, "StageCraft Demo", "Pixel Bar 8", &[("8px RGB", 25), ("Split", 2)]),
        ])
    }
    fn download(&mut self, _: &stagecraft_gdtf::share::ShareEntry) -> Result<std::path::PathBuf, String> {
        Err("offline in this test".into())
    }
}

fn gdtf_shots(h: &mut Harness<'static, Studio>, dir: &str, tn: &str) {
    let run = |h: &mut Harness<'static, Studio>, name: &str, args: serde_json::Value| {
        h.state_mut().run(name, args).unwrap_or_else(|e| panic!("{name}: {e}"));
        h.run();
    };
    let doc = h.state().engine.doc().clone();
    let by_name = |n: &str| doc.objects.values().find(|o| o.name == n).unwrap().id;
    // A patch conflict: LX2 1 onto LX2 2's address; ring in the plan, red in the table.
    let lx21 = by_name("LX2 1");
    run(h, "fixture.update", json!({ "id": lx21, "address": "1.300" }));
    run(h, "ui.set_view", json!({ "view": "plan" }));
    h.state_mut().ui.table.kind = Some("fixture".into());
    run(h, "ui.zoom", json!({ "scale": 0.09 }));
    run(h, "ui.pan", json!({ "center": [-1000, 3500] }));
    run(h, "selection.set", json!({ "ids": [lx21] }));
    // Library: the profile active for the fixture tool, GDTF Share results.
    run(h, "ui.set_tool_options", json!({ "fixture_type": stagecraft_ui::demo::PROFILE }));
    h.state_mut().ui.library.set_backend(Box::new(MockShare));
    run(h, "ui.share_search", json!({ "query": "stagecraft" }));
    for _ in 0..50 {
        h.run();
        if !h.state().ui.library.busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    run(h, "ui.share_search", json!({ "query": "stagecraft" }));
    run(h, "ui.toggle_panel", json!({ "panel": "layers", "visible": false }));
    run(h, "ui.toggle_panel", json!({ "panel": "tags", "visible": false }));
    save(h, dir, &format!("20-gdtf-plan-{tn}"));
    run(h, "history.undo", json!(null));
    run(h, "selection.clear", json!(null));
    // FOH profiles, from the house.
    run(h, "ui.set_view", json!({ "view": "3d" }));
    run(h, "ui.set_tag_visible", json!({ "tag": "ceiling", "visible": false }));
    run(h, "ui.camera", json!({ "eye": [1500, -9000, 5000], "target": [0, -5000, 7600] }));
    save(h, dir, &format!("21-gdtf-foh-{tn}"));
    // LX1 moving heads and LX3 pixel bars from below, masking hidden.
    run(h, "ui.set_tag_visible", json!({ "tag": "masking", "visible": false }));
    run(h, "ui.camera", json!({ "eye": [-2500, 500, 5000], "target": [500, 3000, 6900] }));
    save(h, dir, &format!("22-gdtf-lx1-{tn}"));
    run(h, "ui.camera", json!({ "eye": [-3000, 5500, 5500], "target": [0, 9100, 8300] }));
    save(h, dir, &format!("22-gdtf-lx3-{tn}"));

    // With the real GDTF Share files (git-ignored), if present: LX1 becomes the first
    // moving head found, FOH the first profile, and so on by suggested symbol.
    let private = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/private/gdtf");
    if let Ok(rd) = std::fs::read_dir(&private) {
        let mut keys = Vec::new();
        for f in rd.flatten().map(|f| f.path()).filter(|p| p.extension().is_some_and(|x| x == "gdtf")) {
            let r = h.state_mut().run("gdtf.import", json!({ "path": f })).expect("import");
            keys.push(r["key"].as_str().unwrap().to_owned());
        }
        let doc = h.state().engine.doc().clone();
        let pick = |want: &[&str]| {
            keys.iter()
                .find(|k| doc.fixture_types[*k].suggested_symbol.as_deref().is_some_and(|s| want.contains(&s)))
                .cloned()
        };
        let group = |prefix: &str| -> Vec<stagecraft_core::Id> {
            doc.objects
                .values()
                .filter(|o| o.name.starts_with(prefix) && matches!(o.kind, stagecraft_core::ObjectKind::Fixture { .. }))
                .map(|o| o.id)
                .collect()
        };
        for (prefix, want) in [
            ("LX1", &["mh_spot", "beam"][..]),
            ("LX3", &["mh_wash"]),
            ("FOH", &["profile"]),
            ("LX2", &["par", "led_wash"]),
        ] {
            if let Some(k) = pick(want) {
                run(h, "fixture.set_type", json!({ "ids": group(prefix), "gdtf_type": k }));
            }
        }
        let mut all: Vec<(u32, stagecraft_core::Id)> = doc
            .objects
            .values()
            .filter_map(|o| match o.kind {
                stagecraft_core::ObjectKind::Fixture { fid: Some(f), .. } => Some((f, o.id)),
                _ => None,
            })
            .collect();
        all.sort();
        let ids: Vec<_> = all.into_iter().map(|x| x.1).collect();
        run(h, "patch.auto", json!({ "ids": ids, "start": "1.001" }));
        run(h, "ui.camera", json!({ "eye": [-2500, 500, 5000], "target": [500, 3000, 6900] }));
        save(h, dir, &format!("23-private-lx1-{tn}"));
        run(h, "ui.camera", json!({ "eye": [-3000, 5500, 5500], "target": [0, 9100, 8300] }));
        save(h, dir, &format!("23-private-lx3-{tn}"));
        run(h, "ui.camera", json!({ "eye": [1500, -9000, 5000], "target": [0, -5000, 7600] }));
        save(h, dir, &format!("24-private-foh-{tn}"));
        run(h, "ui.set_view", json!({ "view": "plan" }));
        run(h, "ui.zoom", json!({ "scale": 0.09 }));
        run(h, "ui.pan", json!({ "center": [-1000, 3500] }));
        save(h, dir, &format!("25-private-plan-{tn}"));
    }
    run(h, "ui.set_tag_visible", json!({ "tag": "masking", "visible": true }));
}

/// A real Vectorworks MVR (client data, git-ignored) imported and shown in plan and
/// 3D. Skips without the file.
#[test]
fn imported_mvr() {
    let Ok(dir) = std::env::var("STAGECRAFT_SCREENSHOTS") else { return };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/private/vectorworks/sample_show.mvr");
    if !path.exists() {
        return;
    }
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    let mut e = stagecraft_core::Engine::new();
    e.execute("mvr.import", json!({ "path": path })).expect("import");
    let mut h = harness_with(ThemePref::Light, e);
    h.state_mut().run("ui.set_view", json!({ "view": "plan" })).unwrap();
    h.state_mut().run("ui.zoom_to_fit", json!({})).unwrap();
    save(&mut h, &dir, "26-mvr-plan-light");
    h.state_mut().run("ui.set_view", json!({ "view": "3d" })).unwrap();
    h.state_mut().run("ui.camera", json!({ "view": "iso" })).unwrap();
    save(&mut h, &dir, "27-mvr-3d-light");
}

/// A small parametric room: walls and floor drawn into the sketch with typed lengths,
/// a locked corner (fully constrained, green), a free partition line (normal colour),
/// a reference dimension from a truss to the back wall; then the value editor, a drag
/// in progress, a sketch loaded with a conflict (red) and a sheet with the dimensions.
fn sketch_engine() -> stagecraft_core::Engine {
    let mut e = stagecraft_core::Engine::new();
    let mut run =
        |name: &str, args: serde_json::Value| e.execute(name, args).unwrap_or_else(|err| panic!("{name}: {err}"));
    let seg = |len: f64| json!({ "axis_lock": true, "length": len });
    run(
        "sketch.draw",
        json!({ "points": [[-6000, 0], [6000, 0], [6000, 9000], [-6000, 9000]], "closed": true, "make": "wall",
                "thickness": 240, "segments": [seg(12000.0), seg(9000.0), seg(12000.0)] }),
    );
    run("sketch.make_floor", json!({ "at": [0, 4000] }));
    let s = run("sketch.get", json!({}));
    let p0 = s["sketch"]["points"][0]["id"].clone();
    run(
        "sketch.add_constraint",
        json!({ "constraint": { "type": "fixed", "point": p0, "at": { "x": -6000, "y": 0 } } }),
    );
    let w = run("dimension.list", json!({}))[0]["id"].clone();
    run("dimension.name", json!({ "id": w, "name": "room width" }));
    // A free line across the room (the setting line), not constrained yet.
    run("sketch.draw_line", json!({ "start": [-6000, 3000], "end": [6000, 3100], "snap_radius": 200 }));
    let truss = run("truss.create", json!({ "start": [-4000, 5000, 6000], "end": [4000, 5000, 6000] }));
    let back = s["sketch"]["points"][2]["id"].clone();
    let layer = s["layer"].clone();
    run(
        "dimension.add_reference",
        json!({ "from": { "type": "object", "object": truss["id"], "local": [8000, 0, 0] },
                "to": { "type": "sketch_point", "layer": layer, "point": back },
                "axis": "vertical", "label_offset": [1600, 0], "name": "truss to back wall" }),
    );
    e
}

#[test]
fn sketch_room() {
    let Ok(dir) = std::env::var("STAGECRAFT_SCREENSHOTS") else { return };
    std::fs::create_dir_all(&dir).expect("screenshot dir");
    for (theme, tn) in [(ThemePref::Light, "light"), (ThemePref::Dark, "dark")] {
        let mut h = harness_with(theme, sketch_engine());
        let run = |h: &mut Harness<'static, Studio>, name: &str, args: serde_json::Value| {
            h.state_mut().run(name, args).unwrap_or_else(|e| panic!("{name}: {e}"));
            h.run();
        };
        h.state_mut().run("ui.set_view", json!({ "view": "plan" })).unwrap();
        h.state_mut().run("ui.zoom_to_fit", json!({})).unwrap();
        save(&mut h, &dir, &format!("30-sketch-room-{tn}"));

        // The width selected and its value editor open, "14" typed.
        h.state_mut().run("ui.dimension", json!({ "id": "room width", "edit": true })).unwrap();
        h.run();
        if let Some(e) = &mut h.state_mut().ui.sketch.editing {
            e.text = "14".into();
        }
        save(&mut h, &dir, &format!("31-sketch-edit-{tn}"));
        h.state_mut().run("dimension.set_value", json!({ "id": "room width", "value": 14000 })).unwrap();
        h.state_mut().run("ui.dimension", json!({ "id": null })).unwrap();
        h.state_mut().run("ui.zoom_to_fit", json!({})).unwrap();
        save(&mut h, &dir, &format!("32-sketch-wider-{tn}"));

        // A drag in progress: the free line's right end pulled up.
        let cam = h.state().ui.camera;
        let from = cam.to_screen([8000.0, 3100.0]);
        let to = cam.to_screen([8000.0, 6000.0]);
        h.hover_at(from);
        h.run();
        h.event(Event::PointerButton {
            pos: from,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.run();
        for i in 1..=4 {
            h.event(Event::PointerMoved(from + (to - from) * (i as f32 / 4.0)));
            h.run();
        }
        save(&mut h, &dir, &format!("33-sketch-drag-{tn}"));
        h.event(Event::PointerButton {
            pos: to,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        h.run();

        // A file whose sketch contradicts itself: the conflict shows red.
        let mut doc = h.state().engine.doc().clone();
        let layer = *doc.sketches.keys().next().unwrap();
        let mut sk = doc.sketches[&layer].clone();
        let line = sk.edges[0].id;
        sk.add_dimension(stagecraft_core::sketch::Measure::Length { line }, 10000.0);
        doc.sketches.insert(layer, sk);
        let mut h2 = harness_with(theme, stagecraft_core::Engine::with_document(doc));
        h2.state_mut().run("ui.zoom_to_fit", json!({})).unwrap();
        save(&mut h2, &dir, &format!("34-sketch-conflict-{tn}"));

        // A sheet with a plan viewport: walls, floor, truss and both kinds of dimension.
        if theme == ThemePref::Light {
            let sh = h.state_mut().run("sheet.create", json!({ "name": "Room", "number": "R-1" })).unwrap();
            run(
                &mut h,
                "viewport.create",
                json!({ "sheet": sh["id"], "view": "plan", "scale": 100, "position": [190, 160] }),
            );
            h.state_mut().run("ui.set_view", json!({ "view": "sheets" })).unwrap();
            h.state_mut().run("ui.sheet", json!({ "sheet": "R-1", "fit": true })).unwrap();
            for _ in 0..20 {
                h.run();
            }
            save(&mut h, &dir, "35-sketch-sheet-light");

            // German sheet text and a width with odd millimetres: 6,15⁵ in plan and
            // on the sheet; the title block close up in the Sheets mode.
            run(
                &mut h,
                "doc.set_meta",
                json!({ "sheet_language": "de", "project": "Beispielproduktion mit einem langen Namen 2026",
                        "venue_address": "Theaterstraße 1\n12345 Beispielstadt", "production_dates": "01.05.2026 – 03.05.2026",
                        "company": "Beispiel Licht GmbH", "drawn_by": "StageCraft", "website": "stagecraft.example" }),
            );
            h.state_mut().run("dimension.set_value", json!({ "id": "room width", "value": 6155 })).unwrap();
            for _ in 0..20 {
                h.run();
            }
            h.state_mut().ui.sheets.cam.center = [360.0, 40.0];
            h.state_mut().ui.sheets.cam.scale = 9.0;
            save(&mut h, &dir, "36-sheet-titleblock-light");
            h.state_mut().ui.sheets.cam.center = [190.0, 160.0];
            h.state_mut().ui.sheets.cam.scale = 6.0;
            save(&mut h, &dir, "37-sheet-dims-light");
            h.state_mut().run("ui.set_view", json!({ "view": "plan" })).unwrap();
            h.state_mut().run("ui.zoom_to_fit", json!({})).unwrap();
            save(&mut h, &dir, "38-plan-dims-de-light");
            let pdf = format!("{dir}/room-de.pdf");
            h.state_mut().run("sheet.export_pdf", json!({ "path": pdf })).unwrap();
        }
    }
}
