//! The guided tour "Build a basic stage": a dockable panel that walks through the steps
//! from layers to a PDF. Each step points at its tool or panel with a pulsing outline,
//! shows the tool's key, and moves on by itself once the document (or the UI) has what
//! the step asks for. Back and Skip move by hand; `ui.tutorial` does the same for AIs.
//!
//! Outlines: the current step names the widgets it points at (accessible names such as
//! `Tool Wall`, or a panel's tab). [`mark`] records their rects while the frame draws
//! (`widgets::name` and the tool buttons call it), and [`paint`] outlines the first one
//! found after the frame.

use egui::{Rect, RichText, Stroke, Ui};
use serde::Serialize;
use serde_json::json;
use stagecraft_core::sheet::ViewKind;
use stagecraft_core::{Document, ObjectKind};

use crate::app::Studio;
use crate::i18n::{t, tf};
use crate::state::{PanelKind, Tool, View};
use crate::widgets;

/// Where a step points.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A widget by its accessible name.
    Widget(String),
    /// A panel's tab.
    Panel(PanelKind),
}

fn w(name: impl Into<String>) -> Target {
    Target::Widget(name.into())
}

pub struct Step {
    /// Catalog keys `tutorial.<id>.title` and `tutorial.<id>.text`.
    pub id: &'static str,
    /// The key to press, as shown on the key cap.
    pub key: Option<&'static str>,
    /// Whether the drawing has what the step asks for.
    pub done: fn(&Studio) -> bool,
    /// What to point at now, best first; the first one on screen is outlined.
    pub targets: fn(&Studio) -> Vec<Target>,
}

fn objects<'d>(doc: &'d Document, ty: &'d str) -> impl Iterator<Item = &'d stagecraft_core::Object> + 'd {
    doc.objects.values().filter(move |o| o.kind.type_name() == ty)
}

fn tool(s: &Studio, tool: Tool) -> Vec<Target> {
    if s.ui.tool == tool { Vec::new() } else { vec![w(format!("Tool {}", t(tool.key())))] }
}

fn reddish(color: &str) -> bool {
    stagecraft_core::model::parse_color(color)
        .is_some_and(|[r, g, b]| r >= 0x60 && u16::from(r) > 2 * u16::from(g) && u16::from(r) > 2 * u16::from(b))
}

/// Fixtures with a GDTF type.
fn typed_fixtures(doc: &Document) -> Vec<&stagecraft_core::Object> {
    doc.objects.values().filter(|o| matches!(&o.kind, ObjectKind::Fixture { gdtf: Some(_), .. })).collect()
}

fn sheet_views(doc: &Document) -> (bool, bool, bool) {
    let mut best = (false, false, false);
    for sh in doc.sheets.values() {
        let has = |f: fn(&ViewKind) -> bool| sh.viewports.iter().any(|v| f(&v.view));
        let got = (
            has(|v| matches!(v, ViewKind::Plan | ViewKind::PlanFrom3d { .. })),
            has(|v| matches!(v, ViewKind::Section { .. })),
            has(|v| matches!(v, ViewKind::Rendered { .. })),
        );
        if [got.0, got.1, got.2].iter().filter(|x| **x).count()
            > [best.0, best.1, best.2].iter().filter(|x| **x).count()
        {
            best = got;
        }
    }
    best
}

