//! The spreadsheet panel: objects of one type (and tag) as rows, cells edited through
//! commands. A cell range is selected with click and Shift-click; typing edits every
//! selected cell; "Fill series" counts up from the first cell; ⌘V pastes TSV from
//! Excel or Numbers. Selecting cells selects their objects in the plan.

use egui::{Align2, Event, Key, Modifiers, RichText, Role, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use stagecraft_core::model::{format_address, parse_address};
use stagecraft_core::{Document, Id, Object, ObjectKind};

use crate::app::Studio;
use crate::i18n::{t, tf};
use crate::lengths::{fmt_number, parse_number};
use crate::widgets;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
    /// GDTF fixture type (library key).
    FixtureType,
    /// DMX slots of the mode, per break (read only).
    Footprint,
    /// Patch problems (read only).
    Conflict,
    /// Seats in a seat row.
    Seats,
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
            Col::FixtureType => "col.gdtf_type",
            Col::Footprint => "col.footprint",
            Col::Conflict => "col.conflict",
            Col::Seats => "col.seats",
        }
    }

    /// Columns that hold lengths (shown and typed in the project unit).
    pub fn is_length(self) -> bool {
        matches!(self, Col::Length | Col::Height | Col::Thickness | Col::PosY | Col::Trim)
    }

    /// The header text: lengths with the project unit, "Length (m)".
    pub fn title(self, doc: &Document) -> String {
        if self.is_length() {
            crate::lengths::label(&stagecraft_core::units::Units::of(&doc.meta), t(self.key()))
        } else {
            t(self.key()).to_owned()
        }
    }

    /// Columns that take the room left over when the panel is wider than the table.
    fn flexible(self) -> bool {
        matches!(self, Col::Name | Col::Tags | Col::FixtureType | Col::Conflict | Col::Mode | Col::Layer)
    }

    pub fn width(self) -> f32 {
        match self {
            Col::Name => 140.0,
            Col::Tags => 140.0,
            Col::FixtureType => 180.0,
            Col::Conflict => 130.0,
            Col::Address => 90.0,
            Col::Layer | Col::Mode => 110.0,
            Col::Footprint => 64.0,
            Col::Number => 44.0,
            _ => 70.0,
        }
    }
}

