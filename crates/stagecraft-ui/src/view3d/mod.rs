//! The 3D view: orbit camera, lit meshes from `stagecraft-geom`, section planes with
//! filled cut faces, click picking that shares the plan's selection.
//!
//! - [`camera`]: the orbit camera (pure maths, tested).
//! - [`scene`]: the per-object mesh cache, flattened GPU buffers, cut faces, picking.
//! - [`gpu`]: wgpu pipelines behind an `egui_wgpu` paint callback; [`gpu::install`]
//!   must run once per render state (eframe start-up, or a test's renderer).
//!
//! Input: left-drag orbits, right/middle/Shift-drag pans, wheel and pinch zoom at the
//! pointer, two-finger trackpad scroll orbits (Shift pans), a click selects.

pub mod camera;
pub mod gpu;
pub mod offscreen;
pub mod scene;

use egui::{Align2, Color32, Event, MouseWheelUnit, PointerButton, Rgba, RichText, Role, Sense, Ui, UiBuilder};
use glam::DVec3;
use serde::Serialize;
use serde_json::json;
use stagecraft_geom::section::Plane;

pub use camera::{Camera3d, Projection, StdView};
pub use scene::Scene;

use crate::app::Studio;
use crate::i18n::{t, tf};
use crate::theme;
use crate::widgets;

/// What the section cuts away.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Section {
    #[default]
    Off,
    /// One plane; the side its normal points to is hidden.
    Plane { plane: Plane },
    /// Everything outside the box is hidden.
    Box { min: [f64; 3], max: [f64; 3] },
}

impl Section {
    /// The clip planes, normals pointing to what is hidden.
    pub fn planes(&self) -> Vec<Plane> {
        match self {
            Section::Off => Vec::new(),
            Section::Plane { plane } => vec![*plane],
            Section::Box { min, max } => {
                let mut v = Vec::with_capacity(6);
                for a in 0..3 {
                    let mut n = [0.0; 3];
                    n[a] = -1.0;
                    v.push(Plane::new(*min, n));
                    n[a] = 1.0;
                    v.push(Plane::new(*max, n));
                }
                v
            }
        }
    }
}

pub struct View3d {
    pub camera: Camera3d,
    pub section: Section,
    /// Hatch cut faces (else solid).
    pub hatch: bool,
    pub grid: bool,
    /// Fit the model on the next frame (after the view knows its size).
    pub fit_pending: bool,
    /// Refit whenever the model's bounds change, until the user moves the camera: a
    /// camera fitted to an empty document would otherwise miss what is drawn next.
    pub follow: bool,
    /// The bounds the last fit framed.
    pub fitted: Option<(DVec3, DVec3)>,
    pub scene: Scene,
}

impl Default for View3d {
    fn default() -> Self {
        Self {
            camera: Camera3d::default(),
            section: Section::Off,
            hatch: true,
            grid: true,
            fit_pending: true,
            follow: true,
            fitted: None,
            scene: Scene::default(),
        }
    }
}

impl View3d {
    /// Fit everything drawn, or `ids` when given (ignored if none of them is drawn).
    pub fn fit(&mut self, ids: Option<&[stagecraft_core::Id]>) {
        let b = ids.and_then(|ids| self.scene.bounds_of(ids)).or_else(|| self.scene.bounds());
        self.fitted = self.scene.bounds();
        match b {
            Some((lo, hi)) => self.camera.fit(lo, hi),
            None => self.camera.fit(DVec3::ONE, DVec3::ZERO),
        }
        self.fit_pending = false;
    }

    /// A short description for the overlay: projection and section.
    pub fn caption(&self) -> String {
        let proj = match self.camera.projection {
            Projection::Perspective => t("view3d.perspective"),
            Projection::Orthographic => t("view3d.orthographic"),
        };
        match self.section {
            Section::Off => proj.to_owned(),
            Section::Plane { .. } => format!("{proj} · {}", t("view3d.section_plane")),
            Section::Box { .. } => format!("{proj} · {}", t("view3d.caption_box")),
        }
    }
}

/// Colours of the 3D view, per theme.
#[derive(Clone, Copy, Debug)]
pub struct Palette3d {
    pub background: Color32,
    pub generic: Color32,
    pub floor: Color32,
    pub wall: Color32,
    pub metal: Color32,
    pub fixture: Color32,
    pub lens: Color32,
    pub curtain: Color32,
    pub masking: Color32,
    pub deck: Color32,
    pub cut: Color32,
    pub hatch: Color32,
    pub grid_minor: Color32,
    pub grid_major: Color32,
    pub selected: Color32,
}

