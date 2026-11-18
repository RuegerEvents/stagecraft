//! The command palette (⌘K), in the manner of VS Code, Figma and Raycast.
//!
//! - An empty query shows **Recent** commands (saved per user), commands **for the
//!   selection** (8 fixtures: Auto-Patch, Set Type & Mode, Attach to …) and the **groups**
//!   (File, Edit, View, Draw … `command_meta::Group`). → or Enter opens a group, ← or
//!   Backspace on an empty query goes back; the breadcrumb shows where one is.
//! - Typing searches titles, search aliases (German and Vectorworks names: "Zug",
//!   "Traverse", "Podest", "Klasse" …), groups and summaries of every visible command, and
//!   finds objects by name or FID, layers, tags and sheets; picking one selects it and
//!   zooms to it. Inside a group the search stays in the group.
//! - `>` lists every command, the technical ones too, with their IDs; `@` objects only,
//!   `#` tags only.
//! - Commands that cannot run now are greyed with the reason ("Select fixtures first").
//! - A command with arguments opens a compact form in the palette (`schema_form`):
//!   Enter runs, Esc goes back.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use egui::{Align2, Color32, FontId, Galley, Key, Modifiers, Rect, Role, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use stagecraft_core::{Id, ObjectKind};

use crate::app::Studio;
use crate::command_meta::{self, Context, Group, META, Meta};
use crate::commands::ui_registry;
use crate::fuzzy::subsequence;
use crate::i18n::{t, tf};
use crate::schema_form::{FieldKind, FieldSpec, RefKind, build_args, fields_for};
use crate::theme::{self, Tokens};
use crate::widgets;

#[derive(Default)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    pub cursor: usize,
    /// The group being browsed.
    pub group: Option<Group>,
    /// The command whose form is showing.
    pub chosen: Option<String>,
    /// Its row in [`META`].
    pub chosen_meta: Option<usize>,
    pub values: BTreeMap<String, String>,
    pub error: Option<String>,
    focus_search: bool,
    focus_form: bool,
    /// The keyboard moved the cursor: scroll it into view.
    scroll: bool,
    /// Query, cursor and group to go back to from the form.
    back: Option<(String, usize, Option<Group>)>,
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
    /// Leave the form for the list it came from.
    pub fn back_to_list(&mut self) {
        let (query, cursor, group) = self.back.take().unwrap_or_default();
        self.chosen = None;
        self.chosen_meta = None;
        self.values.clear();
        self.error = None;
        self.query = query;
        self.cursor = cursor;
        self.group = group;
        self.focus_search = true;
    }
}

/// What a row of the list stands for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Item {
    Group(Group),
    /// A row of [`META`].
    Command(usize),
    Object(Id),
    Layer(Id),
    Tag(Id),
    Sheet(Id),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Recent,
    Suggested,
    Groups,
    Commands,
    Objects,
    Places,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub item: Item,
    pub section: Section,
    /// Why the command cannot run now.
    pub blocked: Option<String>,
}

/// What the query's first character asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// `>`: every command, technical ones too, with IDs.
    Commands,
    /// `@`: objects.
    Objects,
    /// `#`: tags.
    Tags,
}

pub fn mode(query: &str) -> (Mode, &str) {
    let q = query.trim_start();
    match q.chars().next() {
        Some('>') => (Mode::Commands, q[1..].trim()),
        Some('@') => (Mode::Objects, q[1..].trim()),
        Some('#') => (Mode::Tags, q[1..].trim()),
        _ => (Mode::Normal, q.trim()),
    }
}

pub fn schema_of(name: &str) -> Value {
    match ui_registry().get(name) {
        Some(e) => e.schema(),
        None => stagecraft_core::registry().get(name).map(|e| e.schema()).unwrap_or(Value::Null),
    }
}

/// A command of either registry.
pub fn is_command(name: &str) -> bool {
    ui_registry().get(name).is_some() || stagecraft_core::registry().get(name).is_some()
}

// ---------------------------------------------------------------- search

/// Alias words per row of [`META`]: for every term of `aliases.json` its title (or ID)
/// contains, the words people also type for it.
fn alias_text(i: usize) -> &'static str {
    static A: OnceLock<Vec<String>> = OnceLock::new();
    &A.get_or_init(|| {
        META.iter()
            .map(|m| {
                let title = m.title().to_lowercase();
                let words: Vec<String> = title
                    .split(|c: char| !c.is_alphanumeric())
                    .chain(m.id.split(['.', '_']))
                    .filter(|w| !w.is_empty())
                    .map(str::to_lowercase)
                    .collect();
                let mut out = String::new();
                for (canon, aliases) in crate::i18n::aliases() {
                    let hit = if canon.contains(' ') {
                        title.contains(canon.as_str())
                    } else {
                        words.iter().any(|w| w.starts_with(canon.as_str()))
                    };
                    if hit {
                        for a in aliases {
                            out.push_str(a);
                            out.push(' ');
                        }
                    }
                }
                out
            })
            .collect()
    })[i]
}

/// A fuzzy match that counts: contiguous, from word starts, not letters strewn across.
fn strong(word: &str, text: &str) -> Option<i32> {
    let n = word.chars().count() as i32;
    subsequence(word, text).filter(|s| *s >= 15 * n)
}

fn word_prefix(word: &str, text: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(word))
}

/// How well `query` (lower case, prefix stripped) matches a row of [`META`].
fn score_meta(i: usize, query: &str, with_ids: bool) -> Option<i32> {
    let m = &META[i];
    let title = m.title();
    let tl = title.to_lowercase();
    let group = m.group.title().to_lowercase();
    let summary = m.summary().to_lowercase();
    let aliases = alias_text(i);
    let mut total = 0;
    for w in query.split_whitespace() {
        let n = w.chars().count() as i32;
        let alias = (n >= 2 && word_prefix(w, aliases)).then(|| {
            let exact = aliases.split_whitespace().any(|a| a == w);
            12 + 7 * n.min(7) + if exact { 10 } else { 0 }
        });
        let id = strong(w, m.id).map(|s| if with_ids { s } else { s / 2 });
        let best = [
            strong(w, title),
            alias,
            id,
            (n >= 2 && word_prefix(w, &group)).then_some(8 * n.min(5)),
            (n >= 3 && word_prefix(w, &summary)).then_some(6 * n.min(5)),
        ]
        .into_iter()
        .flatten()
        .max()?;
        total += best;
    }
    if tl.starts_with(query) {
        total += 40;
    } else if tl.contains(query) {
        total += 20;
    }
    if with_ids && m.id.starts_with(query) {
        total += 40;
    }
    Some(total)
}

/// Rows of [`META`] matching `query`, best first.
pub fn search_commands(query: &str, hidden: bool, group: Option<Group>) -> Vec<(usize, i32)> {
    let q = query.trim().to_lowercase();
    let mut v: Vec<(usize, i32)> = (0..META.len())
        .filter(|&i| (hidden || !META[i].hidden) && group.is_none_or(|g| META[i].group == g))
        .filter_map(|i| score_meta(i, &q, hidden).map(|s| (i, s)))
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

/// Names in reading order: "Batten 2" before "Batten 10".
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    fn key(s: &str) -> Vec<(String, u64)> {
        let mut out = Vec::new();
        let mut text = String::new();
        let mut num = String::new();
        for ch in s.chars() {
            if ch.is_ascii_digit() {
                num.push(ch);
            } else {
                if !num.is_empty() {
                    out.push((std::mem::take(&mut text), num.parse().unwrap_or(0)));
                    num.clear();
                }
                text.extend(ch.to_lowercase());
            }
        }
        out.push((text, num.parse().unwrap_or(0)));
        out
    }
    key(a).cmp(&key(b))
}

