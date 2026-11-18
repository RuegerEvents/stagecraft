//! [`Studio`] is the engine plus UI state and draws the whole window into any
//! `egui::Ui` (so tests drive it headlessly). [`run`] opens the native window with a
//! control server in GUI mode.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use egui::{Event, KeyboardShortcut, Modifiers, Ui};
use serde_json::{Value, json};
use stagecraft_control::{Server, discovery};
use stagecraft_core::{CmdError, Engine};

use crate::commands::{UiCtx, UiExtension, check_shot_path, ui_registry};
use crate::i18n::{t, tf};
use crate::state::{FileRequest, PendingShot, Tool, UiState, View};
use crate::{canvas, palette, panels, table, theme, view3d};

pub struct Studio {
    pub engine: Engine,
    pub ui: UiState,
}

impl Studio {
    pub fn new(engine: Engine) -> Self {
        stagecraft_sheet::install();
        Self { engine, ui: UiState::default() }
    }

    /// Run a command of either registry. Errors also go to the status bar.
    pub fn run(&mut self, name: &str, args: Value) -> Result<Value, CmdError> {
        let out = match ui_registry().get(name) {
            Some(e) => e.run(&mut UiCtx { engine: &mut self.engine, ui: &mut self.ui }, args),
            None => self.engine.execute(name, args),
        };
        match &out {
            Err(e) => self.ui.error(e.message.clone()),
            Ok(_) => self.after(name),
        }
        out
    }

    /// UI consequences of a command, whoever ran it.
    pub fn after(&mut self, name: &str) {
        if matches!(name, "doc.open" | "doc.new") {
            self.ui.fit_pending = true;
            self.ui.view3d.fit_pending = true;
            self.ui.hidden_tags.clear();
            self.ui.active_layer = None;
            self.ui.gesture = Default::default();
            self.ui.sheets = Default::default();
            self.ui.plan_labels = None;
        }
    }

    /// The layer argument for new root objects.
    pub fn active_layer_arg(&self) -> Value {
        match self.ui.active_layer.filter(|l| self.engine.doc().layers.contains_key(l)) {
            Some(l) => json!(l),
            None => Value::Null,
        }
    }

    /// The window title: file name and a dot for unsaved changes.
    pub fn title(&self) -> String {
        let name = self
            .engine
            .path()
            .and_then(|p| p.file_name())
            .map_or_else(|| t("app.untitled").to_owned(), |n| n.to_string_lossy().into_owned());
        let dirty = if self.engine.dirty() { " •" } else { "" };
        format!("{name}{dirty} — StageCraft")
    }

    /// The whole window.
    pub fn show(&mut self, ui: &mut Ui) {
        if !theme::ready(ui.ctx()) {
            return;
        }
        theme::set_preference(ui.ctx(), self.ui.theme);
        // Tab and arrows belong to the typed dimension or the table, not to egui's
        // focus cycling, while those take keys.
        let drawing = self.ui.tool.draws() && !self.ui.gesture.points.is_empty();
        if (drawing || self.ui.table.focused) && ui.memory(|m| m.focused().is_none()) {
            ui.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
        }
        crate::library::poll(self);
        shortcuts(ui, self);
        egui::Panel::top("menu").show(ui, |ui| panels::menu_bar(ui, self));
        egui::Panel::top("toolbar").show(ui, |ui| panels::toolbar(ui, self));
        egui::Panel::bottom("status").show(ui, |ui| panels::status_bar(ui, self));
        if self.ui.panels.table && self.ui.view != View::Sheets {
            egui::Panel::bottom("table")
                .resizable(true)
                .default_size(230.0)
                .min_size(120.0)
                .show(ui, |ui| table::show(ui, self));
        }
        let sheets = self.ui.view == View::Sheets;
        if sheets {
            egui::Panel::left("left").resizable(true).default_size(270.0).min_size(220.0).show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| crate::sheets::list_panel(ui, self));
            });
        } else if self.ui.panels.layers || self.ui.panels.tags || self.ui.panels.library {
            egui::Panel::left("left").resizable(true).default_size(270.0).min_size(220.0).show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.ui.panels.layers {
                        panels::layers(ui, self);
                        ui.add_space(10.0);
                    }
                    if self.ui.panels.tags {
                        panels::tags(ui, self);
                        ui.add_space(10.0);
                    }
                    if self.ui.panels.library {
                        crate::library::show(ui, self);
                    }
                });
            });
        }
        if self.ui.panels.inspector {
            egui::Panel::right("inspector").resizable(true).default_size(280.0).min_size(240.0).show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if sheets { crate::sheets::inspector(ui, self) } else { crate::inspector::show(ui, self) }
                });
            });
        }
        egui::CentralPanel::no_frame().show(ui, |ui| match self.ui.view {
            View::Plan => canvas::show(ui, self),
            View::ThreeD => view3d::show(ui, self),
            View::Sheets => crate::sheets::show(ui, self),
            View::Split => {
                let (left, right) = ui.max_rect().split_left_right_at_fraction(0.5);
                ui.scope_builder(egui::UiBuilder::new().max_rect(left.shrink2(egui::vec2(0.5, 0.0))), |ui| {
                    canvas::show(ui, self)
                });
                ui.scope_builder(egui::UiBuilder::new().max_rect(right.shrink2(egui::vec2(0.5, 0.0))), |ui| {
                    view3d::show(ui, self)
                });
                let x = left.right();
                ui.painter().vline(x, left.y_range(), ui.visuals().widgets.noninteractive.bg_stroke);
            }
        });
        palette::show(ui.ctx(), self);
    }
}

