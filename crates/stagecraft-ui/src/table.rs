//! The spreadsheet panel: objects of one type (and tag) as rows, cells edited through
//! commands. A cell range is selected with click and Shift-click; typing edits every
//! selected cell; "Fill series" counts up from the first cell; ⌘V pastes TSV from
//! Excel or Numbers. Selecting cells selects their objects in the plan.

use egui::{Align2, Event, Key, Modifiers, RichText, Sense, Ui, WidgetType, vec2};
use serde_json::{Value, json};
use stagecraft_core::model::{format_address, parse_address};
use stagecraft_core::{Document, Id, Object, ObjectKind};

use crate::app::Studio;
use crate::i18n::{t, tf};
use crate::panels::{fmt_mm, parse_mm};
use crate::widgets;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Col {
    Name,
    Type,
    Fid,
    Cid,
    Address,
    Mode,
    Profile,
    Length,
    Height,
    Thickness,
    Layer,
    Tags,
    /// Batten number.
    Number,
    /// Batten distance from the setting line.
    PosY,
    /// Batten trim height.
    Trim,
    /// What hangs on it, kg (read only).
    Load,
    /// Batten payload limit, kg.
    MaxLoad,
    /// Fixture weight, kg.
    Weight,
}

impl Col {
    pub fn key(self) -> &'static str {
        match self {
            Col::Name => "col.name",
            Col::Type => "col.type",
            Col::Fid => "col.fid",
            Col::Cid => "col.cid",
            Col::Address => "col.address",
            Col::Mode => "col.mode",
            Col::Profile => "col.profile",
            Col::Length => "col.length",
            Col::Height => "col.height",
            Col::Thickness => "col.thickness",
            Col::Layer => "col.layer",
            Col::Tags => "col.tags",
            Col::Number => "col.number",
            Col::PosY => "col.position_y",
            Col::Trim => "col.trim",
            Col::Load => "col.load",
            Col::MaxLoad => "col.max_load",
            Col::Weight => "col.weight",
        }
    }

    fn width(self) -> f32 {
        match self {
            Col::Name => 170.0,
            Col::Tags => 170.0,
            Col::Layer | Col::Mode => 110.0,
            Col::Number => 50.0,
            _ => 80.0,
        }
    }
}

/// Columns for a type filter (`None` = all types).
pub fn columns(kind: Option<&str>) -> Vec<Col> {
    use Col::*;
    match kind {
        Some("fixture") => vec![Name, Fid, Cid, Address, Mode, Weight, Layer, Tags],
        Some("truss") => vec![Name, Profile, Length, Load, Layer, Tags],
        Some("batten") => vec![Number, Name, PosY, Trim, Length, Load, MaxLoad, Tags],
        Some("wall") => vec![Name, Height, Thickness, Layer, Tags],
        Some("floor") => vec![Name, Thickness, Layer, Tags],
        _ => vec![Name, Type, Layer, Tags],
    }
}

pub struct TableState {
    /// Object type shown; `None` = all.
    pub kind: Option<String>,
    pub tag: Option<Id>,
    /// Range corners: (object, column index).
    pub anchor: Option<(Id, usize)>,
    pub cursor: Option<(Id, usize)>,
    /// Text being typed into the cursor cell.
    pub editing: Option<String>,
    /// The table takes keys (after a click into it).
    pub focused: bool,
    focus_editor: bool,
}

impl Default for TableState {
    fn default() -> Self {
        Self {
            kind: Some("fixture".into()),
            tag: None,
            anchor: None,
            cursor: None,
            editing: None,
            focused: false,
            focus_editor: false,
        }
    }
}