pub const STEPS: [Step; 12] = [
    Step {
        id: "layers",
        key: None,
        done: |s| {
            let names: Vec<String> = s.engine.doc().layers.values().map(|l| l.name.trim().to_lowercase()).collect();
            names.iter().any(|n| n == "stage") && names.iter().any(|n| n == "house")
        },
        targets: |_| vec![w("Add layer"), Target::Panel(PanelKind::Layers)],
    },
    Step {
        id: "walls",
        key: Some("W"),
        done: |s| {
            objects(s.engine.doc(), "wall")
                .any(|o| o.sketch.is_some() && matches!(&o.kind, ObjectKind::Wall { path, .. } if path.len() >= 4))
        },
        targets: |s| tool(s, Tool::Wall),
    },
    Step {
        id: "floor",
        key: Some("F"),
        done: |s| objects(s.engine.doc(), "floor").any(|o| o.placement.position[2] >= 500.0),
        targets: |s| {
            if s.ui.tool == Tool::Floor && s.ui.options.floor_z < 500.0 {
                vec![w(format!("Option {}", t("option.top_height")))]
            } else {
                tool(s, Tool::Floor)
            }
        },
    },
    Step {
        id: "portal",
        key: None,
        done: |s| objects(s.engine.doc(), "proscenium").next().is_some(),
        targets: |s| tool(s, Tool::Proscenium),
    },
    Step {
        id: "curtain",
        key: Some("C"),
        done: |s| {
            objects(s.engine.doc(), "curtain").any(|o| {
                o.name.to_lowercase().contains("main")
                    && matches!(&o.kind, ObjectKind::Curtain { color, .. } if color.is_empty() || reddish(color))
            })
        },
        targets: |s| {
            let doc = s.engine.doc();
            if objects(doc, "curtain").next().is_none() {
                return tool(s, Tool::Curtain);
            }
            let curtain_selected = s
                .engine
                .selection()
                .iter()
                .any(|id| doc.objects.get(id).is_some_and(|o| o.kind.type_name() == "curtain"));
            if curtain_selected {
                vec![w(format!("Inspector {}", t("inspector.name"))), Target::Panel(PanelKind::Inspector)]
            } else {
                vec![w("Tool Select")]
            }
        },
    },
    Step {
        id: "batten",
        key: Some("B"),
        done: |s| objects(s.engine.doc(), "batten").next().is_some(),
        targets: |s| tool(s, Tool::Batten),
    },
    Step {
        id: "truss",
        key: Some("T"),
        done: |s| objects(s.engine.doc(), "truss").next().is_some(),
        targets: |s| tool(s, Tool::Truss),
    },
    Step {
        id: "fixtures",
        key: Some("L"),
        done: |s| typed_fixtures(s.engine.doc()).len() >= 2,
        targets: |s| {
            let doc = s.engine.doc();
            match doc.fixture_types.keys().next() {
                None => vec![w("Demo fixtures"), Target::Panel(PanelKind::Library)],
                Some(_) if s.ui.tool == Tool::Fixture && !s.ui.options.fixture_type.is_empty() => Vec::new(),
                Some(k) => vec![w(format!("Fixture type {k}")), Target::Panel(PanelKind::Library)],
            }
        },
    },
    Step {
        id: "patch",
        key: None,
        done: |s| {
            let doc = s.engine.doc();
            let typed = typed_fixtures(doc);
            typed.len() >= 2
                && typed.iter().all(|o| matches!(&o.kind, ObjectKind::Fixture { fid: Some(_), address: Some(_), .. }))
                && stagecraft_core::patch::conflicting(doc).is_empty()
        },
        targets: |s| {
            let typed = typed_fixtures(s.engine.doc());
            let missing = |f: fn(&ObjectKind) -> bool| typed.iter().any(|o| f(&o.kind));
            if missing(|k| matches!(k, ObjectKind::Fixture { fid: None, .. })) {
                vec![w("FID 1"), Target::Panel(PanelKind::Table)]
            } else if missing(|k| matches!(k, ObjectKind::Fixture { address: None, .. })) {
                vec![w("Address 1"), Target::Panel(PanelKind::Table)]
            } else {
                vec![w("Fill series"), Target::Panel(PanelKind::Table)]
            }
        },
    },
    Step {
        id: "sheet",
        key: Some("⌘4"),
        done: |s| sheet_views(s.engine.doc()) == (true, true, true),
        targets: |s| {
            if s.ui.view != View::Sheets {
                return vec![w(format!("View {}", t("view.sheets")))];
            }
            if s.engine.doc().sheets.is_empty() {
                return vec![w("New sheet"), Target::Panel(PanelKind::Sheets)];
            }
            let (plan, section, rendered) = sheet_views(s.engine.doc());
            let next = if !plan {
                "plan"
            } else if !section {
                "section_y"
            } else if !rendered {
                "rendered"
            } else {
                return Vec::new();
            };
            // The entry in the open + menu, else the + itself.
            vec![w(format!("Add viewport {next}")), w("Add viewport"), Target::Panel(PanelKind::Sheets)]
        },
    },
    Step {
        id: "title",
        key: None,
        done: |s| {
            let m = &s.engine.doc().meta;
            !m.project.trim().is_empty() && !m.venue.trim().is_empty()
        },
        targets: |s| {
            if s.ui.view != View::Sheets {
                return vec![w(format!("View {}", t("view.sheets")))];
            }
            let field = if s.engine.doc().meta.project.trim().is_empty() { "project" } else { "venue" };
            vec![w(format!("Title block {field}")), Target::Panel(PanelKind::Inspector)]
        },
    },
    Step {
        id: "pdf",
        key: None,
        done: |s| s.ui.tutorial.pdf_exported,
        targets: |s| {
            if s.ui.view != View::Sheets {
                return vec![w(format!("View {}", t("view.sheets")))];
            }
            vec![w("Export PDF"), Target::Panel(PanelKind::Inspector)]
        },
    },
];

