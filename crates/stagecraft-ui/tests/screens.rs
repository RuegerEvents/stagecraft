//! Not a check: renders the main views of the demo document to PNG, in light and
//! dark, for a person (or an agent) to look at.
//!
//!     STAGECRAFT_SCREENSHOTS=<dir> cargo test -p stagecraft-ui --test screens

use egui::{Event, Key, Modifiers, PointerButton, vec2};
use egui_kittest::Harness;
use serde_json::json;
use stagecraft_ui::{Studio, ThemePref};

fn harness(theme: ThemePref) -> Harness<'static, Studio> {
    let mut studio = Studio::new(stagecraft_ui::demo::engine());
    studio.ui.theme = theme;
    let mut h =
        Harness::builder().with_size(vec2(1440.0, 900.0)).build_ui_state(|ui, s: &mut Studio| s.show(ui), studio);
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
    }
}
