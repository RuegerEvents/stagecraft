//! The command palette (⌘K): fuzzy search over both registries; commands with
//! arguments get a form generated from their JSON Schema.

use std::collections::BTreeMap;

use egui::{Key, RichText, Role};
use serde_json::Value;

use crate::app::Studio;
use crate::commands::ui_registry;
use crate::i18n::{t, tf};
use crate::schema_form::{FieldKind, FieldSpec, build_args, fields};
use crate::widgets;

#[derive(Default)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    pub cursor: usize,
    /// The command whose form is showing.
    pub chosen: Option<String>,
    pub values: BTreeMap<String, String>,
    pub error: Option<String>,
    focus_search: bool,
}

impl PaletteState {
    pub fn open(&mut self) {
        *self = PaletteState { open: true, focus_search: true, ..Default::default() };
    }
    pub fn close(&mut self) {
        *self = PaletteState::default();
    }
    pub fn toggle(&mut self) {
        if self.open { self.close() } else { self.open() }
    }
}

/// A command either registry knows.
#[derive(Clone)]
pub struct Entry {
    pub name: &'static str,
    pub summary: &'static str,
    pub kind: &'static str,
}

pub fn entries() -> Vec<Entry> {
    let mut v: Vec<Entry> = stagecraft_core::registry()
        .entries()
        .iter()
        .map(|e| Entry {
            name: e.name,
            summary: e.summary,
            kind: match e.kind {
                stagecraft_core::Kind::Query => "query",
                stagecraft_core::Kind::Edit => "edit",
                stagecraft_core::Kind::Meta => "meta",
            },
        })
        .collect();
    v.extend(ui_registry().entries().iter().map(|e| Entry { name: e.name, summary: e.summary, kind: "ui" }));
    v
}

/// Commands matching the query, best first; everything by name for an empty query.
pub fn search(query: &str) -> Vec<Entry> {
    let mut v: Vec<(i32, Entry)> =
        entries().into_iter().filter_map(|e| crate::fuzzy::score(query, e.name, e.summary).map(|s| (s, e))).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(b.1.name)));
    v.into_iter().map(|x| x.1).collect()
}

pub fn schema_of(name: &str) -> Value {
    match ui_registry().get(name) {
        Some(e) => e.schema(),
        None => stagecraft_core::registry().get(name).map(|e| e.schema()).unwrap_or(Value::Null),
    }
}

/// Start the form for a command, or run it right away if it takes nothing.
fn choose(s: &mut Studio, name: &str) {
    let specs = fields(&schema_of(name));
    if specs.is_empty() {
        run(s, name, Value::Object(Default::default()));
        return;
    }
    let sel = s.engine.selection().to_vec();
    let mut values = BTreeMap::new();
    for f in &specs {
        // Ids default to the selection, so "delete", "move" … work on what is picked.
        let prefill = match (f.name.as_str(), &f.kind) {
            ("ids", _) if !sel.is_empty() => Some(sel.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ")),
            ("id", _) if sel.len() == 1 => Some(sel[0].to_string()),
            (_, FieldKind::Bool) => Some("false".to_owned()),
            _ => None,
        };
        if let Some(p) = prefill {
            values.insert(f.name.clone(), p);
        }
    }
    let p = &mut s.ui.palette;
    p.chosen = Some(name.to_owned());
    p.values = values;
    p.error = None;
}

fn run(s: &mut Studio, name: &str, args: Value) {
    let name = name.to_owned();
    // Close first: ui.palette and others may reopen it.
    let keep = std::mem::take(&mut s.ui.palette);
    match s.run(&name, args) {
        Ok(v) => {
            let short = serde_json::to_string(&v).unwrap_or_default();
            let short: String = short.chars().take(80).collect();
            s.ui.info(tf("palette.ran", &[("name", &name), ("result", &short)]));
        }
        Err(e) => {
            s.ui.palette = keep;
            s.ui.palette.error = Some(e.to_string());
            if s.ui.palette.chosen.is_none() {
                // Commands without a form: show the error in the list view.
                s.ui.palette.open = true;
            }
        }
    }
}

pub fn show(ctx: &egui::Context, s: &mut Studio) {
    if !s.ui.palette.open {
        return;
    }
    let modal = egui::Modal::new(egui::Id::unique("palette")).show(ctx, |ui| {
        ui.set_width(560.0);
        match s.ui.palette.chosen.clone() {
            None => list(ui, s),
            Some(name) => form(ui, s, &name),
        }
    });
    if modal.should_close() && s.ui.palette.open {
        s.ui.palette.close();
    }
}

fn list(ui: &mut egui::Ui, s: &mut Studio) {
    let p = &mut s.ui.palette;
    let id = egui::Id::unique("palette_search");
    let r = ui
        .add(egui::TextEdit::singleline(&mut p.query).id(id).hint_text(t("palette.hint")).desired_width(f32::INFINITY));
    widgets::name(&r, Role::TextInput, "Search commands");
    if p.focus_search {
        r.request_focus();
        p.focus_search = false;
    }
    if r.changed() {
        p.cursor = 0;
    }
    let found = search(&p.query);
    let (down, up, enter) = ui.input_mut(|i| {
        (
            i.consume_key(Default::default(), Key::ArrowDown),
            i.consume_key(Default::default(), Key::ArrowUp),
            i.key_pressed(Key::Enter),
        )
    });
    if down {
        p.cursor = (p.cursor + 1).min(found.len().saturating_sub(1));
    }
    if up {
        p.cursor = p.cursor.saturating_sub(1);
    }
    if let Some(e) = &p.error {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
    ui.add_space(4.0);
    let mut picked: Option<&'static str> = None;
    egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
        for (i, e) in found.iter().enumerate().take(200) {
            let on = i == s.ui.palette.cursor;
            let text = RichText::new(e.name).monospace();
            let r = ui
                .horizontal(|ui| {
                    let r = ui.selectable_label(on, text);
                    ui.label(RichText::new(e.summary).weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(e.kind).small().weak());
                    });
                    r
                })
                .inner;
            widgets::name(&r, Role::Button, e.name);
            if on && (down || up) {
                r.scroll_to_me(None);
            }
            if r.clicked() {
                picked = Some(e.name);
            }
        }
    });
    if enter && picked.is_none() {
        picked = found.get(s.ui.palette.cursor).map(|e| e.name);
    }
    if let Some(name) = picked {
        choose(s, name);
    }
}