fn fid_of(o: &stagecraft_core::Object) -> Option<u32> {
    match &o.kind {
        ObjectKind::Fixture { fid, .. } => *fid,
        _ => None,
    }
}

fn score_object(o: &stagecraft_core::Object, q: &str) -> Option<i32> {
    if q.is_empty() {
        return Some(0);
    }
    let name = o.name.to_lowercase();
    if q.chars().all(|c| c.is_ascii_digit()) {
        let fid = fid_of(o).map(|f| f.to_string());
        return match fid {
            Some(f) if f == q => Some(300),
            Some(f) if f.starts_with(q) => Some(150 - f.len() as i32),
            _ if name.contains(q) => Some(60),
            _ => None,
        };
    }
    let kind = kind_words(o.kind.type_name());
    let mut total = 0;
    for w in q.split_whitespace() {
        let n = w.chars().count();
        total += [strong(w, &name), (n >= 2 && word_prefix(w, kind)).then_some(10)].into_iter().flatten().max()?;
    }
    if name == q {
        total += 120;
    } else if name.starts_with(q) {
        total += 60;
    }
    Some(total)
}

/// A kind's name and its aliases ("batten zug lattenzug pipe …"), lower case.
fn kind_words(kind: &str) -> &'static str {
    static K: OnceLock<std::sync::Mutex<BTreeMap<String, &'static str>>> = OnceLock::new();
    let mut m = K.get_or_init(Default::default).lock().expect("not poisoned");
    m.entry(kind.to_owned()).or_insert_with(|| {
        let name = t(&format!("type.{kind}")).to_lowercase();
        let mut out = format!("{name} {} ", kind.replace('_', " "));
        for (canon, aliases) in crate::i18n::aliases() {
            if name.split(' ').any(|w| w.starts_with(canon.as_str())) || kind.starts_with(canon.as_str()) {
                out.push_str(&aliases.join(" "));
                out.push(' ');
            }
        }
        Box::leak(out.into_boxed_str())
    })
}

fn score_name(name: &str, q: &str) -> Option<i32> {
    if q.is_empty() {
        return Some(0);
    }
    let n = name.to_lowercase();
    let mut total = 0;
    for w in q.split_whitespace() {
        total += strong(w, &n)?;
    }
    if n == q {
        total += 120;
    } else if n.starts_with(q) {
        total += 50;
    }
    Some(total)
}

fn command_row(i: usize, section: Section, c: &Context) -> Row {
    Row { item: Item::Command(i), section, blocked: command_meta::available(&META[i], c).err() }
}

/// Plain words for the selection: "8 fixtures", "1 batten", "3 objects".
pub fn selection_words(c: &Context) -> String {
    let n = c.kinds.len();
    match c.common_kind() {
        Some(k) if n == 1 => format!("1 {}", t(&format!("type.{k}")).to_lowercase()),
        Some(k) => format!("{n} {}", t(&format!("type.{k}.plural")).to_lowercase()),
        None => tf("palette.objects_n", &[("n", &n)]),
    }
}

/// The rows the palette shows now.
pub fn rows(s: &Studio) -> Vec<Row> {
    let p = &s.ui.palette;
    let c = Context::of(s);
    let doc = s.engine.doc();
    let (mode, rest) = mode(&p.query);
    let q = rest.to_lowercase();
    let mut out = Vec::new();
    match mode {
        Mode::Commands => {
            let mut found = search_commands(&q, true, None);
            if q.is_empty() {
                found.sort_by_key(|(i, _)| (Group::ALL.iter().position(|g| *g == META[*i].group), *i));
            }
            out.extend(found.into_iter().map(|(i, _)| command_row(i, Section::Commands, &c)));
        }
        Mode::Objects => {
            let mut v: Vec<(i32, &stagecraft_core::Object)> =
                doc.objects.values().filter_map(|o| score_object(o, &q).map(|sc| (sc, o))).collect();
            let sel = s.engine.selection();
            v.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then(sel.contains(&b.1.id).cmp(&sel.contains(&a.1.id)))
                    .then(fid_of(a.1).unwrap_or(u32::MAX).cmp(&fid_of(b.1).unwrap_or(u32::MAX)))
                    .then(natural(&a.1.name, &b.1.name))
            });
            out.extend(v.into_iter().take(80).map(|(_, o)| Row {
                item: Item::Object(o.id),
                section: Section::Objects,
                blocked: None,
            }));
        }
        Mode::Tags => {
            let mut v: Vec<(i32, &stagecraft_core::Tag)> =
                doc.tags.values().filter_map(|tg| score_name(&tg.name, &q).map(|sc| (sc, tg))).collect();
            v.sort_by(|a, b| b.0.cmp(&a.0).then(natural(&a.1.name, &b.1.name)));
            out.extend(v.into_iter().map(|(_, tg)| Row {
                item: Item::Tag(tg.id),
                section: Section::Places,
                blocked: None,
            }));
        }
        Mode::Normal if q.is_empty() => match p.group {
            Some(g) => {
                out.extend(
                    (0..META.len())
                        .filter(|&i| META[i].group == g && !META[i].hidden)
                        .map(|i| command_row(i, Section::Commands, &c)),
                );
            }
            None => {
                for key in s.ui.recent.iter().take(5) {
                    if let Some(i) = META.iter().position(|m| &m.key_str() == key && !m.hidden) {
                        out.push(command_row(i, Section::Recent, &c));
                    }
                }
                if !c.kinds.is_empty() {
                    let fits = (0..META.len())
                        .filter(|&i| {
                            let m = &META[i];
                            !m.hidden && command_meta::fits_selection(m, &c) && command_meta::available(m, &c).is_ok()
                        })
                        .filter(|i| !out.iter().any(|r| r.item == Item::Command(*i)))
                        .take(6)
                        .collect::<Vec<_>>();
                    out.extend(fits.into_iter().map(|i| command_row(i, Section::Suggested, &c)));
                }
                out.extend(Group::ALL.iter().filter(|g| **g != Group::Advanced).map(|g| Row {
                    item: Item::Group(*g),
                    section: Section::Groups,
                    blocked: None,
                }));
            }
        },
        Mode::Normal => {
            let mut cmds = search_commands(&q, false, p.group);
            for (i, sc) in &mut cmds {
                if command_meta::fits_selection(&META[*i], &c) {
                    *sc += 30;
                }
                if command_meta::available(&META[*i], &c).is_err() {
                    *sc -= 20;
                }
            }
            cmds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            cmds.truncate(40);
            let mut sections: Vec<(i32, Vec<Row>)> = Vec::new();
            if let Some(top) = cmds.first().map(|x| x.1) {
                sections.push((top, cmds.iter().map(|(i, _)| command_row(*i, Section::Commands, &c)).collect()));
            }
            if p.group.is_none() {
                let mut objs: Vec<(i32, &stagecraft_core::Object)> =
                    doc.objects.values().filter_map(|o| score_object(o, &q).map(|sc| (sc, o))).collect();
                objs.sort_by(|a, b| b.0.cmp(&a.0).then(natural(&a.1.name, &b.1.name)));
                if let Some(top) = objs.first().map(|x| x.0) {
                    sections.push((
                        top,
                        objs.into_iter()
                            .take(12)
                            .map(|(_, o)| Row { item: Item::Object(o.id), section: Section::Objects, blocked: None })
                            .collect(),
                    ));
                }
                let mut places: Vec<(i32, Item)> = Vec::new();
                places.extend(
                    doc.layers.values().filter_map(|l| score_name(&l.name, &q).map(|sc| (sc, Item::Layer(l.id)))),
                );
                places.extend(
                    doc.tags.values().filter_map(|tg| score_name(&tg.name, &q).map(|sc| (sc, Item::Tag(tg.id)))),
                );
                places.extend(doc.sheets.values().filter_map(|sh| {
                    let num = (sh.number.to_lowercase() == q).then_some(200);
                    num.or_else(|| score_name(&format!("{} {}", sh.number, sh.name), &q))
                        .map(|sc| (sc, Item::Sheet(sh.id)))
                }));
                places.sort_by_key(|p| std::cmp::Reverse(p.0));
                if let Some(top) = places.first().map(|x| x.0) {
                    sections.push((
                        top,
                        places
                            .into_iter()
                            .take(8)
                            .map(|(_, it)| Row { item: it, section: Section::Places, blocked: None })
                            .collect(),
                    ));
                }
            }
            sections.sort_by_key(|x| std::cmp::Reverse(x.0));
            out.extend(sections.into_iter().flat_map(|x| x.1));
        }
    }
    out
}