/// Window-wide shortcuts. Single-letter tool keys only when nothing takes text.
fn shortcuts(ui: &mut Ui, s: &mut Studio) {
    let cmd = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
    let cmd_shift = |k| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, k);
    use egui::Key;
    let pressed = |ui: &mut Ui, sc: KeyboardShortcut| ui.input_mut(|i| i.consume_shortcut(&sc));
    if pressed(ui, cmd(Key::K)) {
        s.ui.palette.toggle();
    }
    if s.ui.palette.open {
        return;
    }
    // Redo before undo: ⌘⇧Z also matches ⌘Z when shift is ignored.
    if pressed(ui, cmd_shift(Key::Z)) {
        let _ = s.run("history.redo", Value::Null);
    }
    if pressed(ui, cmd(Key::Z)) {
        let _ = s.run("history.undo", Value::Null);
    }
    if pressed(ui, cmd(Key::S)) {
        panels::save(s, false);
    }
    if pressed(ui, cmd(Key::O)) {
        s.ui.file_request = Some(FileRequest::Open);
    }
    if pressed(ui, cmd(Key::N)) {
        panels::new_document(s);
    }
    if pressed(ui, cmd(Key::Num0)) {
        let _ = s.run("ui.zoom_to_fit", Value::Null);
    }
    // ⌘1 plan, ⌘2 3D, ⌘3 split, ⌘4 sheets.
    for (key, view) in
        [(Key::Num1, View::Plan), (Key::Num2, View::ThreeD), (Key::Num3, View::Split), (Key::Num4, View::Sheets)]
    {
        if pressed(ui, cmd(key)) {
            let _ = s.run("ui.set_view", json!({ "view": view }));
        }
    }
    let text_focus = ui.ctx().memory(|m| m.focused().is_some()) || s.ui.table.focused;
    if text_focus {
        return;
    }
    if pressed(ui, cmd(Key::A)) {
        let ids = canvas::visible_ids(s.engine.doc(), &s.ui);
        let _ = s.run("selection.set", json!({ "ids": ids }));
    }
    let typing = s.ui.gesture.dim.is_some();
    if !typing {
        for tool in Tool::ALL {
            let Some(key) = tool.shortcut() else { continue };
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, key)) {
                s.ui.set_tool(tool);
            }
        }
        let drawing = !s.ui.gesture.points.is_empty();
        let del = ui.input_mut(|i| {
            i.consume_key(Modifiers::NONE, Key::Delete) || (!drawing && i.consume_key(Modifiers::NONE, Key::Backspace))
        });
        if del && !s.engine.selection().is_empty() {
            let ids = s.engine.selection().to_vec();
            let _ = s.run("object.delete", json!({ "ids": ids }));
        }
    }
}

/// The native app: a [`Studio`] plus the control server answering in the frame loop.
pub struct StageApp {
    pub studio: Studio,
    server: Option<Server>,
    _registration: Option<discovery::Registration>,
    shot_requested: bool,
    title: String,
}

impl StageApp {
    fn answer_calls(&mut self, ctx: &egui::Context) {
        let Some(server) = &self.server else { return };
        let mut any = false;
        while let Ok(call) = server.calls.try_recv() {
            any = true;
            let command = call.params.get("command").and_then(Value::as_str).map(str::to_owned);
            if call.method == "execute" && command.as_deref() == Some("ui.screenshot") {
                let path = call.params.pointer("/args/path").and_then(Value::as_str).map(PathBuf::from);
                match path.map(|p| check_shot_path(&p).map(|_| p)) {
                    Some(Ok(path)) => self.studio.ui.shots.push(PendingShot { path, reply: Some(call) }),
                    Some(Err(e)) => call.reply(Err(e.into())),
                    None => call.reply(Err(CmdError::invalid_args("ui.screenshot needs `path`").into())),
                }
                continue;
            }
            call.answer_with(&mut self.studio.engine, &mut UiExtension { ui: &mut self.studio.ui });
            if let Some(c) = command {
                self.studio.after(&c);
            }
        }
        if any {
            ctx.request_repaint();
        }
    }

