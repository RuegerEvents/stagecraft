//! The plan canvas: camera, grid, objects, overlays. Interaction is in [`crate::tools`].

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Align2, Color32, Event, MouseWheelUnit, Painter, PointerButton, Pos2, Rect, Sense, Shape, Stroke, Ui};
use stagecraft_core::{Document, Id};
use stagecraft_geom::{Detail, LineClass, PlanPrim, PlanStyle, math};

use crate::app::Studio;
use crate::i18n::t;
use crate::state::{Camera, UiState};
use crate::theme::{self, Palette};

/// Objects drawn in the plan: on a visible layer and carrying no hidden tag.
pub fn visible_ids(doc: &Document, ui: &UiState) -> Vec<Id> {
    let mut v: Vec<(u8, Id)> = doc
        .objects
        .values()
        .filter(|o| doc.layers.get(&o.layer).is_none_or(|l| l.visible))
        .filter(|o| !o.tags.iter().any(|t| ui.hidden_tags.contains(t)))
        .map(|o| (stagecraft_geom::pick::rank(&o.kind), o.id))
        .collect();
    // Back to front: floors first, fixtures last.
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    v.into_iter().map(|x| x.1).collect()
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
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, t("canvas.name")));
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
    let hover = crate::tools::hovered(ui, &resp, s, &ids, &style);
    // While a sketch point is dragged, draw the document as the drag would leave it.
    let drag = match &s.ui.gesture.drag {
        Some(crate::state::Drag::SketchPoint { layer, point }) => {
            s.ui.gesture.cursor.map(|c| (*layer, *point, c.point))
        }
        _ => None,
    };
    let preview = s.ui.sketch.preview_doc(s.engine.doc(), drag).cloned();
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
        let prims = stagecraft_geom::plan(doc, o, &style);
        draw_prims(&painter, &s.ui.camera, &pal, &prims, colour, 1.0);
    }

    draw_conflicts(&painter, &s.ui.camera, doc, &ids);
    crate::sketch_ui::draw(&painter, &s.ui.camera, &pal, doc, &mut s.ui.sketch);

    if let Some(t) = &template {
        draw_labels(&painter, &s.ui.camera, &pal, doc, &ids, t);
    }

    crate::tools::interact(ui, &resp, s, &ids, &style);
    crate::tools::draw_overlay(ui, &painter, s, &pal, &style);
    crate::sketch_ui::editor(ui, s);
}

/// A warning ring around every fixture whose patch overlaps another or runs past
/// its universe.
fn draw_conflicts(painter: &Painter, cam: &Camera, doc: &Document, ids: &[Id]) {
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
        painter.circle_stroke(p, r.max(6.0), Stroke::new(2.0, theme::WARNING));
        painter.circle_stroke(p, r.max(6.0) + 3.0, Stroke::new(1.0, theme::WARNING.gamma_multiply(0.5)));
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

/// Scroll and pinch. Mouse wheels (line units) zoom, trackpad scrolling (point units)
/// pans, pinch or ⌘-scroll zooms; middle or Space-drag pans.
fn navigate(ui: &Ui, resp: &egui::Response, cam: &mut Camera) {
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let over = pointer.is_some_and(|p| cam.rect.contains(p));
    if over {
        let (events, zoom) = ui.input(|i| (i.events.clone(), i.zoom_delta()));
        let at = pointer.unwrap_or(cam.rect.center());
        for e in events {
            if let Event::MouseWheel { unit, delta, modifiers, .. } = e {
                if modifiers.command || modifiers.ctrl {
                    continue; // egui turns these into zoom_delta
                }
                match unit {
                    MouseWheelUnit::Line | MouseWheelUnit::Page => cam.zoom_at(at, 1.15f64.powf(delta.y as f64)),
                    MouseWheelUnit::Point => cam.pan_screen(delta),
                }
            }
        }
        if zoom != 1.0 {
            cam.zoom_at(at, zoom as f64);
        }
    }
    let space = ui.input(|i| i.key_down(egui::Key::Space));
    if resp.dragged_by(PointerButton::Middle) || (space && resp.dragged_by(PointerButton::Primary)) {
        cam.pan_screen(resp.drag_delta());
    }
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
    // Origin cross: centre line of the stage and the setting line.
    let o = cam.to_screen([0.0, 0.0]);
    let st = Stroke::new(1.0, pal.axis);
    if r.x_range().contains(o.x) {
        p.add(Shape::dashed_line(&[Pos2::new(o.x, r.top()), Pos2::new(o.x, r.bottom())], st, 10.0, 4.0));
    }
    if r.y_range().contains(o.y) {
        p.add(Shape::dashed_line(&[Pos2::new(r.left(), o.y), Pos2::new(r.right(), o.y)], st, 10.0, 4.0));
    }
}

/// Draw plan primitives. `colour` overrides line colours (selection, previews);
/// `alpha` fades fills for ghosts.
pub fn draw_prims(p: &Painter, cam: &Camera, pal: &Palette, prims: &[PlanPrim], colour: Option<Color32>, alpha: f32) {
    let line_colour = colour.unwrap_or(pal.line);
    let extra = if colour.is_some() { 0.6 } else { 0.0 };
    let stroke = |c: LineClass| Stroke::new(theme::line_width(c) + extra, line_colour);
    let pts = |v: &[[f64; 2]]| v.iter().map(|q| cam.to_screen(*q)).collect::<Vec<_>>();
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
            PlanPrim::Text { pos, text, height, .. } => {
                let px = (*height * cam.scale) as f32 * 1.3;
                if px >= 7.0 {
                    let font = theme::proportional(px.min(28.0));
                    p.text(cam.to_screen(*pos), Align2::CENTER_CENTER, text, font, colour.unwrap_or(pal.text));
                }
            }
        }
    }
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
}
