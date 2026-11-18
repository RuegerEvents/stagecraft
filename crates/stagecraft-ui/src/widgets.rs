//! Small widgets with accessible names, so tests find them.

use egui::{Response, Role, TextEdit, Ui, WidgetInfo};

/// Give a response an accessible name.
pub fn name(resp: &Response, typ: Role, name: impl Into<String>) {
    let name = name.into();
    if crate::tutorial::mark(&resp.ctx, &name, resp.rect) {
        crate::tutorial::reveal(resp, &name);
    }
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
    // The frame's margins come on top of the text width.
    let width = width.min(ui.available_width() - 8.0).max(24.0);
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

/// Width of the label column in property lists (Inspector, sheet fields).
pub const LABEL_W: f32 = 108.0;

/// A property label: right-aligned in a fixed column, dim, cut with "…" when too long
/// (the whole text as a tooltip), as in CADCraft's Properties.
pub fn prop_label(ui: &mut Ui, text: impl Into<String>) {
    let text = text.into();
    let c = crate::theme::of(ui);
    let h = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(egui::vec2(LABEL_W, h), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.set_min_size(egui::vec2(LABEL_W, h));
        ui.add_space(2.0);
        let r = ui.add(egui::Label::new(egui::RichText::new(&text).color(c.text2)).truncate());
        if r.hovered() && r.rect.width() >= LABEL_W - 4.0 {
            r.on_hover_text(&text);
        }
    });
}

/// A section header in a panel: a hairline, then the title.
pub fn section(ui: &mut Ui, title: impl Into<String>) {
    let c = crate::theme::of(ui);
    ui.add_space(4.0);
    rule(ui);
    ui.add(egui::Label::new(egui::RichText::new(title.into()).font(crate::theme::bold(12.0)).color(c.text)));
    ui.add_space(3.0);
}

/// A 1 pt line across the panel.
pub fn rule(ui: &mut Ui) {
    let c = crate::theme::of(ui);
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 7.0), egui::Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y.round() + 0.5, egui::Stroke::new(1.0, c.separator));
}

/// A drop-down `width` wide, with a chevron.
pub fn combo_w(width: f32, id: impl std::hash::Hash + std::fmt::Debug) -> egui::ComboBox<'static> {
    egui::ComboBox::from_id_salt(id).width(width.max(60.0)).icon(|ui, rect, visuals, _open| {
        let r = egui::Rect::from_center_size(rect.center(), egui::vec2(12.0, 12.0));
        crate::icons::paint(ui, r, "chevron-down", 12.0, visuals.fg_stroke.color);
    })
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
                    p.circle_filled(c, 4.5, crate::theme::of(ui).accent);
                } else {
                    p.circle_stroke(c, 4.0, st);
                }
            }
        }
    }
    resp
}

/// Shown/hidden as an eye, for layer and tag lists (a check box to accessibility).
pub fn eye_toggle(ui: &mut Ui, on: bool, accessible: &str) -> Response {
    let c = crate::theme::of(ui);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(20.0, 18.0), egui::Sense::click());
    crate::tutorial::mark(ui.ctx(), accessible, rect);
    resp.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, on, accessible));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, c.hover.gamma_multiply(0.5));
    }
    let colour = if on { c.text } else { c.text3 };
    crate::icons::paint(ui, rect, if on { "eye" } else { "eye-off" }, 14.0, colour);
    resp
}

/// A segmented control (Plan | 3D | Split | Sheets): `(label, accessible name, chosen)` per
/// segment; returns the segment clicked.
pub fn segmented(ui: &mut Ui, c: &crate::theme::Tokens, items: &[(String, String, bool)]) -> Option<usize> {
    let font = crate::theme::body();
    let pad = 12.0;
    let galleys: Vec<_> = items
        .iter()
        .map(|(label, _, on)| {
            ui.painter().layout_no_wrap(label.clone(), font.clone(), if *on { c.text } else { c.text2 })
        })
        .collect();
    let h = 22.0;
    let widths: Vec<f32> = galleys.iter().map(|g| (g.size().x + 2.0 * pad).max(44.0)).collect();
    let total = widths.iter().sum::<f32>() + 2.0;
    let (rect, base) = ui.allocate_exact_size(egui::vec2(total, h), egui::Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, 3.0, c.panel_dark);
    p.rect_stroke(rect, 3.0, egui::Stroke::new(1.0, c.separator), egui::StrokeKind::Inside);
    let mut x = rect.left() + 1.0;
    let mut clicked = None;
    for (i, ((_, a11y, on), g)) in items.iter().zip(galleys).enumerate() {
        let seg = egui::Rect::from_min_size(egui::pos2(x, rect.top() + 1.0), egui::vec2(widths[i], h - 2.0));
        let r = ui.interact(seg, base.id.with(("segment", i)), egui::Sense::click());
        name(&r, Role::Button, a11y.as_str());
        if *on {
            p.rect_filled(seg, 2.0, c.tab_active);
        } else if r.hovered() {
            p.rect_filled(seg, 2.0, c.hover.gamma_multiply(0.5));
        }
        if i > 0 && !on && !items[i - 1].2 {
            p.vline(seg.left(), seg.y_range().shrink(4.0), egui::Stroke::new(1.0, c.separator));
        }
        p.galley(seg.center() - g.size() / 2.0, g, c.text);
        if r.clicked() {
            clicked = Some(i);
        }
        x += widths[i];
    }
    clicked
}
