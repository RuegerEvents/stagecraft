//! The Sheets mode: white paper with the composed sheet, the sheet list, and the
//! inspector parts for sheets, viewports, title block fields and label templates.
//!
//! The page comes from `stagecraft_sheet::compose`, cached until the document changes.
//! Clicking a viewport selects it; dragging moves it, its corner handles change the
//! crop (the scale stays), dragging a label moves that fixture's label in this
//! viewport (`label.move`). Double-click opens the viewport editor. Every change is a
//! command on release.

use egui::epaint::{Mesh, TextShape, Vertex, WHITE_UV};
use egui::{
    Align2, Color32, Event, MouseWheelUnit, Painter, PointerButton, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui,
    vec2,
};
use serde_json::{Value, json};
use stagecraft_core::model::P2;
use stagecraft_core::sheet::{LabelField, LabelRotation, Orientation, Paper, Side, ViewKind, Viewport};
use stagecraft_core::{Document, Id};
use stagecraft_sheet::prim::{ImageData, Page, Pen, Prim, Rgb};

use crate::app::Studio;
use crate::i18n::{t, tf};
use crate::panels::{fmt_mm, parse_mm};
use crate::theme;
use crate::widgets::{self, Icon, button, edit_field, icon_button};

/// The paper view: paper mm at the canvas centre and screen points per mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaperCam {
    pub center: P2,
    pub scale: f64,
    pub rect: Rect,
}

impl Default for PaperCam {
    fn default() -> Self {
        Self { center: [210.0, 148.5], scale: 2.0, rect: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)) }
    }
}

impl PaperCam {
    pub fn to_screen(&self, p: P2) -> Pos2 {
        let c = self.rect.center();
        Pos2::new(
            c.x + ((p[0] - self.center[0]) * self.scale) as f32,
            c.y - ((p[1] - self.center[1]) * self.scale) as f32,
        )
    }

    pub fn to_paper(&self, s: Pos2) -> P2 {
        let c = self.rect.center();
        [self.center[0] + (s.x - c.x) as f64 / self.scale, self.center[1] - (s.y - c.y) as f64 / self.scale]
    }

    fn fit(&mut self, size: P2) {
        let w = (self.rect.width() as f64 - 40.0).max(50.0);
        let h = (self.rect.height() as f64 - 40.0).max(50.0);
        self.scale = (w / size[0]).min(h / size[1]).clamp(0.2, 40.0);
        self.center = [size[0] / 2.0, size[1] / 2.0];
    }
}

/// A press on the paper that may become a drag.
#[derive(Clone, Debug, PartialEq)]
pub enum SheetDrag {
    Move {
        viewport: Id,
        from: P2,
    },
    /// Dragging corner `corner` (0 lower left, 1 lower right, 2 upper right, 3 upper left).
    Resize {
        viewport: Id,
        corner: usize,
        from: P2,
    },
    Label {
        viewport: Id,
        fixture: Id,
        from: P2,
    },
    Pan,
}

pub struct SheetsState {
    /// The sheet shown; `None` = the first one.
    pub current: Option<Id>,
    /// The selected viewport.
    pub viewport: Option<Id>,
    pub cam: PaperCam,
    pub fit_pending: bool,
    pub drag: Option<SheetDrag>,
    /// The viewport editor window (double-click on a viewport).
    pub editor: bool,
    /// The label template open in the template editor.
    pub template: Option<Id>,
    cache: Option<((usize, Id), Page)>,
    /// A page being composed on a worker thread (hidden lines can take a moment).
    pending: Option<((usize, Id), PageRx)>,
}

type PageRx = std::sync::mpsc::Receiver<Option<Page>>;

impl Default for SheetsState {
    fn default() -> Self {
        Self {
            current: None,
            viewport: None,
            cam: PaperCam::default(),
            fit_pending: true,
            drag: None,
            editor: false,
            template: None,
            cache: None,
            pending: None,
        }
    }
}

/// The sheet shown: the chosen one if it exists, else the first.
pub fn current(doc: &Document, st: &SheetsState) -> Option<Id> {
    st.current.filter(|id| doc.sheets.contains_key(id)).or_else(|| doc.sheets_sorted().first().map(|s| s.id))
}

/// The composed page of the current sheet, recomposed when the document changed.
///
/// Composing runs on a worker thread with a snapshot of the document. While it runs
/// the previous page stays on screen (`busy` is set so the caller keeps repainting);
/// only the very first page of a session, or of another sheet, is waited for.
pub fn page(s: &mut Studio) -> Option<Page> {
    page_busy(s).0
}

fn page_busy(s: &mut Studio) -> (Option<Page>, bool) {
    let Some(id) = current(s.engine.doc(), &s.ui.sheets) else { return (None, false) };
    let key = (s.engine.journal().len(), id);
    if let Some((k, p)) = &s.ui.sheets.cache
        && *k == key
    {
        return (Some(p.clone()), false);
    }
    if s.ui.sheets.pending.as_ref().is_none_or(|(k, _)| *k != key) {
        // 3D images need the GPU, which lives on this thread.
        s.render_sheet_images(Some(id));
        let doc = s.engine.doc().clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(stagecraft_sheet::pages(&doc, &[id]).into_iter().next());
        });
        s.ui.sheets.pending = Some((key, rx));
    }
    let stale = s.ui.sheets.cache.as_ref().filter(|(k, _)| k.1 == id).map(|(_, p)| p.clone());
    let (_, rx) = s.ui.sheets.pending.as_ref().expect("set above");
    let wait = if stale.is_some() { std::time::Duration::from_millis(30) } else { std::time::Duration::from_secs(600) };
    let got = match rx.recv_timeout(wait) {
        Ok(p) => Some(p),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
        // The worker died (a bug); keep the old page rather than spin.
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Some(None),
    };
    match got {
        Some(p) => {
            s.ui.sheets.pending = None;
            if let Some(p) = &p {
                s.ui.sheets.cache = Some((key, p.clone()));
            }
            (p.or(stale), false)
        }
        None => (stale, true),
    }
}

