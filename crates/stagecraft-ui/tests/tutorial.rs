//! The tutorial "Build a basic stage", walked from Help > Tutorial to the PDF through
//! the GUI only: clicks on tools, panels, table cells and plan positions, typed text.
//! The file dialog is answered by craft-dialogs' override, as the OS would.
//!
//!     STAGECRAFT_SCREENSHOTS=<dir> cargo test -p stagecraft-ui --test tutorial
//!
//! also writes screens of some steps (`60-tutorial-*`).

use std::cell::RefCell;
use std::rc::Rc;

use craft_dialogs::{Answer, Request};
use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use stagecraft_core::{Engine, ObjectKind};
use stagecraft_ui::state::PanelKind;
use stagecraft_ui::{Studio, ThemePref, Tool};

const W: f32 = 1440.0;

fn harness(shots: bool) -> Harness<'static, Studio> {
    let mut studio = Studio::new(Engine::new());
    studio.ui.theme = ThemePref::Light;
    let mut b = Harness::builder().with_context_setup(stagecraft_ui::framework::install_headless)
        .with_max_steps(8)
        .with_size(vec2(W, 900.0))
        // Real frame times, so separate clicks are not double clicks.
        .with_step_dt(1.0 / 60.0);
    if shots {
        let rs = egui_kittest::wgpu::create_render_state(
            egui_kittest::wgpu::default_wgpu_setup(),
            egui_wgpu::RendererOptions::PREDICTABLE,
        );
        stagecraft_ui::view3d::gpu::install(&rs);
        studio.offscreen = Some(stagecraft_ui::view3d::offscreen::Offscreen::from_render_state(&rs));
        b = b.renderer(egui_kittest::wgpu::WgpuTestRenderer::from_render_state(rs));
    }
    let mut h = b.build_ui_state(|ui, s: &mut Studio| s.show(ui), studio);
    h.run();
    h
}

struct Tour {
    h: Harness<'static, Studio>,
    dir: Option<String>,
}

impl Tour {
    fn state(&self) -> &Studio {
        self.h.state()
    }

    /// The tutorial's step, 1-based (13 = finished).
    fn step(&self) -> usize {
        self.state().ui.tutorial.step + 1
    }

    fn id(&self) -> &'static str {
        self.state().ui.tutorial.current().map_or("finished", |s| s.id)
    }

    fn pause(&mut self) {
        for _ in 0..40 {
            self.h.step();
        }
    }

    fn click(&mut self, label: &str) {
        self.h.get_by_label(label).click();
        self.h.run();
        self.pause();
    }

    fn click_at(&mut self, pos: Pos2, modifiers: Modifiers) {
        self.h.hover_at(pos);
        self.h.run();
        for pressed in [true, false] {
            self.h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers });
        }
        self.h.run();
        self.pause();
    }

    fn click_world(&mut self, p: [f64; 2]) {
        let pos = self.state().ui.camera.to_screen(p);
        assert!(self.state().ui.camera.rect.contains(pos), "{p:?} is off the plan");
        self.click_at(pos, Modifiers::NONE);
    }

    fn hover_world(&mut self, p: [f64; 2]) {
        let pos = self.state().ui.camera.to_screen(p);
        self.h.hover_at(pos);
        self.h.run();
    }

    /// Text into whatever has the keyboard (a typed length, a table cell), then Enter.
    fn type_enter(&mut self, text: &str) {
        self.h.event(Event::Text(text.into()));
        self.h.run();
        self.h.key_press(Key::Enter);
        self.h.run();
    }

    /// Click a field by its accessible name, replace its text, Enter.
    fn fill(&mut self, label: &str, text: &str) {
        self.h.get_by_label(label).click();
        self.h.run();
        self.h.get_by_label(label).type_text(text);
        self.h.run();
        self.h.key_press(Key::Enter);
        self.h.run();
    }

    /// The pulsing outline points at `label`'s widget.
    fn points_at(&mut self, label: &str) {
        self.h.run();
        let target = stagecraft_ui::tutorial::target_rect(&self.h.ctx, self.h.state()).expect("an outline");
        let widget = self.h.get_by_label(label).rect();
        assert!(target.contains_rect(widget.shrink(1.0)), "outline {target:?} not on {label} {widget:?}");
    }

    fn expect_step(&self, n: usize, id: &str) {
        assert_eq!((self.step(), self.id()), (n, id), "status: {:?}", self.state().ui.status);
    }

    fn shot(&mut self, name: &str) {
        let Some(dir) = self.dir.clone() else { return };
        self.h.run();
        let img = self.h.render().expect("render");
        img.save(format!("{dir}/{name}.png")).expect("save");
    }
}

