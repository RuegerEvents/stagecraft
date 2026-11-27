//! The menus: one tree ([`bar`]) built from the studio's state, laid out by `craft-menu` for
//! the platform. On macOS the app installs it as the native menu bar (`craft_menu::mac`), and
//! the window has no menu row; elsewhere, and in headless tests, [`show_in_window`] draws the
//! same menus with craft-menu's egui bar.
//!
//! An item's command is a command of either registry with its arguments (`ui.set_view
//! {"view": "3d"}`, `history.undo`), run through [`Studio::run`], or one of the `app.*`
//! actions below that need the file dialogs or the palette. Shortcuts with ⌘ come back from
//! the native menu as key presses and run through the window's own shortcut handling
//! (`app.rs`), so keys and menus never disagree.

use craft_menu::{
    Chord, CommandRef, Gate, Item, ItemRole, LayoutOptions, Menu, MenuBar, MenuBarUi, MenuRole, Node, Target,
};
use serde_json::{Value, json};

use crate::app::Studio;
use crate::i18n::t;
use crate::state::{FileRequest, PanelKind, ThemePref, Tool, View};

pub const APP_NAME: &str = "StageCraft";

fn chord(text: &str) -> Chord {
    Chord::parse(text).unwrap_or_else(|e| panic!("menu shortcut {text}: {e}"))
}

fn item(id: &str, label: &str) -> Item {
    Item::new(CommandRef::new(id), label)
}

fn with(id: &str, params: Value, label: &str) -> Item {
    Item::new(CommandRef::with_params(id, params), label)
}

fn node(i: Item) -> Node {
    Node::Item(i)
}

/// The tool's menu item: `ui.set_tool`, its single-key shortcut, checked while active.
fn tool_item(s: &Studio, tool: Tool) -> Node {
    let mut i = with("ui.set_tool", json!({ "tool": tool }), t(tool.key())).radio(s.ui.tool == tool);
    if let Some(k) = tool.shortcut() {
        i = i.shortcut(chord(&format!("{k:?}")));
    }
    node(i)
}