fn form(ui: &mut egui::Ui, s: &mut Studio, name: &str) {
    let schema = schema_of(name);
    let specs: Vec<FieldSpec> = fields(&schema);
    let summary = entries().into_iter().find(|e| e.name == name).map_or("", |e| e.summary);
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).monospace().strong());
        ui.label(RichText::new(summary).weak());
    });
    ui.add_space(6.0);
    let mut first = true;
    egui::Grid::new("palette_form").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        for f in &specs {
            let label = if f.required { format!("{} *", f.name) } else { f.name.clone() };
            ui.label(RichText::new(label).monospace()).on_hover_text(&f.description);
            let accessible = format!("Argument {}", f.name);
            let value = s.ui.palette.values.entry(f.name.clone()).or_default();
            match &f.kind {
                FieldKind::Bool => {
                    let mut on = value == "true";
                    let r = ui.checkbox(&mut on, "");
                    widgets::name(&r, Role::CheckBox, &accessible);
                    *value = on.to_string();
                }
                FieldKind::Choice(options) => {
                    let shown = if value.is_empty() { "—".to_owned() } else { value.clone() };
                    let r = egui::ComboBox::from_id_salt(("palette_choice", &f.name)).selected_text(shown).show_ui(
                        ui,
                        |ui| {
                            if !f.required && ui.selectable_label(value.is_empty(), "—").clicked() {
                                value.clear();
                            }
                            for o in options {
                                if ui.selectable_label(value == o, o).clicked() {
                                    *value = o.clone();
                                }
                            }
                        },
                    );
                    widgets::name(&r.response, egui::Role::ComboBox, &accessible);
                }
                _ => {
                    let hint = match f.kind {
                        FieldKind::List => t("palette.list_hint"),
                        FieldKind::Json => t("palette.json_hint"),
                        _ => "",
                    };
                    let r = ui.add(egui::TextEdit::singleline(value).desired_width(360.0).hint_text(hint));
                    widgets::name(&r, Role::TextInput, &accessible);
                    if first && ui.memory(|m| m.focused().is_none()) {
                        r.request_focus();
                    }
                    first = false;
                }
            }
            ui.end_row();
            if !f.description.is_empty() {
                ui.label("");
                ui.label(RichText::new(&f.description).small().weak());
                ui.end_row();
            }
        }
    });
    if let Some(e) = &s.ui.palette.error {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
    ui.add_space(6.0);
    let submit = ui.input(|i| i.key_pressed(Key::Enter) && i.modifiers.command);
    ui.horizontal(|ui| {
        if widgets::button(ui, t("palette.run"), "Run").clicked() || submit {
            match build_args(&specs, &s.ui.palette.values) {
                Ok(args) => run(s, name, args),
                Err(e) => s.ui.palette.error = Some(e),
            }
        }
        if widgets::button(ui, t("palette.back"), "Back").clicked() {
            let p = &mut s.ui.palette;
            p.chosen = None;
            p.error = None;
            p.focus_search = true;
        }
    });
}
