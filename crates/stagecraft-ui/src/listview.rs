//! Compact lists with columns for the browser panels (Library, GDTF Share): a header row,
//! optional group rows, one line per entry with hover and selection, text cut with "…".
//! The look follows the table under the canvas (`table.rs`): `panel_dark` header strip,
//! dim header text, 20 pt rows.

use egui::{Align2, Color32, FontId, Rect, Response, Sense, Ui, pos2, vec2};

use crate::theme;

pub const ROW_H: f32 = 22.0;
const HEADER_H: f32 = 20.0;
/// Text inset inside a cell.
const PAD: f32 = 6.0;

/// A column: its title, its width (`0` takes what the others leave) and whether its text
/// is right-aligned (numbers).
#[derive(Clone, Copy)]
pub struct Col<'a> {
    pub title: &'a str,
    pub width: f32,
    pub right: bool,
}

impl<'a> Col<'a> {
    pub const fn fill(title: &'a str) -> Self {
        Col { title, width: 0.0, right: false }
    }
    pub const fn fixed(title: &'a str, width: f32) -> Self {
        Col { title, width, right: false }
    }
    pub const fn number(title: &'a str, width: f32) -> Self {
        Col { title, width, right: true }
    }
}

/// The columns' x ranges over `rect`: fixed columns keep their width, fill columns share
/// the rest (at least 40 pt each).
pub fn layout(rect: Rect, cols: &[Col]) -> Vec<Rect> {
    let fixed: f32 = cols.iter().map(|c| c.width).sum();
    let fills = cols.iter().filter(|c| c.width == 0.0).count().max(1) as f32;
    let fill = ((rect.width() - fixed) / fills).max(40.0);
    let mut x = rect.left();
    cols.iter()
        .map(|c| {
            let w = if c.width == 0.0 { fill } else { c.width };
            let r = Rect::from_min_max(pos2(x, rect.top()), pos2(x + w, rect.bottom()));
            x += w;
            r
        })
        .collect()
}

/// The header row.
pub fn header(ui: &mut Ui, cols: &[Col]) {
    let c = theme::of(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER_H), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, c.panel_dark);
    let font = FontId::proportional(theme::SMALL);
    for (col, r) in cols.iter().zip(layout(rect, cols)) {
        text(p, r, col.title, &font, c.text2, col.right);
    }
}

/// A group row: a dim title over the rows that follow (a manufacturer).
pub fn group(ui: &mut Ui, title: &str) {
    let c = theme::of(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H - 2.0), Sense::hover());
    let font = theme::bold(theme::SMALL);
    text(ui.painter(), rect, title, &font, c.text2, false);
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0, c.separator));
}

/// One entry: the full-width row, its hover or selection fill, the cell rects and the text
/// colour to paint them with. The response is named `name` for tests and screen readers.
pub fn row(ui: &mut Ui, cols: &[Col], selected: bool, name: &str) -> (Response, Vec<Rect>, Color32) {
    let c = theme::of(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, name));
    if crate::tutorial::mark(ui.ctx(), name, rect) {
        crate::tutorial::reveal(&resp, name);
    }
    let (fill, ink) = if selected {
        (c.select_bg, c.on_accent)
    } else if resp.hovered() {
        (c.hover, c.text)
    } else {
        (Color32::TRANSPARENT, c.text)
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 2.0, fill);
    }
    (resp, layout(rect, cols), ink)
}

/// Text in a cell, on one line, cut with "…" when it does not fit.
pub fn text(p: &egui::Painter, cell: Rect, s: &str, font: &FontId, colour: Color32, right: bool) {
    let mut job = egui::text::LayoutJob::single_section(s.to_owned(), egui::TextFormat::simple(font.clone(), colour));
    job.wrap = egui::text::TextWrapping {
        max_width: (cell.width() - 2.0 * PAD).max(4.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = p.layout_job(job);
    let y = cell.center().y - galley.size().y / 2.0;
    let x = if right { cell.right() - PAD - galley.size().x } else { cell.left() + PAD };
    p.galley(pos2(x, y), galley, colour);
}

/// Body text in a cell, in the row's ink.
pub fn cell(ui: &Ui, cell: Rect, s: &str, ink: Color32, right: bool) {
    text(ui.painter(), cell, s, &FontId::proportional(theme::BODY), ink, right);
}

/// Secondary text in a cell (dim unless the row is selected).
pub fn cell_dim(ui: &Ui, cell: Rect, s: &str, ink: Color32, selected: bool, right: bool) {
    let colour = if selected { ink } else { theme::of(ui).text2 };
    text(ui.painter(), cell, s, &FontId::proportional(theme::BODY), colour, right);
}

/// A label for an empty list, centred in a short row.
pub fn empty(ui: &mut Ui, s: &str) {
    let c = theme::of(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H * 1.5), Sense::hover());
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, s, FontId::proportional(theme::BODY), c.text3);
}

/// `YYYY-MM-DD` of Unix seconds (UTC).
pub fn date(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates_are_civil() {
        assert_eq!(super::date(0), "1970-01-01");
        assert_eq!(super::date(1_709_251_200), "2024-03-01");
        assert_eq!(super::date(951_782_400), "2000-02-29");
    }
}