// ---------------------------------------------------------------- choosing and running

/// Remember a command for the Recent section.
fn remember(s: &mut Studio, i: usize) {
    let key = META[i].key_str();
    s.ui.recent.retain(|k| *k != key);
    s.ui.recent.insert(0, key);
    s.ui.recent.truncate(8);
}

/// Pick a row: open a group, run or open the form of a command, go to an object.
pub fn activate(s: &mut Studio, row: &Row) {
    if let Some(why) = &row.blocked {
        s.ui.palette.error = Some(why.clone());
        return;
    }
    match row.item {
        Item::Group(g) => {
            let p = &mut s.ui.palette;
            p.group = Some(g);
            p.query.clear();
            p.cursor = 0;
            p.error = None;
            p.focus_search = true;
        }
        Item::Command(i) => choose_meta(s, i),
        Item::Object(id) => {
            let name = crate::schema_form::object_label(s.engine.doc(), id);
            go_to(s, vec![id], &name);
        }
        Item::Layer(id) => {
            let doc = s.engine.doc();
            let ids: Vec<Id> = doc.objects.values().filter(|o| o.layer == id).map(|o| o.id).collect();
            let name = doc.layers.get(&id).map(|l| l.name.clone()).unwrap_or_default();
            let _ = s.run("ui.set_active_layer", json!({ "layer": id.to_string() }));
            go_to(s, ids, &name);
        }
        Item::Tag(id) => {
            let doc = s.engine.doc();
            let ids: Vec<Id> = doc.objects.values().filter(|o| o.tags.contains(&id)).map(|o| o.id).collect();
            let name = doc.tags.get(&id).map(|tg| tg.name.clone()).unwrap_or_default();
            go_to(s, ids, &name);
        }
        Item::Sheet(id) => {
            s.ui.palette.close();
            let _ = s.run("ui.set_view", json!({ "view": "sheets" }));
            let _ = s.run("ui.sheet", json!({ "sheet": id.to_string() }));
        }
    }
}

/// Select `ids` and zoom to them (in a model view).
fn go_to(s: &mut Studio, ids: Vec<Id>, name: &str) {
    s.ui.palette.close();
    if s.ui.view == crate::state::View::Sheets {
        let _ = s.run("ui.set_view", json!({ "view": s.ui.model_view }));
    }
    let _ = s.run("selection.set", json!({ "ids": ids }));
    if !ids.is_empty() {
        let _ = s.run("ui.zoom_to_fit", json!({ "selection": true }));
    }
    s.ui.info(tf("palette.went", &[("name", &name)]));
}

/// Start the form for a command (its base row), or run it right away if it takes
/// nothing.
pub fn choose(s: &mut Studio, name: &str) {
    match META.iter().position(|m| m.id == name && m.args.is_none()) {
        Some(i) => choose_meta(s, i),
        None => {
            let specs = fields_for(name, &schema_of(name));
            if specs.is_empty() {
                run_named(s, name, None, json!({}));
            } else {
                open_form(s, name, None, specs);
            }
        }
    }
}

fn choose_meta(s: &mut Studio, i: usize) {
    let m: &'static Meta = &META[i];
    if m.id.starts_with("app.") {
        s.ui.palette.close();
        remember(s, i);
        crate::menu::run(s, &craft_menu::CommandRef::new(m.id));
        return;
    }
    if let Some(args) = m.args_value() {
        run_named(s, m.id, Some(i), args);
        return;
    }
    let specs = fields_for(m.id, &schema_of(m.id));
    if specs.is_empty() {
        run_named(s, m.id, Some(i), json!({}));
        return;
    }
    if m.direct {
        let values = prefill(s, m.applies, &specs);
        if let Ok(args) = build_args(&specs, &values) {
            run_named(s, m.id, Some(i), args);
            return;
        }
    }
    open_form(s, m.id, Some(i), specs);
}

fn open_form(s: &mut Studio, name: &str, i: Option<usize>, specs: Vec<FieldSpec>) {
    let values = prefill(s, i.map_or(&[][..], |i| META[i].applies), &specs);
    let p = &mut s.ui.palette;
    if p.chosen.is_none() {
        p.back = Some((std::mem::take(&mut p.query), p.cursor, p.group));
    }
    p.open = true;
    p.chosen = Some(name.to_owned());
    p.chosen_meta = i;
    p.values = values;
    p.error = None;
    p.focus_form = true;
}