/// Every menu with what is enabled and checked now.
pub fn bar(s: &Studio) -> MenuBar {
    let doc = s.engine.doc();
    let any_selected = !s.engine.selection().is_empty();
    let sheets = !doc.sheets.is_empty();
    let file = Menu::new(
        t("menu.file"),
        vec![
            node(item("app.new", t("menu.new")).shortcut(chord("cmd+N"))),
            node(item("app.open", t("menu.open")).shortcut(chord("cmd+O"))),
            Node::Separator,
            node(item("app.save", t("menu.save")).shortcut(chord("cmd+S"))),
            node(item("app.save_as", t("menu.save_as")).shortcut(chord("cmd+shift+S"))),
            Node::Separator,
            node(item("app.import_mvr", t("menu.import_mvr"))),
            node(item("app.import_gdtf", t("library.import"))),
            node(item("app.import_model", t("menu.import_model"))),
            node(item("app.import_underlay", t("menu.import_underlay"))),
            Node::Separator,
            node(item("app.export_mvr", t("menu.export_mvr"))),
            node(item("app.export_pdf", t("menu.export_pdf")).enabled(sheets)),
            Node::Separator,
            node(item("ui.project_settings", t("menu.project_settings")).shortcut(chord("cmd+Comma"))),
        ],
    )
    .role(MenuRole::File);
    let edit = Menu::new(
        t("menu.edit"),
        vec![
            node(item("history.undo", t("menu.undo")).shortcut(chord("cmd+Z")).enabled(s.engine.can_undo())),
            node(item("history.redo", t("menu.redo")).shortcut(chord("cmd+shift+Z")).enabled(s.engine.can_redo())),
            Node::Separator,
            node(item("app.delete", t("menu.delete")).shortcut(chord("Backspace")).enabled(any_selected)),
            node(item("app.select_all", t("menu.select_all")).shortcut(chord("cmd+A"))),
            node(item("selection.clear", t("menu.select_none")).enabled(any_selected)),
            Node::Separator,
            tool_item(s, Tool::Select),
            tool_item(s, Tool::Move),
            tool_item(s, Tool::Rotate),
            Node::Separator,
            node(item("app.palette", t("menu.palette")).shortcut(chord("cmd+K"))),
        ],
    )
    .role(MenuRole::Edit);
    let views = [
        (View::Plan, "view.plan", "cmd+1"),
        (View::ThreeD, "view.3d", "cmd+2"),
        (View::Split, "view.split", "cmd+3"),
        (View::Sheets, "view.sheets", "cmd+4"),
    ];
    let mut view_items: Vec<Node> = views
        .iter()
        .map(|(v, key, sc)| {
            node(with("ui.set_view", json!({ "view": v }), t(key)).shortcut(chord(sc)).radio(s.ui.view == *v))
        })
        .collect();
    view_items.push(Node::Separator);
    view_items.push(node(item("ui.zoom_to_fit", t("menu.zoom_fit")).shortcut(chord("cmd+0"))));
    view_items.push(Node::submenu(
        t("menu.standard_views"),
        crate::view3d::StdView::ALL.iter().map(|v| node(with("ui.camera", json!({ "view": v }), t(v.key())))).collect(),
    ));
    view_items.push(Node::Separator);
    view_items.push(node(item("ui.reset_layout", t("menu.reset_layout"))));
    view_items.push(Node::submenu(
        t("menu.theme"),
        [(ThemePref::System, "theme.system"), (ThemePref::Light, "theme.light"), (ThemePref::Dark, "theme.dark")]
            .iter()
            .map(|(th, key)| node(with("ui.set_theme", json!({ "theme": th }), t(key)).radio(s.ui.theme == *th)))
            .collect(),
    ));
    let view = Menu::new(t("menu.view"), view_items).role(MenuRole::View);

    let underlays = doc.objects.values().any(|o| o.kind.type_name() == "underlay");
    let mut insert = vec![tool_item(s, Tool::Floor), tool_item(s, Tool::Wall), tool_item(s, Tool::Dimension)];
    if underlays {
        insert.push(tool_item(s, Tool::Calibrate));
    }
    insert.push(Node::Separator);
    insert.extend([Tool::Truss, Tool::Run, Tool::Fixture].map(|tool| tool_item(s, tool)));
    insert.push(Node::Separator);
    insert.extend([Tool::Batten, Tool::Curtain, Tool::Proscenium, Tool::Wings].map(|tool| tool_item(s, tool)));
    insert.push(Node::Separator);
    insert.extend([Tool::Platform, Tool::Support, Tool::Hoist, Tool::Ceiling].map(|tool| tool_item(s, tool)));
    let insert = Menu::new(t("menu.insert"), insert);

    let window = Menu::new(
        t("menu.window"),
        crate::dock::PANELS
            .into_iter()
            .filter(|p| *p != PanelKind::Canvas)
            .map(|p| {
                node(
                    with("ui.toggle_panel", json!({ "panel": p }), crate::dock::label(p))
                        .check(s.ui.layout.layout().contains(&p)),
                )
            })
            .collect(),
    )
    .role(MenuRole::Window);
    let help = Menu::new(
        t("menu.help"),
        vec![
            node(with("ui.tutorial", json!({ "action": "start" }), t("menu.tutorial"))),
            node(item("app.commands", t("menu.search_commands"))),
            node(item("app.about", t("menu.about"))),
        ],
    )
    .role(MenuRole::Help);
    let mut bar = MenuBar::new(vec![file, edit, view, insert, window, help]);
    bar.tag_item(&CommandRef::new("app.about"), ItemRole::About);
    bar.tag_submenu(MenuRole::View, t("menu.theme"), ItemRole::Appearance);
    bar
}

pub fn options() -> LayoutOptions<'static> {
    LayoutOptions::new(APP_NAME)
}

