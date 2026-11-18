//! The plan canvas: camera, grid, objects, overlays. Interaction is in [`crate::tools`].

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{
    Align2, Color32, Event, MouseWheelUnit, Painter, PointerButton, Pos2, Rect, Sense, Shape, Stroke, Ui, vec2,
};
use stagecraft_core::{Document, Id};
use stagecraft_geom::{Detail, LineClass, PlanPrim, PlanStyle, math};

use crate::app::Studio;
use crate::i18n::t;
use crate::state::{Camera, UiState};
use crate::theme::{self, Palette};

/// Objects drawn in the plan: on a visible layer and carrying no hidden tag.
pub fn visible_ids(doc: &Document, ui: &UiState) -> Vec<Id> {
    let shown = doc
        .objects
        .values()
        .filter(|o| doc.layers.get(&o.layer).is_none_or(|l| l.visible))
        .filter(|o| !o.tags.iter().any(|t| ui.hidden_tags.contains(t)));
    // Back to front: floors first (lower ones first), fixtures last.
    stagecraft_geom::pick::back_to_front(doc, shown)
}

/// The objects' plan primitives (world mm), kept while the document and the plan style
/// stay the same: panning and zooming only map them to the screen. Any edit drops them
/// all (an object's drawing can depend on others: parents, attachments, types).
#[derive(Default)]
pub struct PlanCache {
    rev: Option<PlanRev>,
    prims: std::collections::HashMap<Id, Vec<PlanPrim>>,
}

/// What plan primitives are made from: a snapshot of the document (its imbl maps are
/// shared, so this is cheap and compared by identity) and the style.
struct PlanRev {
    doc: Document,
    style: PlanStyle,
}

impl PlanRev {
    fn is(&self, doc: &Document, style: &PlanStyle) -> bool {
        let d = &self.doc;
        d.objects.ptr_eq(&doc.objects)
            && d.layers.ptr_eq(&doc.layers)
            && d.tags.ptr_eq(&doc.tags)
            && d.fixture_types.ptr_eq(&doc.fixture_types)
            && d.assets.ptr_eq(&doc.assets)
            && d.meta == doc.meta
            && self.style == *style
    }
}

impl PlanCache {
    /// Forget everything if `doc` or `style` is not what the primitives were made from.
    pub fn sync(&mut self, doc: &Document, style: &PlanStyle) {
        if self.rev.as_ref().is_some_and(|r| r.is(doc, style)) {
            return;
        }
        self.prims.clear();
        self.rev = Some(PlanRev { doc: doc.clone(), style: style.clone() });
    }

    /// An object's primitives, made now if not cached. Call [`PlanCache::sync`] first.
    pub fn get(&mut self, doc: &Document, o: &stagecraft_core::Object) -> &[PlanPrim] {
        let style = self.rev.as_ref().map(|r| r.style.clone()).unwrap_or_default();
        self.prims.entry(o.id).or_insert_with(|| stagecraft_geom::plan(doc, o, &style))
    }

    /// An object's primitives if they are cached.
    pub fn cached(&self, id: Id) -> Option<&[PlanPrim]> {
        self.prims.get(&id).map(Vec::as_slice)
    }

    /// Whether the cache was made from this document and style.
    pub fn is_for(&self, doc: &Document, style: &PlanStyle) -> bool {
        self.rev.as_ref().is_some_and(|r| r.is(doc, style))
    }
}

/// Detail and labels by zoom: truss outlines only when a 290 mm profile is a few
/// points wide, labels only when readable.
pub fn plan_style(cam: &Camera) -> PlanStyle {
    PlanStyle {
        detail: if cam.scale * 290.0 < 7.0 { Detail::Low } else { Detail::High },
        centre_lines: false,
        labels: cam.scale * stagecraft_geom::kinds::fixture::LABEL_HEIGHT >= 7.0,
    }
}

/// Pick radius in mm for a few screen points.
pub fn tolerance(cam: &Camera) -> f64 {
    6.0 / cam.scale
}