fn colour(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

fn stroke(cam: &PaperCam, p: &Pen) -> Stroke {
    Stroke::new(((p.width * cam.scale) as f32).max(0.6), colour(p.color))
}

fn fill_rings(painter: &Painter, cam: &PaperCam, rings: &[Vec<P2>], fill: Color32) {
    let (pts, tris): (Vec<P2>, Vec<[usize; 3]>) = if rings.len() == 1 {
        (rings[0].clone(), stagecraft_geom::math::triangulate(&rings[0]))
    } else {
        let poly = stagecraft_geom::section::CutPolygon { outer: rings[0].clone(), holes: rings[1..].to_vec() };
        stagecraft_geom::section::triangulate(&poly)
    };
    if pts.len() < 3 {
        return;
    }
    let mut mesh = Mesh::default();
    for p in &pts {
        mesh.vertices.push(Vertex { pos: cam.to_screen(*p), uv: WHITE_UV, color: fill });
    }
    for t in tris {
        mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
    }
    painter.add(Shape::mesh(mesh));
}

/// A texture for a sheet image (rendered 3D view, logo), cached by the identity of
/// its bytes, which stay shared while the image is unchanged.
/// The image as RGBA, scaled down to fit a texture side of `max` pixels.
pub fn fit_texture(d: image::DynamicImage, max: u32) -> image::RgbaImage {
    if d.width() <= max && d.height() <= max {
        return d.to_rgba8();
    }
    d.resize(max, max, image::imageops::FilterType::Triangle).to_rgba8()
}

pub(crate) fn texture(ctx: &egui::Context, data: &ImageData) -> Option<egui::TextureHandle> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static CACHE: Mutex<Option<HashMap<usize, egui::TextureHandle>>> = Mutex::new(None);
    // Pointer plus a sample of the bytes: a freed image's address may be reused.
    let bytes: &[u8] = match data {
        ImageData::Encoded(b) => b,
        ImageData::Rgba { pixels, .. } => pixels,
    };
    let mut key = bytes.as_ptr() as usize ^ bytes.len().rotate_left(32);
    let step = (bytes.len() / 64).max(1);
    for (i, b) in bytes.iter().step_by(step).enumerate() {
        key = key.wrapping_mul(31).wrapping_add(*b as usize ^ i);
    }
    let mut guard = CACHE.lock().ok()?;
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(t) = cache.get(&key) {
        return Some(t.clone());
    }
    let img = match data {
        ImageData::Rgba { width, height, pixels } => {
            egui::ColorImage::from_rgba_unmultiplied([*width as usize, *height as usize], pixels)
        }
        ImageData::Encoded(bytes) => {
            let d = fit_texture(image::load_from_memory(bytes).ok()?, ctx.input(|i| i.max_texture_side) as u32);
            egui::ColorImage::from_rgba_unmultiplied([d.width() as usize, d.height() as usize], d.as_raw())
        }
    };
    if cache.len() > 32 {
        cache.clear();
    }
    let t = ctx.load_texture(format!("sheet-image-{key}"), img, egui::TextureOptions::LINEAR);
    cache.insert(key, t.clone());
    Some(t)
}

