//! Small widgets with accessible names, so tests find them.

use egui::{Response, TextEdit, Ui, WidgetInfo, WidgetType};

/// Give a response an accessible name.
pub fn name(resp: &Response, typ: WidgetType, name: impl Into<String>) {
    let name = name.into();
    resp.widget_info(|| WidgetInfo::labeled(typ, true, name.clone()));
}

/// A button whose accessible name may differ from its text (icons, short labels).
pub fn button(ui: &mut Ui, text: impl Into<egui::WidgetText>, accessible: &str) -> Response {
    let r = ui.button(text);
    name(&r, WidgetType::Button, accessible);
    r
}

/// A text field that edits a copy of `current` and hands back the new text once:
/// on Enter, or when focus leaves with a change. Escape drops the edit.
pub fn edit_field(ui: &mut Ui, id: egui::Id, current: &str, width: f32, accessible: &str) -> Option<String> {
    edit_field_hint(ui, id, current, width, accessible, "")
}

pub fn edit_field_hint(
    ui: &mut Ui,
    id: egui::Id,
    current: &str,
    width: f32,
    accessible: &str,
    hint: &str,
) -> Option<String> {
    let focused = ui.memory(|m| m.has_focus(id));
    let mut buf = if focused {
        ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.to_owned())
    } else {
        current.to_owned()
    };
    let r = ui.add(TextEdit::singleline(&mut buf).id(id).desired_width(width).hint_text(hint));
    name(&r, WidgetType::TextEdit, accessible);
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if r.gained_focus() {
        select_all(ui.ctx(), id, &buf);
    }
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    }
    if r.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(id));
        if !escape && buf != current {
            return Some(buf);
        }
    }
    None
}

/// Select the whole text of a `TextEdit`, so typing replaces it.
pub fn select_all(ctx: &egui::Context, id: egui::Id, text: &str) {
    let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
    let range =
        egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()));
    state.cursor.set_char_range(Some(range));
    state.store(ctx, id);
}
