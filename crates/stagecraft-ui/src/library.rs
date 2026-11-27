//! The library panel: the document's fixture types (pick one for the fixture tool,
//! assign plan symbols, import files) and GDTF Share (search the cached list,
//! download and import into the document).
//!
//! GDTF Share requests run on worker threads through a [`ShareBackend`]; the UI
//! only polls a channel, so a slow or blocked network never stalls a frame. Tests
//! swap in a mock backend.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{RichText, Role, Sense, Ui, vec2};
use serde_json::json;
use stagecraft_core::{Document, Id, Object, ObjectKind};
use stagecraft_gdtf::share::{self, ShareClient, ShareEntry, ShareError};

use crate::app::Studio;
use crate::i18n::{t, tf};
use crate::listview::{self, Col};
use crate::widgets;

/// Where GDTF Share data comes from.
pub trait ShareBackend: Send {
    /// The fixture list. `refresh` asks the server for a new one; otherwise a cached
    /// list is fine.
    fn list(&mut self, refresh: bool) -> Result<Vec<ShareEntry>, String>;
    /// Download a revision; returns the path of the `.gdtf` on disk.
    fn download(&mut self, entry: &ShareEntry) -> Result<PathBuf, String>;
}

/// The real GDTF Share with its disk cache and the keychain login.
pub struct LiveShare {
    client: ShareClient,
    login: Option<Result<(), String>>,
}

impl LiveShare {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { client: ShareClient::new(cache_dir), login: None }
    }

    /// `<data dir>/StageCraft/gdtf-share`.
    pub fn default_cache_dir() -> PathBuf {
        stagecraft_control::discovery::base_dir().join("gdtf-share")
    }

    fn ensure_login(&mut self) -> Result<(), String> {
        if self.client.is_logged_in() {
            return Ok(());
        }
        // Ask the keychain once per session; a refusal is remembered.
        if self.login.is_none() {
            self.login = Some(login(&mut self.client));
        }
        self.login.clone().unwrap_or(Ok(()))
    }
}

#[cfg(target_os = "macos")]
fn login(c: &mut ShareClient) -> Result<(), String> {
    c.login_from_keychain().map_err(|e| explain(&e))
}

#[cfg(not(target_os = "macos"))]
fn login(_: &mut ShareClient) -> Result<(), String> {
    Err(t("share.no_keychain").to_owned())
}

/// A user-facing message for a GDTF Share error.
pub fn explain(e: &ShareError) -> String {
    match e {
        ShareError::NoCredentials(_) => t("share.no_credentials").to_owned(),
        ShareError::Auth(m) => tf("share.auth_failed", &[("error", m)]),
        ShareError::Http(m) => tf("share.unreachable", &[("error", m)]),
        ShareError::Format(m) => tf("share.bad_answer", &[("error", m)]),
        ShareError::Io(m) => tf("share.cache_error", &[("error", m)]),
    }
}

impl ShareBackend for LiveShare {
    fn list(&mut self, refresh: bool) -> Result<Vec<ShareEntry>, String> {
        if !refresh && let Some(l) = self.client.cached_list() {
            return Ok(l);
        }
        if let Err(e) = self.ensure_login() {
            // An old list beats none.
            return self.client.cached_list().ok_or(e);
        }
        let age = if refresh { Duration::ZERO } else { Duration::from_secs(24 * 3600) };
        self.client.list(age).map_err(|e| explain(&e))
    }

    fn download(&mut self, entry: &ShareEntry) -> Result<PathBuf, String> {
        let cached = self.client.file_path(entry.rid);
        let have = std::fs::read(&cached).is_ok_and(|b| b.starts_with(b"PK"));
        if !have {
            self.ensure_login()?;
        }
        let bytes = self.client.download(entry.rid).map_err(|e| explain(&e))?;
        // Keep a copy under the GDTF Share file name, so the document shows a
        // readable file name for the asset.
        let dir = self.client.cache_dir().join("named");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(file_name(entry));
        stagecraft_core::file::write_atomic(&path, &bytes).map_err(|e| e.message)?;
        Ok(path)
    }
}