pub fn show(ui: &mut Ui, s: &mut Studio) {
    let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, t("canvas.name")));
    s.ui.camera.rect = rect;
    if s.ui.fit_pending {
        let ids = visible_ids(s.engine.doc(), &s.ui);
        let b = stagecraft_geom::pick::objects_bounds(s.engine.doc(), ids, &PlanStyle::default());
        crate::sketch_ui::fit(&mut s.ui.camera, s.engine.doc(), b);
        s.ui.fit_pending = false;
    }
    navigate(ui, &resp, &mut s.ui.camera);

    let pal = theme::palette(ui.visuals().dark_mode);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, pal.canvas);
    draw_grid(&painter, &s.ui.camera, &pal);

    let ids = visible_ids(s.engine.doc(), &s.ui);
    let mut style = plan_style(&s.ui.camera);
    let template = s.ui.plan_labels.and_then(|t| s.engine.doc().label_templates.get(&t)).cloned();
    if template.is_some() {
        style.labels = false;
    }
    // While a sketch point is dragged, draw the document as the drag would leave it.
    let drag = match &s.ui.gesture.drag {
        Some(crate::state::Drag::SketchPoint { layer, point }) => {
            s.ui.gesture.cursor.map(|c| (*layer, *point, c.point))
        }
        _ => None,
    };
    let preview = s.ui.sketch.preview_doc(s.engine.doc(), drag).cloned();
    let doc = preview.as_ref().unwrap_or(s.engine.doc());
    s.ui.plan.sync(doc, &style);
    for id in &ids {
        if let Some(o) = doc.objects.get(id) {
            s.ui.plan.get(doc, o);
        }
    }
    let hover = crate::tools::hovered(ui, &resp, s, &ids, &style);
    let doc = preview.as_ref().unwrap_or(s.engine.doc());
    let sel = s.engine.selection();
    for id in &ids {
        let o = &doc.objects[id];
        let colour = if sel.contains(id) {
            Some(pal.selected)
        } else if hover == Some(*id) {
            Some(pal.hover)
        } else {
            None
        };
        let Some(prims) = s.ui.plan.cached(*id) else { continue };
        // Underlays: pictures from the assets, linework faded by their opacity.
        let colour = match &o.kind {
            stagecraft_core::ObjectKind::Underlay { opacity, .. } => {
                draw_images(&painter, &s.ui.camera, doc, prims);
                colour.or(Some(pal.line.gamma_multiply(*opacity as f32)))
            }
            _ => colour,
        };
        draw_prims(&painter, &s.ui.camera, &pal, prims, colour, 1.0);
    }

    draw_conflicts(&painter, &s.ui.camera, &pal, doc, &ids);
    crate::sketch_ui::draw(&painter, &s.ui.camera, &pal, doc, &mut s.ui.sketch);

    if let Some(t) = &template {
        draw_labels(&painter, &s.ui.camera, &pal, doc, &ids, t);
    }

    crate::tools::interact(ui, &resp, s, &ids, &style);
    crate::tools::draw_overlay(ui, &painter, s, &pal, &style);
    crate::sketch_ui::editor(ui, s);
    corner_marks(&painter, rect, &theme::of(ui), &s.ui.camera);
}

/// The view's name in the top left corner and the axis icon in the bottom left, as a CAD
/// model space shows them.
fn corner_marks(p: &Painter, rect: Rect, c: &theme::Tokens, cam: &Camera) {
    let ink = c.plan.ink;
    // The plan at 1:n on screen (a point is 0.3528 mm).
    let n = 1.0 / (cam.scale * 25.4 / 72.0);
    let scale = if n >= 10.0 { format!("1:{}", (n / 5.0).round() * 5.0) } else { format!("1:{n:.1}") };
    let label = format!("{}  |  {}  |  {scale}", t("view3d.top"), t("view.plan"));
    let g = p.layout_no_wrap(label, theme::small(), ink);
    let at = rect.left_top() + vec2(10.0, 9.0);
    p.rect_filled(Rect::from_min_size(at - vec2(5.0, 3.0), g.size() + vec2(10.0, 6.0)), 3.0, c.canvas_chip);
    p.galley(at, g, ink);
    let o = Pos2::new(rect.left() + 22.0, rect.bottom() - 22.0);
    let st = Stroke::new(1.0, ink);
    let len = 34.0;
    p.line_segment([o, o + vec2(len, 0.0)], st);
    p.line_segment([o, o - vec2(0.0, len)], st);
    p.rect_stroke(Rect::from_min_size(o - vec2(0.0, 6.0), vec2(6.0, 6.0)), 0.0, st, egui::StrokeKind::Middle);
    p.text(o + vec2(len + 8.0, 0.0), Align2::LEFT_CENTER, "X", theme::proportional(10.5), c.axes[0]);
    p.text(o - vec2(0.0, len + 8.0), Align2::CENTER_BOTTOM, "Y", theme::proportional(10.5), c.axes[1]);
}