/// A texture stretched over a screen quad: lower left, lower right, upper right, upper
/// left corner of the picture.
pub fn image_quad(painter: &Painter, tex: egui::TextureId, c: [egui::Pos2; 4], opacity: f32) {
    let tint = Color32::from_white_alpha((opacity.clamp(0.0, 1.0) * 255.0) as u8);
    let mut mesh = egui::epaint::Mesh::with_texture(tex);
    for (p, uv) in c.iter().zip([[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]) {
        mesh.vertices.push(egui::epaint::Vertex { pos: *p, uv: egui::pos2(uv[0], uv[1]), color: tint });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// Draw composed primitives (paper mm) on screen.
pub fn paint_prims(painter: &Painter, cam: &PaperCam, prims: &[Prim]) {
    for p in prims {
        match p {
            Prim::Path { rings, closed, pen, fill } => {
                if let Some(f) = fill
                    && !rings.is_empty()
                    && rings[0].len() >= 3
                {
                    fill_rings(painter, cam, rings, colour(*f));
                }
                let Some(pen) = pen else { continue };
                let st = stroke(cam, pen);
                for r in rings {
                    let mut pts: Vec<Pos2> = r.iter().map(|q| cam.to_screen(*q)).collect();
                    if *closed && pts.len() > 2 {
                        pts.push(pts[0]);
                    }
                    match &pen.dash {
                        Some(d) if d.len() >= 2 => {
                            let on = (d[0] * cam.scale).max(1.0) as f32;
                            let off = (d[1] * cam.scale).max(1.0) as f32;
                            painter.extend(Shape::dashed_line(&pts, st, on, off));
                        }
                        _ => {
                            painter.add(Shape::line(pts, st));
                        }
                    }
                }
            }
            Prim::Circle { center, radius, pen, fill } => {
                let c = cam.to_screen(*center);
                let r = (*radius * cam.scale) as f32;
                painter.circle(
                    c,
                    r,
                    fill.map_or(Color32::TRANSPARENT, colour),
                    pen.as_ref().map_or(Stroke::NONE, |p| stroke(cam, p)),
                );
            }
            Prim::Text { pos, text, size, angle, h, v, bold, color } => {
                let px = (*size * cam.scale) as f32;
                if px < 1.6 {
                    continue;
                }
                let font_size = px * 1.55;
                let font = if *bold { theme::bold(font_size) } else { theme::proportional(font_size) };
                let o = stagecraft_sheet::render::text_origin(*pos, text, *size, *angle, *h, *v, *bold);
                let base = cam.to_screen(o);
                let galley = painter.layout_no_wrap(text.clone(), font, colour(*color));
                let a = -(angle.to_radians()) as f32;
                let up = vec2(a.sin(), -a.cos());
                let asc = font_size * 0.79;
                let top_left = base + up * asc;
                painter.add(Shape::Text(TextShape::new(top_left, galley, colour(*color)).with_angle(a)));
            }
            Prim::Image { min, max, angle, opacity, data } => {
                let (w, h) = (max[0] - min[0], max[1] - min[1]);
                let (sn, cs) = angle.to_radians().sin_cos();
                let at = |u: f64, v: f64| cam.to_screen([min[0] + u * cs - v * sn, min[1] + u * sn + v * cs]);
                let corners = [at(0.0, 0.0), at(w, 0.0), at(w, h), at(0.0, h)];
                match texture(painter.ctx(), data) {
                    Some(t) => image_quad(painter, t.id(), corners, *opacity as f32),
                    None => {
                        let r = Rect::from_two_pos(cam.to_screen(*min), cam.to_screen(*max));
                        painter.rect_filled(r, 0.0, Color32::from_gray(232));
                    }
                }
            }
            Prim::Clip { min, max, prims } => {
                let r = Rect::from_two_pos(cam.to_screen(*min), cam.to_screen(*max));
                let clipped = painter.with_clip_rect(r.intersect(painter.clip_rect()));
                paint_prims(&clipped, cam, prims);
            }
        }
    }
}

fn navigate(ui: &Ui, resp: &egui::Response, cam: &mut PaperCam, panning: bool) {
    let pointer = ui.input(|i| i.pointer.hover_pos());
    if pointer.is_some_and(|p| cam.rect.contains(p)) {
        let (events, zoom) = ui.input(|i| (i.events.clone(), i.zoom_delta()));
        let at = pointer.unwrap_or(cam.rect.center());
        let zoom_at = |cam: &mut PaperCam, f: f64| {
            let before = cam.to_paper(at);
            cam.scale = (cam.scale * f).clamp(0.2, 40.0);
            let after = cam.to_paper(at);
            cam.center = [cam.center[0] + before[0] - after[0], cam.center[1] + before[1] - after[1]];
        };
        for e in events {
            if let Event::MouseWheel { unit, delta, modifiers, .. } = e {
                if modifiers.command || modifiers.ctrl {
                    continue;
                }
                match unit {
                    MouseWheelUnit::Line | MouseWheelUnit::Page => zoom_at(cam, 1.15f64.powf(delta.y as f64)),
                    MouseWheelUnit::Point => {
                        cam.center =
                            [cam.center[0] - delta.x as f64 / cam.scale, cam.center[1] + delta.y as f64 / cam.scale]
                    }
                }
            }
        }
        if zoom != 1.0 {
            zoom_at(cam, zoom as f64);
        }
    }
    let space = ui.input(|i| i.key_down(egui::Key::Space));
    if resp.dragged_by(PointerButton::Middle) || panning || (space && resp.dragged_by(PointerButton::Primary)) {
        let d = resp.drag_delta();
        cam.center = [cam.center[0] - d.x as f64 / cam.scale, cam.center[1] + d.y as f64 / cam.scale];
    }
}

fn corners(min: P2, max: P2) -> [P2; 4] {
    [min, [max[0], min[1]], max, [min[0], max[1]]]
}

/// The rectangle a drag would give the viewport, paper mm.
fn dragged_rect(drag: &SheetDrag, min: P2, max: P2, to: P2) -> (P2, P2) {
    match drag {
        SheetDrag::Move { from, .. } => {
            let d = [to[0] - from[0], to[1] - from[1]];
            ([min[0] + d[0], min[1] + d[1]], [max[0] + d[0], max[1] + d[1]])
        }
        SheetDrag::Resize { corner, .. } => {
            let c = corners(min, max);
            let opposite = c[(corner + 2) % 4];
            ([opposite[0].min(to[0]), opposite[1].min(to[1])], [opposite[0].max(to[0]), opposite[1].max(to[1])])
        }
        _ => (min, max),
    }
}

pub fn show(ui: &mut Ui, s: &mut Studio) {
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    widgets::name(&resp, egui::WidgetType::Other, "Sheet canvas");
    let dark = ui.visuals().dark_mode;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, if dark { Color32::from_gray(0x2a) } else { Color32::from_gray(0xd9) });
    s.ui.sheets.cam.rect = rect;
    let (page, busy) = page_busy(s);
    if busy {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    }
    let Some(page) = page else {
        painter.text(rect.center(), Align2::CENTER_CENTER, t("sheets.none"), theme::proportional(15.0), Color32::GRAY);
        return;
    };
    if s.ui.sheets.fit_pending {
        s.ui.sheets.cam.fit(page.size);
        s.ui.sheets.fit_pending = false;
    }
    let panning = matches!(s.ui.sheets.drag, Some(SheetDrag::Pan));
    navigate(ui, &resp, &mut s.ui.sheets.cam, panning);
    let cam = s.ui.sheets.cam;

    // Paper with a shadow; it stays white in both themes.
    let paper = Rect::from_two_pos(cam.to_screen([0.0, 0.0]), cam.to_screen(page.size));
    painter.rect_filled(paper.translate(vec2(3.0, 4.0)), 0.0, Color32::from_black_alpha(60));
    painter.rect_filled(paper, 0.0, Color32::WHITE);
    paint_prims(&painter.with_clip_rect(paper.intersect(rect)), &cam, &page.prims);

    let sel = s.ui.sheets.viewport.filter(|v| page.viewports.iter().any(|h| h.id == *v));
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let at = pointer.map(|p| cam.to_paper(p));
    let handle_px = 5.0;
    let accent = theme::palette(dark).selected;
    let hit_viewport = |p: P2| {
        page.viewports
            .iter()
            .rev()
            .find(|h| p[0] >= h.min[0] && p[0] <= h.max[0] && p[1] >= h.min[1] && p[1] <= h.max[1])
            .map(|h| h.id)
    };
    let hit_label = |p: P2| {
        page.labels
            .iter()
            .rev()
            .find(|h| p[0] >= h.min[0] && p[0] <= h.max[0] && p[1] >= h.min[1] && p[1] <= h.max[1])
            .cloned()
    };
    let hit_handle = |p: Pos2| -> Option<usize> {
        let h = page.viewports.iter().find(|h| Some(h.id) == sel)?;
        corners(h.min, h.max).iter().position(|c| cam.to_screen(*c).distance(p) <= handle_px + 3.0)
    };

    // Hover feedback.
    if s.ui.sheets.drag.is_none()
        && let (Some(p), Some(sp)) = (at, pointer)
        && rect.contains(sp)
    {
        if let Some(l) = hit_label(p) {
            let r = Rect::from_two_pos(cam.to_screen(l.min), cam.to_screen(l.max)).expand(1.5);
            painter.rect_stroke(r, 1.0, Stroke::new(1.0, accent.gamma_multiply(0.7)), egui::StrokeKind::Outside);
        } else if let Some(v) = hit_viewport(p)
            && Some(v) != sel
            && let Some(h) = page.viewports.iter().find(|h| h.id == v)
        {
            let r = Rect::from_two_pos(cam.to_screen(h.min), cam.to_screen(h.max));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, accent.gamma_multiply(0.5)), egui::StrokeKind::Outside);
        }
    }

    // Press: decide what the drag does.
    // Measure drags from where the button went down, not from where egui saw a drag.
    let origin = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
    if resp.drag_started_by(PointerButton::Primary)
        && let Some(sp) = origin
    {
        let p = cam.to_paper(sp);
        let space = ui.input(|i| i.key_down(egui::Key::Space));
        s.ui.sheets.drag = if space {
            Some(SheetDrag::Pan)
        } else if let (Some(v), Some(corner)) = (sel, hit_handle(sp)) {
            Some(SheetDrag::Resize { viewport: v, corner, from: p })
        } else if let Some(l) = hit_label(p) {
            s.ui.sheets.viewport = Some(l.viewport);
            Some(SheetDrag::Label { viewport: l.viewport, fixture: l.fixture, from: p })
        } else if let Some(v) = hit_viewport(p) {
            s.ui.sheets.viewport = Some(v);
            Some(SheetDrag::Move { viewport: v, from: p })
        } else {
            Some(SheetDrag::Pan)
        };
    }
    if resp.clicked() {
        s.ui.sheets.viewport = at.and_then(hit_viewport);
    }
    if resp.double_clicked() && at.and_then(hit_viewport).is_some() {
        s.ui.sheets.viewport = at.and_then(hit_viewport);
        s.ui.sheets.editor = true;
    }

    // While dragging: a ghost of the result.
    let drag = s.ui.sheets.drag.clone();
    if let (Some(d), Some(p)) = (&drag, at) {
        match d {
            SheetDrag::Move { viewport, .. } | SheetDrag::Resize { viewport, .. } => {
                if let Some(h) = page.viewports.iter().find(|h| h.id == *viewport) {
                    let (a, b) = dragged_rect(d, h.min, h.max, p);
                    let r = Rect::from_two_pos(cam.to_screen(a), cam.to_screen(b));
                    painter.rect_stroke(r, 0.0, Stroke::new(1.5, accent), egui::StrokeKind::Middle);
                }
            }
            SheetDrag::Label { viewport, fixture, from } => {
                if let Some(l) = page.labels.iter().find(|l| l.viewport == *viewport && l.fixture == *fixture) {
                    let dd = [p[0] - from[0], p[1] - from[1]];
                    let r = Rect::from_two_pos(
                        cam.to_screen([l.min[0] + dd[0], l.min[1] + dd[1]]),
                        cam.to_screen([l.max[0] + dd[0], l.max[1] + dd[1]]),
                    );
                    painter.rect_stroke(r, 1.0, Stroke::new(1.5, accent), egui::StrokeKind::Outside);
                }
            }
            SheetDrag::Pan => {}
        }
    }
    if resp.drag_stopped() {
        if let (Some(d), Some(p)) = (drag, at) {
            finish_drag(s, &page, d, p);
        }
        s.ui.sheets.drag = None;
    }

    // The selected viewport: outline and corner handles.
    if let Some(h) = page.viewports.iter().find(|h| Some(h.id) == s.ui.sheets.viewport) {
        let r = Rect::from_two_pos(cam.to_screen(h.min), cam.to_screen(h.max));
        painter.rect_stroke(r, 0.0, Stroke::new(1.5, accent), egui::StrokeKind::Outside);
        for c in corners(h.min, h.max) {
            let q = cam.to_screen(c);
            painter.rect(
                Rect::from_center_size(q, vec2(handle_px * 2.0, handle_px * 2.0)),
                1.0,
                Color32::WHITE,
                Stroke::new(1.5, accent),
                egui::StrokeKind::Middle,
            );
        }
    }
    if s.ui.sheets.editor {
        viewport_window(ui.ctx(), s);
    }
}