pub fn palette(dark: bool) -> Palette3d {
    let p = theme::palette(dark);
    if dark {
        Palette3d {
            background: Color32::from_rgb(0x1b, 0x1c, 0x1f),
            generic: Color32::from_rgb(0xa0, 0xa4, 0xab),
            floor: Color32::from_rgb(0x6a, 0x63, 0x58),
            wall: Color32::from_rgb(0x9a, 0x9c, 0xa2),
            metal: Color32::from_rgb(0xc4, 0xc8, 0xce),
            fixture: Color32::from_rgb(0x4a, 0x4d, 0x55),
            lens: Color32::from_rgb(0xff, 0xe4, 0xa8),
            curtain: Color32::from_rgb(0x8b, 0x1a, 0x1a),
            masking: Color32::from_rgb(0x2a, 0x2a, 0x2c),
            deck: Color32::from_rgb(0x3a, 0x38, 0x36),
            cut: Color32::from_rgb(0xd2, 0x5a, 0x4a),
            hatch: Color32::from_rgb(0x7a, 0x2c, 0x22),
            grid_minor: Color32::from_rgba_unmultiplied(0x5a, 0x5e, 0x66, 50),
            grid_major: Color32::from_rgba_unmultiplied(0x78, 0x7c, 0x86, 110),
            selected: p.selected,
        }
    } else {
        Palette3d {
            background: Color32::from_rgb(0xf2, 0xf3, 0xf5),
            generic: Color32::from_rgb(0xb4, 0xb8, 0xbf),
            floor: Color32::from_rgb(0xc9, 0xbf, 0xae),
            wall: Color32::from_rgb(0xe4, 0xe2, 0xdd),
            metal: Color32::from_rgb(0x9c, 0xa2, 0xab),
            fixture: Color32::from_rgb(0x3a, 0x3d, 0x44),
            lens: Color32::from_rgb(0xff, 0xd8, 0x80),
            curtain: Color32::from_rgb(0x8b, 0x1a, 0x1a),
            masking: Color32::from_rgb(0x26, 0x26, 0x28),
            deck: Color32::from_rgb(0x44, 0x41, 0x3e),
            cut: Color32::from_rgb(0xc8, 0x4b, 0x3a),
            hatch: Color32::from_rgb(0xf3, 0xc2, 0xb8),
            grid_minor: Color32::from_rgba_unmultiplied(0xa8, 0xad, 0xb6, 55),
            grid_major: Color32::from_rgba_unmultiplied(0x88, 0x8d, 0x96, 120),
            selected: p.selected,
        }
    }
}

fn lin(c: Color32) -> [f32; 4] {
    let r = Rgba::from(c);
    [r.r(), r.g(), r.b(), 1.0]
}

/// Straight (not premultiplied) linear colour with the colour's alpha.
fn lin_alpha(c: Color32) -> [f32; 4] {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    let l = Rgba::from(Color32::from_rgb(r, g, b));
    [l.r(), l.g(), l.b(), a as f32 / 255.0]
}

pub(crate) fn uniforms(v: &View3d, pal: &Palette3d, ppp: f32) -> gpu::Uniforms {
    let cam = &v.camera;
    let radius = v.scene.radius();
    let mut u = gpu::Uniforms { view_proj: cam.view_proj(radius).to_cols_array_2d(), ..Default::default() };
    let eye = cam.eye();
    let ortho = cam.projection == Projection::Orthographic;
    u.eye = [eye.x as f32, eye.y as f32, eye.z as f32, if ortho { 1.0 } else { 0.0 }];
    let look = -cam.back();
    u.view_dir = [look.x as f32, look.y as f32, look.z as f32, ppp];
    // Key light from the upper right of the viewer, so every view reads.
    let (r, up) = cam.right_up();
    let light = (r * 0.45 + up * 0.65 + cam.back() * 0.6).normalize();
    u.light = [light.x as f32, light.y as f32, light.z as f32, 0.0];
    let planes = v.section.planes();
    for (slot, p) in u.planes.iter_mut().zip(&planes) {
        let n = p.n();
        *slot = [n.x as f32, n.y as f32, n.z as f32, n.dot(p.o()) as f32];
    }
    use stagecraft_geom::MaterialClass as M;
    let colour = |m: M| match m {
        M::Generic => pal.generic,
        M::Floor => pal.floor,
        M::Wall => pal.wall,
        M::Metal => pal.metal,
        M::Fixture => pal.fixture,
        M::Lens => pal.lens,
        M::Curtain => pal.curtain,
        M::Masking => pal.masking,
        M::Deck => pal.deck,
    };
    for m in M::ALL {
        u.colors[m.index() as usize] = lin(colour(m));
    }
    u.colors[scene::CAP_MATERIAL as usize] = lin(pal.cut);
    u.selected = lin(pal.selected);
    u.hatch = lin(pal.hatch);
    // Grid steps like the plan's, by the scale at the target.
    // Coarser than the plan: a perspective grid gets dense towards the horizon.
    let scale = cam.points_per_mm_at(cam.target) * 0.4;
    let (minor, major) = crate::canvas::grid_steps(scale);
    let reach = (radius * 1.6).max(cam.distance * 3.0).min(1.0e6);
    u.grid = [minor as f32, major as f32, reach as f32, -1.0];
    u.grid_minor = lin_alpha(pal.grid_minor);
    u.grid_major = lin_alpha(pal.grid_major);
    let snap = major * 10.0;
    u.grid_center =
        [((cam.target.x / snap).round() * snap) as f32, ((cam.target.y / snap).round() * snap) as f32, 0.0, 0.0];
    u.counts = [planes.len() as u32, u32::from(v.hatch), u32::from(v.grid), 0];
    u
}