/// Objects in plan order: left to right, then downstage to upstage. Stable under
/// edits, and the order fixtures are numbered in.
pub fn plan_order(doc: &Document, ids: &[Id]) -> Vec<Id> {
    let mut v: Vec<(f64, f64, Id)> = ids
        .iter()
        .filter_map(|id| {
            let p = stagecraft_core::geom::world_placement(doc, *id)?.position;
            Some((p[0], p[1], *id))
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
    v.into_iter().map(|x| x.2).collect()
}

/// Rows: fixtures with a FID by FID, then everything else in plan order.
pub fn rows(doc: &Document, st: &TableState) -> Vec<Id> {
    let ids: Vec<Id> = doc
        .objects
        .values()
        .filter(|o| st.kind.as_deref().is_none_or(|k| o.kind.type_name() == k))
        .filter(|o| st.tag.is_none_or(|t| o.tags.contains(&t)))
        .map(|o| o.id)
        .collect();
    let fid = |id: &Id| match doc.objects[id].kind {
        ObjectKind::Fixture { fid: Some(f), .. } => Some(f),
        _ => None,
    };
    let (mut numbered, rest): (Vec<Id>, Vec<Id>) = ids.into_iter().partition(|id| fid(id).is_some());
    numbered.sort_by_key(fid);
    // Battens read by number, like the house's fly bar list.
    let batten = |id: &Id| match doc.objects[id].kind {
        ObjectKind::Batten { number, .. } => Some(number),
        _ => None,
    };
    let (mut battens, rest): (Vec<Id>, Vec<Id>) = rest.into_iter().partition(|id| batten(id).is_some());
    battens.sort_by_key(batten);
    numbered.extend(battens);
    numbered.extend(plan_order(doc, &rest));
    numbered
}

pub fn cell_text(doc: &Document, o: &Object, col: Col) -> String {
    match (col, &o.kind) {
        (Col::Name, _) => o.name.clone(),
        (Col::Type, k) => t(&format!("type.{}", k.type_name())).to_owned(),
        (Col::Fid, ObjectKind::Fixture { fid, .. }) => fid.map(|f| f.to_string()).unwrap_or_default(),
        (Col::Cid, ObjectKind::Fixture { cid, .. }) => cid.clone().unwrap_or_default(),
        (Col::Address, ObjectKind::Fixture { address, .. }) => address.map(format_address).unwrap_or_default(),
        (Col::Mode, ObjectKind::Fixture { mode, .. }) => mode.clone().unwrap_or_default(),
        (Col::Profile, ObjectKind::Truss { profile, .. }) => profile.clone(),
        (Col::Length, ObjectKind::Truss { length, .. } | ObjectKind::Batten { length, .. }) => fmt_mm(*length),
        (Col::Number, ObjectKind::Batten { number, .. }) => number.to_string(),
        (Col::PosY, ObjectKind::Batten { .. }) => fmt_mm(o.placement.position[1]),
        (Col::Trim, ObjectKind::Batten { .. }) => fmt_mm(o.placement.position[2]),
        (Col::MaxLoad, ObjectKind::Batten { max_load, .. }) => fmt_mm(*max_load),
        (Col::Weight, ObjectKind::Fixture { weight_kg, .. }) => weight_kg.map(fmt_mm).unwrap_or_default(),
        (Col::Load, _) => {
            let l = stagecraft_core::load::load_of(doc, o.id);
            let over = matches!(o.kind, ObjectKind::Batten { max_load, .. } if l.attached_kg > max_load);
            let unknown = if l.unknown > 0 { "+?" } else { "" };
            format!("{:.1}{unknown}{}", l.attached_kg, if over { " !" } else { "" })
        }
        (Col::Height, ObjectKind::Wall { height, .. }) => fmt_mm(*height),
        (Col::Thickness, ObjectKind::Wall { thickness, .. } | ObjectKind::Floor { thickness, .. }) => {
            fmt_mm(*thickness)
        }
        (Col::Layer, _) => doc.layers.get(&o.layer).map(|l| l.name.clone()).unwrap_or_default(),
        (Col::Tags, _) => {
            let mut names: Vec<String> =
                o.tags.iter().filter_map(|t| doc.tags.get(t).map(|t| t.name.clone())).collect();
            names.sort();
            names.join(", ")
        }
        _ => String::new(),
    }
}

fn step(command: &str, args: Value) -> Value {
    json!({ "command": command, "args": args })
}

/// The commands that set one cell to `text`.
pub fn edit_steps(doc: &Document, o: &Object, col: Col, text: &str) -> Result<Vec<Value>, String> {
    let id = o.id;
    let text = text.trim();
    let mm = || parse_mm(text).ok_or_else(|| tf("status.bad_number", &[("text", &text)]));
    let s = match col {
        Col::Name => step("object.rename", json!({ "id": id, "name": text })),
        Col::Type => return Err(t("table.read_only").to_owned()),
        Col::Fid => {
            let f: u32 = if text.is_empty() {
                0
            } else {
                text.parse().map_err(|_| tf("status.bad_number", &[("text", &text)]))?
            };
            step("fixture.update", json!({ "id": id, "fid": f }))
        }
        Col::Cid => step("fixture.update", json!({ "id": id, "cid": text })),
        Col::Address => {
            if !text.is_empty() && parse_address(text).is_none() {
                return Err(tf("table.bad_address", &[("text", &text)]));
            }
            step("fixture.update", json!({ "id": id, "address": text }))
        }
        Col::Mode => step("fixture.update", json!({ "id": id, "mode": text })),
        Col::Profile => step("truss.update", json!({ "id": id, "profile": text })),
        Col::Length if o.kind.type_name() == "batten" => step("batten.update", json!({ "id": id, "length": mm()? })),
        Col::Length => step("truss.update", json!({ "id": id, "length": mm()? })),
        Col::Number => {
            let n: u32 = text.parse().map_err(|_| tf("status.bad_number", &[("text", &text)]))?;
            step("batten.update", json!({ "id": id, "number": n }))
        }
        Col::PosY => step("batten.update", json!({ "id": id, "y": mm()? })),
        Col::Trim => step("batten.update", json!({ "id": id, "trim": mm()? })),
        Col::MaxLoad => step("batten.update", json!({ "id": id, "max_load": mm()? })),
        Col::Load => return Err(t("table.read_only").to_owned()),
        Col::Weight => {
            let w = if text.is_empty() { 0.0 } else { mm()? };
            step("fixture.update", json!({ "id": id, "weight_kg": w }))
        }
        Col::Height => step("wall.update", json!({ "id": id, "height": mm()? })),
        Col::Thickness => {
            let cmd = if o.kind.type_name() == "wall" { "wall.update" } else { "floor.update" };
            step(cmd, json!({ "id": id, "thickness": mm()? }))
        }
        Col::Layer => step("object.set_layer", json!({ "ids": [id], "layer": text })),
        Col::Tags => {
            let want: Vec<String> = text.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect();
            let have: Vec<String> = o.tags.iter().filter_map(|t| doc.tags.get(t).map(|t| t.name.clone())).collect();
            let add: Vec<&String> = want.iter().filter(|w| !have.contains(w)).collect();
            let remove: Vec<&String> = have.iter().filter(|h| !want.contains(h)).collect();
            let mut v = Vec::new();
            if !remove.is_empty() {
                v.push(step("object.untag", json!({ "ids": [id], "tags": remove })));
            }
            if !add.is_empty() {
                v.push(step("object.tag", json!({ "ids": [id], "tags": add })));
            }
            return Ok(v);
        }
    };
    // Cells of other types in a mixed table are left alone.
    let fits = match col {
        Col::Fid | Col::Cid | Col::Address | Col::Mode => matches!(o.kind, ObjectKind::Fixture { .. }),
        Col::Profile => matches!(o.kind, ObjectKind::Truss { .. }),
        Col::Length => matches!(o.kind, ObjectKind::Truss { .. } | ObjectKind::Batten { .. }),
        Col::Number | Col::PosY | Col::Trim | Col::MaxLoad => matches!(o.kind, ObjectKind::Batten { .. }),
        Col::Weight => matches!(o.kind, ObjectKind::Fixture { .. }),
        Col::Height => matches!(o.kind, ObjectKind::Wall { .. }),
        Col::Thickness => matches!(o.kind, ObjectKind::Wall { .. } | ObjectKind::Floor { .. }),
        _ => true,
    };
    Ok(if fits { vec![s] } else { Vec::new() })
}

/// Text split into a number and the text around it: "Spot 12" → ("Spot ", 12, "").
fn split_number(s: &str) -> Option<(String, i64, String)> {
    let chars: Vec<char> = s.chars().collect();
    let end = chars.iter().rposition(|c| c.is_ascii_digit())? + 1;
    let start = chars[..end].iter().rposition(|c| !c.is_ascii_digit()).map_or(0, |i| i + 1);
    let n: i64 = chars[start..end].iter().collect::<String>().parse().ok()?;
    Some((chars[..start].iter().collect(), n, chars[end..].iter().collect()))
}

/// Values for `n` cells counting up from `first` (step from `second` when given).
/// Addresses step in DMX channels and roll over into the next universe.
pub fn fill_series(col: Col, first: &str, second: Option<&str>, n: usize) -> Vec<String> {
    if col == Col::Address
        && let Some(a) = parse_address(first)
    {
        let step = second.and_then(parse_address).map_or(1, |b| b as i64 - a as i64);
        return (0..n)
            .map(|i| {
                let v = a as i64 + i as i64 * step;
                if v >= 1 { format_address(v as u32) } else { String::new() }
            })
            .collect();
    }
    match split_number(first) {
        Some((pre, a, post)) => {
            let step = second
                .and_then(split_number)
                .filter(|(p2, _, q2)| *p2 == pre && *q2 == post)
                .map_or(1, |(_, b, _)| b - a);
            (0..n).map(|i| format!("{pre}{}{post}", a + i as i64 * step)).collect()
        }
        None => vec![first.to_owned(); n],
    }
}

/// Tab-separated rows; a trailing newline does not make an empty row.
pub fn parse_tsv(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_suffix('\n').unwrap_or(text);
    let text = text.strip_suffix('\r').unwrap_or(text);
    text.split('\n').map(|line| line.trim_end_matches('\r').split('\t').map(str::to_owned).collect()).collect()
}

/// Run steps as one undo step.
fn apply(s: &mut Studio, steps: Vec<Value>) {
    match steps.len() {
        0 => {}
        1 => {
            let st = &steps[0];
            let _ = s.run(st["command"].as_str().unwrap_or_default(), st["args"].clone());
        }
        _ => {
            let _ = s.run("batch", json!({ "commands": steps }));
        }
    }
}

/// The selected range as row and column index bounds.
fn range(rows: &[Id], st: &TableState) -> Option<(usize, usize, usize, usize)> {
    let (ai, ac) = st.anchor.or(st.cursor)?;
    let (ci, cc) = st.cursor?;
    let ar = rows.iter().position(|r| *r == ai)?;
    let cr = rows.iter().position(|r| *r == ci)?;
    Some((ar.min(cr), ar.max(cr), ac.min(cc), ac.max(cc)))
}

/// Set every selected cell to `text`.
pub fn edit_selected(s: &mut Studio, text: &str) {
    let doc = s.engine.doc().clone();
    let rows = rows(&doc, &s.ui.table);
    let cols = columns(s.ui.table.kind.as_deref());
    let Some((r0, r1, c0, c1)) = range(&rows, &s.ui.table) else { return };
    let mut steps = Vec::new();
    for r in &rows[r0..=r1] {
        for col in &cols[c0..=c1] {
            match edit_steps(&doc, &doc.objects[r], *col, text) {
                Ok(v) => steps.extend(v),
                Err(e) => {
                    s.ui.error(e);
                    return;
                }
            }
        }
    }
    // One FID for several fixtures would clash: number them instead.
    if c0 == c1
        && cols[c0] == Col::Fid
        && r1 > r0
        && let Ok(n) = text.trim().parse::<u32>()
    {
        let _ = s.run("fixture.number", json!({ "ids": &rows[r0..=r1], "start": n }));
        return;
    }
    apply(s, steps);
}

/// Count up down each selected column from its first cell.
pub fn fill_selected(s: &mut Studio) {
    let doc = s.engine.doc().clone();
    let rows = rows(&doc, &s.ui.table);
    let cols = columns(s.ui.table.kind.as_deref());
    let Some((r0, r1, c0, c1)) = range(&rows, &s.ui.table) else { return };
    if r1 == r0 {
        s.ui.error(t("table.fill_needs_rows"));
        return;
    }
    let ids = &rows[r0..=r1];
    let mut steps = Vec::new();
    for col in &cols[c0..=c1] {
        let first = cell_text(&doc, &doc.objects[&ids[0]], *col);
        if first.trim().is_empty() {
            continue;
        }
        let second = cell_text(&doc, &doc.objects[&ids[1]], *col);
        // Two filled cells set the step; otherwise count by one.
        let second = (!second.trim().is_empty() && ids.len() > 2).then_some(second);
        let values = fill_series(*col, &first, second.as_deref(), ids.len());
        if *col == Col::Fid {
            let start: u32 = values[0].parse().unwrap_or(1);
            let step = values.get(1).and_then(|v| v.parse::<u32>().ok()).map_or(1, |b| b.saturating_sub(start).max(1));
            steps.push(json!({ "command": "fixture.number", "args": { "ids": ids, "start": start, "step": step } }));
            continue;
        }
        for (id, v) in ids.iter().zip(&values) {
            match edit_steps(&doc, &doc.objects[id], *col, v) {
                Ok(st) => steps.extend(st),
                Err(e) => {
                    s.ui.error(e);
                    return;
                }
            }
        }
    }
    apply(s, steps);
    s.ui.info(tf("table.filled", &[("n", &ids.len())]));
}

/// Paste rows of tab-separated text from the cursor cell down and right.
pub fn paste(s: &mut Studio, text: &str) {
    let doc = s.engine.doc().clone();
    let rows = rows(&doc, &s.ui.table);
    let cols = columns(s.ui.table.kind.as_deref());
    let Some((r0, _, c0, _)) = range(&rows, &s.ui.table) else { return };
    let data = parse_tsv(text);
    let mut steps = Vec::new();
    let mut skipped = 0;
    for (i, line) in data.iter().enumerate() {
        let Some(id) = rows.get(r0 + i) else {
            skipped += 1;
            continue;
        };
        for (j, cell) in line.iter().enumerate() {
            let Some(col) = cols.get(c0 + j) else { continue };
            if *col == Col::Type {
                continue;
            }
            match edit_steps(&doc, &doc.objects[id], *col, cell) {
                Ok(v) => steps.extend(v),
                Err(e) => {
                    s.ui.error(e);
                    return;
                }
            }
        }
    }
    apply(s, steps);
    if skipped > 0 {
        s.ui.error(tf("table.paste_skipped", &[("n", &skipped)]));
    }
}

fn select_rows_in_plan(s: &mut Studio, rows: &[Id]) {
    let Some((r0, r1, _, _)) = range(rows, &s.ui.table) else { return };
    let ids = &rows[r0..=r1];
    if s.engine.selection() != ids {
        let _ = s.run("selection.set", json!({ "ids": ids }));
    }
}

const ROW_H: f32 = 22.0;

pub fn show(ui: &mut Ui, s: &mut Studio) {
    let doc = s.engine.doc().clone();
    // Filters.
    ui.horizontal(|ui| {
        ui.heading(t("panel.table"));
        ui.add_space(8.0);
        let kinds = [
            (None, "table.all"),
            (Some("fixture"), "type.fixture.plural"),
            (Some("truss"), "type.truss.plural"),
            (Some("batten"), "table.fly_bars"),
            (Some("wall"), "type.wall.plural"),
            (Some("floor"), "type.floor.plural"),
        ];
        for (k, key) in kinds {
            let on = s.ui.table.kind.as_deref() == k;
            let r = ui.selectable_label(on, t(key));
            widgets::name(&r, WidgetType::Button, format!("Show {}", t(key)));
            if r.clicked() && !on {
                s.ui.table = TableState { kind: k.map(String::from), tag: s.ui.table.tag, ..Default::default() };
            }
        }
        ui.separator();
        let tag_name =
            s.ui.table
                .tag
                .and_then(|t| doc.tags.get(&t))
                .map_or_else(|| t("table.any_tag").to_owned(), |t| t.name.clone());
        egui::ComboBox::from_id_salt("table_tag").selected_text(tag_name).show_ui(ui, |ui| {
            if ui.selectable_label(s.ui.table.tag.is_none(), t("table.any_tag")).clicked() {
                s.ui.table.tag = None;
            }
            let mut tags: Vec<_> = doc.tags.values().collect();
            tags.sort_by(|a, b| a.name.cmp(&b.name));
            for tg in tags {
                if ui.selectable_label(s.ui.table.tag == Some(tg.id), &tg.name).clicked() {
                    s.ui.table.tag = Some(tg.id);
                }
            }
        });
        ui.separator();
        if widgets::button(ui, t("table.fill"), "Fill series").on_hover_text(t("table.fill_tip")).clicked() {
            fill_selected(s);
        }
    });

    let rows = rows(&doc, &s.ui.table);
    let cols = columns(s.ui.table.kind.as_deref());
    let sel_range = range(&rows, &s.ui.table);
    keys(ui, s, &rows, &cols);

    // Header.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(36.0);
        for c in &cols {
            let (r, p) = ui.allocate_painter(vec2(c.width(), ROW_H), Sense::hover());
            p.text(
                r.rect.left_center() + vec2(6.0, 0.0),
                Align2::LEFT_CENTER,
                t(c.key()),
                egui::TextStyle::Small.resolve(ui.style()),
                ui.visuals().weak_text_color(),
            );
        }
    });
    if rows.is_empty() {
        ui.label(RichText::new(t("table.empty")).weak());
        return;
    }
    let selected_objects: Vec<Id> = s.engine.selection().to_vec();
    let mut clicked: Option<(Id, usize, Modifiers)> = None;
    let mut double: Option<(Id, usize)> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, rows.len(), |ui, visible| {
        for ri in visible {
            let id = rows[ri];
            let o = &doc.objects[&id];
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let (r, p) = ui.allocate_painter(vec2(36.0, ROW_H), Sense::hover());
                p.text(
                    r.rect.right_center() - vec2(6.0, 0.0),
                    Align2::RIGHT_CENTER,
                    (ri + 1).to_string(),
                    egui::TextStyle::Small.resolve(ui.style()),
                    ui.visuals().weak_text_color(),
                );
                for (ci, c) in cols.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(c.width(), ROW_H), Sense::click());
                    let label = format!("{} {}", t(c.key()), ri + 1);
                    let text = cell_text(&doc, o, *c);
                    resp.widget_info(|| egui::WidgetInfo::labeled(WidgetType::Other, true, label.clone()));
                    let in_range =
                        sel_range.is_some_and(|(r0, r1, c0, c1)| (r0..=r1).contains(&ri) && (c0..=c1).contains(&ci));
                    let is_cursor = s.ui.table.cursor == Some((id, ci));
                    let v = ui.visuals();
                    let bg = if in_range {
                        v.selection.bg_fill
                    } else if selected_objects.contains(&id) {
                        v.selection.bg_fill.gamma_multiply(0.35)
                    } else if ri % 2 == 1 {
                        v.faint_bg_color
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    let painter = ui.painter();
                    painter.rect_filled(rect.shrink(0.5), 0.0, bg);
                    if is_cursor && s.ui.table.focused {
                        painter.rect_stroke(
                            rect.shrink(1.0),
                            0.0,
                            egui::Stroke::new(1.5, crate::theme::ACCENT),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if is_cursor && s.ui.table.editing.is_some() {
                        let mut buf = s.ui.table.editing.clone().unwrap_or_default();
                        let ed_id = egui::Id::new("table_editor");
                        let r = ui.put(
                            rect.shrink(1.0),
                            egui::TextEdit::singleline(&mut buf).id(ed_id).margin(vec2(4.0, 2.0)),
                        );
                        widgets::name(&r, WidgetType::TextEdit, "Cell editor");
                        if s.ui.table.focus_editor {
                            r.request_focus();
                            // Put the caret at the end of what was typed.
                            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), ed_id) {
                                let end = egui::text::CCursor::new(buf.chars().count());
                                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                                state.store(ui.ctx(), ed_id);
                            }
                            s.ui.table.focus_editor = false;
                        }
                        let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
                        if r.lost_focus() || enter || escape {
                            s.ui.table.editing = None;
                            if !escape {
                                edit_selected(s, &buf);
                            }
                        } else {
                            s.ui.table.editing = Some(buf);
                        }
                    } else {
                        ui.painter().with_clip_rect(rect.shrink(2.0)).text(
                            rect.left_center() + vec2(6.0, 0.0),
                            Align2::LEFT_CENTER,
                            &text,
                            egui::TextStyle::Body.resolve(ui.style()),
                            ui.visuals().text_color(),
                        );
                    }
                    if resp.double_clicked() {
                        double = Some((id, ci));
                    } else if resp.clicked() {
                        clicked = Some((id, ci, ui.input(|i| i.modifiers)));
                    }
                }
            });
        }
    });
    if let Some((id, ci, m)) = clicked {
        let st = &mut s.ui.table;
        if st.editing.is_some() {
            st.editing = None;
        }
        st.focused = true;
        if m.shift && st.anchor.is_some() {
            st.cursor = Some((id, ci));
        } else {
            st.anchor = Some((id, ci));
            st.cursor = Some((id, ci));
        }
        select_rows_in_plan(s, &rows);
    }
    if let Some((id, ci)) = double {
        let st = &mut s.ui.table;
        st.focused = true;
        st.anchor = Some((id, ci));
        st.cursor = Some((id, ci));
        st.editing = Some(cell_text(&doc, &doc.objects[&id], cols[ci]));
        st.focus_editor = true;
        select_rows_in_plan(s, &rows);
    }
}