/// Where the tour is.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Tutorial {
    pub active: bool,
    /// Index into [`STEPS`]; `STEPS.len()` once finished.
    pub step: usize,
    /// The step was already done when it was entered (after Back or Skip): it waits for
    /// Next instead of moving on by itself.
    pub entered_done: bool,
    /// A PDF was exported since the tour started (the last step).
    pub pdf_exported: bool,
    /// Moved by hand: [`update`] enters the step afresh.
    #[serde(skip)]
    pub reenter: bool,
}

impl Tutorial {
    pub fn finished(&self) -> bool {
        self.step >= STEPS.len()
    }

    pub fn current(&self) -> Option<&'static Step> {
        STEPS.get(self.step).filter(|_| self.active)
    }
}

/// Go to step `i` (clamped), remembering whether it is already done.
fn enter(s: &mut Studio, i: usize) {
    let i = i.min(STEPS.len());
    s.ui.tutorial.step = i;
    s.ui.tutorial.entered_done = STEPS.get(i).is_some_and(|st| (st.done)(s));
    // Panels the step works in come to the front of their tab group (the Library shares
    // one with GDTF Share).
    let panels: Vec<PanelKind> = STEPS
        .get(i)
        .map(|st| (st.targets)(s))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|t| match t {
            Target::Panel(p) if s.ui.layout.layout().contains(&p) => Some(p),
            _ => None,
        })
        .collect();
    for p in panels {
        crate::dock::bring_forward(&mut s.ui.layout, p);
    }
}

/// What `ui.tutorial` and the panel's buttons do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// From the first step; opens the Tutorial panel.
    Start,
    /// The next step (skips the current one if it is not done).
    Next,
    Back,
    /// End the tour (the panel stays open).
    Stop,
}

/// Move the tour. The checks need the whole [`Studio`], so the new step is entered
/// (done or not) by [`update`] on the next frame.
pub fn act(ui: &mut crate::UiState, a: Action) {
    match a {
        Action::Start => {
            ui.tutorial = Tutorial { active: true, ..Default::default() };
            crate::dock::bring_forward(&mut ui.layout, PanelKind::Tutorial);
        }
        Action::Next => ui.tutorial.step = (ui.tutorial.step + 1).min(STEPS.len()),
        Action::Back => ui.tutorial.step = ui.tutorial.step.saturating_sub(1),
        Action::Stop => ui.tutorial.active = false,
    }
    // Until `update` has looked, the step does not move on by itself.
    ui.tutorial.entered_done = true;
    ui.tutorial.reenter = true;
}

/// Once per frame before drawing: move on when the current step is done.
pub fn update(s: &mut Studio) {
    if !s.ui.tutorial.active {
        return;
    }
    if std::mem::take(&mut s.ui.tutorial.reenter) {
        let i = s.ui.tutorial.step;
        enter(s, i);
    }
    let Some(step) = s.ui.tutorial.current() else { return };
    let done = (step.done)(s);
    if done && !s.ui.tutorial.entered_done {
        let title = t(&format!("tutorial.{}.title", step.id)).to_owned();
        let i = s.ui.tutorial.step + 1;
        enter(s, i);
        if s.ui.status.as_ref().is_none_or(|(_, err)| !err) {
            s.ui.info(tf("tutorial.step_done", &[("step", &title)]));
        }
    } else if !done && s.ui.tutorial.entered_done {
        // Undone again (undo, deleted): wait for it once more.
        s.ui.tutorial.entered_done = false;
    }
}