/// A warning ring around every fixture whose patch overlaps another or runs past
/// its universe.
fn draw_conflicts(painter: &Painter, cam: &Camera, pal: &Palette, doc: &Document, ids: &[Id]) {
    let bad = stagecraft_core::patch::conflicting(doc);
    if bad.is_empty() {
        return;
    }
    for id in ids.iter().filter(|id| bad.contains(id)) {
        let Some(o) = doc.objects.get(id) else { continue };
        let Some(w) = stagecraft_core::geom::world(doc, *id) else { continue };
        let (lo, hi) = stagecraft_geom::gdtf::plan_bounds(doc, o);
        let frame = stagecraft_geom::kinds::fixture::plan_frame(&w);
        let c = frame([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]);
        let r = ((hi[0] - lo[0]).hypot(hi[1] - lo[1]) / 2.0 * cam.scale) as f32 + 4.0;
        let p = cam.to_screen(c);
        painter.circle_stroke(p, r.max(6.0), Stroke::new(2.0, pal.conflict));
        painter.circle_stroke(p, r.max(6.0) + 3.0, Stroke::new(1.0, pal.conflict.gamma_multiply(0.5)));
    }
}

/// Screen points per paper mm for plan labels: label sizes are paper mm, as on a
/// sheet, so they keep their size on screen while zooming.
pub const LABEL_PX_PER_MM: f64 = 4.5;

/// Fixture labels with a template, placed as on a sheet (same rotation modes and
/// collision avoidance), in the canvas colours.
fn draw_labels(
    painter: &Painter,
    cam: &Camera,
    pal: &Palette,
    doc: &Document,
    ids: &[Id],
    t: &stagecraft_core::sheet::LabelTemplate,
) {
    // Too far out, labels would only pile up.
    if cam.scale * stagecraft_geom::kinds::fixture::BODY_WIDTH < 5.0 {
        return;
    }
    let k = cam.scale / LABEL_PX_PER_MM;
    let view = [cam.to_world(cam.rect.left_bottom()), cam.to_world(cam.rect.right_top())];
    let inputs: Vec<_> = ids
        .iter()
        .filter_map(|id| doc.objects.get(id))
        .filter_map(|o| stagecraft_sheet::labels::fixture_input(doc, o, t, &|p| [p[0] * k, p[1] * k], k, None))
        .filter(|f| {
            let w = [f.center[0] / k, f.center[1] / k];
            w[0] >= view[0][0] && w[0] <= view[1][0] && w[1] >= view[0][1] && w[1] <= view[1][1]
        })
        .collect();
    let placed = stagecraft_sheet::labels::place(t, &inputs, &stagecraft_sheet::fonts::text_width, &[]);
    let mut prims = Vec::new();
    stagecraft_sheet::compose::label_prims(&placed, &mut prims);
    let rgb = |c: Color32| stagecraft_sheet::prim::Rgb(c.r(), c.g(), c.b());
    let (fg, bg) = (rgb(pal.text), rgb(pal.canvas));
    for p in &mut prims {
        match p {
            stagecraft_sheet::Prim::Path { pen, fill, .. } | stagecraft_sheet::Prim::Circle { pen, fill, .. } => {
                if let Some(pen) = pen {
                    pen.color = fg;
                }
                if let Some(f) = fill {
                    *f = if *f == stagecraft_sheet::prim::Rgb::WHITE { bg } else { fg };
                }
            }
            stagecraft_sheet::Prim::Text { color, .. } => *color = fg,
            _ => {}
        }
    }
    let pcam = crate::sheets::PaperCam {
        center: [cam.center[0] * k, cam.center[1] * k],
        scale: LABEL_PX_PER_MM,
        rect: cam.rect,
    };
    crate::sheets::paint_prims(painter, &pcam, &prims);
}

/// Zoom per mouse wheel notch (one line of scrolling): about what CAD programs use.
pub const WHEEL_ZOOM: f64 = 1.25;
/// Zoom per point of ⌘- or Ctrl-scrolling on a trackpad: e-fold per 100 points.
pub const SCROLL_ZOOM: f64 = 0.01;
/// Trackpad pinch: the platform's magnification raised to this power, so a pinch
/// covers a plan's range of scales in a gesture or two.
pub const PINCH_GAIN: f64 = 1.5;
/// The most one frame zooms by (fast free-spinning wheels).
const MAX_FRAME_ZOOM: f64 = 4.0;