    fn take_screenshots(&mut self, ui: &Ui) {
        let image = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            self.shot_requested = false;
            for shot in std::mem::take(&mut self.studio.ui.shots) {
                let out = save_png(&image, &shot.path)
                    .map(|_| json!({ "path": shot.path, "width": image.size[0], "height": image.size[1] }));
                match &out {
                    Ok(_) => self.studio.ui.info(tf("status.screenshot", &[("path", &shot.path.display())])),
                    Err(e) => self.studio.ui.error(e.message.clone()),
                }
                if let Some(call) = shot.reply {
                    call.reply(out.map_err(Into::into));
                }
            }
        }
        if !self.studio.ui.shots.is_empty() && !self.shot_requested {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.shot_requested = true;
            ui.ctx().request_repaint();
        }
    }

    fn file_dialogs(&mut self) {
        let Some(req) = self.studio.ui.file_request.take() else { return };
        let s = &mut self.studio;
        let filter = |d: rfd::FileDialog| d.add_filter("StageCraft", &["stagecraft"]);
        match req {
            FileRequest::Open => {
                if s.engine.dirty() && !confirm_discard_or_test(s) {
                    return;
                }
                if let Some(path) = filter(rfd::FileDialog::new()).pick_file() {
                    let _ = s.run("doc.open", json!({ "path": path, "discard": true }));
                }
            }
            FileRequest::SaveAs => {
                if let Some(path) = filter(rfd::FileDialog::new()).set_file_name("Untitled.stagecraft").save_file() {
                    let _ = s.run("doc.save", json!({ "path": path }));
                }
            }
            FileRequest::ExportPdf => {
                let name = s
                    .engine
                    .path()
                    .and_then(|p| p.file_stem())
                    .map_or_else(|| "Sheets".to_owned(), |n| n.to_string_lossy().into_owned());
                if let Some(path) =
                    rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name(format!("{name}.pdf")).save_file()
                    && s.run("sheet.export_pdf", json!({ "path": path })).is_ok()
                {
                    s.ui.info(tf("status.exported", &[("path", &path.display())]));
                }
            }
            FileRequest::ImportGdtf => {
                if let Some(paths) = rfd::FileDialog::new().add_filter("GDTF", &["gdtf"]).pick_files() {
                    let steps: Vec<Value> =
                        paths.iter().map(|p| json!({ "command": "gdtf.import", "args": { "path": p } })).collect();
                    if s.run("batch", json!({ "commands": steps })).is_ok() {
                        s.ui.info(tf("library.imported", &[("n", &paths.len())]));
                    }
                }
            }
            FileRequest::Logo => {
                if let Some(path) = rfd::FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg"]).pick_file() {
                    let _ = s.run("doc.set_meta", json!({ "logo_file": path }));
                }
            }
        }
    }
}

/// Ask before throwing away changes; headless (tests) always says yes.
pub fn confirm_discard_or_test(s: &Studio) -> bool {
    !s.ui.native_dialogs || confirm_discard()
}

pub fn confirm_discard() -> bool {
    rfd::MessageDialog::new()
        .set_title(t("dialog.discard.title"))
        .set_description(t("dialog.discard.text"))
        .set_buttons(rfd::MessageButtons::OkCancel)
        .show()
        == rfd::MessageDialogResult::Ok
}

fn save_png(image: &egui::ColorImage, path: &std::path::Path) -> Result<(), CmdError> {
    let [w, h] = image.size;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let img = image::RgbaImage::from_raw(w as u32, h as u32, bytes)
        .ok_or_else(|| CmdError::io("screenshot has the wrong size"))?;
    img.save(path).map_err(|e| CmdError::io(format!("{}: {e}", path.display())))
}

impl eframe::App for StageApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.answer_calls(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.take_screenshots(ui);
        self.studio.show(ui);
        self.file_dialogs();
        let title = self.studio.title();
        if title != self.title {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Open the window. The control server listens on 127.0.0.1 and announces itself with
/// mode "gui", so `stagecraft exec` and MCP drive this window.
pub fn run(engine: Engine) -> Result<(), String> {
    let title = Studio::new(Engine::new()).title();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title.clone())
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "StageCraft",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            theme::apply(&ctx);
            match &cc.wgpu_render_state {
                Some(rs) => view3d::gpu::install(rs),
                None => eprintln!("stagecraft: no wgpu renderer, the 3D view stays empty"),
            }
            let token = std::env::var("STAGECRAFT_TOKEN").unwrap_or_else(|_| discovery::new_token());
            let wake = ctx.clone();
            let (server, registration) = match Server::spawn(token.clone(), 0, move || wake.request_repaint()) {
                Ok(server) => {
                    let reg = discovery::register(&discovery::Instance {
                        pid: std::process::id(),
                        addr: server.addr,
                        token,
                        mode: "gui".into(),
                        started: now(),
                    });
                    if let Err(e) = &reg {
                        eprintln!("stagecraft: could not write the instance file: {e}");
                    }
                    eprintln!("stagecraft: control channel on {}", server.addr);
                    (Some(server), reg.ok())
                }
                Err(e) => {
                    eprintln!("stagecraft: control channel not available: {e}");
                    (None, None)
                }
            };
            let mut studio = Studio::new(engine);
            studio.ui.native_dialogs = true;
            Ok(Box::new(StageApp { studio, server, _registration: registration, shot_requested: false, title }))
        }),
    )
    .map_err(|e| e.to_string())
}