/// The rects of this frame's targets, recorded while drawing.
#[derive(Clone, Default)]
struct Marks {
    wanted: Vec<String>,
    found: Vec<(String, Rect)>,
}

fn marks_id() -> egui::Id {
    egui::Id::unique("stagecraft_tutorial_marks")
}

/// Note where a named widget is, if the tutorial points at it. True if it does.
pub fn mark(ctx: &egui::Context, name: &str, rect: Rect) -> bool {
    let wanted = ctx.data(|d| d.get_temp::<Marks>(marks_id()).is_some_and(|m| m.wanted.iter().any(|x| x == name)));
    if wanted {
        ctx.data_mut(|d| d.get_temp_mut_or_default::<Marks>(marks_id()).found.push((name.to_owned(), rect)));
    }
    wanted
}

/// A target the tutorial points at that its panel has scrolled out of view scrolls in,
/// once per target (the user may scroll away again).
pub fn reveal(resp: &egui::Response, name: &str) {
    let id = egui::Id::unique("stagecraft_tutorial_revealed");
    let clipped = resp.interact_rect.height() < resp.rect.height() - 0.5;
    let done = resp.ctx.data(|d| d.get_temp::<String>(id)).is_some_and(|n| n == name);
    if clipped && !done {
        resp.scroll_to_me(Some(egui::Align::Center));
        resp.ctx.data_mut(|d| d.insert_temp(id, name.to_owned()));
    }
}

/// Before the frame draws: what to look for.
pub fn begin_frame(ctx: &egui::Context, s: &Studio) {
    let wanted =
        s.ui.tutorial
            .current()
            .map(|st| (st.targets)(s))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|t| match t {
                Target::Widget(n) => Some(n),
                Target::Panel(_) => None,
            })
            .collect();
    ctx.data_mut(|d| d.insert_temp(marks_id(), Marks { wanted, found: Vec::new() }));
}

/// The rect the tutorial points at this frame, if it is on screen.
pub fn target_rect(ctx: &egui::Context, s: &Studio) -> Option<Rect> {
    let step = s.ui.tutorial.current()?;
    let marks = ctx.data(|d| d.get_temp::<Marks>(marks_id())).unwrap_or_default();
    (step.targets)(s).into_iter().find_map(|t| match t {
        Target::Widget(n) => marks.found.iter().find(|(m, _)| *m == n).map(|(_, r)| *r),
        Target::Panel(p) => {
            let id = egui::Id::unique(crate::dock::DOCK_ID).with(("tab", p));
            ctx.read_response(id).map(|r| r.rect)
        }
    })
}

/// After the frame: a pulsing accent outline around the target.
pub fn paint(ctx: &egui::Context, s: &Studio) {
    let Some(rect) = target_rect(ctx, s) else { return };
    let time = ctx.input(|i| i.time);
    let pulse = (0.5 + 0.5 * (time * 4.0).sin()) as f32;
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("tutorial_pulse"));
    let p = ctx.layer_painter(layer);
    let accent = crate::theme::tokens(ctx.theme() == egui::Theme::Dark).accent;
    p.rect_stroke(rect.expand(2.0 + 2.0 * pulse), 7.0, Stroke::new(2.0, accent), egui::StrokeKind::Outside);
    p.rect_stroke(
        rect.expand(6.0 + 3.0 * pulse),
        9.0,
        Stroke::new(3.0, accent.gamma_multiply(0.25 * (1.0 - pulse) + 0.1)),
        egui::StrokeKind::Outside,
    );
    ctx.request_repaint_after(std::time::Duration::from_millis(33));
}

fn key_cap(ui: &mut Ui, key: &str) {
    let c = crate::theme::of(ui);
    egui::Frame::new()
        .fill(c.field)
        .stroke(Stroke::new(1.0, c.separator))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| ui.label(RichText::new(key).monospace().strong()));
}