/// The zoom factor this frame's input asks for: wheel notches, ⌘/Ctrl-scroll on a
/// trackpad and pinch, proportional to their deltas and applied as they arrive (no
/// smoothing, which lags behind the fingers). Plain trackpad scrolling is not zoom.
pub fn zoom_factor(events: &[Event]) -> f64 {
    let mut f = 1.0f64;
    for e in events {
        match e {
            Event::MouseWheel { unit, delta, modifiers, .. } => {
                let dy = delta.y as f64;
                match unit {
                    MouseWheelUnit::Line => f *= WHEEL_ZOOM.powf(dy),
                    MouseWheelUnit::Page => f *= WHEEL_ZOOM.powf(dy * 3.0),
                    MouseWheelUnit::Point if modifiers.command || modifiers.ctrl => f *= (dy * SCROLL_ZOOM).exp(),
                    MouseWheelUnit::Point => {}
                }
            }
            Event::Zoom(z) if *z > 0.0 => f *= (*z as f64).powf(PINCH_GAIN),
            _ => {}
        }
    }
    f.clamp(1.0 / MAX_FRAME_ZOOM, MAX_FRAME_ZOOM)
}

/// Scroll and pinch. Mouse wheels (line units) zoom, trackpad scrolling (point units)
/// pans, pinch or ⌘-scroll zooms ([`zoom_factor`]); middle or Space-drag pans.
fn navigate(ui: &Ui, resp: &egui::Response, cam: &mut Camera) {
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let over = pointer.is_some_and(|p| cam.rect.contains(p));
    let before = cam.scale;
    if over {
        let events = ui.input(|i| i.events.clone());
        let at = pointer.unwrap_or(cam.rect.center());
        for e in &events {
            if let Event::MouseWheel { unit: MouseWheelUnit::Point, delta, modifiers, .. } = e
                && !(modifiers.command || modifiers.ctrl)
            {
                cam.pan_screen(*delta);
            }
        }
        let zoom = zoom_factor(&events);
        if zoom != 1.0 {
            cam.zoom_at(at, zoom);
        }
    }
    let space = ui.input(|i| i.key_down(egui::Key::Space));
    if resp.dragged_by(PointerButton::Middle) || (space && resp.dragged_by(PointerButton::Primary)) {
        cam.pan_screen(resp.drag_delta());
    }
    note_zoom(ui.ctx(), before != cam.scale);
}

/// How long after the last zoom step plan text is laid out at its own size again.
const ZOOM_SETTLE: f64 = 0.25;

fn zoom_key() -> egui::Id {
    egui::Id::unique("plan_zoomed_at")
}

/// Remember when the plan last zoomed, and ask for the frame after it settles (text
/// laid out at its exact size again).
fn note_zoom(ctx: &egui::Context, zoomed: bool) {
    let now = ctx.input(|i| i.time);
    if zoomed {
        ctx.data_mut(|d| d.insert_temp(zoom_key(), now));
    }
    if let Some(t) = ctx.data(|d| d.get_temp::<f64>(zoom_key())) {
        let left = t + ZOOM_SETTLE - now;
        if left > 0.0 {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(left));
        }
    }
}

/// Whether the plan is being zoomed (a zoom step in the last [`ZOOM_SETTLE`] seconds).
pub fn zooming(ctx: &egui::Context) -> bool {
    let now = ctx.input(|i| i.time);
    ctx.data(|d| d.get_temp::<f64>(zoom_key())).is_some_and(|t| now - t < ZOOM_SETTLE)
}

/// The text size glyphs are rasterised at while zooming: the next half octave at or
/// above `px`, so a zoom gesture meets only a few sizes and finds them in egui's glyph
/// atlas.
pub fn ladder_size(px: f32) -> f32 {
    2f32.powf((px.max(0.5).log2() * 2.0 - 1e-4).ceil() / 2.0).min(MAX_TEXT_PX)
}

/// Plan text is never rasterised bigger than this (points); larger text is the glyphs
/// of this size scaled up. A note zoomed to half a metre on screen would otherwise put
/// glyphs of a thousand pixels into egui's atlas, which fills and is rebuilt.
pub const MAX_TEXT_PX: f32 = 96.0;

/// Plan text laid out at `px` points. Rasterising glyphs at a new size is what made
/// zooming slow (every frame of a zoom met new sizes for every note and label): while
/// the plan zooms, the text is laid out at [`ladder_size`] and its glyph quads are scaled
/// down to `px`; once the zoom settles it is laid out at its own size, crisp.
pub fn plan_galley(p: &Painter, text: String, px: f32, colour: Color32, zooming: bool) -> std::sync::Arc<egui::Galley> {
    if !zooming && px <= MAX_TEXT_PX {
        return p.layout_no_wrap(text, theme::proportional(px), colour);
    }
    let q = if zooming { ladder_size(px) } else { MAX_TEXT_PX };
    let g = p.layout_no_wrap(text, theme::proportional(q), colour);
    scale_galley(&g, px / q)
}