/// Columns for a type filter (`None` = all types).
pub fn columns(kind: Option<&str>) -> Vec<Col> {
    use Col::*;
    match kind {
        Some("fixture") => vec![Name, Fid, Cid, FixtureType, Mode, Footprint, Address, Conflict, Weight, Layer, Tags],
        Some("truss") => vec![Name, Profile, Length, Load, Layer, Tags],
        Some("batten") => vec![Number, Name, PosY, Trim, Length, Load, MaxLoad, Tags],
        Some("wall") => vec![Name, Height, Thickness, Layer, Tags],
        Some("floor") => vec![Name, Thickness, Layer, Tags],
        Some("seat_row") => vec![Name, Seats, Layer, Tags],
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
    /// Column widths the user dragged (pt); the others use their default.
    pub widths: std::collections::BTreeMap<Col, f32>,
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
            widths: Default::default(),
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
    let u = stagecraft_core::units::Units::of(&doc.meta);
    let len = |v: f64| u.field(v);
    match (col, &o.kind) {
        (Col::Name, _) => o.name.clone(),
        (Col::Type, k) => t(&format!("type.{}", k.type_name())).to_owned(),
        (Col::Fid, ObjectKind::Fixture { fid, .. }) => fid.map(|f| f.to_string()).unwrap_or_default(),
        (Col::Cid, ObjectKind::Fixture { cid, .. }) => cid.clone().unwrap_or_default(),
        (Col::Address, ObjectKind::Fixture { .. }) => stagecraft_core::patch::address_text(o),
        (Col::Mode, ObjectKind::Fixture { .. }) => stagecraft_sheet::labels::mode_name(doc, o),
        (Col::FixtureType, ObjectKind::Fixture { gdtf, .. }) => gdtf.clone().unwrap_or_default(),
        (Col::Footprint, ObjectKind::Fixture { .. }) => {
            stagecraft_core::patch::mode(doc, o).map(|m| m.footprint_text()).unwrap_or_default()
        }
        (Col::Conflict, ObjectKind::Fixture { .. }) => {
            stagecraft_core::patch::conflicts(doc).remove(&o.id).map(|p| problem_text(doc, &p)).unwrap_or_default()
        }
        (Col::Profile, ObjectKind::Truss { profile, .. }) => profile.clone(),
        (Col::Length, ObjectKind::Truss { length, .. } | ObjectKind::Batten { length, .. }) => len(*length),
        (Col::Number, ObjectKind::Batten { number, .. }) => number.to_string(),
        (Col::PosY, ObjectKind::Batten { .. }) => len(o.placement.position[1]),
        (Col::Trim, ObjectKind::Batten { .. }) => len(o.placement.position[2]),
        (Col::MaxLoad, ObjectKind::Batten { max_load, .. }) => fmt_number(*max_load),
        (Col::Weight, ObjectKind::Fixture { weight_kg, .. }) => weight_kg.map(fmt_number).unwrap_or_default(),
        (Col::Load, _) => {
            let l = stagecraft_core::load::load_of(doc, o.id);
            let over = matches!(o.kind, ObjectKind::Batten { max_load, .. } if l.attached_kg > max_load);
            let unknown = if l.unknown > 0 { "+?" } else { "" };
            format!("{:.1}{unknown}{}", l.attached_kg, if over { " !" } else { "" })
        }
        (Col::Height, ObjectKind::Wall { height, .. }) => len(*height),
        (Col::Seats, ObjectKind::SeatRow { count, .. }) => count.to_string(),
        (Col::Thickness, ObjectKind::Wall { thickness, .. } | ObjectKind::Floor { thickness, .. }) => len(*thickness),
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
    // Lengths in the project unit (a bare number from Excel is in it too), kg plain.
    let units = stagecraft_core::units::Units::of(&doc.meta);
    let mm = || units.parse(text).ok_or_else(|| tf("status.bad_number", &[("text", &text)]));
    let kg = || parse_number(text).ok_or_else(|| tf("status.bad_number", &[("text", &text)]));
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
            let (first, extra) = address_args(text)?;
            step("fixture.update", json!({ "id": id, "address": first, "extra_addresses": extra }))
        }
        // An empty mode keeps the current one (a pasted blank cell).
        Col::Mode if text.is_empty() => return Ok(Vec::new()),
        Col::Mode => step("fixture.update", json!({ "id": id, "mode": text })),
        Col::FixtureType => {
            // A key, or a display name / unique part of one.
            let key = resolve_type(doc, text)?;
            step("fixture.update", json!({ "id": id, "gdtf": key }))
        }
        Col::Footprint | Col::Conflict => return Err(t("table.read_only").to_owned()),
        Col::Profile => step("truss.update", json!({ "id": id, "profile": text })),
        Col::Length if o.kind.type_name() == "batten" => step("batten.update", json!({ "id": id, "length": mm()? })),
        Col::Length => step("truss.update", json!({ "id": id, "length": mm()? })),
        Col::Number => {
            let n: u32 = text.parse().map_err(|_| tf("status.bad_number", &[("text", &text)]))?;
            step("batten.update", json!({ "id": id, "number": n }))
        }
        Col::PosY => step("batten.update", json!({ "id": id, "y": mm()? })),
        Col::Trim => step("batten.update", json!({ "id": id, "trim": mm()? })),
        Col::MaxLoad => step("batten.update", json!({ "id": id, "max_load": kg()? })),
        Col::Load => return Err(t("table.read_only").to_owned()),
        Col::Weight => {
            let w = if text.is_empty() { 0.0 } else { kg()? };
            step("fixture.update", json!({ "id": id, "weight_kg": w }))
        }
        Col::Height => step("wall.update", json!({ "id": id, "height": mm()? })),
        Col::Seats => {
            let n: u32 = text.parse().map_err(|_| tf("status.bad_number", &[("text", &text)]))?;
            step("seating.update", json!({ "id": id, "count": n }))
        }
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
        Col::Fid | Col::Cid | Col::Address | Col::Mode | Col::FixtureType => {
            matches!(o.kind, ObjectKind::Fixture { .. })
        }
        Col::Profile => matches!(o.kind, ObjectKind::Truss { .. }),
        Col::Length => matches!(o.kind, ObjectKind::Truss { .. } | ObjectKind::Batten { .. }),
        Col::Number | Col::PosY | Col::Trim | Col::MaxLoad => matches!(o.kind, ObjectKind::Batten { .. }),
        Col::Weight => matches!(o.kind, ObjectKind::Fixture { .. }),
        Col::Height => matches!(o.kind, ObjectKind::Wall { .. }),
        Col::Seats => matches!(o.kind, ObjectKind::SeatRow { .. }),
        Col::Thickness => matches!(o.kind, ObjectKind::Wall { .. } | ObjectKind::Floor { .. }),
        _ => true,
    };
    Ok(if fits { vec![s] } else { Vec::new() })
}