/// `Manufacturer@Fixture@Revision.gdtf` without path separators.
pub fn file_name(e: &ShareEntry) -> String {
    let clean = |s: &str| s.replace(['/', '\\', ':'], "_");
    if e.revision.is_empty() {
        format!("{}@{}.gdtf", clean(&e.manufacturer), clean(&e.fixture))
    } else {
        format!("{}@{}@{}.gdtf", clean(&e.manufacturer), clean(&e.fixture), clean(&e.revision))
    }
}

pub type SharedBackend = Arc<Mutex<Box<dyn ShareBackend>>>;

enum Done {
    List(Result<Vec<ShareEntry>, String>),
    Download(Result<PathBuf, String>),
}

pub struct Job {
    /// What the job does, for the status line.
    pub label: String,
    pub started: Instant,
    rx: Receiver<Done>,
}

#[derive(Default)]
pub struct LibraryState {
    pub query: String,
    /// Narrows the document's fixture types in the Library panel.
    pub filter: String,
    /// The GDTF Share revision whose details show under the list.
    pub selected: Option<u64>,
    pub list: Option<Arc<Vec<ShareEntry>>>,
    pub jobs: Vec<Job>,
    /// Last outcome; `true` = error.
    pub message: Option<(String, bool)>,
    backend: Option<SharedBackend>,
}

impl LibraryState {
    pub fn set_backend(&mut self, b: Box<dyn ShareBackend>) {
        self.backend = Some(Arc::new(Mutex::new(b)));
        self.list = None;
    }

    fn backend(&mut self) -> SharedBackend {
        self.backend
            .get_or_insert_with(|| Arc::new(Mutex::new(Box::new(LiveShare::new(LiveShare::default_cache_dir())))))
            .clone()
    }

    pub fn busy(&self) -> bool {
        !self.jobs.is_empty()
    }

    fn spawn(&mut self, label: String, f: impl FnOnce(&mut dyn ShareBackend) -> Done + Send + 'static) {
        let backend = self.backend();
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new().name("gdtf-share".into()).spawn(move || {
            let out = match backend.lock() {
                Ok(mut b) => f(b.as_mut()),
                Err(_) => Done::List(Err("GDTF Share worker crashed earlier".into())),
            };
            let _ = tx.send(out);
        });
        match spawned {
            Ok(_) => self.jobs.push(Job { label, started: Instant::now(), rx }),
            Err(e) => self.message = Some((e.to_string(), true)),
        }
    }

    /// Load the list (once, or again with `refresh`).
    pub fn load_list(&mut self, refresh: bool) -> bool {
        if self.jobs.iter().any(|j| j.label == t("share.loading")) || (self.list.is_some() && !refresh) {
            return false;
        }
        self.spawn(t("share.loading").to_owned(), move |b| Done::List(b.list(refresh)));
        true
    }

    /// Download one revision and import it when done.
    pub fn add(&mut self, entry: ShareEntry) {
        let label = tf("share.downloading", &[("name", &format!("{} {}", entry.manufacturer, entry.fixture))]);
        self.spawn(label, move |b| Done::Download(b.download(&entry)));
    }

    /// Search hits in the loaded list, newest revision first, at most `max`.
    pub fn hits(&self, max: usize) -> Vec<ShareEntry> {
        let Some(list) = &self.list else { return Vec::new() };
        if self.query.trim().is_empty() {
            return Vec::new();
        }
        let mut seen = std::collections::HashSet::new();
        share::search(list, &self.query)
            .into_iter()
            .filter(|e| seen.insert((e.manufacturer.clone(), e.fixture.clone())))
            .take(max)
            .cloned()
            .collect()
    }
}

/// Collect finished jobs; downloads go into the document through `gdtf.import`.
pub fn poll(s: &mut Studio) {
    let mut done = Vec::new();
    s.ui.library.jobs.retain(|j| match j.rx.try_recv() {
        Ok(d) => {
            done.push(d);
            false
        }
        Err(TryRecvError::Empty) => true,
        Err(TryRecvError::Disconnected) => {
            done.push(Done::List(Err(t("share.worker_gone").to_owned())));
            false
        }
    });
    for d in done {
        match d {
            Done::List(Ok(list)) => {
                let n = list.len();
                s.ui.library.list = Some(Arc::new(list));
                s.ui.library.message = Some((tf("share.list_loaded", &[("n", &n)]), false));
            }
            Done::List(Err(e)) | Done::Download(Err(e)) => s.ui.library.message = Some((e, true)),
            Done::Download(Ok(path)) => match s.run("gdtf.import", json!({ "path": path })) {
                Ok(v) => {
                    let key = v["key"].as_str().unwrap_or_default().to_owned();
                    s.ui.options.fixture_type = key.clone();
                    s.ui.options.fixture_mode.clear();
                    s.ui.library.message = Some((tf("share.imported", &[("name", &key)]), false));
                }
                Err(e) => s.ui.library.message = Some((e.message, true)),
            },
        }
    }
}