fn finish_drag(s: &mut Studio, page: &Page, d: SheetDrag, to: P2) {
    match d {
        SheetDrag::Move { viewport, from } => {
            let by = [to[0] - from[0], to[1] - from[1]];
            if by[0].abs() + by[1].abs() > 0.2 {
                let _ = s.run("viewport.update", json!({ "viewport": viewport, "by": by }));
            }
        }
        SheetDrag::Resize { viewport, corner, from } => {
            let Some(h) = page.viewports.iter().find(|h| h.id == viewport) else { return };
            let doc = s.engine.doc();
            let Some(vp) = doc.sheet_of_viewport(viewport).and_then(|sh| sh.viewport(viewport)).cloned() else {
                return;
            };
            if matches!(vp.view, ViewKind::FixtureList) || (to[0] - from[0]).abs() + (to[1] - from[1]).abs() < 0.2 {
                return;
            }
            let Some(m) = stagecraft_sheet::compose::plan_mapping(doc, &vp, stagecraft_sheet::default_source()) else {
                return;
            };
            let (a, b) = dragged_rect(&SheetDrag::Resize { viewport, corner, from }, h.min, h.max, to);
            if b[0] - a[0] < 5.0 || b[1] - a[1] < 5.0 {
                return;
            }
            let crop = [m.to_view(a), m.to_view(b)];
            let position = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            let _ = s.run("viewport.update", json!({ "viewport": viewport, "crop": crop, "position": position }));
        }
        SheetDrag::Label { viewport, fixture, from } => {
            let by = [to[0] - from[0], to[1] - from[1]];
            if by[0].abs() + by[1].abs() > 0.2 {
                let _ = s.run(
                    "label.move",
                    json!({ "viewport": viewport, "fixture": fixture, "offset": by, "relative": true }),
                );
            }
        }
        SheetDrag::Pan => {}
    }
}

// ---- the left panel ----

pub fn list_panel(ui: &mut Ui, s: &mut Studio) {
    ui.horizontal(|ui| {
        ui.heading(t("panel.sheets"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_button(ui, Icon::Plus, "New sheet").on_hover_text(t("sheets.new")).clicked() {
                new_sheet(s);
            }
        });
    });
    let doc = s.engine.doc().clone();
    let cur = current(&doc, &s.ui.sheets);
    if doc.sheets.is_empty() {
        ui.label(RichText::new(t("sheets.none")).weak());
        if button(ui, t("sheets.new"), "Create sheet").clicked() {
            new_sheet(s);
        }
    }
    for sh in doc.sheets_sorted() {
        let text = format!("{}  {}", sh.number, sh.name);
        let r = ui.selectable_label(cur == Some(sh.id), text);
        widgets::name(&r, egui::WidgetType::Button, format!("Sheet {}", sh.number));
        if r.clicked() {
            let _ = s.run("ui.sheet", json!({ "sheet": sh.id }));
        }
    }
    let Some(cur) = cur else { return };
    ui.add_space(10.0);
    ui.label(RichText::new(t("sheets.add_viewport")).strong());
    ui.horizontal_wrapped(|ui| {
        for (key, view) in [
            ("sheets.view.plan", "plan"),
            ("sheets.view.plan_3d", "plan_3d"),
            ("sheets.view.front", "front"),
            ("sheets.view.side", "left"),
            ("sheets.view.section", "section_y"),
            ("sheets.view.rendered", "rendered"),
            ("sheets.view.list", "fixture_list"),
        ] {
            if button(ui, t(key), &format!("Add viewport {view}")).clicked() {
                let [w, h] = doc.sheets[&cur].size();
                let scale = if matches!(view, "plan" | "plan_3d" | "front" | "left" | "section_y") { 100 } else { 50 };
                let label = doc.label_templates.values().next().map(|t| t.id);
                let mut args = json!({ "sheet": cur, "view": view, "scale": scale, "position": [w * 0.4, h * 0.55] });
                if matches!(view, "plan" | "plan_3d")
                    && let Some(l) = label
                {
                    args["label_template"] = json!(l);
                }
                if let Ok(v) = s.run("viewport.create", args) {
                    s.ui.sheets.viewport = v["id"].as_str().and_then(|x| x.parse().ok());
                }
            }
        }
    });
    ui.add_space(10.0);
    ui.label(RichText::new(t("sheets.viewports")).strong());
    for vp in &doc.sheets[&cur].viewports {
        let text = match vp.view {
            ViewKind::FixtureList | ViewKind::Rendered { .. } => vp.name.clone(),
            _ => format!("{}  1:{}", vp.name, vp.scale),
        };
        let r = ui.selectable_label(s.ui.sheets.viewport == Some(vp.id), text);
        widgets::name(&r, egui::WidgetType::Button, format!("Viewport {}", vp.name));
        if r.clicked() {
            s.ui.sheets.viewport = Some(vp.id);
        }
        if r.double_clicked() {
            s.ui.sheets.viewport = Some(vp.id);
            s.ui.sheets.editor = true;
        }
    }
}