/// The Tutorial panel.
pub fn panel(ui: &mut Ui, s: &mut Studio) {
    let c = crate::theme::of(ui);
    ui.label(RichText::new(t("tutorial.title")).strong().size(14.0));
    ui.add_space(4.0);
    let tut = s.ui.tutorial.clone();
    if !tut.active {
        ui.label(RichText::new(t("tutorial.intro")).color(c.text2));
        ui.add_space(8.0);
        let key = if tut.finished() { "tutorial.restart" } else { "tutorial.start" };
        if widgets::button(ui, t(key), "Tutorial start").clicked() {
            let _ = s.run("ui.tutorial", json!({ "action": "start" }));
        }
        return;
    }
    let Some(step) = tut.current() else {
        ui.label(RichText::new(t("tutorial.finished.title")).strong());
        ui.label(RichText::new(t("tutorial.finished.text")).color(c.text2));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if widgets::button(ui, t("tutorial.back"), "Tutorial back").clicked() {
                let _ = s.run("ui.tutorial", json!({ "action": "back" }));
            }
            if widgets::button(ui, t("tutorial.close"), "Tutorial close").clicked() {
                let _ = s.run("ui.tutorial", json!({ "action": "stop" }));
            }
        });
        return;
    };
    // Progress: n of total, a thin bar.
    ui.label(
        RichText::new(tf("tutorial.step_of", &[("n", &(tut.step + 1)), ("total", &STEPS.len())]))
            .small()
            .color(c.text2),
    );
    let (bar, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 3.0), egui::Sense::hover());
    ui.painter().rect_filled(bar, 1.5, c.separator);
    let frac = (tut.step as f32 + 1.0) / STEPS.len() as f32;
    ui.painter().rect_filled(Rect::from_min_size(bar.min, egui::vec2(bar.width() * frac, bar.height())), 1.5, c.accent);
    // The buttons stay on top, so a long step text never pushes them out of view.
    let done = (step.done)(s);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_enabled_ui(tut.step > 0, |ui| {
            if widgets::button(ui, t("tutorial.back"), "Tutorial back").clicked() {
                let _ = s.run("ui.tutorial", json!({ "action": "back" }));
            }
        });
        let next = if done { "tutorial.next" } else { "tutorial.skip" };
        if widgets::button(ui, t(next), "Tutorial next").clicked() {
            let _ = s.run("ui.tutorial", json!({ "action": "next" }));
        }
        if widgets::button(ui, t("tutorial.stop"), "Tutorial stop").clicked() {
            let _ = s.run("ui.tutorial", json!({ "action": "stop" }));
        }
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(t(&format!("tutorial.{}.title", step.id))).strong().size(13.5));
        if let Some(k) = step.key {
            ui.add_space(4.0);
            key_cap(ui, k);
        }
    });
    ui.add_space(2.0);
    ui.label(t(&format!("tutorial.{}.text", step.id)));
    ui.add_space(6.0);
    if done {
        ui.label(RichText::new(t("tutorial.done_step")).color(c.accent).strong());
    } else if target_rect(ui.ctx(), s).is_none() && !(step.targets)(s).is_empty() {
        ui.label(RichText::new(t("tutorial.look")).small().color(c.text2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_step_has_texts() {
        for st in &STEPS {
            for part in ["title", "text"] {
                let key = format!("tutorial.{}.{part}", st.id);
                assert!(crate::i18n::has(&key), "{key}");
            }
        }
    }

    #[test]
    fn red_is_red() {
        assert!(reddish("#8b1a1a"));
        assert!(!reddish("#1a1a1a"));
        assert!(!reddish("#8b8b1a"));
    }

    #[test]
    fn steps_advance_from_the_document_and_wait_after_back() {
        let mut s = Studio::new(stagecraft_core::Engine::new());
        s.run("ui.tutorial", json!({ "action": "start" })).unwrap();
        update(&mut s);
        assert_eq!(s.ui.tutorial.step, 0);
        assert!(s.ui.layout.layout().contains(&PanelKind::Tutorial), "the panel opens");
        s.run("layer.create", json!({ "name": "House" })).unwrap();
        update(&mut s);
        assert_eq!(s.ui.tutorial.step, 1, "Stage and House exist");
        s.run("ui.tutorial", json!({ "action": "back" })).unwrap();
        update(&mut s);
        update(&mut s);
        assert_eq!(s.ui.tutorial.step, 0, "a done step waits for Next after Back");
        s.run("ui.tutorial", json!({ "action": "next" })).unwrap();
        update(&mut s);
        assert_eq!(s.ui.tutorial.step, 1);
    }
}