/// A fixture object of a type, for symbol lookups.
fn sample(key: &str) -> Object {
    Object {
        id: Id::nil(),
        name: String::new(),
        layer: Id::nil(),
        tags: Vec::new(),
        parent: None,
        placement: Default::default(),
        kind: ObjectKind::Fixture {
            fid: None,
            cid: None,
            gdtf: Some(key.to_owned()),
            mode: None,
            address: None,
            extra_addresses: Vec::new(),
            weight_kg: None,
        },
        style: Default::default(),
        attrs: Default::default(),
        sketch: None,
    }
}

/// Paint a type's plan symbol into `rect` in `fg`.
fn symbol_preview(ui: &Ui, doc: &Document, key: &str, rect: egui::Rect, fg: egui::Color32) {
    let Some(sym) = stagecraft_geom::gdtf::symbol(doc, &sample(key)) else { return };
    let (lo, hi) = sym.bounds;
    let k = ((rect.width() - 4.0) as f64 / (hi[0] - lo[0]).max(1e-6))
        .min((rect.height() - 4.0) as f64 / (hi[1] - lo[1]).max(1e-6));
    let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let c = rect.center();
    let to = |p: &[f64; 2]| egui::pos2(c.x + ((p[0] - mid[0]) * k) as f32, c.y - ((p[1] - mid[1]) * k) as f32);
    let bg = ui.visuals().extreme_bg_color;
    let p = ui.painter();
    for prim in &sym.prims {
        match prim {
            stagecraft_geom::gdtf::LocalPrim::Polygon { points, line, .. } => {
                let pts: Vec<egui::Pos2> = points.iter().map(to).collect();
                // A light fill under the outline (shapes are near convex at this size).
                p.add(egui::Shape::convex_polygon(pts.clone(), bg, egui::Stroke::NONE));
                if line.is_some() {
                    p.add(egui::Shape::closed_line(pts, egui::Stroke::new(1.0, fg)));
                }
            }
            stagecraft_geom::gdtf::LocalPrim::Polyline { points, closed, .. } => {
                let pts: Vec<egui::Pos2> = points.iter().map(to).collect();
                let st = egui::Stroke::new(0.8, fg);
                if *closed {
                    p.add(egui::Shape::closed_line(pts, st));
                } else {
                    p.add(egui::Shape::line(pts, st));
                }
            }
        }
    }
}

/// The bundled demo GDTFs into the document's library, one undo step.
fn add_demo_types(s: &mut Studio, files: &[&(&str, &[u8])]) {
    let steps: Vec<serde_json::Value> = files
        .iter()
        .map(|(name, bytes)| {
            let data = stagecraft_core::sheet::base64_encode(bytes);
            json!({ "command": "gdtf.import", "args": { "data_base64": data, "file_name": name } })
        })
        .collect();
    if s.run("batch", json!({ "commands": steps })).is_ok() {
        s.ui.info(tf("library.imported", &[("n", &files.len())]));
    }
}

