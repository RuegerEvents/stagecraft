//! Small widgets with accessible names, so tests find them.

use egui::{Response, Role, TextEdit, Ui, WidgetInfo};

/// Give a response an accessible name.
pub fn name(resp: &Response, typ: Role, name: impl Into<String>) {
    let name = name.into();
    resp.widget_info(|| WidgetInfo::labeled(typ, true, name.clone()));
}

/// A button whose accessible name may differ from its text (icons, short labels).
pub fn button(ui: &mut Ui, text: impl Into<egui::WidgetText>, accessible: &str) -> Response {
    let r = ui.button(text);
    name(&r, Role::Button, accessible);
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
    name(&r, Role::TextInput, accessible);
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

/// Small painted icons: Roboto lacks arrows and crosses, and painted shapes look the
/// same in every font.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Up,
    Down,
    Close,
    Plus,
    /// Filled when on: the active layer.
    Dot(bool),
}

pub fn icon_button(ui: &mut Ui, icon: Icon, accessible: &str) -> Response {
    let size = egui::vec2(18.0, 18.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    name(&resp, Role::Button, accessible);
    if ui.is_rect_visible(rect) {
        let v = ui.style().interact(&resp);
        let p = ui.painter();
        if resp.hovered() {
            p.rect_filled(rect, 3.0, v.weak_bg_fill);
        }
        let c = rect.center();
        let col = v.fg_stroke.color;
        let st = egui::Stroke::new(1.4, col);
        let d = 4.0;
        match icon {
            Icon::Up => {
                p.add(egui::Shape::convex_polygon(
                    vec![c + egui::vec2(0.0, -d), c + egui::vec2(d, d * 0.6), c + egui::vec2(-d, d * 0.6)],
                    col,
                    egui::Stroke::NONE,
                ));
            }
            Icon::Down => {
                p.add(egui::Shape::convex_polygon(
                    vec![c + egui::vec2(-d, -d * 0.6), c + egui::vec2(d, -d * 0.6), c + egui::vec2(0.0, d)],
                    col,
                    egui::Stroke::NONE,
                ));
            }
            Icon::Close => {
                p.line_segment([c + egui::vec2(-d, -d), c + egui::vec2(d, d)], st);
                p.line_segment([c + egui::vec2(-d, d), c + egui::vec2(d, -d)], st);
            }
            Icon::Plus => {
                p.line_segment([c + egui::vec2(-d, 0.0), c + egui::vec2(d, 0.0)], st);
                p.line_segment([c + egui::vec2(0.0, -d), c + egui::vec2(0.0, d)], st);
            }
            Icon::Dot(on) => {
                if on {
                    p.circle_filled(c, 4.5, crate::theme::ACCENT);
                } else {
                    p.circle_stroke(c, 4.0, st);
                }
            }
        }
    }
    resp
}

/// A segmented control (Plan | 3D | Split | Sheets): `(label, accessible name, chosen)` per
/// segment; returns the segment clicked.
pub fn segmented(ui: &mut Ui, c: &crate::theme::Chrome, items: &[(String, String, bool)]) -> Option<usize> {
    let font = egui::FontId::proportional(12.5);
    let pad = egui::vec2(11.0, 4.0);
    let galleys: Vec<_> = items
        .iter()
        .map(|(label, _, on)| {
            ui.painter().layout_no_wrap(label.clone(), font.clone(), if *on { c.text } else { c.text2 })
        })
        .collect();
    let h = 26.0;
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 2.0 * pad.x).collect();
    let total = widths.iter().sum::<f32>() + 4.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total, h), egui::Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, 7.0, c.segment);
    let mut x = rect.left() + 2.0;
    let mut clicked = None;
    for (i, ((_, a11y, on), g)) in items.iter().zip(galleys).enumerate() {
        let seg = egui::Rect::from_min_size(egui::pos2(x, rect.top() + 2.0), egui::vec2(widths[i], h - 4.0));
        let r = ui.interact(seg, ui.scope_id().with(("segment", i)), egui::Sense::click());
        name(&r, Role::Button, a11y.as_str());
        if *on {
            p.rect_filled(seg.translate(egui::vec2(0.0, 0.5)), 5.0, egui::Color32::from_black_alpha(40));
            p.rect_filled(seg, 5.0, c.segment_on);
        } else if r.hovered() {
            p.rect_filled(seg, 5.0, c.hover);
        }
        p.galley(seg.center() - g.size() / 2.0, g, c.text);
        if r.clicked() {
            clicked = Some(i);
        }
        x += widths[i];
    }
    clicked
}
