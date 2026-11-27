//! [`Studio`] is the engine plus UI state and draws the whole window into any
//! `egui::Ui` (so tests drive it headlessly). [`run`] opens the native window with a
//! control server in GUI mode.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use egui::{KeyboardShortcut, Modifiers, Ui};
use serde_json::{Value, json};
use stagecraft_control::{Server, discovery};
use stagecraft_core::{CmdError, Engine};

use crate::commands::{UiCtx, UiExtension, check_shot_path, ui_registry};
use crate::i18n::{t, tf};
use crate::state::{FileRequest, PendingShot, Tool, UiState, View};
use crate::{canvas, chrome, palette, panels, theme, view3d};

pub struct Studio {
    pub engine: Engine,
    pub ui: UiState,
    /// Renders the 3D view into images for `rendered` sheet viewports. `None` without
    /// a GPU: those viewports keep a placeholder.
    pub offscreen: Option<view3d::offscreen::Offscreen>,
    /// File and message dialogs: queued during the frame, answered on a later one.
    pub dialogs: craft_dialogs::Dialogs<crate::dialogs::Then>,
    /// Per-user settings: panel layout, theme and tool options. In memory unless [`Studio::with_prefs`]
    /// got a store on disk.
    pub prefs: craft_settings::SettingsStore<crate::dock::Prefs>,
    /// The workspace the dock shows and the panels' specs, kept across frames.
    pub(crate) dock: crate::dock::Live,
}

impl Studio {
    pub fn new(engine: Engine) -> Self {
        Self::with_prefs(engine, craft_settings::SettingsStore::open(craft_settings::Location::Memory).0)
    }

    /// A studio that starts from saved settings and keeps them there.
    pub fn with_prefs(engine: Engine, prefs: craft_settings::SettingsStore<crate::dock::Prefs>) -> Self {
        stagecraft_sheet::install();
        let ui = UiState {
            layout: crate::dock::load(prefs.get().layout.workspace.clone()),
            theme: prefs.get().appearance.theme,
            options: prefs.get().tools.clone(),
            recent: prefs.get().palette.recent.clone(),
            tool_columns: prefs.get().tool_palette.columns,
            ..Default::default()
        };
        Self { engine, ui, offscreen: None, dialogs: crate::dialogs::queue(), prefs, dock: Default::default() }
    }

    /// Run a command of either registry. Errors also go to the status bar.
    pub fn run(&mut self, name: &str, args: Value) -> Result<Value, CmdError> {
        if matches!(name, "sheet.export_pdf" | "sheet.export_svg") {
            self.render_sheet_images(None);
        }
        let out = match ui_registry().get(name) {
            Some(e) => e.run(&mut UiCtx { engine: &mut self.engine, ui: &mut self.ui }, args),
            None => self.engine.execute(name, args),
        };
        match &out {
            Err(e) if e.code == "conflict" => self.ui.error(tf("status.rejected", &[("message", &e.message)])),
            Err(e) => self.ui.error(e.message.clone()),
            Ok(_) => {
                if name == "sheet.export_pdf" {
                    self.ui.tutorial.pdf_exported = true;
                }
                self.after(name)
            }
        }
        out
    }

    /// Render the 3D images of `rendered` viewports on `sheet` (all sheets if `None`)
    /// that are out of date. Does nothing without a GPU.
    pub fn render_sheet_images(&mut self, sheet: Option<stagecraft_core::Id>) {
        let Some(off) = &mut self.offscreen else { return };
        let doc = self.engine.doc();
        let ids: Vec<_> = match sheet {
            Some(id) => vec![id],
            None => doc.sheets.keys().copied().collect(),
        };
        off.render_sheets(doc, &ids);
    }