/// The Library panel: the document's fixture types as a list grouped by manufacturer
/// (symbol, type, modes, weight, fixtures using it); a click picks the type for the fixture
/// tool, and the picked type's mode and symbol are set below the list.
pub fn show(ui: &mut Ui, s: &mut Studio) {
    let doc = s.engine.doc().clone();
    ui.horizontal(|ui| {
        let missing: Vec<_> = crate::demo::DEMO_GDTFS
            .iter()
            .filter(|(file, _)| !doc.fixture_types.contains_key(file.trim_end_matches(".gdtf")))
            .collect();
        let buttons = if missing.is_empty() { 70.0 } else { 180.0 };
        let r = ui.add(
            egui::TextEdit::singleline(&mut s.ui.library.filter)
                .hint_text(t("library.filter_hint"))
                .desired_width((ui.available_width() - buttons).max(60.0)),
        );
        widgets::name(&r, Role::TextInput, "Filter fixture types");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::button(ui, t("library.import"), "Import GDTF").on_hover_text(t("library.import_tip")).clicked()
            {
                s.ui.file_request = Some(crate::state::FileRequest::ImportGdtf);
            }
            if !missing.is_empty()
                && widgets::button(ui, t("library.demo"), "Demo fixtures")
                    .on_hover_text(t("library.demo_tip"))
                    .clicked()
            {
                add_demo_types(s, &missing);
            }
        });
    });
    ui.add_space(4.0);
    let cols = [
        Col::fixed("", 28.0),
        Col::fill(t("library.col_type")),
        Col::number(t("library.col_modes"), 54.0),
        Col::number(t("library.col_kg"), 44.0),
        Col::number(t("library.col_used"), 40.0),
    ];
    listview::header(ui, &cols);
    if doc.fixture_types.is_empty() {
        ui.label(RichText::new(t("library.empty")).weak());
        return;
    }
    let used = |key: &str| {
        doc.objects
            .values()
            .filter(|o| matches!(&o.kind, ObjectKind::Fixture { gdtf: Some(k), .. } if k == key))
            .count()
    };
    let filter = s.ui.library.filter.trim().to_lowercase();
    let mut types: Vec<_> = doc
        .fixture_types
        .values()
        .filter(|ft| filter.split_whitespace().all(|w| ft.display_name().to_lowercase().contains(w)))
        .collect();
    types.sort_by_key(|ft| (ft.manufacturer.to_lowercase(), ft.name.to_lowercase()));
    if types.is_empty() {
        listview::empty(ui, t("share.no_hits"));
    }
    let mut maker: Option<&str> = None;
    for ft in &types {
        if maker != Some(ft.manufacturer.as_str()) {
            maker = Some(ft.manufacturer.as_str());
            listview::group(ui, if ft.manufacturer.is_empty() { t("library.no_maker") } else { &ft.manufacturer });
        }
        let active = s.ui.options.fixture_type == ft.key;
        let (r, cells, ink) = listview::row(ui, &cols, active, &format!("Fixture type {}", ft.key));
        symbol_preview(ui, &doc, &ft.key, cells[0].shrink(2.0), ink);
        listview::cell(ui, cells[1], &ft.name, ink, false);
        listview::cell_dim(ui, cells[2], &ft.modes.len().to_string(), ink, active, true);
        let kg = ft.weight_kg.map_or_else(|| "–".to_owned(), crate::lengths::fmt_number);
        listview::cell_dim(ui, cells[3], &kg, ink, active, true);
        listview::cell_dim(ui, cells[4], &used(&ft.key).to_string(), ink, active, true);
        if r.on_hover_text(format!(
            "{}\n{}",
            if ft.long_name.is_empty() { &ft.name } else { &ft.long_name },
            t("library.pick_tip")
        ))
        .clicked()
        {
            let key = if active { "" } else { ft.key.as_str() };
            let _ = s.run("ui.set_tool_options", json!({ "fixture_type": key }));
            if !active {
                s.ui.set_tool(crate::state::Tool::Fixture);
            }
        }
    }
    let Some(ft) = doc.fixture_types.get(&s.ui.options.fixture_type) else { return };
    ui.add_space(6.0);
    widgets::section(ui, ft.display_name());
    egui::Grid::new(("library_type", &ft.key)).num_columns(2).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
        crate::widgets::prop_label(ui, t("inspector.mode"));
        let cur =
            ft.mode_or_first((!s.ui.options.fixture_mode.is_empty()).then_some(s.ui.options.fixture_mode.as_str()));
        let shown = cur.map(|m| format!("{} · {} ch", m.name, m.footprint_text())).unwrap_or_default();
        let r = crate::widgets::combo_w(ui.available_width() - 8.0, ("library_mode", &ft.key))
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for m in &ft.modes {
                    let label = format!("{} · {} ch", m.name, m.footprint_text());
                    if ui.selectable_label(cur.is_some_and(|c| c.name == m.name), label).clicked() {
                        let _ = s.run("ui.set_tool_options", json!({ "fixture_mode": m.name }));
                    }
                }
            });
        widgets::name(&r.response, egui::Role::ComboBox, format!("Mode {}", ft.key));
        ui.end_row();
        crate::widgets::prop_label(ui, t("library.symbol"));
        let auto = if ft.has_plan_svg { t("library.symbol_gdtf") } else { t("library.symbol_silhouette") };
        let shown = ft.symbol.clone().map_or_else(|| auto.to_owned(), |n| t(&format!("symbol.{n}")).to_owned());
        let r = crate::widgets::combo_w(ui.available_width() - 8.0, ("library_symbol", &ft.key))
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui.selectable_label(ft.symbol.is_none(), auto).clicked() {
                    let _ = s.run("symbol.assign", json!({ "gdtf_type": ft.key, "symbol": "" }));
                }
                for (name, _) in stagecraft_core::fixture_type::SYMBOLS {
                    let mut label = t(&format!("symbol.{name}")).to_owned();
                    if ft.suggested_symbol.as_deref() == Some(name) {
                        label.push_str(" ★");
                    }
                    if ui.selectable_label(ft.symbol.as_deref() == Some(name), label).clicked() {
                        let _ = s.run("symbol.assign", json!({ "gdtf_type": ft.key, "symbol": name }));
                    }
                }
            });
        widgets::name(&r.response, egui::Role::ComboBox, format!("Symbol {}", ft.key));
        ui.end_row();
        let u = crate::lengths::of(s);
        let info = |ui: &mut Ui, label: &str, value: String| {
            crate::widgets::prop_label(ui, label);
            ui.label(RichText::new(value).color(crate::theme::of(ui).text2));
            ui.end_row();
        };
        let weight = ft
            .weight_kg
            .map_or_else(|| t("library.no_weight").to_owned(), |w| format!("{} kg", crate::lengths::fmt_number(w)));
        info(ui, t("library.weight"), weight);
        if let Some(w) = ft.power_w {
            info(ui, t("library.power"), format!("{} W", crate::lengths::fmt_number(w)));
        }
        if ft.size_mm.iter().any(|v| *v > 0.0) {
            let [x, y, z] = ft.size_mm;
            info(ui, t("library.size"), format!("{} × {} × {}", u.fixed(x), u.fixed(y), u.with_unit(z)));
        }
    });
}

