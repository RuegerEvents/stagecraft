//! The craft-ui runtime under StageCraft's own eframe `App` (craft-libs
//! `docs/craft-framework.md` §2.2). [`install`] puts it on the window's context once, before
//! the first frame; tests install it with [`install_headless`] (kittest's
//! `with_context_setup`). Each frame a plugin hands the runtime its input before the pass
//! ([`FrameHook`]): the theme mode egui resolved, so the dock's chrome follows StageCraft's
//! Light/Dark switch, and it keeps the commands the in-window menu bar fired for
//! [`take_menu_picks`]. The app runs the root window's close path through [`close_path`].
//!
//! The runtime takes Tab, Escape and the clipboard events for the whole window unless one of
//! StageCraft's own widgets has the keyboard. The canvas is declared each pass with
//! [`host_canvas`], which keeps those keys in egui's input for the drawing tools and the
//! typed dimensions while nothing else has the focus.

use std::sync::Arc;

use craft_host::target::ShellInfo;
use craft_menu::CommandRef;
use craft_ui::runtime::commands::FireSource;
use craft_ui::runtime::env::RootEnv;
use craft_ui::runtime::focus::{EditSet, KeepKeys, KeyUse};
use craft_ui::runtime::frame::FrameInput;
use craft_ui::shell::embed::{self, CloseDecision};
use egui::{Context, Id, ViewportId};

/// The window's frame as craft-window decides it (inset title bar on macOS).
pub use craft_window::policy::Decision;

fn picks_id() -> Id {
    Id::unique("stagecraft.framework.menu_picks")
}

/// Hands the runtime this frame's input before each root pass, from egui's input hook: the
/// same moment as eframe's `raw_input_hook`, and kittest runs it too.
struct FrameHook;

impl egui::Plugin for FrameHook {
    fn debug_name(&self) -> &'static str {
        "stagecraft.frame_hook"
    }

    fn input_hook(&mut self, ctx: &Context, input: &mut egui::RawInput) {
        if input.viewport_id != ViewportId::ROOT {
            return;
        }
        let mode = match ctx.theme() {
            egui::Theme::Light => craft_theme::Mode::Light,
            egui::Theme::Dark => craft_theme::Mode::Dark,
        };
        embed::before_input(ctx, |output| {
            let picks: Vec<CommandRef> =
                output.fired.into_iter().filter(|f| f.source == FireSource::Menu).map(|f| f.command).collect();
            if !picks.is_empty() {
                ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<CommandRef>>(picks_id()).extend(picks));
                ctx.request_repaint();
            }
            FrameInput::new(RootEnv::new().mode(mode))
        });
    }
}

fn install_with(
    ctx: &Context,
    shell: ShellInfo,
    windows: Arc<dyn craft_window::system::WindowSystem>,
    frame: Option<Decision>,
) {
    // Before the runtime's own plugin, so its input is in place when the runtime's hooks run.
    ctx.add_plugin(FrameHook);
    let mut spec = craft_ui::InstallSpec::new(shell, windows).app_id("stagecraft");
    if let Some(frame) = frame {
        spec = spec.frame(frame);
    }
    if let Err(e) = craft_ui::install(ctx, spec) {
        eprintln!("stagecraft: the UI runtime could not be installed: {e}");
    }
}

/// The desktop window's runtime, once, from eframe's creation context. `frame` is the
/// decision the window was opened with.
pub fn install(ctx: &Context, frame: Option<Decision>) {
    install_with(ctx, ShellInfo::default(), Arc::new(craft_window::system::SystemWindows), frame);
}

/// The runtime on a context without a native window: tests and the offscreen renderers.
pub fn install_headless(ctx: &Context) {
    // No system menu bar there: the menus are the in-window bar.
    let mut shell = ShellInfo::default();
    shell.caps.remove(craft_host::target::Caps::NATIVE_MENU_BAR);
    install_with(ctx, shell, Arc::new(craft_window::system::NoWindows), None);
}

/// The commands chosen in the in-window menu bar since the last call.
pub fn take_menu_picks(ctx: &Context) -> Vec<CommandRef> {
    ctx.data_mut(|d| d.remove_temp::<Vec<CommandRef>>(picks_id())).unwrap_or_default()
}

/// The root window's close path, last in `App::logic`: `hold` keeps the window open while
/// StageCraft asks about unsaved changes.
pub fn close_path(ctx: &Context, hold: impl FnOnce() -> bool) {
    embed::logic(ctx, || if hold() { CloseDecision::Hold } else { CloseDecision::Close });
}

/// The canvas widget's id: the plan, 3D view or sheet that has the window's keys.
pub fn canvas_id() -> Id {
    Id::unique("stagecraft.canvas")
}

/// The canvas's widget over the room `ui` has left: the window's key target ([`canvas_id`])
/// when `main`, else (the 3D half of the split view) one of its own.
pub fn canvas_widget(ui: &mut egui::Ui, main: bool) -> (egui::Rect, egui::Response) {
    let rect = ui.available_rect_before_wrap();
    let id = if main { canvas_id() } else { ui.scope_id().with("canvas") };
    let mut resp = ui.interact(rect, id, egui::Sense::click_and_drag());
    // The canvas holds the keyboard for the tools: Enter and Space are theirs (take the
    // point, finish the drawing), not a click on the canvas as egui makes of them.
    if ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space)) {
        resp.flags.remove(egui::response::Flags::FAKE_PRIMARY_CLICKED);
    }
    ui.advance_cursor_after_rect(rect);
    (rect, resp)
}

/// Whether a widget other than the canvas has the keyboard (a text field, a button reached
/// with Tab): StageCraft's own keys leave the keys to it.
pub fn other_focus(ctx: &Context) -> bool {
    ctx.memory(|m| m.focused().is_some_and(|id| id != canvas_id()))
}

/// Declares the canvas and gives it the keyboard when nothing else has it, so Tab, Escape,
/// Enter and the arrows reach the tools, the typed dimensions and the table. `drawing`: a
/// tool takes typed dimensions, so Tab is the canvas's (length → angle); otherwise the
/// canvas keeps the arrows.
pub fn canvas_keys(ctx: &Context, drawing: bool) {
    let keys = if drawing { KeyUse::CANVAS.with_keep(KeepKeys::Tab) } else { KeyUse::CANVAS };
    craft_ui::runtime::focus::host_canvas(ctx, canvas_id(), keys.with_edits(EditSet::ALL));
    if ctx.memory(|m| m.focused().is_none()) {
        ctx.memory_mut(|m| m.request_focus(canvas_id()));
    }
}