/// Values a form starts with: schema defaults, the selection for object ids, the active
/// layer, the current sheet, the first of a list where one must be picked.
fn prefill(s: &Studio, applies: &[&str], specs: &[FieldSpec]) -> BTreeMap<String, String> {
    let doc = s.engine.doc();
    let sel: Vec<Id> = s.engine.selection().to_vec();
    let of_kind: Vec<Id> = sel
        .iter()
        .copied()
        .filter(|id| {
            applies.is_empty()
                || applies.contains(&"*")
                || doc.objects.get(id).is_some_and(|o| applies.contains(&o.kind.type_name()))
        })
        .collect();
    let mut values = BTreeMap::new();
    for f in specs {
        let first = |kind: RefKind| crate::schema_form::options(doc, kind).into_iter().next().map(|o| o.0);
        let v = match (&f.kind, f.name.as_str()) {
            (FieldKind::Objects, "ids") if !of_kind.is_empty() => {
                Some(of_kind.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", "))
            }
            (FieldKind::Object, "id") if of_kind.len() == 1 => Some(of_kind[0].to_string()),
            (FieldKind::Bool, _) => Some(f.default.clone().unwrap_or_else(|| "false".to_owned())),
            (FieldKind::Ref(RefKind::Layer), _) => {
                let active = s.ui.active_layer.and_then(|l| doc.layers.get(&l)).map(|l| l.name.clone());
                if f.required { active.or_else(|| first(RefKind::Layer)) } else { active }
            }
            (FieldKind::Ref(RefKind::Sheet), _) => crate::sheets::current(doc, &s.ui.sheets).map(|i| i.to_string()),
            (FieldKind::Ref(RefKind::Viewport), _) => {
                s.ui.sheets.viewport.map(|v| v.to_string()).or_else(|| first(RefKind::Viewport))
            }
            (FieldKind::Ref(RefKind::FixtureType), _) => {
                let of_selection = of_kind.iter().find_map(|id| match &doc.objects.get(id)?.kind {
                    ObjectKind::Fixture { gdtf: Some(g), .. } => Some(g.clone()),
                    _ => None,
                });
                let tool = Some(s.ui.options.fixture_type.clone()).filter(|t| doc.fixture_types.contains_key(t));
                if f.required { of_selection.or(tool).or_else(|| first(RefKind::FixtureType)) } else { tool }
            }
            (FieldKind::Ref(k), _) if f.required => first(*k),
            (FieldKind::Choice(o), _) if f.required && f.default.is_none() => o.first().cloned(),
            _ => f.default.clone(),
        };
        if let Some(v) = v {
            values.insert(f.name.clone(), v);
        }
    }
    values
}

fn run_named(s: &mut Studio, name: &str, i: Option<usize>, args: Value) {
    let name = name.to_owned();
    // Close first: ui.palette and others may reopen it.
    let keep = std::mem::take(&mut s.ui.palette);
    match s.run(&name, args) {
        Ok(v) => {
            if let Some(i) = i {
                remember(s, i);
            }
            let title = i.map_or(name.as_str(), |i| META[i].title());
            let short = serde_json::to_string(&v).unwrap_or_default();
            if short == "{}" || short == "null" {
                s.ui.info(tf("palette.done", &[("name", &title)]));
            } else {
                let short: String = short.chars().take(80).collect();
                s.ui.info(tf("palette.ran", &[("name", &title), ("result", &short)]));
            }
        }
        Err(e) => {
            s.ui.palette = keep;
            s.ui.palette.open = true;
            s.ui.palette.error = Some(e.message.clone());
        }
    }
}

// ---------------------------------------------------------------- drawing

/// The command with its fixed arguments, as MCP and the CLI know it: `ui.set_tool tool=batten`.
/// Shown only in `>` mode ([`technical`]).
pub fn display_id(m: &Meta) -> String {
    match m.args_value() {
        Some(Value::Object(a)) => {
            let pairs: Vec<String> = a
                .iter()
                .map(|(k, v)| format!("{k}={}", v.as_str().map_or_else(|| v.to_string(), str::to_owned)))
                .collect();
            format!("{} {}", m.id, pairs.join(" "))
        }
        _ => m.id.to_owned(),
    }
}

/// Whether the palette shows command IDs and arguments: in `>` mode, and in a form opened
/// from it. Everywhere else people see titles only.
pub fn technical(s: &Studio) -> bool {
    let p = &s.ui.palette;
    match (&p.chosen, &p.back) {
        (Some(_), Some((query, ..))) => mode(query).0 == Mode::Commands,
        (Some(_), None) => false,
        (None, _) => mode(&p.query).0 == Mode::Commands,
    }
}

/// The ID a row shows at its right end: only in `>` mode.
pub fn shown_id(s: &Studio, item: &Item) -> Option<String> {
    match item {
        Item::Command(m) if technical(s) => Some(display_id(&META[*m])),
        _ => None,
    }
}

const WIDTH: f32 = 640.0;
const HEADER: f32 = 46.0;
const ROW: f32 = 30.0;
const DESC: f32 = 15.0;
const SECTION: f32 = 26.0;
const FOOTER: f32 = 30.0;
const LIST_MAX: f32 = 404.0;

fn title_font() -> FontId {
    theme::proportional(12.5)
}

fn dim_font() -> FontId {
    theme::proportional(11.0)
}

/// One line of text, cut with "…" to `width`.
fn line(ui: &Ui, text: &str, font: FontId, colour: Color32, width: f32) -> Arc<Galley> {
    let mut job = egui::text::LayoutJob::single_section(text.to_owned(), egui::TextFormat::simple(font, colour));
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(8.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.fonts_mut(|f| f.layout_job(job))
}

/// A key cap: "⌘K", "↵".
fn chip(painter: &egui::Painter, right: f32, cy: f32, text: &str, c: &Tokens) -> f32 {
    let g = painter.layout_no_wrap(text.to_owned(), theme::proportional(10.5), c.text2);
    let w = (g.size().x + 10.0).max(20.0);
    let r = Rect::from_min_size(pos2(right - w, cy - 9.0), vec2(w, 18.0));
    painter.rect_filled(r, 4.0, c.chip);
    painter.rect_stroke(r, 4.0, egui::Stroke::new(1.0, c.pal_border.gamma_multiply(0.6)), egui::StrokeKind::Inside);
    painter.galley(pos2(r.center().x - g.size().x / 2.0, cy - g.size().y / 2.0), g, c.text2);
    w
}

pub fn show(ctx: &egui::Context, s: &mut Studio) {
    if !s.ui.palette.open {
        return;
    }
    // Esc goes back one step: form → list, query → empty, group → top, top → closed.
    if !egui::Popup::is_any_open(ctx) && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        let p = &mut s.ui.palette;
        if p.chosen.is_some() {
            p.back_to_list();
        } else if !p.query.is_empty() {
            p.query.clear();
            p.cursor = 0;
            p.focus_search = true;
        } else if p.group.is_some() {
            p.group = None;
            p.cursor = 0;
            p.focus_search = true;
        } else {
            p.close();
            return;
        }
    }
    let c = theme::tokens(ctx.theme() == egui::Theme::Dark);
    let screen = ctx.content_rect();
    let width = WIDTH.min(screen.width() - 32.0);
    let top = (screen.height() * 0.11).clamp(24.0, 120.0);
    let id = egui::Id::unique("palette");
    let frame = egui::Frame::NONE
        .fill(c.pal_bg)
        .stroke(egui::Stroke::new(1.0, c.pal_border))
        .corner_radius(10.0)
        .shadow(egui::Shadow { offset: [0, 14], blur: 36, spread: 0, color: c.shadow });
    let modal = egui::Modal::new(id)
        .area(egui::Modal::default_area(id).anchor(Align2::CENTER_TOP, vec2(0.0, top)))
        .frame(frame)
        .backdrop_color(c.shadow.gamma_multiply(0.35))
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            match s.ui.palette.chosen.clone() {
                None => list(ui, s, &c),
                Some(name) => form(ui, s, &name, &c),
            }
        });
    if modal.backdrop_response.clicked() && s.ui.palette.open {
        s.ui.palette.close();
    }
}