pub fn show(ui: &mut Ui, s: &mut Studio) {
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    widgets::name(&resp, Role::Unknown, t("view3d.name"));
    let v = &mut s.ui.view3d;
    v.camera.rect = rect;
    let ids = crate::canvas::visible_ids(s.engine.doc(), &s.ui);
    let selection = s.engine.selection().to_vec();
    let v = &mut s.ui.view3d;
    v.scene.sync(s.engine.doc(), &ids, &selection, &v.camera);
    if v.fit_pending || (v.follow && bounds_changed(v.fitted, v.scene.bounds())) {
        v.fit(None);
        v.scene.sync(s.engine.doc(), &ids, &selection, &v.camera);
    }
    let before = v.camera;
    navigate(ui, &resp, &mut v.camera);
    if v.camera != before {
        v.follow = false;
    }
    // Detail levels follow the camera.
    v.scene.sync(s.engine.doc(), &ids, &selection, &v.camera);
    let planes = v.section.planes();
    v.scene.update_caps(&planes);

    if resp.clicked_by(PointerButton::Primary)
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let hit = v.scene.pick(&v.camera.ray(pos), &planes);
        let shift = ui.input(|i| i.modifiers.shift);
        click_select(s, hit, shift);
    }

    let dark = ui.visuals().dark_mode;
    let pal = palette(dark);
    let ppp = ui.ctx().pixels_per_point();
    let v = &s.ui.view3d;
    let size = rect.size() * ppp;
    let frame = gpu::Frame3d {
        uniforms: uniforms(v, &pal, ppp),
        size_px: [size.x.round().max(1.0) as u32, size.y.round().max(1.0) as u32],
        clear: {
            let c = Rgba::from(pal.background);
            [c.r() as f64, c.g() as f64, c.b() as f64, 1.0]
        },
        mesh: v.scene.buffers.clone(),
        caps: v.scene.caps.clone(),
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, pal.background);
    painter.add(egui_wgpu::Callback::new_paint_callback(rect, frame));
    overlay(ui, s, rect);
}

/// Whether the model's box moved by more than a few percent of its size (detail levels
/// change bounds a little; that must not refit).
fn bounds_changed(old: Option<(DVec3, DVec3)>, new: Option<(DVec3, DVec3)>) -> bool {
    match (old, new) {
        (None, None) => false,
        (Some((a0, a1)), Some((b0, b1))) => {
            let tol = ((b1 - b0).length() * 0.02).max(50.0);
            (a0 - b0).abs().max_element() > tol || (a1 - b1).abs().max_element() > tol
        }
        _ => true,
    }
}

/// Bring the mesh cache up to date outside a frame (commands that fit or pick).
pub fn sync_scene(engine: &stagecraft_core::Engine, ui: &mut crate::UiState) {
    let ids = crate::canvas::visible_ids(engine.doc(), ui);
    let v = &mut ui.view3d;
    v.scene.sync(engine.doc(), &ids, engine.selection(), &v.camera);
}

fn click_select(s: &mut Studio, hit: Option<stagecraft_core::Id>, shift: bool) {
    match (hit, shift) {
        (Some(id), false) => {
            let _ = s.run("selection.set", json!({ "ids": [id] }));
        }
        (Some(id), true) => {
            let mut ids = s.engine.selection().to_vec();
            if let Some(i) = ids.iter().position(|x| *x == id) {
                ids.remove(i);
            } else {
                ids.push(id);
            }
            let _ = s.run("selection.set", json!({ "ids": ids }));
        }
        (None, false) => {
            let _ = s.run("selection.clear", serde_json::Value::Null);
        }
        (None, true) => {}
    }
}