/// The GDTF Share panel: search the cached list, a list of hits (fixture, maker, last
/// change, add), and the picked revision's details below it.
pub fn share_panel(ui: &mut Ui, s: &mut Studio) {
    let mut search = false;
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(&mut s.ui.library.query)
                .hint_text(t("share.search_hint"))
                .desired_width((ui.available_width() - 100.0).max(80.0)),
        );
        widgets::name(&r, Role::TextInput, "Search GDTF Share");
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            search = true;
        }
        if widgets::button(ui, t("share.search"), "Search GDTF Share now").clicked() {
            search = true;
        }
        if crate::chrome::icon(ui, "Refresh GDTF Share list", "refresh-cw", 22.0, 16.0, false, t("share.refresh_tip"))
            .clicked()
        {
            s.ui.library.load_list(true);
        }
    });
    if search {
        s.ui.library.load_list(false);
    }
    // Status: running jobs, last message.
    let lib = &s.ui.library;
    for j in &lib.jobs {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new(format!("{} {} s", j.label, j.started.elapsed().as_secs())).small());
        });
    }
    if !lib.jobs.is_empty() {
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }
    if let Some((m, err)) = &lib.message {
        let c = crate::theme::of(ui);
        ui.label(RichText::new(m).small().color(if *err { c.warning } else { c.text2 }));
    }
    ui.add_space(4.0);
    let cols = [
        Col::fill(t("share.col_fixture")),
        Col::fixed(t("share.col_maker"), 84.0),
        Col::number(t("share.col_modes"), 54.0),
        Col::fixed("", 26.0),
    ];
    listview::header(ui, &cols);
    if lib.list.is_none() && lib.jobs.is_empty() {
        ui.label(RichText::new(t("share.hint")).weak().small());
        return;
    }
    let hits = lib.hits(200);
    if lib.list.is_some() && !lib.query.trim().is_empty() && hits.is_empty() {
        listview::empty(ui, t("share.no_hits"));
    }
    // In the library already: the same fixture type GUID, or the same maker and name.
    let owned: std::collections::HashSet<String> = s
        .engine
        .doc()
        .fixture_types
        .values()
        .flat_map(|ft| [ft.fixture_type_id.to_lowercase(), format!("{}@{}", ft.manufacturer, ft.name).to_lowercase()])
        .collect();
    let have = |e: &ShareEntry| {
        (!e.uuid.is_empty() && owned.contains(&e.uuid.to_lowercase()))
            || owned.contains(&format!("{}@{}", e.manufacturer, e.fixture).to_lowercase())
    };
    let mut add: Option<ShareEntry> = None;
    let mut pick: Option<u64> = None;
    for e in &hits {
        let selected = s.ui.library.selected == Some(e.rid);
        let (r, cells, ink) = listview::row(ui, &cols, selected, &format!("Share {} {}", e.manufacturer, e.fixture));
        listview::cell(ui, cells[0], &e.fixture, ink, false);
        listview::cell_dim(ui, cells[1], &e.manufacturer, ink, selected, false);
        listview::cell_dim(ui, cells[2], &e.modes.len().to_string(), ink, selected, true);
        let have = have(e);
        let b = cells[3].shrink2(vec2(3.0, 2.0));
        if have {
            let p = ui.painter();
            p.text(b.center(), egui::Align2::CENTER_CENTER, "✓", egui::FontId::proportional(crate::theme::BODY), ink);
        } else {
            let br = ui.interact(b, ui.scope_id().with(("share_add", e.rid)), Sense::click());
            widgets::name(&br, Role::Button, format!("Add {}", e.rid));
            if br.hovered() {
                ui.painter().rect_filled(b, 3.0, crate::theme::of(ui).pressed);
            }
            crate::icons::paint(ui, b, "plus", 12.0, ink);
            if br.on_hover_text(t("share.add_tip")).clicked() {
                add = Some(e.clone());
            }
        }
        if r.clicked() {
            pick = Some(e.rid);
        }
    }
    if let Some(rid) = pick {
        s.ui.library.selected = if s.ui.library.selected == Some(rid) { None } else { Some(rid) };
    }
    if let Some(e) = hits.iter().find(|e| Some(e.rid) == s.ui.library.selected) {
        ui.add_space(6.0);
        widgets::section(ui, format!("{} {}", e.manufacturer, e.fixture));
        egui::Grid::new(("share_entry", e.rid)).num_columns(2).spacing(vec2(8.0, 3.0)).show(ui, |ui| {
            // Rows without a value (an old upload has no creator or GDTF version) are left out.
            let info = |ui: &mut Ui, label: &str, value: String| {
                if value.trim().is_empty() {
                    return;
                }
                crate::widgets::prop_label(ui, label);
                ui.add(egui::Label::new(RichText::new(value).color(crate::theme::of(ui).text2)).wrap());
                ui.end_row();
            };
            info(ui, t("share.revision"), e.revision.clone());
            if e.last_modified > 0 {
                info(ui, t("share.changed"), listview::date(e.last_modified));
            }
            let modes: Vec<String> = e.modes.iter().map(|m| format!("{} ({} ch)", m.name, m.dmx_footprint)).collect();
            info(ui, t("library.modes_label"), modes.join("\n"));
            info(ui, t("share.gdtf_version"), e.version.clone());
            let by = if e.creator.is_empty() || e.creator == e.uploader {
                e.uploader.clone()
            } else {
                format!("{} / {}", e.creator, e.uploader)
            };
            info(ui, t("share.by"), by);
            if e.rating > 0.0 {
                info(ui, t("share.rating"), format!("{:.1} / 5", e.rating));
            }
            info(ui, t("share.file"), tf("share.size", &[("kb", &(e.filesize / 1024))]));
        });
        ui.add_space(4.0);
        if have(e) {
            ui.label(RichText::new(t("share.in_library")).color(crate::theme::of(ui).text2));
        } else if widgets::button(ui, t("share.add_to_library"), &format!("Add to library {}", e.rid))
            .on_hover_text(t("share.add_tip"))
            .clicked()
        {
            add = Some(e.clone());
        }
    }
    if let Some(e) = add {
        s.ui.library.add(e);
    }
}