/// `1.001`, `1.001 / 3.001` (one address per DMX break) or empty → the
/// `fixture.update` arguments `address` and `extra_addresses`.
pub fn address_args(text: &str) -> Result<(String, Vec<String>), String> {
    let parts: Vec<&str> = text.split('/').map(str::trim).collect();
    for p in &parts {
        if !p.is_empty() && *p != "–" && *p != "-" && parse_address(p).is_none() {
            return Err(tf("table.bad_address", &[("text", p)]));
        }
    }
    let norm = |p: &&str| if *p == "–" || *p == "-" { String::new() } else { (*p).to_owned() };
    let first = parts.first().map(norm).unwrap_or_default();
    Ok((first, parts.iter().skip(1).map(norm).collect()))
}

/// A library key from what was typed: the key itself, else a type whose display
/// name contains the text (case-insensitive) if only one does. Empty clears.
pub fn resolve_type(doc: &Document, text: &str) -> Result<String, String> {
    if text.is_empty() || doc.fixture_types.contains_key(text) {
        return Ok(text.to_owned());
    }
    let low = text.to_lowercase();
    let hits: Vec<&String> = doc
        .fixture_types
        .values()
        .filter(|t| t.display_name().to_lowercase().contains(&low) || t.key.to_lowercase().contains(&low))
        .map(|t| &t.key)
        .collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        _ => Err(tf("table.unknown_type", &[("text", &text)])),
    }
}

/// Patch problems in a few words: "overlaps 101, 102 · crosses universe".
pub fn problem_text(doc: &Document, problems: &[stagecraft_core::patch::Problem]) -> String {
    use stagecraft_core::patch::Problem;
    let mut with: Vec<String> = Vec::new();
    let mut crosses = false;
    for p in problems {
        match p {
            Problem::Overlap { with: w } => {
                let name = match doc.objects.get(w).map(|o| &o.kind) {
                    Some(ObjectKind::Fixture { fid: Some(f), .. }) => f.to_string(),
                    _ => doc.objects.get(w).map(|o| o.name.clone()).unwrap_or_default(),
                };
                if !with.contains(&name) {
                    with.push(name);
                }
            }
            Problem::CrossesUniverse { .. } => crosses = true,
        }
    }
    let mut parts = Vec::new();
    if !with.is_empty() {
        parts.push(tf("table.overlaps", &[("with", &with.join(", "))]));
    }
    if crosses {
        parts.push(t("table.crosses_universe").to_owned());
    }
    parts.join(" · ")
}