fn count(s: &Studio, ty: &str) -> usize {
    s.engine.doc().objects.values().filter(|o| o.kind.type_name() == ty).count()
}

#[test]
fn the_tutorial_builds_a_basic_stage_through_the_gui() {
    let dir = std::env::var("STAGECRAFT_SCREENSHOTS").ok();
    let shots = dir.is_some() && stagecraft_ui::view3d::offscreen::Offscreen::headless().is_some();
    if let Some(d) = &dir {
        std::fs::create_dir_all(d).expect("screenshot dir");
    }
    let mut tour = Tour { h: harness(shots), dir: dir.filter(|_| shots) };

    // Help > Tutorial opens the panel and starts.
    assert!(!tour.state().ui.layout.layout().contains(&PanelKind::Tutorial));
    tour.click("Help");
    tour.h.get_by_role_and_label(egui::accesskit::Role::MenuItem, "Tutorial").click();
    tour.h.run();
    assert!(tour.state().ui.layout.layout().contains(&PanelKind::Tutorial));
    tour.expect_step(1, "layers");
    tour.points_at("Add layer");
    tour.shot("60-tutorial-layers");

    // 1. Layers: keep Stage, add House.
    tour.click("Add layer");
    assert_eq!(tour.step(), 1, "a layer called Layer 2 is not House yet");
    tour.fill("Layer name Layer 2", "House");
    tour.expect_step(2, "walls");

    // 2. The room: 14 × 9 m with typed lengths, closed on the first corner.
    tour.points_at("Tool Wall");
    tour.click("Tool Wall");
    tour.click_world([-7000.0, -4500.0]);
    tour.hover_world([0.0, -4500.0]);
    tour.type_enter("14m");
    tour.hover_world([7000.0, 0.0]);
    tour.type_enter("9m");
    tour.hover_world([0.0, 4500.0]);
    tour.type_enter("14m");
    tour.click_world([-7000.0, -4500.0]);
    assert_eq!(count(tour.state(), "wall"), 1);
    tour.expect_step(3, "floor");

    // 3. Stage floor: Top height first (the outline moves there), then the upstage half.
    tour.points_at("Tool Floor");
    tour.click("Tool Floor");
    tour.points_at("Option Top height");
    tour.shot("61-tutorial-floor-option");
    tour.fill("Option Top height", "1000");
    for p in [[-7000.0, 0.0], [7000.0, 0.0], [7000.0, 4500.0]] {
        tour.click_world(p);
    }
    tour.hover_world([-7000.0, 4500.0]);
    tour.click_world([-7000.0, 4500.0]);
    tour.shot("62-tutorial-floor-close");
    tour.click_world([-7000.0, 0.0]);
    let floor = tour.state().engine.doc().objects.values().find(|o| o.kind.type_name() == "floor").cloned();
    assert_eq!(floor.expect("a floor").placement.position[2], 1000.0);
    tour.expect_step(4, "portal");

    // 4. Portal on the setting line.
    tour.points_at("Tool Portal");
    tour.click("Tool Portal");
    tour.click_world([0.0, 0.0]);
    assert_eq!(count(tour.state(), "proscenium"), 1);
    tour.expect_step(5, "curtain");

    // 5. Curtain upstage of the portal, renamed in the Inspector.
    tour.points_at("Tool Curtain");
    tour.click("Tool Curtain");
    tour.click_world([-4000.0, 1200.0]);
    tour.click_world([4000.0, 1200.0]);
    assert_eq!(count(tour.state(), "curtain"), 1);
    assert_eq!(tour.step(), 5, "not named yet");
    tour.points_at("Inspector Name");
    tour.fill("Inspector Name", "Main curtain");
    tour.expect_step(6, "batten");

    // 6.–7. A batten and a truss.
    tour.points_at("Tool Batten");
    tour.click("Tool Batten");
    tour.click_world([0.0, 2200.0]);
    tour.expect_step(7, "truss");
    tour.points_at("Tool Truss");
    tour.click("Tool Truss");
    tour.click_world([-4000.0, 3500.0]);
    tour.click_world([4000.0, 3500.0]);
    tour.expect_step(8, "fixtures");

    // 8. Demo fixture types, pick one, hang two on the batten.
    tour.points_at("Demo fixtures");
    tour.shot("63-tutorial-fixtures");
    tour.click("Demo fixtures");
    assert_eq!(tour.state().engine.doc().fixture_types.len(), 3);
    // The outline is on the first type in the list.
    let first = tour.state().engine.doc().fixture_types.keys().next().unwrap().clone();
    tour.points_at(&format!("Fixture type {first}"));
    tour.click(&format!("Fixture type {first}"));
    assert_eq!(tour.state().ui.tool, Tool::Fixture);
    tour.click_world([-2000.0, 2200.0]);
    tour.click_world([2000.0, 2200.0]);
    let hung = tour
        .state()
        .engine
        .doc()
        .objects
        .values()
        .filter(|o| matches!(o.kind, ObjectKind::Fixture { .. }) && o.parent.is_some())
        .count();
    assert_eq!(hung, 2, "both hang on the batten");
    tour.expect_step(9, "patch");

    // 9. Patch in the table: FIDs and addresses with Fill series.
    tour.points_at("FID 1");
    tour.click("FID 1");
    tour.type_enter("101");
    tour.click("FID 1");
    tour.h.get_by_label("FID 2").click_modifiers(Modifiers::SHIFT);
    tour.h.run();
    tour.click("Fill series");
    tour.points_at("Address 1");
    tour.shot("64-tutorial-patch");
    tour.click("Address 1");
    tour.type_enter("1.001");
    tour.click("Address 1");
    tour.h.get_by_label("Address 2").click_modifiers(Modifiers::SHIFT);
    tour.h.run();
    tour.click("Fill series");
    let patched: Vec<(Option<u32>, Option<u32>)> = tour
        .state()
        .engine
        .doc()
        .objects
        .values()
        .filter_map(|o| match &o.kind {
            ObjectKind::Fixture { fid, address, .. } => Some((*fid, *address)),
            _ => None,
        })
        .collect();
    assert!(patched.iter().all(|(f, a)| f.is_some() && a.is_some()), "{patched:?}");
    tour.expect_step(10, "sheet");

    // 10. A sheet with plan, section and 3D viewports.
    tour.points_at("View Sheets");
    tour.click("View Sheets");
    tour.points_at("New sheet");
    tour.click("New sheet");
    for v in ["plan", "section_y", "rendered"] {
        tour.points_at(&format!("Add viewport {v}"));
        tour.click(&format!("Add viewport {v}"));
    }
    tour.expect_step(11, "title");

    // 11. Title block fields: the sheet in the Inspector (no viewport selected).
    tour.click("Sheet 1");
    tour.points_at("Title block project");
    tour.fill("Title block project", "Basic stage");
    tour.points_at("Title block venue");
    tour.fill("Title block venue", "Studio");
    tour.expect_step(12, "pdf");

    // 12. Export PDF; the save dialog answers with a path.
    let out = tempfile::tempdir().unwrap();
    let pdf = out.path().join("basic-stage.pdf");
    let asked = Rc::new(RefCell::new(Vec::<Request>::new()));
    let log = asked.clone();
    let answer = pdf.clone();
    tour.h.state_mut().dialogs.set_override(Some(Box::new(move |r: &Request| {
        log.borrow_mut().push(r.clone());
        Answer::Paths(vec![answer.clone()])
    })));
    tour.points_at("Export PDF");
    tour.shot("65-tutorial-pdf");
    tour.click("Export PDF");
    for _ in 0..2 {
        tour.h.state_mut().file_dialogs();
        tour.h.state_mut().dialogs.show_pending::<eframe::Frame>(None);
    }
    tour.h.run();
    assert_eq!(asked.borrow().len(), 1);
    assert!(std::fs::metadata(&pdf).map(|m| m.len() > 1000).unwrap_or(false), "a PDF was written");
    tour.expect_step(13, "finished");
    assert!(tour.state().ui.tutorial.finished());
    tour.shot("66-tutorial-finished");
    if let Some(d) = &tour.dir {
        std::fs::copy(&pdf, format!("{d}/tutorial-basic-stage.pdf")).expect("copy pdf");
        tour.click("View 3D");
        tour.shot("67-tutorial-result-3d");
    }
    tour.click("Tutorial close");
    assert!(!tour.state().ui.tutorial.active);
}