/// Header: search icon, breadcrumb, the query field (or the command's title in a form).
fn header(
    ui: &mut Ui,
    s: &mut Studio,
    c: &Tokens,
    crumbs: &[(&'static str, String)],
    field: bool,
) -> Option<egui::Response> {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADER), Sense::hover());
    let p = ui.painter().clone();
    let mut x = rect.left() + 16.0;
    crate::icons::paint_with(
        &p,
        ui.ctx(),
        Rect::from_center_size(pos2(x + 8.0, rect.center().y), vec2(16.0, 16.0)),
        "search",
        16.0,
        c.text2,
    );
    x += 28.0;
    let n = crumbs.len();
    for (k, (icon, text)) in crumbs.iter().enumerate() {
        let last_in_form = !field && k + 1 == n;
        let font = if last_in_form { theme::medium(13.0) } else { theme::proportional(11.5) };
        let colour = if last_in_form { c.text } else { c.text2 };
        let g = p.layout_no_wrap(text.clone(), font, colour);
        let w = g.size().x + 30.0;
        let r = Rect::from_min_size(pos2(x, rect.center().y - 12.0), vec2(w, 24.0));
        if !last_in_form {
            p.rect_filled(r, 6.0, c.chip);
        }
        crate::icons::paint_with(
            &p,
            ui.ctx(),
            Rect::from_center_size(pos2(r.left() + 13.0, r.center().y), vec2(16.0, 16.0)),
            icon,
            16.0,
            colour,
        );
        p.galley(pos2(r.left() + 24.0, r.center().y - g.size().y / 2.0), g, colour);
        x = r.right() + 4.0;
        if k + 1 < n {
            crate::icons::paint_with(
                &p,
                ui.ctx(),
                Rect::from_center_size(pos2(x + 6.0, rect.center().y), vec2(12.0, 12.0)),
                "chevron-right",
                12.0,
                c.text3,
            );
            x += 16.0;
        } else {
            x += 4.0;
        }
    }
    p.hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0, c.pal_border.gamma_multiply(0.7)));
    if !field {
        return None;
    }
    let pal = &mut s.ui.palette;
    let hint = match (mode(&pal.query).0, pal.group) {
        (Mode::Commands, _) => t("palette.hint_commands").to_owned(),
        (Mode::Objects, _) => t("palette.hint_objects").to_owned(),
        (Mode::Tags, _) => t("palette.hint_tags").to_owned(),
        (Mode::Normal, Some(g)) => tf("palette.hint_group", &[("group", &g.title())]),
        (Mode::Normal, None) => t("palette.hint_root").to_owned(),
    };
    let field_rect = Rect::from_min_max(pos2(x, rect.top() + 8.0), pos2(rect.right() - 14.0, rect.bottom() - 8.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new().max_rect(field_rect).layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let r = child.add(
        egui::TextEdit::singleline(&mut pal.query)
            .id(egui::Id::unique("palette_search"))
            .frame(egui::Frame::NONE)
            .font(theme::proportional(14.0))
            .hint_text(egui::RichText::new(hint).color(c.text3))
            .desired_width(field_rect.width()),
    );
    widgets::name(&r, Role::TextInput, "Search commands");
    if pal.focus_search {
        r.request_focus();
        pal.focus_search = false;
    }
    Some(r)
}

fn section_title(s: &Studio, sec: Section, c: &Context) -> String {
    match sec {
        Section::Recent => t("palette.recent").to_owned(),
        Section::Suggested => tf("palette.suggested", &[("what", &selection_words(c))]),
        Section::Groups => t("palette.groups").to_owned(),
        Section::Commands => match s.ui.palette.group {
            Some(g) if s.ui.palette.query.trim().is_empty() => g.title().to_owned(),
            _ => t("palette.commands").to_owned(),
        },
        Section::Objects => t("palette.objects").to_owned(),
        Section::Places => t("palette.places").to_owned(),
    }
}

/// Icon, title, dim note and accessible name of a row.
fn row_text(s: &Studio, row: &Row) -> (&'static str, String, String, String) {
    let doc = s.engine.doc();
    match row.item {
        Item::Group(g) => {
            let n = META.iter().filter(|m| m.group == g && !m.hidden).count();
            (g.icon(), g.title().to_owned(), tf("palette.count", &[("n", &n)]), format!("Group {}", g.title()))
        }
        Item::Command(i) => {
            let m = &META[i];
            (m.icon, m.title().to_owned(), m.group.title().to_owned(), m.title().to_owned())
        }
        Item::Object(id) => {
            let Some(o) = doc.objects.get(&id) else { return ("box", String::new(), String::new(), String::new()) };
            let kind = o.kind.type_name();
            let icon = match kind {
                "truss" | "truss_node" => "truss",
                "truss_run" => "run",
                "fixture" | "batten" | "curtain" | "platform" | "support" | "hoist" | "ceiling" | "floor" | "wall" => {
                    crate::icons::names().find(|n| *n == kind).unwrap_or("box")
                }
                "proscenium" => "portal",
                "wing_set" | "masking" => "wings",
                "underlay" => "image",
                "note" => "sticky-note",
                _ => "box",
            };
            let mut note = t(&format!("type.{kind}")).to_owned();
            if let Some(f) = fid_of(o) {
                note = format!("{note} · {}", tf("palette.fid", &[("fid", &f)]));
            }
            if let Some(l) = doc.layers.get(&o.layer) {
                note = format!("{note} · {}", l.name);
            }
            let title = if o.name.is_empty() { t(&format!("type.{kind}")).to_owned() } else { o.name.clone() };
            (icon, title.clone(), note, format!("Object {title}"))
        }
        Item::Layer(id) => {
            let name = doc.layers.get(&id).map(|l| l.name.clone()).unwrap_or_default();
            ("layers", name.clone(), t("palette.layer").to_owned(), format!("Layer {name}"))
        }
        Item::Tag(id) => {
            let name = doc.tags.get(&id).map(|tg| tg.name.clone()).unwrap_or_default();
            let n = doc.objects.values().filter(|o| o.tags.contains(&id)).count();
            ("tag", name.clone(), tf("palette.tag_n", &[("n", &n)]), format!("Tag {name}"))
        }
        Item::Sheet(id) => {
            let (num, name) = doc.sheets.get(&id).map(|sh| (sh.number.clone(), sh.name.clone())).unwrap_or_default();
            ("sheet", format!("{num} {name}").trim().to_owned(), t("palette.sheet").to_owned(), format!("Sheet {num}"))
        }
    }
}

fn list(ui: &mut Ui, s: &mut Studio, c: &Tokens) {
    let mut rows = rows(s);
    // Keys first, before the query field takes them.
    let cur = s.ui.palette.cursor.min(rows.len().saturating_sub(1));
    let on_group = matches!(rows.get(cur).map(|r| r.item), Some(Item::Group(_)));
    let empty = s.ui.palette.query.is_empty();
    let in_group = s.ui.palette.group.is_some();
    let (down, up, right, back) = ui.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            on_group && i.consume_key(Modifiers::NONE, Key::ArrowRight),
            empty
                && in_group
                && (i.consume_key(Modifiers::NONE, Key::Backspace) || i.consume_key(Modifiers::NONE, Key::ArrowLeft)),
        )
    });
    {
        let p = &mut s.ui.palette;
        if down && !rows.is_empty() {
            p.cursor = (cur + 1) % rows.len();
            p.scroll = true;
        }
        if up && !rows.is_empty() {
            p.cursor = (cur + rows.len() - 1) % rows.len();
            p.scroll = true;
        }
        if back {
            p.group = None;
            p.cursor = 0;
            p.error = None;
        }
    }
    if right && let Some(r) = rows.get(cur).cloned() {
        activate(s, &r);
        return;
    }
    let crumbs: Vec<(&'static str, String)> = match s.ui.palette.group {
        Some(g) => vec![(g.icon(), g.title().to_owned())],
        None => Vec::new(),
    };
    let before = s.ui.palette.query.clone();
    let r = header(ui, s, c, &crumbs, true);
    if s.ui.palette.query != before || back {
        s.ui.palette.cursor = 0;
        s.ui.palette.error = None;
        rows = self::rows(s);
    }
    let enter = r.is_some_and(|r| r.lost_focus()) && ui.input(|i| i.key_pressed(Key::Enter))
        || ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.any());
    let ctxt = Context::of(s);
    let cursor = s.ui.palette.cursor.min(rows.len().saturating_sub(1));
    let flat = !s.ui.palette.query.trim().is_empty();
    let mut picked: Option<Row> = None;
    let mut hover_to: Option<usize> = None;
    let scroll = std::mem::take(&mut s.ui.palette.scroll);
    let moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);

    ui.add_space(4.0);
    if rows.is_empty() {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::hover());
        ui.painter().text(r.center(), Align2::CENTER_CENTER, t("palette.nothing"), title_font(), c.text3);
    }
    egui::ScrollArea::vertical().max_height(LIST_MAX).min_scrolled_height(LIST_MAX).auto_shrink([false, true]).show(
        ui,
        |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let mut last: Option<Section> = None;
            for (i, row) in rows.iter().enumerate() {
                if last != Some(row.section) {
                    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), SECTION), Sense::hover());
                    ui.painter().text(
                        pos2(r.left() + 18.0, r.bottom() - 8.0),
                        Align2::LEFT_BOTTOM,
                        section_title(s, row.section, &ctxt),
                        theme::medium(10.5),
                        c.text3,
                    );
                    last = Some(row.section);
                }
                let on = i == cursor;
                let (icon, title, note, accessible) = row_text(s, row);
                let desc = match row.item {
                    Item::Command(m) if on => {
                        let d = META[m].summary();
                        (!d.is_empty()).then_some(d)
                    }
                    _ => None,
                };
                let h = ROW + if desc.is_some() { DESC } else { 0.0 };
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
                widgets::name(&resp, Role::Button, accessible);
                if on && scroll {
                    ui.scroll_to_rect(rect, None);
                }
                if resp.hovered() && moved && !on {
                    hover_to = Some(i);
                }
                if resp.clicked() {
                    picked = Some(row.clone());
                }
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                let p = ui.painter();
                let inner = rect.shrink2(vec2(8.0, 1.0));
                if on {
                    p.rect_filled(inner, 6.0, c.pal_row);
                }
                let blocked = row.blocked.is_some();
                let (fg, fg2) = if blocked { (c.text3, c.text3) } else { (c.text, c.text2) };
                let cy = inner.top() + ROW / 2.0 - 1.0;
                let icon_col = if blocked {
                    c.text3
                } else if on {
                    c.accent
                } else {
                    c.icon
                };
                crate::icons::paint_with(
                    p,
                    ui.ctx(),
                    Rect::from_center_size(pos2(inner.left() + 18.0, cy), vec2(16.0, 16.0)),
                    icon,
                    16.0,
                    icon_col,
                );
                let text_x = inner.left() + 38.0;
                // Right side: chevron for groups, the shortcut chip, the reason it is greyed, the ID.
                let mut right = inner.right() - 10.0;
                match row.item {
                    Item::Group(_) => {
                        crate::icons::paint_with(
                            p,
                            ui.ctx(),
                            Rect::from_center_size(pos2(right - 6.0, cy), vec2(12.0, 12.0)),
                            "chevron-right",
                            12.0,
                            c.text3,
                        );
                        right -= 22.0;
                    }
                    Item::Command(m) => {
                        if let Some(k) = META[m].shortcut {
                            right -= chip(p, right, cy, k, c) + 8.0;
                        }
                    }
                    _ => {}
                }
                if let Some(why) = &row.blocked {
                    let g = line(ui, why, dim_font(), c.text3, 200.0);
                    right -= g.size().x;
                    p.galley(pos2(right, cy - g.size().y / 2.0), g, c.text3);
                    right -= 12.0;
                } else if let Some(id) = shown_id(s, &row.item) {
                    let g = p.layout_no_wrap(id, FontId::monospace(10.0), if on { c.text2 } else { c.text3 });
                    right -= g.size().x;
                    p.galley(pos2(right, cy - g.size().y / 2.0), g, c.text3);
                    right -= 12.0;
                }
                let tg = line(ui, &title, title_font(), fg, (right - text_x).max(40.0));
                let tw = tg.size().x;
                p.galley(pos2(text_x, cy - tg.size().y / 2.0), tg, fg);
                // The group as a dim breadcrumb while searching, the count for groups, kind and FID for objects.
                let show_note = match row.item {
                    Item::Command(_) => flat || matches!(row.section, Section::Recent | Section::Suggested),
                    _ => true,
                };
                if show_note && !note.is_empty() && right - (text_x + tw + 10.0) > 30.0 {
                    let ng = line(ui, &note, dim_font(), c.text3, right - (text_x + tw + 10.0));
                    p.galley(pos2(text_x + tw + 10.0, cy - ng.size().y / 2.0 + 0.5), ng, c.text3);
                }
                if let Some(d) = desc {
                    let dg = line(ui, d, dim_font(), fg2, inner.right() - 12.0 - text_x);
                    p.galley(pos2(text_x, cy + ROW / 2.0 - 4.0), dg, fg2);
                }
            }
            ui.add_space(4.0);
        },
    );
    if let Some(i) = hover_to {
        s.ui.palette.cursor = i;
    }
    footer(ui, s, c, &rows.get(cursor).map(|r| r.item));
    if enter && picked.is_none() {
        picked = rows.get(cursor).cloned();
    }
    if let Some(row) = picked {
        activate(s, &row);
    }
}