fn new_sheet(s: &mut Studio) {
    if let Ok(v) = s.run("sheet.create", json!({})) {
        let id = v["id"].clone();
        let _ = s.run("ui.sheet", json!({ "sheet": id }));
    }
}

// ---- inspector ----

/// The inspector in Sheets mode: the selected viewport, else the sheet with its title
/// block fields; label templates below.
pub fn inspector(ui: &mut Ui, s: &mut Studio) {
    ui.heading(t("panel.inspector"));
    ui.add_space(4.0);
    let doc = s.engine.doc().clone();
    let Some(cur) = current(&doc, &s.ui.sheets) else {
        ui.label(RichText::new(t("sheets.none")).weak());
        return;
    };
    let vp = s.ui.sheets.viewport.and_then(|v| doc.sheets[&cur].viewport(v)).cloned();
    match vp {
        Some(vp) => {
            ui.label(RichText::new(tf("sheets.viewport_title", &[("name", &vp.name)])).strong());
            viewport_fields(ui, s, &doc, &vp, "insp");
        }
        None => sheet_fields(ui, s, &doc, cur),
    }
    ui.add_space(8.0);
    ui.separator();
    label_templates(ui, s, &doc);
}

fn viewport_window(ctx: &egui::Context, s: &mut Studio) {
    let doc = s.engine.doc().clone();
    let Some(vp) =
        s.ui.sheets
            .viewport
            .and_then(|v| doc.sheet_of_viewport(v))
            .and_then(|sh| sh.viewport(s.ui.sheets.viewport?))
            .cloned()
    else {
        s.ui.sheets.editor = false;
        return;
    };
    let mut open = true;
    egui::Window::new(tf("sheets.viewport_title", &[("name", &vp.name)]))
        .id(egui::Id::new("viewport_editor"))
        .open(&mut open)
        .resizable(false)
        .default_width(320.0)
        .show(ctx, |ui| viewport_fields(ui, s, &doc, &vp, "win"));
    if !open {
        s.ui.sheets.editor = false;
    }
}

const SCALES: [f64; 12] = [10.0, 20.0, 25.0, 50.0, 75.0, 100.0, 125.0, 150.0, 200.0, 250.0, 500.0, 1000.0];
const PENS: [f64; 7] = [0.13, 0.18, 0.25, 0.35, 0.5, 0.7, 1.0];

/// Per tag: line class and pen of its objects in this viewport (`viewport.update
/// {tag_lines}`).
fn tag_line_fields(ui: &mut Ui, s: &mut Studio, doc: &Document, vp: &Viewport, salt: &str) {
    let set = |s: &mut Studio, tag: Id, f: &dyn Fn(&mut stagecraft_core::sheet::TagLines)| {
        let mut m: std::collections::BTreeMap<String, stagecraft_core::sheet::TagLines> =
            vp.tag_lines.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let e = m.entry(tag.to_string()).or_default();
        f(e);
        if e.class.is_none() && e.weight.is_none() {
            m.remove(&tag.to_string());
        }
        let _ = s.run("viewport.update", json!({ "viewport": vp.id, "tag_lines": m }));
    };
    egui::Grid::new(("vp_tags", salt)).num_columns(3).show(ui, |ui| {
        for tag in doc.tags.values() {
            let cur = vp.tag_lines.get(&tag.id).cloned().unwrap_or_default();
            ui.label(&tag.name);
            let text = cur
                .class
                .as_deref()
                .map_or_else(|| t("sheets.tag_default").to_owned(), |c| t(&format!("sheets.class.{c}")).to_owned());
            egui::ComboBox::from_id_salt(("tag_class", salt, tag.id)).selected_text(text).show_ui(ui, |ui| {
                if ui.selectable_label(cur.class.is_none(), t("sheets.tag_default")).clicked() {
                    set(s, tag.id, &|e| e.class = None);
                }
                for c in ["thin", "medium", "thick", "hidden", "centre"] {
                    if ui.selectable_label(cur.class.as_deref() == Some(c), t(&format!("sheets.class.{c}"))).clicked() {
                        set(s, tag.id, &|e| e.class = Some(c.to_owned()));
                    }
                }
            });
            let text = cur.weight.map_or_else(|| t("sheets.tag_default").to_owned(), |w| format!("{w} mm"));
            egui::ComboBox::from_id_salt(("tag_pen", salt, tag.id)).selected_text(text).show_ui(ui, |ui| {
                if ui.selectable_label(cur.weight.is_none(), t("sheets.tag_default")).clicked() {
                    set(s, tag.id, &|e| e.weight = None);
                }
                for p in PENS {
                    if ui.selectable_label(cur.weight == Some(p), format!("{p} mm")).clicked() {
                        set(s, tag.id, &|e| e.weight = Some(p));
                    }
                }
            });
            ui.end_row();
        }
    });
}

fn view_label(v: &ViewKind) -> String {
    match v {
        ViewKind::Plan => t("sheets.view.plan").into(),
        ViewKind::Elevation { side } => match side {
            Side::Front => t("sheets.view.front").into(),
            Side::Back => t("sheets.view.back").into(),
            Side::Left => t("sheets.view.side").into(),
            Side::Right => t("sheets.view.side_right").into(),
        },
        ViewKind::Section { .. } => t("sheets.view.section").into(),
        ViewKind::PlanFrom3d { .. } => t("sheets.view.plan_3d").into(),
        ViewKind::Rendered { .. } => t("sheets.view.rendered").into(),
        ViewKind::FixtureList => t("sheets.view.list").into(),
    }
}

