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

/// Paint a type's plan symbol into a small square.
fn symbol_preview(ui: &mut Ui, doc: &Document, key: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
    let Some(sym) = stagecraft_geom::gdtf::symbol(doc, &sample(key)) else { return };
    let (lo, hi) = sym.bounds;
    let k = ((rect.width() - 4.0) as f64 / (hi[0] - lo[0]).max(1e-6))
        .min((rect.height() - 4.0) as f64 / (hi[1] - lo[1]).max(1e-6));
    let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let c = rect.center();
    let to = |p: &[f64; 2]| egui::pos2(c.x + ((p[0] - mid[0]) * k) as f32, c.y - ((p[1] - mid[1]) * k) as f32);
    let fg = ui.visuals().text_color();
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

pub fn show(ui: &mut Ui, s: &mut Studio) {
    let doc = s.engine.doc().clone();
    ui.horizontal(|ui| {
        ui.heading(t("panel.library"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::button(ui, t("library.import"), "Import GDTF").on_hover_text(t("library.import_tip")).clicked()
            {
                s.ui.file_request = Some(crate::state::FileRequest::ImportGdtf);
            }
        });
    });
    if doc.fixture_types.is_empty() {
        ui.label(RichText::new(t("library.empty")).weak());
    }
    let counts = |key: &str| {
        doc.objects
            .values()
            .filter(|o| matches!(&o.kind, ObjectKind::Fixture { gdtf: Some(k), .. } if k == key))
            .count()
    };
    for ft in doc.fixture_types.values() {
        let active = s.ui.options.fixture_type == ft.key;
        ui.horizontal(|ui| {
            symbol_preview(ui, &doc, &ft.key);
            let r = ui.selectable_label(active, ft.display_name());
            widgets::name(&r, Role::Button, format!("Fixture type {}", ft.key));
            if r.on_hover_text(t("library.pick_tip")).clicked() {
                let key = if active { "" } else { ft.key.as_str() };
                let _ = s.run("ui.set_tool_options", json!({ "fixture_type": key }));
                if !active {
                    s.ui.set_tool(crate::state::Tool::Fixture);
                }
            }
        });
        let weight = ft.weight_kg.map(|w| format!("{w:.1} kg")).unwrap_or_else(|| t("library.no_weight").to_owned());
        let n = counts(&ft.key);
        ui.label(
            RichText::new(format!(
                "{} · {weight} · {}",
                tf("library.modes", &[("n", &ft.modes.len())]),
                tf("library.fixtures", &[("n", &n)])
            ))
            .weak()
            .small(),
        );
        if active {
            egui::Grid::new(("library_type", &ft.key)).num_columns(2).show(ui, |ui| {
                ui.label(t("inspector.mode"));
                let cur = ft.mode_or_first(
                    (!s.ui.options.fixture_mode.is_empty()).then_some(s.ui.options.fixture_mode.as_str()),
                );
                let shown = cur.map(|m| format!("{} · {} ch", m.name, m.footprint_text())).unwrap_or_default();
                let r =
                    egui::ComboBox::from_id_salt(("library_mode", &ft.key)).selected_text(shown).show_ui(ui, |ui| {
                        for m in &ft.modes {
                            let label = format!("{} · {} ch", m.name, m.footprint_text());
                            if ui.selectable_label(cur.is_some_and(|c| c.name == m.name), label).clicked() {
                                let _ = s.run("ui.set_tool_options", json!({ "fixture_mode": m.name }));
                            }
                        }
                    });
                widgets::name(&r.response, egui::Role::ComboBox, format!("Mode {}", ft.key));
                ui.end_row();
                ui.label(t("library.symbol"));
                let auto = if ft.has_plan_svg { t("library.symbol_gdtf") } else { t("library.symbol_silhouette") };
                let shown = ft.symbol.clone().map_or_else(|| auto.to_owned(), |n| t(&format!("symbol.{n}")).to_owned());
                let r =
                    egui::ComboBox::from_id_salt(("library_symbol", &ft.key)).selected_text(shown).show_ui(ui, |ui| {
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
            });
        }
    }

    ui.add_space(10.0);
    share_section(ui, s);
}

fn share_section(ui: &mut Ui, s: &mut Studio) {
    ui.label(RichText::new(t("share.title")).strong());
    let mut search = false;
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(&mut s.ui.library.query).hint_text(t("share.search_hint")).desired_width(150.0),
        );
        widgets::name(&r, Role::TextInput, "Search GDTF Share");
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            search = true;
        }
        if widgets::button(ui, t("share.search"), "Search GDTF Share now").clicked() {
            search = true;
        }
        if widgets::button(ui, t("share.refresh"), "Refresh GDTF Share list")
            .on_hover_text(t("share.refresh_tip"))
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
            ui.label(format!("{} {} s", j.label, j.started.elapsed().as_secs()));
        });
    }
    if !lib.jobs.is_empty() {
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }
    if let Some((m, err)) = &lib.message {
        let mut text = RichText::new(m).small();
        if *err {
            text = text.color(crate::theme::WARNING);
        }
        ui.label(text);
    }
    if lib.list.is_none() && lib.jobs.is_empty() && lib.message.is_none() {
        ui.label(RichText::new(t("share.hint")).weak().small());
    }
    let hits = lib.hits(40);
    if lib.list.is_some() && !lib.query.trim().is_empty() && hits.is_empty() {
        ui.label(RichText::new(t("share.no_hits")).weak());
    }
    let mut add: Option<ShareEntry> = None;
    for e in hits {
        ui.horizontal(|ui| {
            let r = widgets::button(ui, t("share.add"), &format!("Add {}", e.rid)).on_hover_text(t("share.add_tip"));
            if r.clicked() {
                add = Some(e.clone());
            }
            let modes: Vec<String> = e.modes.iter().map(|m| format!("{} ({})", m.name, m.dmx_footprint)).collect();
            ui.label(format!("{} {}", e.manufacturer, e.fixture)).on_hover_text(format!(
                "{}\n{}: {}\n{}",
                e.revision,
                t("library.modes_label"),
                modes.join(", "),
                tf("share.size", &[("kb", &(e.filesize / 1024))])
            ));
        });
    }
    if let Some(e) = add {
        s.ui.library.add(e);
    }
}