fn footer(ui: &mut Ui, s: &Studio, c: &Tokens, current: &Option<Item>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), FOOTER), Sense::hover());
    let p = ui.painter();
    let bg = Rect::from_min_max(rect.min, rect.max);
    p.rect_filled(bg, egui::CornerRadius { nw: 0, ne: 0, sw: 10, se: 10 }, c.pal_footer);
    p.hline(rect.x_range(), rect.top() + 0.5, egui::Stroke::new(1.0, c.pal_border.gamma_multiply(0.7)));
    let cy = rect.center().y;
    if let Some(e) = &s.ui.palette.error {
        let g = line(ui, e, dim_font(), c.warning, rect.width() - 32.0);
        p.galley(pos2(rect.left() + 16.0, cy - g.size().y / 2.0), g, c.warning);
        return;
    }
    let mut x = rect.left() + 14.0;
    let enter_label = match current {
        Some(Item::Group(_)) => t("palette.foot.open"),
        Some(Item::Command(i))
            if !fields_for(META[*i].id, &schema_of(META[*i].id)).is_empty() && META[*i].args.is_none() =>
        {
            t("palette.foot.open")
        }
        Some(Item::Command(_)) => t("palette.foot.run"),
        Some(_) => t("palette.foot.go"),
        None => t("palette.foot.open"),
    };
    let back_or_close = if s.ui.palette.group.is_some() || !s.ui.palette.query.is_empty() {
        t("palette.foot.back")
    } else {
        t("palette.foot.close")
    };
    let mut hints: Vec<(Vec<&str>, &str)> =
        vec![(vec!["↑", "↓"], t("palette.foot.navigate")), (vec!["↵"], enter_label)];
    if s.ui.palette.group.is_some() && s.ui.palette.query.is_empty() {
        hints.push((vec!["←", "esc"], back_or_close));
    } else {
        hints.push((vec!["esc"], back_or_close));
    }
    for (keys, what) in hints {
        for key in keys {
            let w = chip_left(p, x, cy, key, c);
            x += w + 3.0;
        }
        x += 2.0;
        let g = p.layout_no_wrap(what.to_owned(), dim_font(), c.text3);
        let gw = g.size().x;
        p.galley(pos2(x, cy - g.size().y / 2.0), g, c.text3);
        x += gw + 14.0;
    }
    let g = p.layout_no_wrap(t("palette.foot.prefixes").to_owned(), dim_font(), c.text3);
    if rect.right() - 14.0 - g.size().x > x {
        p.galley(pos2(rect.right() - 14.0 - g.size().x, cy - g.size().y / 2.0), g, c.text3);
    }
}