fn viewport_fields(ui: &mut Ui, s: &mut Studio, doc: &Document, vp: &Viewport, salt: &str) {
    let up = |s: &mut Studio, args: Value| {
        let mut a = args;
        a["viewport"] = json!(vp.id);
        let _ = s.run("viewport.update", a);
    };
    egui::Grid::new(("vp_grid", salt)).num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label(t("inspector.name"));
        if let Some(v) = edit_field(ui, egui::Id::new(("vp_name", salt, vp.id)), &vp.name, 170.0, "Viewport name") {
            up(s, json!({ "name": v }));
        }
        ui.end_row();

        ui.label(t("sheets.view"));
        egui::ComboBox::from_id_salt(("vp_view", salt)).selected_text(view_label(&vp.view)).show_ui(ui, |ui| {
            for name in [
                "plan",
                "plan_3d",
                "front",
                "back",
                "left",
                "right",
                "section_y",
                "section_x",
                "rendered",
                "fixture_list",
            ] {
                let label = match name {
                    "plan_3d" => t("sheets.view.plan_3d").to_owned(),
                    "section_y" => t("sheets.view.section_across").to_owned(),
                    "section_x" => t("sheets.view.section_along").to_owned(),
                    "right" => t("sheets.view.side_right").to_owned(),
                    "back" => t("sheets.view.back").to_owned(),
                    "left" => t("sheets.view.side").to_owned(),
                    "front" => t("sheets.view.front").to_owned(),
                    "plan" => t("sheets.view.plan").to_owned(),
                    "rendered" => t("sheets.view.rendered").to_owned(),
                    _ => t("sheets.view.list").to_owned(),
                };
                if ui.selectable_label(false, label).clicked() {
                    up(s, json!({ "view": name, "fit": true }));
                }
            }
        });
        ui.end_row();

        if let ViewKind::Section { origin, normal } = vp.view {
            ui.label(t("sheets.cut_at"));
            let off = origin[0] * normal[0] + origin[1] * normal[1] + origin[2] * normal[2];
            if let Some(v) =
                edit_field(ui, egui::Id::new(("vp_cut", salt, vp.id)), &fmt_mm(off.abs()), 90.0, "Cut position")
            {
                match parse_mm(&v) {
                    Some(mm) => {
                        let sign = if off < 0.0 { -1.0 } else { 1.0 };
                        let o = normal.map(|c| c * mm * sign);
                        up(s, json!({ "view": { "type": "section", "origin": o, "normal": normal } }));
                    }
                    None => s.ui.error(tf("status.bad_number", &[("text", &v)])),
                }
            }
            ui.end_row();
        }

        if vp.view != ViewKind::FixtureList {
            ui.label(t("sheets.scale"));
            egui::ComboBox::from_id_salt(("vp_scale", salt)).selected_text(format!("1:{}", vp.scale)).show_ui(
                ui,
                |ui| {
                    for sc in SCALES {
                        if ui.selectable_label(vp.scale == sc, format!("1:{sc}")).clicked() {
                            up(s, json!({ "scale": sc }));
                        }
                    }
                },
            );
            ui.end_row();

            ui.label(t("sheets.crop"));
            ui.horizontal(|ui| {
                let text = match vp.crop {
                    Some(c) => format!("{} × {} m", fmt_m(c[1][0] - c[0][0]), fmt_m(c[1][1] - c[0][1])),
                    None => t("sheets.crop_fit").to_owned(),
                };
                ui.label(text);
                if vp.crop.is_some() && button(ui, t("sheets.fit"), "Fit viewport").clicked() {
                    up(s, json!({ "fit": true }));
                }
            });
            ui.end_row();
        }

        if vp.view == ViewKind::Plan {
            ui.label(t("sheets.labels"));
            let cur = vp
                .label_template
                .and_then(|l| doc.label_templates.get(&l))
                .map_or(t("sheets.no_labels").to_owned(), |l| l.name.clone());
            egui::ComboBox::from_id_salt(("vp_labels", salt)).selected_text(cur).show_ui(ui, |ui| {
                if ui.selectable_label(vp.label_template.is_none(), t("sheets.no_labels")).clicked() {
                    up(s, json!({ "label_template": "" }));
                }
                for lt in doc.label_templates.values() {
                    if ui.selectable_label(vp.label_template == Some(lt.id), &lt.name).clicked() {
                        up(s, json!({ "label_template": lt.id }));
                    }
                }
            });
            ui.end_row();
        }

        let mut title = vp.show_title;
        ui.label(t("sheets.title"));
        if ui.checkbox(&mut title, "").changed() {
            up(s, json!({ "show_title": title }));
        }
        ui.end_row();
        if vp.view != ViewKind::FixtureList {
            let mut bar = vp.scale_bar;
            ui.label(t("sheets.scale_bar"));
            if ui.checkbox(&mut bar, "").changed() {
                up(s, json!({ "scale_bar": bar }));
            }
            ui.end_row();
        }
        if matches!(vp.view, ViewKind::Plan | ViewKind::PlanFrom3d { .. }) {
            for (key, field, on) in [
                ("sheets.sketch_dimensions", "sketch_dimensions", vp.sketch_dimensions),
                ("sheets.reference_dimensions", "reference_dimensions", vp.reference_dimensions),
            ] {
                let mut v = on;
                ui.label(t(key));
                let r = ui.checkbox(&mut v, "");
                crate::widgets::name(&r, egui::WidgetType::Checkbox, t(key));
                if r.changed() {
                    up(s, json!({ field: v }));
                }
                ui.end_row();
            }
        }
    });

    if !doc.tags.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new(t("sheets.shown_tags")).strong());
        ui.horizontal_wrapped(|ui| {
            for tag in doc.tags.values() {
                let mut shown = !vp.hidden_tags.contains(&tag.id);
                let r = ui.checkbox(&mut shown, &tag.name);
                widgets::name(&r, egui::WidgetType::Checkbox, format!("Viewport tag {}", tag.name));
                if r.changed() {
                    let mut hidden: Vec<Id> = vp.hidden_tags.clone();
                    if shown {
                        hidden.retain(|x| *x != tag.id);
                    } else {
                        hidden.push(tag.id);
                    }
                    up(s, json!({ "hidden_tags": hidden }));
                }
            }
        });
    }
    if doc.layers.len() > 1 {
        ui.label(RichText::new(t("sheets.shown_layers")).strong());
        ui.horizontal_wrapped(|ui| {
            for l in doc.layers_sorted() {
                let mut shown = !vp.hidden_layers.contains(&l.id);
                if ui.checkbox(&mut shown, &l.name).changed() {
                    let mut hidden: Vec<Id> = vp.hidden_layers.clone();
                    if shown {
                        hidden.retain(|x| *x != l.id);
                    } else {
                        hidden.push(l.id);
                    }
                    up(s, json!({ "hidden_layers": hidden }));
                }
            }
        });
    }
    if stagecraft_sheet::compose::is_vector_3d(&vp.view) {
        let mut hidden = vp.hidden_lines;
        let r = ui.checkbox(&mut hidden, t("sheets.hidden_lines"));
        widgets::name(&r, egui::WidgetType::Checkbox, "Hidden lines");
        if r.changed() {
            up(s, json!({ "hidden_lines": hidden }));
        }
        if let ViewKind::PlanFrom3d { cut } = vp.view {
            ui.horizontal(|ui| {
                ui.label(t("sheets.cut_height"));
                let text = cut.map(fmt_mm).unwrap_or_default();
                if let Some(v) = edit_field(ui, egui::Id::new(("vp_cut_z", salt, vp.id)), &text, 90.0, "Cut height") {
                    let cut = if v.trim().is_empty() { None } else { parse_mm(&v) };
                    if cut.is_none() && !v.trim().is_empty() {
                        s.ui.error(tf("status.bad_number", &[("text", &v)]));
                    } else {
                        up(s, json!({ "view": { "type": "plan_3d", "cut": cut } }));
                    }
                }
            });
        }
    }
    if vp.view == ViewKind::Plan || stagecraft_sheet::compose::is_vector_3d(&vp.view) {
        ui.collapsing(t("sheets.line_weights"), |ui| {
            egui::Grid::new(("vp_pens", salt)).num_columns(2).show(ui, |ui| {
                for class in ["thin", "medium", "thick", "hidden", "centre", "cut"] {
                    ui.label(t(&format!("sheets.class.{class}")));
                    let cur = stagecraft_sheet::compose::weight(vp, class);
                    egui::ComboBox::from_id_salt(("pen", salt, class)).selected_text(format!("{cur} mm")).show_ui(
                        ui,
                        |ui| {
                            for p in PENS {
                                if ui.selectable_label(cur == p, format!("{p} mm")).clicked() {
                                    let mut w = vp.line_weights.clone();
                                    w.insert(class.to_owned(), p);
                                    up(s, json!({ "line_weights": w }));
                                }
                            }
                        },
                    );
                    ui.end_row();
                }
            });
        });
    }
    if (vp.view == ViewKind::Plan || stagecraft_sheet::compose::is_vector_3d(&vp.view)) && !doc.tags.is_empty() {
        ui.collapsing(t("sheets.tag_lines"), |ui| tag_line_fields(ui, s, doc, vp, salt));
    }
    ui.horizontal(|ui| {
        if !vp.label_offsets.is_empty() && button(ui, t("sheets.reset_labels"), "Reset labels").clicked() {
            up(s, json!({ "reset_labels": true }));
        }
        if button(ui, t("sheets.to_front"), "Bring to front").clicked() {
            up(s, json!({ "to_front": true }));
        }
        if button(ui, t("sheets.delete_viewport"), "Delete viewport").clicked() {
            let _ = s.run("viewport.delete", json!({ "viewport": vp.id }));
            s.ui.sheets.viewport = None;
            s.ui.sheets.editor = false;
        }
    });
}