/// A galley with its glyph quads (and rows, bounds) scaled by `k` about its origin. The
/// texture coordinates stay, so the glyphs come from the atlas at the size laid out.
pub fn scale_galley(g: &egui::Galley, k: f32) -> std::sync::Arc<egui::Galley> {
    let rect = |r: Rect| Rect::from_min_max((r.min.to_vec2() * k).to_pos2(), (r.max.to_vec2() * k).to_pos2());
    let mut g = g.clone();
    for placed in &mut g.rows {
        placed.pos = (placed.pos.to_vec2() * k).to_pos2();
        let row = std::sync::Arc::make_mut(&mut placed.row);
        for v in &mut row.visuals.mesh.vertices {
            v.pos = (v.pos.to_vec2() * k).to_pos2();
        }
        row.visuals.mesh_bounds = rect(row.visuals.mesh_bounds);
        row.size *= k;
        for glyph in &mut row.glyphs {
            glyph.pos = (glyph.pos.to_vec2() * k).to_pos2();
            glyph.advance_width *= k;
            glyph.line_height *= k;
        }
    }
    g.rect = rect(g.rect);
    g.mesh_bounds = rect(g.mesh_bounds);
    std::sync::Arc::new(g)
}

const GRID_STEPS: [f64; 13] =
    [10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0, 50000.0, 100000.0];

/// Minor and major grid spacing for the zoom, mm.
pub fn grid_steps(scale: f64) -> (f64, f64) {
    let minor = GRID_STEPS.iter().copied().find(|s| s * scale >= 10.0).unwrap_or(100000.0);
    let major = GRID_STEPS.iter().copied().find(|s| *s > minor && s * scale >= 70.0).unwrap_or(minor * 10.0);
    (minor, major)
}

fn draw_grid(p: &Painter, cam: &Camera, pal: &Palette) {
    let r = cam.rect;
    let a = cam.to_world(r.left_bottom());
    let b = cam.to_world(r.right_top());
    let (minor, major) = grid_steps(cam.scale);
    for (step, colour) in [(minor, pal.grid_minor), (major, pal.grid_major)] {
        let mut x = (a[0] / step).floor() * step;
        while x <= b[0] {
            let sx = cam.to_screen([x, 0.0]).x;
            p.line_segment([Pos2::new(sx, r.top()), Pos2::new(sx, r.bottom())], Stroke::new(1.0, colour));
            x += step;
        }
        let mut y = (a[1] / step).floor() * step;
        while y <= b[1] {
            let sy = cam.to_screen([0.0, y]).y;
            p.line_segment([Pos2::new(r.left(), sy), Pos2::new(r.right(), sy)], Stroke::new(1.0, colour));
            y += step;
        }
    }
    // Origin cross: the centre line of the stage (y axis, green) and the setting line
    // (x axis, red), faint like a CAD model space.
    let o = cam.to_screen([0.0, 0.0]);
    if r.x_range().contains(o.x) {
        p.line_segment([Pos2::new(o.x, r.top()), Pos2::new(o.x, r.bottom())], Stroke::new(1.0, pal.axis_y));
    }
    if r.y_range().contains(o.y) {
        p.line_segment([Pos2::new(r.left(), o.y), Pos2::new(r.right(), o.y)], Stroke::new(1.0, pal.axis_x));
    }
}

/// The picture primitives of an object (underlays), textured from the document's assets.
pub fn draw_images(p: &Painter, cam: &Camera, doc: &Document, prims: &[PlanPrim]) {
    for prim in prims {
        let PlanPrim::Image { corners, asset, opacity } = prim else { continue };
        let Some(t) = image_texture(p.ctx(), asset, doc) else { continue };
        crate::sheets::image_quad(p, t.id(), corners.map(|c| cam.to_screen(c)), *opacity as f32);
    }
}