/// The menus as this platform lays them out.
pub fn laid_out(s: &Studio) -> craft_menu::Layout {
    craft_menu::layout(&bar(s), Target::current(), &options())
}

/// What may run from the menus this frame.
pub fn gate(s: &Studio, ctx: &egui::Context) -> Gate {
    Gate::default().modal(s.ui.palette.open).text_editing(ctx.text_edit_focused())
}

/// The in-window menu bar (Windows, Linux, web, headless tests).
pub fn show_in_window(ui: &mut egui::Ui, s: &mut Studio) {
    // The picks come back as fired commands (`framework::take_menu_picks`).
    // Off macOS the target is this platform; on a Mac (headless tests) Windows', since the
    // Mac's menus are the system bar's and the in-window bar would show none.
    let target = if Target::current() == Target::Mac { Target::Windows } else { Target::current() };
    let bar = craft_ui::contract::Tracked::new(craft_menu::layout(&bar(s), target, &options()).bar);
    {
        use craft_ui::contract::ShowContainer as _;
        craft_ui::layout_components::Stack::new().show(ui, |ui, _| {
            MenuBarUi::new(&bar).target(target).show(ui);
        });
    }
}

/// Run a menu item's command.
pub fn run(s: &mut Studio, c: &CommandRef) {
    let request = |s: &mut Studio, r: FileRequest| s.ui.file_request = Some(r);
    match c.id.as_str() {
        "app.new" => crate::panels::new_document(s),
        "app.open" => request(s, FileRequest::Open),
        "app.save" => crate::panels::save(s, false),
        "app.save_as" => crate::panels::save(s, true),
        "app.import_mvr" => request(s, FileRequest::ImportMvr),
        "app.import_gdtf" => request(s, FileRequest::ImportGdtf),
        "app.import_model" => request(s, FileRequest::ImportModel),
        "app.import_underlay" => request(s, FileRequest::ImportUnderlay),
        "app.export_mvr" => request(s, FileRequest::ExportMvr),
        "app.export_pdf" => request(s, FileRequest::ExportPdf),
        "app.palette" | "app.commands" => s.ui.palette.open(),
        "app.about" => s.ui.info(format!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"))),
        "app.delete" => {
            let ids = s.engine.selection().to_vec();
            let _ = s.run("object.delete", json!({ "ids": ids }));
        }
        "app.select_all" => {
            let ids = crate::canvas::visible_ids(s.engine.doc(), &s.ui);
            let _ = s.run("selection.set", json!({ "ids": ids }));
        }
        id => {
            let _ = s.run(id, c.params.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(bar: &MenuBar) -> Vec<&Item> {
        bar.items()
    }

    #[test]
    fn every_item_runs_a_known_command() {
        let s = Studio::new(crate::demo::engine());
        let reg = stagecraft_core::registry();
        for i in items(&bar(&s)) {
            let id = i.command.id.as_str();
            assert!(
                id.starts_with("app.") || crate::commands::ui_registry().get(id).is_some() || reg.get(id).is_some(),
                "{id}"
            );
        }
    }

    #[test]
    fn the_mac_layout_keeps_the_standard_shortcuts() {
        let s = Studio::new(crate::demo::engine());
        let laid = craft_menu::mac_layout(&bar(&s), &options());
        assert!(laid.clashes.is_empty(), "{:?}", laid.clashes);
        let titles: Vec<&str> = laid.bar.menus.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, [APP_NAME, "File", "Edit", "View", "Insert", "Window", "Help"]);
        let find = |id: &str| items(&laid.bar).into_iter().find(|i| i.command.id == id).map(|i| i.shortcut);
        assert_eq!(find("app.save_as"), Some(Some(chord("cmd+shift+S"))));
        assert_eq!(find("history.redo"), Some(Some(chord("cmd+shift+Z"))));
        assert_eq!(find("app.palette"), Some(Some(chord("cmd+K"))));
    }
}