fn fmt_m(mm: f64) -> String {
    let m = mm / 1000.0;
    format!("{m:.1}")
}

fn sheet_fields(ui: &mut Ui, s: &mut Studio, doc: &Document, id: Id) {
    let sh = doc.sheets[&id].clone();
    ui.label(RichText::new(tf("sheets.sheet_title", &[("number", &sh.number)])).strong());
    let up = |s: &mut Studio, args: Value| {
        let mut a = args;
        a["sheet"] = json!(id);
        let _ = s.run("sheet.update", a);
    };
    egui::Grid::new("sheet_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label(t("inspector.name"));
        if let Some(v) = edit_field(ui, egui::Id::new(("sheet_name", id)), &sh.name, 170.0, "Sheet name") {
            up(s, json!({ "name": v }));
        }
        ui.end_row();
        ui.label(t("sheets.number"));
        if let Some(v) = edit_field(ui, egui::Id::new(("sheet_number", id)), &sh.number, 90.0, "Sheet number") {
            up(s, json!({ "number": v }));
        }
        ui.end_row();
        ui.label(t("sheets.paper"));
        egui::ComboBox::from_id_salt("sheet_paper").selected_text(sh.paper.name()).show_ui(ui, |ui| {
            for (p, name) in
                [(Paper::A4, "a4"), (Paper::A3, "a3"), (Paper::A2, "a2"), (Paper::A1, "a1"), (Paper::A0, "a0")]
            {
                if ui.selectable_label(sh.paper == p, p.name()).clicked() {
                    up(s, json!({ "paper": name }));
                }
            }
        });
        ui.end_row();
        ui.label(t("sheets.orientation"));
        ui.horizontal(|ui| {
            for (o, key) in [(Orientation::Landscape, "sheets.landscape"), (Orientation::Portrait, "sheets.portrait")] {
                if ui.radio(sh.orientation == o, t(key)).clicked() && sh.orientation != o {
                    up(s, json!({ "orientation": o }));
                    s.ui.sheets.fit_pending = true;
                }
            }
        });
        ui.end_row();
        ui.label(t("sheets.title_block"));
        let cur = if !sh.show_title_block {
            t("sheets.none_block").to_owned()
        } else {
            sh.title_block.and_then(|t| doc.title_blocks.get(&t)).map_or("Generic".to_owned(), |t| t.name.clone())
        };
        egui::ComboBox::from_id_salt("sheet_tb").selected_text(cur).show_ui(ui, |ui| {
            if ui.selectable_label(false, "Generic").clicked() {
                up(s, json!({ "title_block": "generic" }));
            }
            for tb in doc.title_blocks.values() {
                if ui.selectable_label(sh.title_block == Some(tb.id), &tb.name).clicked() {
                    up(s, json!({ "title_block": tb.id }));
                }
            }
            if ui.selectable_label(!sh.show_title_block, t("sheets.none_block")).clicked() {
                up(s, json!({ "title_block": "none" }));
            }
        });
        ui.end_row();
        ui.label(t("sheets.legend"));
        let mut legend = sh.show_legend;
        if ui.checkbox(&mut legend, "").changed() {
            up(s, json!({ "legend": legend }));
        }
        ui.end_row();
    });
    ui.horizontal(|ui| {
        if button(ui, t("sheets.export_pdf"), "Export PDF").clicked() {
            s.ui.file_request = Some(crate::state::FileRequest::ExportPdf);
        }
        if button(ui, t("sheets.delete_sheet"), "Delete sheet").clicked() {
            let _ = s.run("sheet.delete", json!({ "sheet": id }));
            s.ui.sheets.fit_pending = true;
        }
    });

    ui.add_space(8.0);
    ui.separator();
    ui.label(RichText::new(t("sheets.title_fields")).strong());
    let meta = doc.meta.clone();
    egui::Grid::new("sheet_meta").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        for (key, field, v) in [
            ("meta.project", "project", meta.project),
            ("meta.venue", "venue", meta.venue),
            ("meta.drawn_by", "drawn_by", meta.drawn_by),
            ("meta.date", "date", meta.date),
            ("meta.revision", "revision", meta.revision),
            ("meta.company", "company", meta.company),
        ] {
            ui.label(t(key));
            if let Some(v) =
                edit_field(ui, egui::Id::new(("sheet_meta", field)), &v, 170.0, &format!("Title block {field}"))
            {
                let _ = s.run("doc.set_meta", json!({ field: v }));
            }
            ui.end_row();
        }
        ui.label(t("meta.logo"));
        ui.horizontal(|ui| {
            if button(ui, t("sheets.choose_logo"), "Choose logo").clicked() {
                s.ui.file_request = Some(crate::state::FileRequest::Logo);
            }
            if meta.logo.is_some() && button(ui, t("sheets.remove_logo"), "Remove logo").clicked() {
                let _ = s.run("doc.set_meta", json!({ "logo_file": "" }));
            }
        });
        ui.end_row();
    });
}