/// Underlay textures by asset hash (decoding a 4k PNG takes a while; do it once), kept
/// in the context's memory: a texture belongs to one egui context.
fn image_texture(ctx: &egui::Context, hash: &str, doc: &Document) -> Option<egui::TextureHandle> {
    type Cache = std::collections::HashMap<String, Option<egui::TextureHandle>>;
    let key = egui::Id::unique("underlay_textures");
    if let Some(t) = ctx.data(|d| d.get_temp::<Cache>(key).and_then(|c| c.get(hash).cloned())) {
        return t;
    }
    let bytes = doc.assets.get(hash)?.data.clone();
    let max = ctx.input(|i| i.max_texture_side) as u32;
    let t = image::load_from_memory(bytes.as_slice()).ok().map(|d| {
        let d = crate::sheets::fit_texture(d, max);
        let img = egui::ColorImage::from_rgba_unmultiplied([d.width() as usize, d.height() as usize], d.as_raw());
        ctx.load_texture(format!("underlay-{hash}"), img, egui::TextureOptions::LINEAR)
    });
    ctx.data_mut(|d| {
        let c = d.get_temp_mut_or_default::<Cache>(key);
        if c.len() > 16 {
            c.clear();
        }
        c.insert(hash.to_owned(), t.clone());
    });
    t
}

/// Draw plan primitives. `colour` overrides line colours (selection, previews);
/// `alpha` fades fills for ghosts.
pub fn draw_prims(p: &Painter, cam: &Camera, pal: &Palette, prims: &[PlanPrim], colour: Option<Color32>, alpha: f32) {
    let line_colour = colour.unwrap_or(pal.line);
    let extra = if colour.is_some() { 0.6 } else { 0.0 };
    let stroke = |c: LineClass| Stroke::new(theme::line_width(c) + extra, line_colour);
    let pts = |v: &[[f64; 2]]| v.iter().map(|q| cam.to_screen(*q)).collect::<Vec<_>>();
    let zooming = zooming(p.ctx());
    for prim in prims {
        match prim {
            PlanPrim::Polygon { points, fill, line } => {
                let sp = pts(points);
                // Floors let the grid show through a little.
                let a = if *fill == stagecraft_geom::FillClass::Floor { alpha * 0.82 } else { alpha };
                let fill = pal.fill(*fill).gamma_multiply(a);
                fill_polygon(p, points, &sp, fill);
                if let Some(l) = line {
                    let mut closed = sp.clone();
                    closed.push(sp[0]);
                    p.add(Shape::line(closed, stroke(*l)));
                }
            }
            PlanPrim::Polyline { points, closed, line } => {
                let mut sp = pts(points);
                if *closed && !sp.is_empty() {
                    sp.push(sp[0]);
                }
                match line {
                    LineClass::Hidden => {
                        p.extend(Shape::dashed_line(&sp, stroke(*line), 6.0, 4.0));
                    }
                    LineClass::Centre => {
                        p.extend(Shape::dashed_line(&sp, stroke(*line), 14.0, 4.0));
                    }
                    _ => {
                        p.add(Shape::line(sp, stroke(*line)));
                    }
                }
            }
            PlanPrim::Circle { center, radius, fill, line } => {
                let c = cam.to_screen(*center);
                let r = (*radius * cam.scale) as f32;
                let f = fill.map_or(Color32::TRANSPARENT, |f| pal.fill(f).gamma_multiply(alpha));
                p.circle(c, r, f, line.map_or(Stroke::NONE, stroke));
            }
            PlanPrim::Arc { center, radius, start, sweep, line } => {
                let n = ((sweep.abs() / 5.0).ceil() as usize).max(2);
                let v: Vec<Pos2> = (0..=n)
                    .map(|i| cam.to_screen(math::polar(*center, *radius, start + sweep * i as f64 / n as f64)))
                    .collect();
                p.add(Shape::line(v, stroke(*line)));
            }
            PlanPrim::Text { pos, text, height, rotation, anchor } => {
                let px = (*height * cam.scale) as f32 * 1.3;
                if !text_may_show(p.clip_rect(), cam.to_screen(*pos), text, px) {
                    continue;
                }
                let c = colour.unwrap_or(pal.text);
                match anchor {
                    // Symbol labels: capped so they stay labels when zooming in.
                    stagecraft_geom::TextAnchor::Center if *rotation == 0.0 => {
                        if px >= 7.0 {
                            let galley = plan_galley(p, text.clone(), px.min(28.0), c, zooming);
                            let at = cam.to_screen(*pos) - galley.size() / 2.0;
                            p.galley(at, galley, c);
                        }
                    }
                    // Notes and drawing text scale with the plan, like on paper.
                    _ => {
                        if px >= 4.0 {
                            let galley = plan_galley(p, text.clone(), px, c, zooming);
                            let size = galley.size();
                            let a = -(rotation.to_radians() as f32);
                            let dx = if *anchor == stagecraft_geom::TextAnchor::Center { -size.x / 2.0 } else { 0.0 };
                            let (sn, cs) = a.sin_cos();
                            let off = egui::vec2(dx * cs + size.y / 2.0 * sn, dx * sn - size.y / 2.0 * cs);
                            let at = cam.to_screen(*pos) + off;
                            p.add(egui::epaint::TextShape::new(at, galley, c).with_angle(a));
                        }
                    }
                }
            }
            PlanPrim::Image { .. } => {}
        }
    }
}