#[test]
fn back_and_skip_move_by_hand_and_ui_tutorial_reports() {
    let mut h = harness(false);
    let r = h.state_mut().run("ui.tutorial", serde_json::json!({ "action": "start" })).unwrap();
    assert_eq!(r["step"], 1);
    assert_eq!(r["of"], 12);
    h.run();
    h.get_by_label("Tutorial next").click();
    h.run();
    assert_eq!(h.state().ui.tutorial.step, 1, "Skip moves on without the step done");
    h.get_by_label("Tutorial back").click();
    h.run();
    assert_eq!(h.state().ui.tutorial.step, 0);
    let r = h.state_mut().run("ui.tutorial", serde_json::json!({})).unwrap();
    assert_eq!(r["id"], "layers");
    h.get_by_label("Tutorial stop").click();
    h.run();
    assert!(!h.state().ui.tutorial.active);
    assert!(h.query_by_label("Tutorial start").is_some());
}

/// On a fresh profile the app offers the tutorial in a small modal; the answer is kept
/// in the settings. A plain `Studio` (as in every other test) does not ask.
#[test]
fn a_fresh_profile_is_offered_the_tutorial_once() {
    use stagecraft_ui::onboarding::TutorialOffer;
    let build = |studio: Studio| {
        let mut h = Harness::builder().with_context_setup(stagecraft_ui::framework::install_headless)
            .with_max_steps(8)
            .with_size(egui::vec2(1400.0, 900.0))
            .build_ui_state(|ui, s: &mut Studio| s.show(ui), studio);
        h.run();
        h
    };
    let quiet = build(Studio::new(stagecraft_core::Engine::new()));
    assert!(quiet.query_by_label("Start tutorial").is_none(), "tests are not asked");

    let mut studio = Studio::new(stagecraft_core::Engine::new());
    assert_eq!(studio.prefs.get().onboarding.tutorial, TutorialOffer::Ask, "a fresh profile");
    studio.offer_tutorial();
    let mut h = build(studio);
    assert!(h.query_by_label_contains("Build a basic stage?").is_some());
    assert!(h.query_by_label("Not now").is_some() && h.query_by_label("Don't ask again").is_some());
    // Not modal: the rest of the window keeps working while the offer shows.
    h.get_by_label("Tool Wall").click();
    h.run();
    assert_eq!(h.state().ui.tool, Tool::Wall, "the window stays usable");
    assert!(h.state().ui.tutorial_offer, "the offer stays until answered");
    h.get_by_label("Start tutorial").click();
    h.run();
    assert!(h.state().ui.tutorial.active, "the tour starts");
    assert!(!h.state().ui.tutorial_offer);
    assert_eq!(h.state().prefs.get().onboarding.tutorial, TutorialOffer::Started);
    // Started once: not asked again.
    h.state_mut().ui.tutorial.active = false;
    h.state_mut().offer_tutorial();
    assert!(!h.state().ui.tutorial_offer);

    // "Not now" asks again next time, "Don't ask again" never.
    let mut studio = Studio::new(stagecraft_core::Engine::new());
    studio.offer_tutorial();
    let mut h = build(studio);
    h.get_by_label("Not now").click();
    h.run();
    assert!(!h.state().ui.tutorial.active && !h.state().ui.tutorial_offer);
    assert_eq!(h.state().prefs.get().onboarding.tutorial, TutorialOffer::Later);
    h.state_mut().offer_tutorial();
    h.run();
    h.get_by_label("Don't ask again").click();
    h.run();
    assert_eq!(h.state().prefs.get().onboarding.tutorial, TutorialOffer::Never);
    h.state_mut().offer_tutorial();
    assert!(!h.state().ui.tutorial_offer);
}