const FIELDS: [&str; 12] =
    ["fid", "cid", "address", "mode", "gel", "circuit", "focus", "note", "name", "type", "weight", "footprint"];

fn label_templates(ui: &mut Ui, s: &mut Studio, doc: &Document) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(t("sheets.label_templates")).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_button(ui, Icon::Plus, "New label template").on_hover_text(t("sheets.new_template")).clicked() {
                let mut n = doc.label_templates.len() + 1;
                let mut name = tf("sheets.template_name", &[("n", &n)]);
                while doc.label_templates.values().any(|x| x.name == name) {
                    n += 1;
                    name = tf("sheets.template_name", &[("n", &n)]);
                }
                if let Ok(v) = s.run("label.create", json!({ "name": name })) {
                    s.ui.sheets.template = v["id"].as_str().and_then(|x| x.parse().ok());
                }
            }
        });
    });
    let cur =
        s.ui.sheets
            .template
            .filter(|x| doc.label_templates.contains_key(x))
            .or_else(|| doc.label_templates.keys().next().copied());
    let Some(cur) = cur else {
        ui.label(RichText::new(t("sheets.no_templates")).weak());
        return;
    };
    let lt = doc.label_templates[&cur].clone();
    egui::ComboBox::from_id_salt("lt_pick").selected_text(&lt.name).show_ui(ui, |ui| {
        for x in doc.label_templates.values() {
            if ui.selectable_label(x.id == cur, &x.name).clicked() {
                s.ui.sheets.template = Some(x.id);
            }
        }
    });
    let up = |s: &mut Studio, args: Value| {
        let mut a = args;
        a["template"] = json!(cur);
        let _ = s.run("label.update", a);
    };
    egui::Grid::new("lt_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
        ui.label(t("inspector.name"));
        if let Some(v) = edit_field(ui, egui::Id::new(("lt_name", cur)), &lt.name, 160.0, "Template name") {
            up(s, json!({ "name": v }));
        }
        ui.end_row();
        ui.label(t("sheets.rotation"));
        ui.vertical(|ui| {
            for (r, key) in [
                (LabelRotation::AnchoredUpright, "sheets.rot.anchored"),
                (LabelRotation::SheetFixed, "sheets.rot.fixed"),
                (LabelRotation::Follow, "sheets.rot.follow"),
            ] {
                let resp = ui.radio(lt.rotation == r, t(key));
                widgets::name(&resp, egui::WidgetType::RadioButton, format!("Label rotation {r:?}"));
                if resp.clicked() && lt.rotation != r {
                    up(s, json!({ "rotation": r }));
                }
            }
        });
        ui.end_row();
    });
    ui.label(RichText::new(t("sheets.fields")).small().weak());
    let mut fields = lt.fields.clone();
    let mut changed = false;
    let mut remove = None;
    egui::Grid::new("lt_fields").num_columns(6).spacing([4.0, 4.0]).show(ui, |ui| {
        ui.label(RichText::new(t("sheets.field")).small());
        ui.label(RichText::new("x").small());
        ui.label(RichText::new("y").small());
        ui.label(RichText::new(t("sheets.size")).small());
        ui.label(RichText::new(t("sheets.box")).small());
        ui.label("");
        ui.end_row();
        for (i, f) in fields.iter_mut().enumerate() {
            egui::ComboBox::from_id_salt(("lt_field", cur, i)).width(70.0).selected_text(&f.field).show_ui(ui, |ui| {
                for name in FIELDS {
                    if ui.selectable_label(f.field == name, name).clicked() && f.field != name {
                        f.field = name.into();
                        changed = true;
                    }
                }
            });
            for (axis, salt) in [(0usize, "x"), (1, "y")] {
                if let Some(v) = edit_field(
                    ui,
                    egui::Id::new(("lt_anchor", cur, i, salt)),
                    &fmt_mm(f.anchor[axis]),
                    42.0,
                    &format!("Field {i} {salt}"),
                ) && let Some(mm) = parse_mm(&v)
                {
                    f.anchor[axis] = mm;
                    changed = true;
                }
            }
            if let Some(v) = edit_field(
                ui,
                egui::Id::new(("lt_size", cur, i)),
                &format!("{}", f.size),
                32.0,
                &format!("Field {i} size"),
            ) && let Ok(x) = v.trim().parse::<f64>()
            {
                f.size = x;
                changed = true;
            }
            if ui.checkbox(&mut f.boxed, "").changed() {
                changed = true;
            }
            if icon_button(ui, Icon::Close, &format!("Remove field {i}")).clicked() {
                remove = Some(i);
            }
            ui.end_row();
        }
    });
    if let Some(i) = remove {
        fields.remove(i);
        changed = true;
    }
    if button(ui, t("sheets.add_field"), "Add label field").clicked() {
        fields.push(LabelField {
            field: "note".into(),
            anchor: [0.0, -300.0],
            size: 1.6,
            boxed: false,
            bold: false,
            prefix: String::new(),
        });
        changed = true;
    }
    if changed {
        up(s, json!({ "fields": fields }));
    }
    ui.horizontal(|ui| {
        let on = s.ui.plan_labels == Some(cur);
        let text = if on { t("sheets.plan_labels_off") } else { t("sheets.plan_labels_on") };
        if button(ui, text, "Labels in plan").clicked() {
            let v = if on { Value::Null } else { json!(cur) };
            let _ = s.run("ui.set_label_template", json!({ "template": v }));
        }
        if button(ui, t("sheets.delete_template"), "Delete label template").clicked() {
            let _ = s.run("label.delete", json!({ "template": cur }));
        }
    });
}