    /// UI consequences of a command, whoever ran it.
    pub fn after(&mut self, name: &str) {
        if matches!(name, "doc.open" | "doc.new") {
            self.ui.fit_pending = true;
            self.ui.view3d.fit_pending = true;
            self.ui.view3d.follow = true;
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

    /// The document's file name, or "Untitled".
    fn file_name(&self) -> String {
        self.engine
            .path()
            .and_then(|p| p.file_name())
            .map_or_else(|| t("app.untitled").to_owned(), |n| n.to_string_lossy().into_owned())
    }

    /// The OS window title: file name and a dot for unsaved changes.
    pub fn title(&self) -> String {
        let dirty = if self.engine.dirty() { " •" } else { "" };
        format!("{}{dirty} — StageCraft", self.file_name())
    }

    /// The title the window's own title row shows (macOS inset title bar).
    pub fn window_title(&self) -> String {
        let dirty = if self.engine.dirty() { " •" } else { "" };
        format!("StageCraft — {}{dirty}", self.file_name())
    }

    /// The whole window.
    pub fn show(&mut self, ui: &mut Ui) {
        if !theme::ready(ui.ctx()) {
            return;
        }
        theme::set_preference(ui.ctx(), self.ui.theme);
        // The in-window menu bar's picks (on macOS the native menu's come from `StageApp`).
        for c in crate::framework::take_menu_picks(ui.ctx()) {
            crate::menu::run(self, &c);
        }
        crate::library::poll(self);
        crate::tutorial::update(self);
        crate::tutorial::begin_frame(ui.ctx(), self);
        shortcuts(ui, self);
        if !self.ui.native_menu {
            egui::Panel::top("menu").show(ui, |ui| crate::menu::show_in_window(ui, self));
        }
        let none = egui::Frame::NONE;
        if self.ui.inset_title_bar {
            let bar = theme::of(ui).bar;
            egui::Panel::top("title")
                .exact_size(chrome::TITLE_BAR)
                .frame(none.fill(bar))
                .show(ui, |ui| chrome::title_row(ui, self));
        }
        let bar = theme::of(ui).bar;
        egui::Panel::top("toolbar")
            .exact_size(chrome::TOOL_BAR)
            .frame(none.fill(bar))
            .show(ui, |ui| chrome::toolbar(ui, self));
        egui::Panel::top("options")
            .exact_size(chrome::OPTIONS_BAR)
            .frame(none)
            .show(ui, |ui| chrome::options_row(ui, self));
        egui::Panel::bottom("status")
            .exact_size(chrome::STATUS_BAR)
            .frame(none)
            .show(ui, |ui| chrome::status_bar(ui, self));
        egui::CentralPanel::no_frame().show(ui, |ui| crate::dock::show(ui, self));
        palette::show(ui.ctx(), self);
        crate::settings::show(ui.ctx(), self);
        crate::onboarding::show(ui.ctx(), self);
        crate::tutorial::paint(ui.ctx(), self);
        self.keep_prefs(ui.ctx());
    }

    /// Hand layout, theme and tool option changes to the settings store, which saves them shortly after.
    fn keep_prefs(&mut self, ctx: &egui::Context) {
        let (layout, theme) = (&self.ui.layout, self.ui.theme);
        let tools = crate::state::ToolOptions {
            fixture_type: String::new(),
            fixture_mode: String::new(),
            ..self.ui.options.clone()
        };
        let p = self.prefs.get();
        let recent = &self.ui.recent;
        if p.layout.workspace.as_ref() != Some(layout)
            || p.appearance.theme != theme
            || p.tools != tools
            || &p.palette.recent != recent
            || p.tool_palette.columns != self.ui.tool_columns
        {
            let columns = self.ui.tool_columns;
            let (layout, recent) = (layout.clone(), recent.clone());
            self.prefs.update(|p| {
                p.layout.workspace = Some(layout);
                p.appearance.theme = theme;
                p.tools = tools;
                p.palette.recent = recent;
                p.tool_palette.columns = columns;
            });
        }
        let tick = self.prefs.tick(std::time::Instant::now());
        if let craft_settings::SaveStatus::Failed(e) = &tick.status {
            self.ui.error(format!("settings: {e}"));
        }
        if let Some(deadline) = tick.next_deadline {
            ctx.request_repaint_after(deadline.saturating_duration_since(std::time::Instant::now()));
        }
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
    // Shifted first: ⌘⇧Z also matches ⌘Z when shift is ignored.
    if pressed(ui, cmd_shift(Key::S)) {
        panels::save(s, true);
    }
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
    if pressed(ui, cmd(Key::Comma)) {
        let _ = s.run("ui.project_settings", Value::Null);
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
    let text_focus = crate::framework::other_focus(ui.ctx()) || s.ui.table.focused;
    if text_focus {
        return;
    }
    if pressed(ui, cmd(Key::A)) {
        let ids = canvas::visible_ids(s.engine.doc(), &s.ui);
        let _ = s.run("selection.set", json!({ "ids": ids }));
    }
    let typing = crate::tools::typing_now(ui, s);
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
        } else if del && crate::sketch_ui::delete_picks(s) {
        } else if del && let Some(src) = s.ui.sketch.selected {
            let mut args = crate::sketch_ui::dim_args(src);
            let id = args["id"].take();
            args["ids"] = json!([id]);
            if s.run("dimension.remove", args).is_ok() {
                s.ui.sketch.selected = None;
            }
        }
    }
}

/// The native app: a [`Studio`] plus the control server answering in the frame loop.
pub struct StageApp {
    pub studio: Studio,
    server: Option<Server>,
    _registration: Option<discovery::Registration>,
    shot_requested: bool,
    /// The macOS menu bar, when AppKit took it.
    #[cfg(target_os = "macos")]
    native_menu: Option<craft_menu::NativeMenu>,
    /// Close already sent after the user confirmed.
    closing: bool,
    /// Filled by the screenshot callback once the frame arrives.
    shot_slot: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<egui::ColorImage>>>>,
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
        let image = self.shot_slot.lock().ok().and_then(|mut s| s.take());
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
            let slot = self.shot_slot.clone();
            let ctx = ui.ctx().clone();
            let done = egui::ScreenshotCallback::new(move |image| {
                if let Ok(mut s) = slot.lock() {
                    *s = Some(image);
                }
                ctx.request_repaint();
            });
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(done));
            self.shot_requested = true;
            ui.ctx().request_repaint();
        }
    }
}