/// Whether text at `at` (its anchor, screen) of `px` points can reach `clip`, judged
/// before laying it out: a circle around the anchor as wide as the longest line could
/// be. Zoomed in, most notes are off screen; laying them out is what costs.
pub fn text_may_show(clip: Rect, at: Pos2, text: &str, px: f32) -> bool {
    let longest = text.lines().map(|l| l.chars().count()).max().unwrap_or(0) as f32;
    let lines = text.lines().count().max(1) as f32;
    let r = longest * px * 0.7 + lines * px * 1.4 + 4.0;
    clip.expand(r).contains(at)
}

fn fill_polygon(p: &Painter, world: &[[f64; 2]], screen: &[Pos2], fill: Color32) {
    if screen.len() < 3 || fill.a() == 0 {
        return;
    }
    let mut mesh = Mesh::default();
    for s in screen {
        mesh.vertices.push(Vertex { pos: *s, uv: WHITE_UV, color: fill });
    }
    for t in math::triangulate(world) {
        mesh.add_triangle(t[0] as u32, t[1] as u32, t[2] as u32);
    }
    p.add(Shape::mesh(mesh));
}

/// A screen rectangle from two world points.
pub fn screen_rect(cam: &Camera, a: [f64; 2], b: [f64; 2]) -> Rect {
    Rect::from_two_pos(cam.to_screen(a), cam.to_screen(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_steps_grow_when_zooming_out() {
        assert_eq!(grid_steps(0.1), (100.0, 1000.0));
        assert_eq!(grid_steps(0.01), (1000.0, 10000.0));
        let (minor, major) = grid_steps(0.05);
        assert!(minor * 0.05 >= 10.0 && major > minor);
    }

    #[test]
    fn camera_round_trips_and_zooms_about_the_pointer() {
        let mut cam = Camera {
            rect: Rect::from_min_size(Pos2::new(100.0, 50.0), egui::vec2(800.0, 600.0)),
            ..Default::default()
        };
        let w = [1234.0, -567.0];
        let back = cam.to_world(cam.to_screen(w));
        assert!((back[0] - w[0]).abs() < 0.1 && (back[1] - w[1]).abs() < 0.1);
        let at = Pos2::new(300.0, 200.0);
        let before = cam.to_world(at);
        cam.zoom_at(at, 2.5);
        let after = cam.to_world(at);
        assert!((before[0] - after[0]).abs() < 1e-6 && (before[1] - after[1]).abs() < 1e-6);
        // y is up in the world, down on screen.
        assert!(cam.to_screen([0.0, 1000.0]).y < cam.to_screen([0.0, 0.0]).y);
    }

    fn wheel(unit: MouseWheelUnit, dy: f32, modifiers: egui::Modifiers) -> Event {
        Event::MouseWheel {
            unit,
            delta: egui::vec2(0.0, dy),
            modifiers,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
        }
    }

    #[test]
    fn zoom_follows_the_deltas() {
        let none = egui::Modifiers::NONE;
        let f = zoom_factor(&[wheel(MouseWheelUnit::Line, 1.0, none)]);
        assert!((f - WHEEL_ZOOM).abs() < 1e-9);
        // Two notches in one frame zoom twice as far (in log), half a notch half.
        let two = zoom_factor(&[wheel(MouseWheelUnit::Line, 1.0, none), wheel(MouseWheelUnit::Line, 1.0, none)]);
        assert!((two - WHEEL_ZOOM * WHEEL_ZOOM).abs() < 1e-9);
        assert!((zoom_factor(&[wheel(MouseWheelUnit::Line, 0.5, none)]) - WHEEL_ZOOM.sqrt()).abs() < 1e-9);
        // Notches out undo notches in.
        let back = zoom_factor(&[wheel(MouseWheelUnit::Line, 1.0, none), wheel(MouseWheelUnit::Line, -1.0, none)]);
        assert!((back - 1.0).abs() < 1e-9);
        // Trackpad scrolling pans; with ⌘ it zooms by its points.
        assert_eq!(zoom_factor(&[wheel(MouseWheelUnit::Point, 40.0, none)]), 1.0);
        let cmd = zoom_factor(&[wheel(MouseWheelUnit::Point, 40.0, egui::Modifiers::COMMAND)]);
        assert!((cmd - (40.0 * SCROLL_ZOOM).exp()).abs() < 1e-9);
        // Pinch is amplified, and a free-spinning wheel is capped.
        let pinch = zoom_factor(&[Event::Zoom(1.1)]);
        assert!(pinch > 1.1 && (pinch - 1.1f64.powf(PINCH_GAIN)).abs() < 1e-6);
        assert_eq!(zoom_factor(&[wheel(MouseWheelUnit::Line, 40.0, none)]), MAX_FRAME_ZOOM);
    }

    #[test]
    fn zooming_text_meets_few_sizes() {
        // Every size maps to a half octave at or above it, so a zoom over 10..40 points
        // rasterises five sizes, not one per frame.
        let mut sizes: Vec<u32> = (0..300).map(|i| ladder_size(10.0 * 1.0047f32.powi(i)).to_bits()).collect();
        sizes.dedup();
        assert!(sizes.len() <= 5, "{} sizes", sizes.len());
        for px in [4.0f32, 7.3, 11.5, 16.0, 31.9, 60.0] {
            let q = ladder_size(px);
            assert!(q >= px - 1e-3 && q < px * 1.42, "{px} -> {q}");
        }
        assert_eq!(ladder_size(500.0), MAX_TEXT_PX);
    }

    #[test]
    fn a_scaled_galley_keeps_its_glyphs_and_shrinks() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let p = ui.painter();
            let g = p.layout_no_wrap("Note A1".into(), theme::proportional(32.0), Color32::WHITE);
            let s = scale_galley(&g, 0.5);
            assert!((s.size().x - g.size().x * 0.5).abs() < 0.01 && (s.size().y - g.size().y * 0.5).abs() < 0.01);
            let (a, b) = (&g.rows[0].visuals.mesh, &s.rows[0].visuals.mesh);
            assert_eq!(a.vertices.len(), b.vertices.len());
            for (va, vb) in a.vertices.iter().zip(&b.vertices) {
                assert_eq!(va.uv, vb.uv, "same atlas texels");
                assert!((vb.pos.x - va.pos.x * 0.5).abs() < 1e-4);
            }
            // While zooming the plan lays out at a ladder size and scales: same size.
            let z = plan_galley(p, "Note A1".into(), 27.0, Color32::WHITE, true);
            let exact = plan_galley(p, "Note A1".into(), 27.0, Color32::WHITE, false);
            assert!((z.size().x - exact.size().x).abs() < exact.size().x * 0.05);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn off_screen_text_is_skipped_before_layout() {
        let clip = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        assert!(text_may_show(clip, Pos2::new(400.0, 300.0), "A1", 12.0));
        // Starts left of the screen but is long enough to reach into it.
        assert!(text_may_show(clip, Pos2::new(-200.0, 300.0), "a long note that reaches in", 20.0));
        assert!(!text_may_show(clip, Pos2::new(-2000.0, 300.0), "A1", 12.0));
        assert!(!text_may_show(clip, Pos2::new(400.0, 5000.0), "line\nline", 30.0));
    }

    #[test]
    fn plan_primitives_are_kept_until_the_document_changes() {
        let mut e = crate::demo::engine();
        let style = PlanStyle::default();
        let mut cache = PlanCache::default();
        let id = *e.doc().objects.keys().next().unwrap();
        let o = e.doc().objects[&id].clone();
        cache.sync(e.doc(), &style);
        let made = cache.get(e.doc(), &o).to_vec();
        assert_eq!(made, stagecraft_geom::plan(e.doc(), &o, &style));
        // Another frame of the same document: kept (panning and zooming).
        cache.sync(e.doc(), &style);
        assert!(cache.cached(id).is_some() && cache.is_for(e.doc(), &style));
        // Another style (detail by zoom) or an edit: made again.
        let low = PlanStyle { detail: Detail::Low, ..style.clone() };
        assert!(!cache.is_for(e.doc(), &low));
        e.execute("object.move", serde_json::json!({ "ids": [id], "delta": [100.0, 0.0, 0.0] })).unwrap();
        assert!(!cache.is_for(e.doc(), &style));
        cache.sync(e.doc(), &style);
        assert!(cache.cached(id).is_none());
        // A selection is not an edit of the drawing.
        cache.get(e.doc(), &e.doc().objects[&id].clone());
        e.execute("selection.set", serde_json::json!({ "ids": [id] })).unwrap();
        assert!(cache.is_for(e.doc(), &style));
    }
}