fn chip_left(p: &egui::Painter, x: f32, cy: f32, text: &str, c: &Tokens) -> f32 {
    let g = p.layout_no_wrap(text.to_owned(), theme::proportional(10.0), c.text2);
    let w = (g.size().x + 8.0).max(18.0);
    let r = Rect::from_min_size(pos2(x, cy - 8.0), vec2(w, 16.0));
    p.rect_filled(r, 4.0, c.chip);
    p.galley(pos2(r.center().x - g.size().x / 2.0, cy - g.size().y / 2.0), g, c.text2);
    w
}

// ---------------------------------------------------------------- the form

const LABEL_W: f32 = 132.0;
const FIELD_H: f32 = 24.0;

fn form(ui: &mut Ui, s: &mut Studio, name: &str, c: &Tokens) {
    let meta = s.ui.palette.chosen_meta.map(|i| &META[i]);
    let title = meta.map_or_else(|| name.to_owned(), |m| m.title().trim_end_matches('…').to_owned());
    let mut crumbs: Vec<(&'static str, String)> = Vec::new();
    if let Some(m) = meta {
        crumbs.push((m.group.icon(), m.group.title().to_owned()));
        crumbs.push((m.icon, title.clone()));
    } else {
        crumbs.push(("terminal", title.clone()));
    }
    header(ui, s, c, &crumbs, false);
    let ids = technical(s);
    let specs: Vec<FieldSpec> = fields_for(name, &schema_of(name));
    let summary = meta.map_or_else(|| stagecraft_core::registry().get(name).map_or("", |e| e.summary), |m| m.summary());
    egui::Frame::NONE.inner_margin(egui::Margin { left: 18, right: 18, top: 10, bottom: 12 }).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(egui::RichText::new(summary).font(dim_font()).color(c.text2)).wrap());
            if ids {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(name).font(FontId::monospace(10.0)).color(c.text3));
                });
            }
        });
        ui.add_space(4.0);
        let (simple, advanced): (Vec<&FieldSpec>, Vec<&FieldSpec>) =
            specs.iter().partition(|f| f.required || !f.is_advanced());
        let mut first = s.ui.palette.focus_form;
        egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
            for f in &simple {
                field_row(ui, s, f, &mut first, c);
            }
            if !advanced.is_empty() {
                let r = egui::CollapsingHeader::new(egui::RichText::new(t("palette.form.more")).color(c.text2))
                    .id_salt(("palette_more", name))
                    .default_open(false)
                    .show(ui, |ui| {
                        for f in &advanced {
                            field_row(ui, s, f, &mut first, c);
                        }
                    });
                widgets::name(&r.header_response, Role::Button, "More options");
            }
        });
        s.ui.palette.focus_form = false;
    });
    // Footer: the error or the keys, the Back and Run buttons.
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), FOOTER + 8.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, egui::CornerRadius { nw: 0, ne: 0, sw: 10, se: 10 }, c.pal_footer);
    p.hline(rect.x_range(), rect.top() + 0.5, egui::Stroke::new(1.0, c.pal_border.gamma_multiply(0.7)));
    let cy = rect.center().y;
    let mut back = false;
    let mut run = ui.input(|i| i.key_pressed(Key::Enter)) && !egui::Popup::is_any_open(ui.ctx());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(12.0, 5.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    let run_btn = child.add(
        egui::Button::new(egui::RichText::new(t("palette.run")).color(c.on_accent))
            .fill(c.accent)
            .min_size(vec2(64.0, 24.0)),
    );
    widgets::name(&run_btn, Role::Button, "Run");
    run |= run_btn.clicked();
    let back_btn = child.add(egui::Button::new(t("palette.back")).min_size(vec2(56.0, 24.0)));
    widgets::name(&back_btn, Role::Button, "Back");
    back |= back_btn.clicked();
    let left = rect.left() + 16.0;
    match &s.ui.palette.error {
        Some(e) => {
            let g = line(ui, e, dim_font(), c.warning, back_btn.rect.left() - left - 12.0);
            p.galley(pos2(left, cy - g.size().y / 2.0), g, c.warning);
        }
        None => {
            let mut x = left;
            for (key, what) in [("↵", t("palette.foot.run")), ("esc", t("palette.foot.back"))] {
                x += chip_left(&p, x, cy, key, c) + 5.0;
                let g = p.layout_no_wrap(what.to_owned(), dim_font(), c.text3);
                let w = g.size().x;
                p.galley(pos2(x, cy - g.size().y / 2.0), g, c.text3);
                x += w + 14.0;
            }
        }
    }
    if back {
        s.ui.palette.back_to_list();
        return;
    }
    if run {
        match build_args(&specs, &s.ui.palette.values) {
            Ok(args) => run_named(s, name, s.ui.palette.chosen_meta, args),
            Err(e) => s.ui.palette.error = Some(e),
        }
    }
}