fn save_png(image: &egui::ColorImage, path: &std::path::Path) -> Result<(), CmdError> {
    let [w, h] = image.size;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let img = image::RgbaImage::from_raw(w as u32, h as u32, bytes)
        .ok_or_else(|| CmdError::io("screenshot has the wrong size"))?;
    img.save(path).map_err(|e| CmdError::io(format!("{}: {e}", path.display())))
}

impl eframe::App for StageApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let _ = raw_input;
        #[cfg(target_os = "macos")]
        if let Some(menu) = &mut self.native_menu {
            menu.poll(_ctx);
        }
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.answer_calls(ctx);
        // Quit and the close button: ask first when there are unsaved changes. The runtime
        // holds the close while the question is up; the answer closes the window again.
        if self.studio.ui.quit_confirmed && !self.closing {
            self.closing = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let dirty = self.studio.engine.dirty();
        let ui = &mut self.studio.ui;
        crate::framework::close_path(ctx, || {
            let hold = dirty && !ui.quit_confirmed && ui.native_dialogs;
            if hold {
                ui.file_request = Some(FileRequest::Quit);
            }
            hold
        });
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        self.take_screenshots(ui);
        #[cfg(target_os = "macos")]
        if let Some(menu) = &mut self.native_menu {
            menu.set_gate(crate::menu::gate(&self.studio, ui.ctx()));
            for c in menu.take_clicks(ui.ctx()) {
                crate::menu::run(&mut self.studio, &c);
            }
        }
        self.studio.show(ui);
        #[cfg(target_os = "macos")]
        {
            if let Some(menu) = &mut self.native_menu {
                let studio = &self.studio;
                let clashes = menu.sync_with(&EveryPass, || {
                    craft_menu::mac_layout(&crate::menu::bar(studio), &crate::menu::options())
                });
                for c in clashes {
                    eprintln!("stagecraft: menu shortcut clash: {c:?}");
                }
            }
        }
        // Never a blocking dialog here: see `dialogs.rs`.
        self.studio.file_dialogs();
        self.studio.dialogs.show_pending(Some(&*frame));
        let title = self.studio.title();
        if title != self.title {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn on_exit(&mut self) {
        if let Err(e) = self.studio.prefs.flush() {
            eprintln!("stagecraft: could not save settings: {e}");
        }
    }
}

/// A menu key that changes every pass: the native menu is laid out again each frame, as
/// the menus read the document and the UI state.
struct EveryPass;

impl craft_ui::contract::MemoProps for EveryPass {
    fn key(&self, key: &mut craft_ui::contract::MemoKey) {
        key.changes_every_pass();
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Open the window. The control server listens on 127.0.0.1 and announces itself with
/// mode "gui", so `stagecraft exec` and MCP drive this window.
pub fn run(engine: Engine) -> Result<(), String> {
    let title = Studio::new(Engine::new()).title();
    let viewport = egui::ViewportBuilder::default()
        .with_title(title.clone())
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([900.0, 560.0]);
    // macOS: a full-size content view whose top 28 pt are the title row (`chrome::title_row`,
    // AppKit centres the window buttons in it), as craft-window decides for every Craft app. Elsewhere the desktop draws the
    // frame: StageCraft has no caption buttons of its own. `STAGECRAFT_TITLEBAR=system` or
    // `CRAFT_TITLEBAR=system` keeps the macOS title bar too.
    use craft_window::policy::{Backend, Frame, TitleBarPref, apply, decide_for};
    let pref = if cfg!(target_os = "macos") { TitleBarPref::Integrated } else { TitleBarPref::System };
    let decision = decide_for(craft_window::Prefix::new("STAGECRAFT"), Backend::from_env(), pref);
    let inset = matches!(decision.frame, Frame::Inset(_));
    let viewport = if inset || !cfg!(target_os = "macos") { apply(viewport, &decision.frame) } else { viewport };
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "StageCraft",
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            crate::framework::install(&ctx, inset.then(|| decision.clone()));
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
            let (prefs, report) = craft_settings::SettingsStore::open(craft_settings::Location::Auto);
            if report.has_problems() {
                eprintln!("stagecraft: settings: {report:?}");
            }
            let mut studio = Studio::with_prefs(engine, prefs);
            studio.ui.native_dialogs = true;
            // First launch (or "Not now" last time): offer the tutorial.
            studio.offer_tutorial();
            let wake = ctx.clone();
            studio.dialogs = craft_dialogs::Dialogs::new(craft_dialogs::Wake::new(move || wake.request_repaint()));
            studio.offscreen = cc.wgpu_render_state.as_ref().map(view3d::offscreen::Offscreen::from_render_state);
            #[cfg(target_os = "macos")]
            let native_menu = {
                let menu = craft_menu::mac::install(&ctx, &crate::menu::laid_out(&studio).bar);
                if menu.is_none() {
                    eprintln!("stagecraft: AppKit refused the menu bar, keeping the in-window menus");
                }
                menu
            };
            #[cfg(target_os = "macos")]
            {
                studio.ui.native_menu = native_menu.is_some();
                studio.ui.inset_title_bar = inset;
            }
            Ok(Box::new(StageApp {
                studio,
                server,
                _registration: registration,
                shot_requested: false,
                shot_slot: Default::default(),
                #[cfg(target_os = "macos")]
                native_menu,
                closing: false,
                title,
            }))
        }),
    )
    .map_err(|e| e.to_string())
}