/// Choices offered while editing a cell: modes of the row's type, library types.
pub fn cell_options(doc: &Document, o: &Object, col: Col) -> Vec<(String, String)> {
    match col {
        Col::Mode => match stagecraft_core::patch::fixture_type(doc, o) {
            Some(ft) => {
                ft.modes.iter().map(|m| (m.name.clone(), format!("{} · {} ch", m.name, m.footprint_text()))).collect()
            }
            None => Vec::new(),
        },
        Col::FixtureType => doc.fixture_types.values().map(|t| (t.key.clone(), t.display_name())).collect(),
        _ => Vec::new(),
    }
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

/// A series of lengths from `first` (mm) by the step to `second`, else by one project
/// unit, as the cells show them.
pub fn fill_lengths(u: &stagecraft_core::units::Units, first: f64, second: Option<f64>, n: usize) -> Vec<String> {
    let step = second.map_or(u.unit.mm(), |b| b - first);
    (0..n).map(|i| u.field(first + i as f64 * step)).collect()
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
        let first_addr = |t: &str| t.split('/').next().unwrap_or_default().trim().to_owned();
        let (first, second) = if *col == Col::Address {
            (first_addr(&first), second.as_deref().map(first_addr))
        } else {
            (first, second)
        };
        let units = stagecraft_core::units::Units::of(&doc.meta);
        let values = match (col.is_length(), units.parse(&first)) {
            (true, Some(a)) => fill_lengths(&units, a, second.as_deref().and_then(|t| units.parse(t)), ids.len()),
            _ => fill_series(*col, &first, second.as_deref(), ids.len()),
        };
        // Addresses without a given step follow the footprints: auto-patch in row order.
        if *col == Col::Address && second.is_none() {
            let start = first.split('/').next().unwrap_or_default().trim().to_owned();
            if parse_address(&start).is_some() {
                steps.push(json!({ "command": "patch.auto", "args": { "ids": ids, "start": start } }));
                continue;
            }
        }
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
            if matches!(col, Col::Type | Col::Footprint | Col::Conflict | Col::Load) {
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

const ROW_H: f32 = 20.0;

pub fn show(ui: &mut Ui, s: &mut Studio) {
    let doc = s.engine.doc().clone();
    // Filters.
    ui.horizontal(|ui| {
        let kinds = [
            (None, "table.all"),
            (Some("fixture"), "type.fixture.plural"),
            (Some("truss"), "type.truss.plural"),
            (Some("batten"), "table.fly_bars"),
            (Some("wall"), "type.wall.plural"),
            (Some("floor"), "type.floor.plural"),
            (Some("seat_row"), "type.seat_row.plural"),
        ];
        for (k, key) in kinds {
            let on = s.ui.table.kind.as_deref() == k;
            let r = ui.selectable_label(on, t(key));
            widgets::name(&r, Role::Button, format!("Show {}", t(key)));
            if r.clicked() && !on {
                let widths = std::mem::take(&mut s.ui.table.widths);
                s.ui.table =
                    TableState { kind: k.map(String::from), tag: s.ui.table.tag, widths, ..Default::default() };
            }
        }
        ui.separator();
        let tag_name =
            s.ui.table
                .tag
                .and_then(|t| doc.tags.get(&t))
                .map_or_else(|| t("table.any_tag").to_owned(), |t| t.name.clone());
        let r = crate::widgets::combo_w(150.0, "table_tag").selected_text(tag_name).show_ui(ui, |ui| {
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
        widgets::name(&r.response, egui::Role::ComboBox, "Table tag");
        ui.separator();
        if widgets::button(ui, t("table.fill"), "Fill series").on_hover_text(t("table.fill_tip")).clicked() {
            fill_selected(s);
        }
    });

    let rows = rows(&doc, &s.ui.table);
    let cols = columns(s.ui.table.kind.as_deref());
    let sel_range = range(&rows, &s.ui.table);
    keys(ui, s, &rows, &cols);

    // Header: a strip in the inset colour, hairlines between the columns; drag a hairline
    // to resize, double-click it to go back to the default. The table scrolls sideways
    // when its columns are wider than the panel.
    let tk = crate::theme::of(ui);
    ui.add_space(2.0);
    // Feet and inches need wider length columns ("45' 11 3/16\"").
    let mut user = s.ui.table.widths.clone();
    if stagecraft_core::units::Units::of(&s.engine.doc().meta).unit == stagecraft_core::units::LengthUnit::FtIn {
        for c in cols.iter().filter(|c| c.is_length()) {
            user.entry(*c).or_insert(96.0);
        }
    }
    let widths = fit_widths(&cols, &user, ui.available_width() - 12.0);
    let table_w = NUM_W + widths.iter().sum::<f32>();
    egui::ScrollArea::horizontal().id_salt("table_h").auto_shrink([false, false]).show(ui, |ui| {
        ui.set_min_width(table_w);
        ui.vertical(|ui| body(ui, s, &doc, &rows, &cols, &widths, sel_range, tk));
    });
}

#[allow(clippy::too_many_arguments)]
fn body(
    ui: &mut Ui,
    s: &mut Studio,
    doc: &Document,
    rows: &[Id],
    cols: &[Col],
    widths: &[f32],
    sel_range: Option<(usize, usize, usize, usize)>,
    tk: crate::theme::Tokens,
) {
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let strip = egui::Rect::from_min_size(
            ui.cursor().min,
            vec2(ui.available_width().max(NUM_W + widths.iter().sum::<f32>()), ROW_H),
        );
        ui.painter().rect_filled(strip, 0.0, tk.panel_dark);
        ui.painter().hline(strip.x_range(), strip.bottom() - 0.5, egui::Stroke::new(1.0, tk.separator));
        ui.add_space(NUM_W);
        for (ci, c) in cols.iter().enumerate() {
            let (r, p) = ui.allocate_painter(vec2(widths[ci], ROW_H), Sense::hover());
            p.vline(r.rect.left().round() + 0.5, r.rect.y_range().shrink(3.0), egui::Stroke::new(1.0, tk.separator));
            p.with_clip_rect(r.rect.shrink2(vec2(2.0, 0.0))).text(
                r.rect.left_center() + vec2(6.0, 0.0),
                Align2::LEFT_CENTER,
                c.title(doc),
                crate::theme::medium(crate::theme::SMALL),
                tk.text2,
            );
            let handle = egui::Rect::from_center_size(r.rect.right_center(), vec2(7.0, ROW_H));
            let h = ui.interact(handle, egui::Id::unique(("table_resize", *c)), Sense::click_and_drag());
            widgets::name(&h, Role::Unknown, format!("Resize {}", t(c.key())));
            if h.hovered() || h.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
            }
            if h.dragged() {
                let w = (widths[ci] + h.drag_delta().x).max(MIN_W);
                s.ui.table.widths.insert(*c, w);
            }
            if h.double_clicked() {
                s.ui.table.widths.remove(c);
            }
        }
    });
    if rows.is_empty() {
        ui.label(RichText::new(t("table.empty")).weak());
        return;
    }
    let selected_objects: Vec<Id> = s.engine.selection().to_vec();
    let conflicting = stagecraft_core::patch::conflicting(doc);
    let mut picked: Option<String> = None;
    let mut clicked: Option<(Id, usize, Modifiers)> = None;
    let mut double: Option<(Id, usize)> = None;
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, rows.len(), |ui, visible| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for ri in visible {
            let id = rows[ri];
            let o = &doc.objects[&id];
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let (r, p) = ui.allocate_painter(vec2(NUM_W, ROW_H), Sense::hover());
                p.text(
                    r.rect.right_center() - vec2(6.0, 0.0),
                    Align2::RIGHT_CENTER,
                    (ri + 1).to_string(),
                    egui::TextStyle::Small.resolve(ui.style()),
                    ui.visuals().weak_text_color(),
                );
                for (ci, c) in cols.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(widths[ci], ROW_H), Sense::click());
                    let label = format!("{} {}", t(c.key()), ri + 1);
                    let text = cell_text(doc, o, *c);
                    widgets::name(&resp, Role::Unknown, label.clone());
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
                            egui::Stroke::new(1.5, crate::theme::of(ui).accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if is_cursor && s.ui.table.editing.is_some() {
                        let mut buf = s.ui.table.editing.clone().unwrap_or_default();
                        let ed_id = egui::Id::unique("table_editor");
                        let r = ui.put(
                            rect.shrink(1.0),
                            egui::TextEdit::singleline(&mut buf).id(ed_id).margin(vec2(4.0, 2.0)),
                        );
                        widgets::name(&r, Role::TextInput, "Cell editor");
                        let options = cell_options(doc, o, *c);
                        if !options.is_empty() {
                            egui::Area::new(egui::Id::unique("table_choices"))
                                .order(egui::Order::Foreground)
                                .fixed_pos(rect.left_bottom())
                                .show(ui.ctx(), |ui| {
                                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                                        ui.set_min_width(rect.width());
                                        for (value, label) in &options {
                                            let b = ui.selectable_label(text == *value, label);
                                            widgets::name(&b, Role::Button, format!("Choose {value}"));
                                            if b.clicked() {
                                                picked = Some(value.clone());
                                            }
                                        }
                                    });
                                });
                        }
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
                        let chose = picked.is_some();
                        if chose {
                            s.ui.table.editing = None;
                        } else if !options.is_empty()
                            && r.lost_focus()
                            && ui.input(|i| i.pointer.any_pressed())
                            && !enter
                            && !escape
                        {
                            // A click into the choice list arrives after focus is lost;
                            // keep the editor until the click lands.
                            s.ui.table.editing = Some(buf);
                        } else if r.lost_focus() || enter || escape {
                            s.ui.table.editing = None;
                            if !escape {
                                edit_selected(s, &buf);
                            }
                        } else {
                            s.ui.table.editing = Some(buf);
                        }
                    } else {
                        let warn = matches!(c, Col::Address | Col::Conflict) && conflicting.contains(&id);
                        let colour = if warn { crate::theme::of(ui).warning } else { ui.visuals().text_color() };
                        // Too long for the column: cut with "…", the whole text as the tooltip.
                        let mut job = egui::text::LayoutJob::single_section(
                            text.clone(),
                            egui::TextFormat::simple(egui::TextStyle::Body.resolve(ui.style()), colour),
                        );
                        job.wrap = egui::text::TextWrapping {
                            max_width: (rect.width() - 10.0).max(4.0),
                            max_rows: 1,
                            break_anywhere: true,
                            overflow_character: Some('…'),
                        };
                        let g = ui.fonts_mut(|f| f.layout_job(job));
                        let elided = g.elided;
                        ui.painter().galley(pos2(rect.left() + 6.0, rect.center().y - g.size().y / 2.0), g, colour);
                        if elided && resp.hovered() {
                            resp.clone().on_hover_text(&text);
                        }
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
    if let Some(v) = picked {
        s.ui.table.editing = None;
        edit_selected(s, &v);
    }
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
        select_rows_in_plan(s, rows);
    }
    if let Some((id, ci)) = double {
        let st = &mut s.ui.table;
        st.focused = true;
        st.anchor = Some((id, ci));
        st.cursor = Some((id, ci));
        st.editing = Some(cell_text(doc, &doc.objects[&id], cols[ci]));
        st.focus_editor = true;
        select_rows_in_plan(s, rows);
    }
}

/// Width of the row-number column.
const NUM_W: f32 = 36.0;
const MIN_W: f32 = 36.0;

/// Column widths for a panel `avail` wide: dragged widths, else defaults; when the table
/// is narrower than the panel the flexible columns (name, type, tags …) share the rest.
/// Wider tables scroll sideways.
pub fn fit_widths(cols: &[Col], user: &std::collections::BTreeMap<Col, f32>, avail: f32) -> Vec<f32> {
    let mut w: Vec<f32> = cols.iter().map(|c| user.get(c).copied().unwrap_or(c.width())).collect();
    let total: f32 = NUM_W + w.iter().sum::<f32>();
    let flex: f32 = cols.iter().zip(&w).filter(|(c, _)| c.flexible() && !user.contains_key(c)).map(|x| *x.1).sum();
    if total < avail && flex > 0.0 {
        let extra = avail - total;
        for (c, x) in cols.iter().zip(w.iter_mut()) {
            if c.flexible() && !user.contains_key(c) {
                *x += extra * *x / flex;
            }
        }
    } else if total > avail && flex > 0.0 {
        // Narrow: the flexible columns give up to 30 % of their width before the table scrolls.
        let cut = ((total - avail) / flex).min(0.3);
        for (c, x) in cols.iter().zip(w.iter_mut()) {
            if c.flexible() && !user.contains_key(c) {
                *x *= 1.0 - cut;
            }
        }
    }
    w
}

/// Keys while the table has focus and no cell is being edited.
fn keys(ui: &mut Ui, s: &mut Studio, rows: &[Id], cols: &[Col]) {
    let st = &s.ui.table;
    if !st.focused || st.editing.is_some() || s.ui.palette.open || crate::framework::other_focus(ui.ctx()) {
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
    fn columns_fill_a_wide_panel_and_keep_dragged_widths() {
        let cols = columns(Some("fixture"));
        let defaults: f32 = NUM_W + cols.iter().map(|c| c.width()).sum::<f32>();
        // Wide: the flexible columns take the rest, the fixed ones keep their width.
        let w = fit_widths(&cols, &Default::default(), defaults + 300.0);
        assert!((NUM_W + w.iter().sum::<f32>() - (defaults + 300.0)).abs() < 0.5);
        let addr = cols.iter().position(|c| *c == Col::Address).unwrap();
        assert_eq!(w[addr], Col::Address.width());
        // A bit narrow: the flexible columns give way, so the fixed ones still show.
        let w = fit_widths(&cols, &Default::default(), defaults - 100.0);
        assert!((NUM_W + w.iter().sum::<f32>() - (defaults - 100.0)).abs() < 0.5);
        assert_eq!(w[addr], Col::Address.width());
        // Very narrow: the table scrolls; a dragged width stays as dragged.
        let mut user = std::collections::BTreeMap::new();
        user.insert(Col::Name, 300.0);
        let w = fit_widths(&cols, &user, 400.0);
        assert_eq!(w[0], 300.0);
        assert_eq!(w[addr], Col::Address.width());
        assert!(NUM_W + w.iter().sum::<f32>() > 400.0);
    }

    #[test]
    fn tsv_from_spreadsheets() {
        assert_eq!(parse_tsv("a\tb\r\nc\td\r\n"), vec![vec!["a", "b"], vec!["c", "d"]]);
        assert_eq!(parse_tsv("101"), vec![vec!["101"]]);
        assert_eq!(parse_tsv("x\t\ty\n"), vec![vec!["x", "", "y"]]);
    }
}