/// Orbit, pan and zoom from mouse and trackpad.
fn navigate(ui: &Ui, resp: &egui::Response, cam: &mut Camera3d) {
    let pointer = ui.input(|i| i.pointer.hover_pos());
    if pointer.is_some_and(|p| cam.rect.contains(p)) {
        let at = pointer.unwrap_or(cam.rect.center());
        let (events, zoom) = ui.input(|i| (i.events.clone(), i.zoom_delta()));
        for e in events {
            if let Event::MouseWheel { unit, delta, modifiers, .. } = e {
                if modifiers.command || modifiers.ctrl {
                    continue; // egui turns these into zoom_delta
                }
                match unit {
                    MouseWheelUnit::Line | MouseWheelUnit::Page => cam.zoom_at(at, 1.15f64.powf(delta.y as f64)),
                    MouseWheelUnit::Point if modifiers.shift => cam.pan_screen(delta),
                    MouseWheelUnit::Point => cam.orbit(delta),
                }
            }
        }
        if zoom != 1.0 {
            cam.zoom_at(at, zoom as f64);
        }
    }
    let (shift, space) = ui.input(|i| (i.modifiers.shift, i.key_down(egui::Key::Space)));
    let d = resp.drag_delta();
    if resp.dragged_by(PointerButton::Secondary)
        || resp.dragged_by(PointerButton::Middle)
        || ((shift || space) && resp.dragged_by(PointerButton::Primary))
    {
        cam.pan_screen(d);
    } else if resp.dragged_by(PointerButton::Primary) {
        cam.orbit(d);
    }
}

/// View buttons and the caption in the corner of the 3D view.
fn overlay(ui: &mut Ui, s: &mut Studio, rect: egui::Rect) {
    let bar = egui::Rect::from_min_size(rect.min + egui::vec2(8.0, 8.0), egui::vec2(rect.width() - 16.0, 60.0));
    let layout = egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true);
    let mut child = ui.new_child(UiBuilder::new().max_rect(bar).layout(layout));
    for v in [StdView::Top, StdView::Front, StdView::Side, StdView::Iso] {
        let label = t(v.key());
        let r = child.small_button(label);
        widgets::name(&r, Role::Button, tf("a11y.view3d", &[("view", &label)]));
        if r.clicked() {
            let _ = s.run("ui.camera", json!({ "view": v }));
        }
    }
    let ortho = s.ui.view3d.camera.projection == Projection::Orthographic;
    let r = child.selectable_label(ortho, t("view3d.ortho_short"));
    widgets::name(&r, Role::Button, t("view3d.orthographic"));
    if r.clicked() {
        let p = if ortho { Projection::Perspective } else { Projection::Orthographic };
        let _ = s.run("ui.camera", json!({ "projection": p }));
    }
    let r = child.small_button(t("view3d.fit"));
    widgets::name(&r, Role::Button, t("menu.zoom_fit"));
    if r.clicked() {
        s.ui.view3d.fit(None);
        s.ui.view3d.follow = true;
    }
    child.separator();
    section_controls(&mut child, s);
    let caption = s.ui.view3d.caption();
    ui.painter().text(
        rect.left_bottom() + egui::vec2(10.0, -10.0),
        Align2::LEFT_BOTTOM,
        caption,
        theme::proportional(12.0),
        ui.visuals().weak_text_color(),
    );
}

fn section_controls(ui: &mut Ui, s: &mut Studio) {
    let current = match s.ui.view3d.section {
        Section::Off => "off",
        Section::Box { .. } => "box",
        Section::Plane { plane } => {
            let n = plane.normal;
            if n[0].abs() > 0.999 {
                "x"
            } else if n[1].abs() > 0.999 {
                "y"
            } else if n[2].abs() > 0.999 {
                "z"
            } else {
                "free"
            }
        }
    };
    ui.label(RichText::new(t("view3d.section")).weak());
    for (mode, key) in [
        ("off", "view3d.section_off"),
        ("x", "view3d.axis_x"),
        ("y", "view3d.axis_y"),
        ("z", "view3d.axis_z"),
        ("box", "view3d.section_box"),
    ] {
        let label = t(key);
        let r = ui.selectable_label(current == mode, label);
        widgets::name(&r, Role::Button, tf("a11y.section", &[("mode", &label)]));
        if r.clicked() && current != mode {
            let args = match mode {
                "off" => json!({ "off": true }),
                "box" => json!({ "box": {} }),
                axis => json!({ "axis": axis }),
            };
            let _ = s.run("ui.section", args);
        }
    }
    // The offset of an axis plane, in mm along its axis.
    if let Section::Plane { plane } = s.ui.view3d.section
        && current != "free"
    {
        let n = plane.n();
        let mut offset = plane.o().dot(n.abs());
        let r = ui.add(egui::DragValue::new(&mut offset).speed(20.0).suffix(" mm"));
        widgets::name(&r, Role::SpinButton, t("view3d.offset"));
        if r.changed() {
            let o = n.abs() * offset;
            let _ = s.run("ui.section", json!({ "origin": [o.x, o.y, o.z], "normal": plane.normal }));
        }
    }
}