/// One labelled field.
fn field_row(ui: &mut Ui, s: &mut Studio, f: &FieldSpec, first: &mut bool, c: &Tokens) {
    let doc = s.engine.doc().clone();
    let sel = s.engine.selection().to_vec();
    ui.horizontal(|ui| {
        ui.set_min_height(FIELD_H);
        let label = crate::schema_form::label(&f.name);
        ui.allocate_ui_with_layout(vec2(LABEL_W, FIELD_H), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.set_min_size(vec2(LABEL_W, FIELD_H));
            let colour = if f.required { c.text } else { c.text2 };
            let r = ui.add(egui::Label::new(egui::RichText::new(&label).color(colour)).truncate());
            if !f.description.is_empty() {
                r.on_hover_text(&f.description);
            }
        });
        let accessible = format!("Argument {}", f.name);
        let unit_w = if f.unit.is_some() { 26.0 } else { 0.0 };
        let w = ui.available_width() - unit_w;
        let value = s.ui.palette.values.entry(f.name.clone()).or_default();
        let updates = s.ui.palette.chosen.as_deref().is_some_and(|n| n.ends_with(".update") || n == "doc.set_meta");
        let hint = if updates && f.nullable && !f.required { t("palette.form.unchanged") } else { "" };
        let text_field = |ui: &mut Ui, value: &mut String, w: f32, hint: &str, name: &str, first: &mut bool| {
            let id = egui::Id::unique(("palette_field", name));
            let r = ui.add_sized(
                vec2(w, FIELD_H),
                egui::TextEdit::singleline(value)
                    .id(id)
                    .hint_text(egui::RichText::new(hint).color(c.text3))
                    .align(Align2::LEFT_CENTER),
            );
            widgets::name(&r, Role::TextInput, name);
            if *first {
                r.request_focus();
                *first = false;
            }
            // Typing replaces a prefilled value.
            if r.gained_focus() {
                widgets::select_all(ui.ctx(), id, value);
            }
            r
        };
        match &f.kind {
            FieldKind::Bool => {
                let mut on = value == "true";
                let r = ui.checkbox(&mut on, "");
                widgets::name(&r, Role::CheckBox, &accessible);
                *value = on.to_string();
            }
            FieldKind::Choice(options) => {
                let shown = if value.is_empty() { t("palette.form.none").to_owned() } else { value.clone() };
                let r = widgets::combo_w(w, ("palette_choice", &f.name)).selected_text(shown).show_ui(ui, |ui| {
                    if !f.required && ui.selectable_label(value.is_empty(), t("palette.form.none")).clicked() {
                        value.clear();
                    }
                    for o in options {
                        if ui.selectable_label(value == o, o).clicked() {
                            *value = o.clone();
                        }
                    }
                });
                widgets::name(&r.response, Role::ComboBox, &accessible);
            }
            FieldKind::Ref(kind) => {
                let options = crate::schema_form::options(&doc, *kind);
                picker(ui, value, &options, f.required, w, &accessible, &f.name);
            }
            FieldKind::Object => {
                let options =
                    crate::schema_form::object_options(&doc, &f.name, if f.name == "parent" { &sel } else { &[] });
                picker(ui, value, &options, f.required, w, &accessible, &f.name);
            }
            FieldKind::Objects => {
                let n = value.split(',').filter(|x| !x.trim().is_empty()).count();
                let (text, colour) = if n == 0 {
                    (t("palette.need.objects").to_owned(), c.warning)
                } else {
                    let names: Vec<String> = value
                        .split(',')
                        .filter_map(|x| x.trim().parse::<Id>().ok())
                        .take(3)
                        .map(|id| crate::schema_form::object_label(&doc, id))
                        .collect();
                    let more = if n > 3 { "…" } else { "" };
                    (format!("{} — {}{more}", tf("palette.form.selection", &[("n", &n)]), names.join(", ")), c.text2)
                };
                let r = ui
                    .allocate_ui_with_layout(vec2(w, FIELD_H), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.set_min_size(vec2(w, FIELD_H));
                        ui.add_space(4.0);
                        ui.add(egui::Label::new(egui::RichText::new(text).color(colour)).truncate())
                    })
                    .inner;
                widgets::name(&r, Role::Label, &accessible);
            }
            FieldKind::Point(n) => {
                let mut parts: Vec<String> = value.split(',').map(|x| x.trim().to_owned()).collect();
                parts.resize(*n, String::new());
                let axes = ["X", "Y", "Z"];
                let each = (w - 14.0 * *n as f32 - 4.0 * (*n as f32 - 1.0)) / *n as f32;
                ui.spacing_mut().item_spacing.x = 4.0;
                for (k, part) in parts.iter_mut().enumerate() {
                    ui.label(egui::RichText::new(axes[k]).font(dim_font()).color(c.text3));
                    text_field(ui, part, each, "", &format!("{accessible} {}", axes[k].to_lowercase()), first);
                }
                *value = parts.join(", ");
            }
            FieldKind::List => {
                text_field(ui, value, w, t("palette.list_hint"), &accessible, first);
            }
            FieldKind::Json => {
                text_field(ui, value, w, t("palette.json_hint"), &accessible, first);
            }
            _ => {
                text_field(ui, value, w, hint, &accessible, first);
            }
        }
        if let Some(u) = f.unit {
            ui.label(egui::RichText::new(u).font(dim_font()).color(c.text3));
        }
    });
}

/// A drop-down of (value, label) options; optional ones can be left empty.
fn picker(
    ui: &mut Ui,
    value: &mut String,
    options: &[(String, String)],
    required: bool,
    w: f32,
    accessible: &str,
    salt: &str,
) {
    let shown = options
        .iter()
        .find(|(v, _)| v == value)
        .map(|o| o.1.clone())
        .unwrap_or_else(|| if value.is_empty() { t("palette.form.none").to_owned() } else { value.clone() });
    let r = widgets::combo_w(w, ("palette_pick", salt)).selected_text(shown).show_ui(ui, |ui| {
        if !required && ui.selectable_label(value.is_empty(), t("palette.form.none")).clicked() {
            value.clear();
        }
        for (v, l) in options {
            if ui.selectable_label(value == v, l).clicked() {
                *value = v.clone();
            }
        }
    });
    widgets::name(&r.response, Role::ComboBox, accessible);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn top(query: &str) -> Vec<&'static str> {
        search_commands(query, false, None).into_iter().take(5).map(|(i, _)| META[i].key_str().leak() as &str).collect()
    }

    #[test]
    fn titles_aliases_and_ids_find_commands() {
        assert!(top("create truss").contains(&"truss.create"), "{:?}", top("create truss"));
        assert!(top("traverse").iter().any(|k| k.starts_with("truss.")), "{:?}", top("traverse"));
        assert!(top("zug").iter().any(|k| k.contains("batten")), "{:?}", top("zug"));
        assert!(top("lattenzug").iter().any(|k| k.contains("batten")));
        assert!(top("podest").iter().any(|k| k.contains("platform")));
        assert!(
            top("plankopf").iter().any(|k| k.contains("titleblock") || k.contains("set_meta")),
            "{:?}",
            top("plankopf")
        );
        assert!(top("klasse").iter().any(|k| k.contains("tag")), "{:?}", top("klasse"));
        assert!(top("scheinwerfer").iter().any(|k| k.contains("fixture")));
        assert!(top("auto patch").contains(&"patch.auto"));
        assert!(top("patch.auto").contains(&"patch.auto"));
        assert!(top("export pdf").contains(&"app.export_pdf"), "{:?}", top("export pdf"));
        // Technical commands stay out unless asked for with `>`.
        assert!(!search_commands("batch", false, None).iter().any(|(i, _)| META[*i].id == "batch"));
        assert_eq!(META[search_commands("batch", true, None)[0].0].id, "batch");
    }

    #[test]
    fn names_sort_as_people_read_them() {
        let mut v = vec!["Batten 10", "Batten 2", "batten 1", "Apron"];
        v.sort_by(|a, b| natural(a, b));
        assert_eq!(v, ["Apron", "batten 1", "Batten 2", "Batten 10"]);
    }

    #[test]
    fn prefixes() {
        assert_eq!(mode("> ui.lay"), (Mode::Commands, "ui.lay"));
        assert_eq!(mode("@101"), (Mode::Objects, "101"));
        assert_eq!(mode("#light"), (Mode::Tags, "light"));
        assert_eq!(mode("truss"), (Mode::Normal, "truss"));
    }
}