/// Keys while the table has focus and no cell is being edited.
fn keys(ui: &mut Ui, s: &mut Studio, rows: &[Id], cols: &[Col]) {
    let st = &s.ui.table;
    if !st.focused || st.editing.is_some() || s.ui.palette.open || ui.ctx().memory(|m| m.focused().is_some()) {
        return;
    }
    let Some((cid, cc)) = st.cursor else { return };
    let Some(cr) = rows.iter().position(|r| *r == cid) else { return };
    let (pasted, typed) = ui.input_mut(|i| {
        let mut pasted = None;
        let mut typed = String::new();
        i.events.retain(|e| match e {
            Event::Paste(p) => {
                pasted = Some(p.clone());
                false
            }
            Event::Text(t) => {
                typed.push_str(t);
                false
            }
            _ => true,
        });
        (pasted, typed)
    });
    if let Some(p) = pasted {
        paste(s, &p);
        return;
    }
    if !typed.is_empty() {
        let st = &mut s.ui.table;
        st.editing = Some(typed);
        st.focus_editor = true;
        return;
    }
    let mut mv = |dr: i64, dc: i64, extend: bool| {
        let r = (cr as i64 + dr).clamp(0, rows.len() as i64 - 1) as usize;
        let c = (cc as i64 + dc).clamp(0, cols.len() as i64 - 1) as usize;
        let st = &mut s.ui.table;
        st.cursor = Some((rows[r], c));
        if !extend {
            st.anchor = st.cursor;
        }
    };
    let (shift, keys) = ui.input(|i| {
        (i.modifiers.shift, [Key::ArrowUp, Key::ArrowDown, Key::ArrowLeft, Key::ArrowRight].map(|k| i.key_pressed(k)))
    });
    let moves = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    let mut moved = false;
    for (pressed, (dr, dc)) in keys.into_iter().zip(moves) {
        if pressed {
            mv(dr, dc, shift);
            moved = true;
        }
    }
    if moved {
        select_rows_in_plan(s, rows);
    }
    if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
        let doc = s.engine.doc();
        let text = cell_text(doc, &doc.objects[&cid], cols[cc]);
        s.ui.table.editing = Some(text);
        s.ui.table.focus_editor = true;
    }
    if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::D)) {
        fill_selected(s);
    }
    if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        s.ui.table.focused = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_count_up() {
        assert_eq!(fill_series(Col::Fid, "101", None, 3), ["101", "102", "103"]);
        assert_eq!(fill_series(Col::Fid, "101", Some("111"), 3), ["101", "111", "121"]);
        assert_eq!(fill_series(Col::Name, "Spot 9", None, 2), ["Spot 9", "Spot 10"]);
        assert_eq!(fill_series(Col::Name, "Wash", None, 2), ["Wash", "Wash"]);
        assert_eq!(fill_series(Col::Address, "1.001", Some("1.017"), 3), ["1.001", "1.017", "1.033"]);
        assert_eq!(fill_series(Col::Address, "1.510", Some("1.511"), 4)[3], "2.001");
    }

    #[test]
    fn tsv_from_spreadsheets() {
        assert_eq!(parse_tsv("a\tb\r\nc\td\r\n"), vec![vec!["a", "b"], vec!["c", "d"]]);
        assert_eq!(parse_tsv("101"), vec![vec!["101"]]);
        assert_eq!(parse_tsv("x\t\ty\n"), vec![vec!["x", "", "y"]]);
    }
}
