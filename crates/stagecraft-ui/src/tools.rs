//! What the tools do with the pointer and the keyboard on the canvas. Every change is
//! a core command through [`Studio::run`]; drags only change [`Gesture`] until release.

use egui::{Align2, Color32, Event, Key, PointerButton, Pos2, Response, Shape, Stroke, Ui, vec2};
use serde_json::{Value, json};
use stagecraft_core::model::P2;
use stagecraft_core::{Id, Object, ObjectKind, Placement};
use stagecraft_geom::math;
use stagecraft_geom::pick::{BoxMode, in_box, pick, truss_at};
use stagecraft_geom::snap::{Features, SnapKind, Snapped, round_to, snap};
use stagecraft_geom::{Bounds, PlanStyle};

use crate::app::Studio;
use crate::canvas::{draw_prims, screen_rect, tolerance};
use crate::i18n::{t, tf};
use crate::state::{DimInput, Drag, Gesture, Tool};
use crate::theme::Palette;

/// A typed length in mm. Plain numbers are metres ("4.5" = 4500 mm); `mm`, `cm` and
/// `m` suffixes are understood. `None` for anything else.
pub fn parse_length(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let (num, factor) = if let Some(n) = s.strip_suffix("mm") {
        (n, 1.0)
    } else if let Some(n) = s.strip_suffix("cm") {
        (n, 10.0)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 1000.0)
    } else {
        (s.as_str(), 1000.0)
    };
    let v: f64 = num.trim().parse().ok()?;
    v.is_finite().then_some(v * factor)
}

/// A typed angle in degrees ("30", "-45°").
pub fn parse_angle(s: &str) -> Option<f64> {
    let s = s.trim().trim_end_matches('°').replace(',', ".");
    s.trim().parse().ok().filter(|v: &f64| v.is_finite())
}

/// Where a typed length (and optional angle) lands from `from`. Without an angle, the
/// direction is towards the pointer, or along +x if the pointer is on `from`.
pub fn typed_point(from: P2, towards: Option<P2>, dim: &DimInput) -> Option<P2> {
    let len = parse_length(&dim.length)?;
    let angle = match parse_angle(&dim.angle) {
        Some(a) if !dim.angle.trim().is_empty() => a,
        _ => match towards {
            Some(p) if math::dist(p, from) > 1e-6 => math::angle_deg(math::sub(p, from)),
            _ => 0.0,
        },
    };
    Some(math::polar(from, len, angle))
}

fn accepts_char(c: char) -> bool {
    c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+' | 'm' | 'c' | '°')
}

/// The object under the pointer for hover highlighting (select-like tools only).
pub fn hovered(ui: &Ui, resp: &Response, s: &Studio, ids: &[Id], style: &PlanStyle) -> Option<Id> {
    if !matches!(s.ui.tool, Tool::Select | Tool::Move | Tool::Rotate) || s.ui.gesture.drag.is_some() {
        return None;
    }
    let _ = ui;
    let p = s.ui.camera.to_world(resp.hover_pos()?);
    pick(s.engine.doc(), ids.iter().copied(), p, tolerance(&s.ui.camera), style)
}

pub fn interact(ui: &mut Ui, resp: &Response, s: &mut Studio, ids: &[Id], style: &PlanStyle) {
    let cam = s.ui.camera;
    let tol = tolerance(&cam);
    let shift = ui.input(|i| i.modifiers.shift);
    let pointer = resp.hover_pos().or_else(|| resp.interact_pointer_pos());
    let raw = pointer.map(|p| cam.to_world(p));

    // Snapping: not onto what is being moved.
    let moving: Vec<Id> = match (&s.ui.gesture.drag, s.ui.tool) {
        (Some(Drag::Move { ids, .. }), _) => ids.clone(),
        (_, Tool::Move | Tool::Rotate) if !s.ui.gesture.points.is_empty() => s.engine.selection().to_vec(),
        _ => Vec::new(),
    };
    let features = Features::collect(s.engine.doc(), ids.iter().copied(), &moving);
    let from = match &s.ui.gesture.drag {
        Some(Drag::Move { from, .. }) => Some(*from),
        _ => s.ui.gesture.points.last().copied(),
    };
    let ortho = shift && (s.ui.tool.draws() || s.ui.gesture.drag.is_some());
    s.ui.gesture.cursor = raw.map(|p| snap(p, from, ortho, &features, &s.ui.snap, tol));

    if resp.clicked() || resp.drag_started() {
        s.ui.table.focused = false;
    }
    keyboard(ui, s);

    if s.ui.tool == Tool::Select {
        select_tool(ui, resp, s, ids, style, raw, shift, &features, tol);
        return;
    }
    let Some(cursor) = s.ui.gesture.cursor else { return };
    if resp.double_clicked() {
        finish(s);
        return;
    }
    if !resp.clicked_by(PointerButton::Primary) {
        return;
    }
    let raw = raw.unwrap_or(cursor.point);
    let p = cursor.point;
    match s.ui.tool {
        Tool::Select => unreachable!(),
        Tool::Wall => s.ui.gesture.points.push(p),
        Tool::Floor => {
            let pts = &s.ui.gesture.points;
            if pts.len() >= 3 && math::dist(pts[0], p) <= tol {
                finish(s);
            } else {
                s.ui.gesture.points.push(p);
            }
        }
        Tool::Truss => {
            if s.ui.gesture.points.is_empty() {
                s.ui.gesture.points.push(p);
            } else {
                let a = s.ui.gesture.points[0];
                create_truss(s, a, p);
            }
        }
        Tool::Fixture => place_fixture(s, ids, raw, cursor, tol),
        Tool::Move | Tool::Rotate => {
            if s.engine.selection().is_empty() {
                if let Some(id) = pick(s.engine.doc(), ids.iter().copied(), raw, tol, style) {
                    let _ = s.run("selection.set", json!({ "ids": [id] }));
                }
                return;
            }
            s.ui.gesture.points.push(p);
            let pts = s.ui.gesture.points.clone();
            match (s.ui.tool, pts.len()) {
                (Tool::Move, 2) => commit_move(s, math::sub(pts[1], pts[0])),
                (Tool::Rotate, 3) => {
                    let a0 = math::angle_deg(math::sub(pts[1], pts[0]));
                    let a1 = math::angle_deg(math::sub(pts[2], pts[0]));
                    commit_rotate(s, pts[0], a1 - a0);
                }
                _ => {}
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn select_tool(
    ui: &Ui,
    resp: &Response,
    s: &mut Studio,
    ids: &[Id],
    style: &PlanStyle,
    raw: Option<P2>,
    shift: bool,
    features: &Features,
    tol: f64,
) {
    let cam = s.ui.camera;
    if ui.input(|i| i.key_down(Key::Space)) {
        return; // panning
    }
    if resp.drag_started_by(PointerButton::Primary) {
        let press = ui.input(|i| i.pointer.press_origin()).map(|p| cam.to_world(p)).or(raw);
        let Some(press) = press else { return };
        match pick(s.engine.doc(), ids.iter().copied(), press, tol, style) {
            Some(hit) => {
                if !s.engine.selection().contains(&hit) {
                    let ids = if shift { add_to(s.engine.selection(), hit) } else { vec![hit] };
                    let _ = s.run("selection.set", json!({ "ids": ids }));
                }
                // Grab on the object's own snap points, so endpoints land on endpoints.
                let own = Features::collect(s.engine.doc(), s.engine.selection().iter().copied(), &[]);
                let grab = snap(press, None, false, &own, &s.ui.snap, tol);
                let grab = if grab.kind == SnapKind::Grid { press } else { grab.point };
                s.ui.gesture.drag = Some(Drag::Move { from: grab, ids: s.engine.selection().to_vec() });
            }
            None => s.ui.gesture.drag = Some(Drag::Box { from: press }),
        }
        return;
    }
    if resp.drag_stopped() {
        let drag = s.ui.gesture.drag.take();
        let Some(raw) = raw else { return };
        match drag {
            Some(Drag::Box { from }) => {
                let mode = if raw[0] >= from[0] { BoxMode::Window } else { BoxMode::Crossing };
                let mut hit = in_box(s.engine.doc(), ids.iter().copied(), Bounds::from_points(from, raw), mode, style);
                if shift {
                    for id in s.engine.selection() {
                        if !hit.contains(id) {
                            hit.push(*id);
                        }
                    }
                }
                let _ = s.run("selection.set", json!({ "ids": hit }));
            }
            Some(Drag::Move { from, ids: moved }) => {
                let to = snap(raw, Some(from), shift, features, &s.ui.snap, tol).point;
                let d = math::sub(to, from);
                if math::length(d) > 1e-6 {
                    let _ = s.run("object.move", json!({ "ids": moved, "delta": [d[0], d[1], 0.0] }));
                }
            }
            None => {}
        }
        return;
    }
    if resp.clicked_by(PointerButton::Primary) {
        let Some(raw) = raw else { return };
        let hit = pick(s.engine.doc(), ids.iter().copied(), raw, tol, style);
        let sel = s.engine.selection().to_vec();
        let ids = match (hit, shift) {
            (Some(h), true) if sel.contains(&h) => sel.into_iter().filter(|x| *x != h).collect(),
            (Some(h), true) => add_to(&sel, h),
            (Some(h), false) => vec![h],
            (None, true) => sel,
            (None, false) => Vec::new(),
        };
        let _ = s.run("selection.set", json!({ "ids": ids }));
    }
}

fn add_to(sel: &[Id], id: Id) -> Vec<Id> {
    let mut v = sel.to_vec();
    if !v.contains(&id) {
        v.push(id);
    }
    v
}

/// Typed dimensions, Enter, Escape, Backspace while a tool draws. Only when no text
/// field has focus, so typing in the inspector is left alone.
fn keyboard(ui: &mut Ui, s: &mut Studio) {
    if ui.ctx().memory(|m| m.focused().is_some()) || s.ui.palette.open || s.ui.table.focused {
        return;
    }
    let g = &mut s.ui.gesture;
    let typing = s.ui.tool.draws() && !g.points.is_empty();
    if typing {
        let text: String = ui.input_mut(|i| {
            let mut out = String::new();
            i.events.retain(|e| match e {
                Event::Text(t) if t.chars().all(accepts_char) => {
                    out.push_str(t);
                    false
                }
                _ => true,
            });
            out
        });
        if !text.is_empty() {
            let dim = g.dim.get_or_insert_with(DimInput::default);
            let field = if dim.on_angle { &mut dim.angle } else { &mut dim.length };
            field.push_str(&text.replace(',', "."));
        }
        if let Some(d) = g.dim.as_mut()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab))
        {
            d.on_angle = !d.on_angle;
        }
        if let Some(d) = g.dim.as_mut()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Backspace))
        {
            let field = if d.on_angle { &mut d.angle } else { &mut d.length };
            field.pop();
        }
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter)) {
        enter(s);
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
        let g = &mut s.ui.gesture;
        if g.dim.is_some() {
            g.dim = None;
        } else if !g.points.is_empty() || g.drag.is_some() {
            *g = Gesture::default();
        } else if s.ui.tool != Tool::Select {
            s.ui.set_tool(Tool::Select);
        } else {
            let _ = s.run("selection.clear", Value::Null);
        }
    }
}

/// Enter: take the typed value, or finish the polyline/polygon.
fn enter(s: &mut Studio) {
    let g = &s.ui.gesture;
    let dim = g.dim.clone().filter(|d| !d.length.trim().is_empty() || !d.angle.trim().is_empty());
    let Some(dim) = dim else {
        finish(s);
        return;
    };
    let Some(&last) = g.points.last() else { return };
    let towards = g.cursor.map(|c| c.point);
    if s.ui.tool == Tool::Rotate {
        // After the pivot, the typed number is the angle.
        let text = if dim.length.trim().is_empty() { &dim.angle } else { &dim.length };
        match parse_angle(text) {
            Some(a) => commit_rotate(s, g.points[0], a),
            None => s.ui.error(tf("status.bad_angle", &[("text", text)])),
        }
        return;
    }
    let Some(p) = typed_point(last, towards, &dim) else {
        s.ui.error(tf("status.bad_length", &[("text", &dim.length)]));
        return;
    };
    s.ui.gesture.dim = None;
    match s.ui.tool {
        Tool::Wall | Tool::Floor => s.ui.gesture.points.push(p),
        Tool::Truss => create_truss(s, last, p),
        Tool::Move => commit_move(s, math::sub(p, last)),
        _ => {}
    }
}

/// End the polyline or polygon being drawn.
fn finish(s: &mut Studio) {
    let mut pts = std::mem::take(&mut s.ui.gesture.points);
    pts.dedup_by(|a, b| math::dist(*a, *b) < 1e-6);
    s.ui.gesture = Gesture::default();
    let layer = s.active_layer_arg();
    match s.ui.tool {
        Tool::Wall if pts.len() >= 2 => {
            let o = &s.ui.options;
            let args = json!({ "path": pts, "height": o.wall_height, "thickness": o.wall_thickness, "layer": layer });
            created(s, "wall.create", args, "tool.wall");
        }
        Tool::Floor if pts.len() >= 3 => {
            created(s, "floor.create", json!({ "outline": pts, "layer": layer }), "tool.floor");
        }
        Tool::Wall | Tool::Floor if !pts.is_empty() => s.ui.error(t("status.too_few_points")),
        _ => {}
    }
}

fn created(s: &mut Studio, cmd: &str, args: Value, what: &str) {
    if let Ok(v) = s.run(cmd, args) {
        if let Some(id) = v.get("id") {
            let _ = s.run("selection.set", json!({ "ids": [id] }));
        }
        s.ui.info(tf("status.created", &[("what", &t(what))]));
    }
}

fn create_truss(s: &mut Studio, a: P2, b: P2) {
    s.ui.gesture = Gesture::default();
    if math::dist(a, b) < 1.0 {
        return;
    }
    let o = &s.ui.options;
    let h = o.truss_height;
    let args = json!({
        "start": [a[0], a[1], h], "end": [b[0], b[1], h],
        "profile": o.truss_profile, "layer": s.active_layer_arg(),
    });
    created(s, "truss.create", args, "tool.truss");
}

/// Click with the fixture tool: on a truss it hangs there (under the bottom chord),
/// elsewhere it stands on the layer.
fn place_fixture(s: &mut Studio, ids: &[Id], raw: P2, cursor: Snapped, tol: f64) {
    let doc = s.engine.doc();
    let args = match truss_at(doc, ids.iter().copied(), raw, tol) {
        Some(hit) => {
            let length = match &doc.objects[&hit.id].kind {
                ObjectKind::Truss { length, .. } => *length,
                _ => 0.0,
            };
            let along = match s.ui.snap.grid {
                Some(g) => round_to(hit.along, g).clamp(0.0, length),
                None => hit.along,
            };
            json!({ "parent": hit.id, "position": [along, 0.0, -hit.height / 2.0] })
        }
        None => json!({ "position": [cursor.point[0], cursor.point[1], 0.0], "layer": s.active_layer_arg() }),
    };
    created(s, "fixture.create", args, "tool.fixture");
}

fn commit_move(s: &mut Studio, d: P2) {
    s.ui.gesture = Gesture::default();
    let ids = s.engine.selection().to_vec();
    if ids.is_empty() || math::length(d) < 1e-6 {
        return;
    }
    let _ = s.run("object.move", json!({ "ids": ids, "delta": [d[0], d[1], 0.0] }));
}

fn commit_rotate(s: &mut Studio, pivot: P2, angle: f64) {
    s.ui.gesture = Gesture::default();
    let ids = s.engine.selection().to_vec();
    if ids.is_empty() || angle.abs() < 1e-9 {
        return;
    }
    let _ = s.run("object.rotate", json!({ "ids": ids, "angle": angle, "pivot": [pivot[0], pivot[1], 0.0] }));
}

/// The ghost of an object kind at a placement, for previews of things not yet created.
fn ghost(kind: ObjectKind, placement: Placement, style: &PlanStyle) -> Vec<stagecraft_geom::PlanPrim> {
    let obj = Object {
        id: Id::nil(),
        name: String::new(),
        layer: Id::nil(),
        tags: Vec::new(),
        parent: None,
        placement,
        kind,
    };
    let w = stagecraft_core::geom::placement_to_affine(&obj.placement);
    match &obj.kind {
        ObjectKind::Truss { .. } => stagecraft_geom::kinds::truss::plan(&w, &obj, style),
        ObjectKind::Wall { .. } => stagecraft_geom::kinds::wall::plan(&w, &obj, style),
        ObjectKind::Floor { .. } => stagecraft_geom::kinds::floor::plan(&w, &obj, style),
        ObjectKind::Fixture { .. } => stagecraft_geom::kinds::fixture::plan(&w, &obj, style),
    }
}

/// Previews, the snap marker, the selection box and the typed-dimension box.
pub fn draw_overlay(ui: &Ui, p: &egui::Painter, s: &Studio, pal: &Palette, style: &PlanStyle) {
    let cam = &s.ui.camera;
    let g = &s.ui.gesture;
    let cursor = g.cursor.map(|c| c.point);
    let thin = Stroke::new(1.0, pal.preview);
    let doc = s.engine.doc();
    let ghosts = |prims: Vec<stagecraft_geom::PlanPrim>| draw_prims(p, cam, pal, &prims, Some(pal.preview), 0.5);
    let sel_prims = |f: &dyn Fn(&stagecraft_geom::PlanPrim) -> stagecraft_geom::PlanPrim| {
        for id in s.engine.selection() {
            if let Some(o) = doc.objects.get(id) {
                let prims: Vec<_> = stagecraft_geom::plan(doc, o, style).iter().map(f).collect();
                draw_prims(p, cam, pal, &prims, Some(pal.preview), 0.5);
            }
        }
    };

    match (&g.drag, cursor) {
        (Some(Drag::Box { from }), _) => {
            if let Some(raw) = ui.input(|i| i.pointer.hover_pos()).map(|q| cam.to_world(q)) {
                let r = screen_rect(cam, *from, raw);
                let window = raw[0] >= from[0];
                let (fill, stroke) = if window { (pal.box_window, pal.selected) } else { (pal.box_crossing, pal.snap) };
                p.rect_filled(r, 0.0, fill);
                p.rect_stroke(r, 0.0, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
            }
        }
        (Some(Drag::Move { from, .. }), Some(c)) => {
            let d = math::sub(c, *from);
            sel_prims(&|q| q.translated(d));
            p.line_segment([cam.to_screen(*from), cam.to_screen(c)], thin);
            length_label(p, cam, pal, *from, c);
        }
        _ => {}
    }

    let pts = &g.points;
    let o = &s.ui.options;
    match (s.ui.tool, cursor) {
        (Tool::Wall, Some(c)) if !pts.is_empty() => {
            let mut path = pts.clone();
            path.push(c);
            ghosts(ghost(
                ObjectKind::Wall { path, height: o.wall_height, thickness: o.wall_thickness },
                Placement::default(),
                style,
            ));
            length_label(p, cam, pal, *pts.last().expect("non-empty"), c);
        }
        (Tool::Floor, Some(c)) if !pts.is_empty() => {
            let mut sp: Vec<Pos2> = pts.iter().map(|q| cam.to_screen(*q)).collect();
            sp.push(cam.to_screen(c));
            p.add(Shape::line(sp.clone(), Stroke::new(1.5, pal.preview)));
            if pts.len() >= 2 {
                p.extend(Shape::dashed_line(&[sp[sp.len() - 1], sp[0]], thin, 6.0, 4.0));
            }
            length_label(p, cam, pal, *pts.last().expect("non-empty"), c);
        }
        (Tool::Truss, Some(c)) if !pts.is_empty() => {
            let a = pts[0];
            let d = math::sub(c, a);
            let len = math::length(d);
            if len > 1.0 {
                let placement = Placement { position: [a[0], a[1], 0.0], rotation: [0.0, 0.0, math::angle_deg(d)] };
                ghosts(ghost(ObjectKind::Truss { length: len, profile: o.truss_profile.clone() }, placement, style));
            }
            length_label(p, cam, pal, a, c);
        }
        (Tool::Move, Some(c)) if !pts.is_empty() => {
            let d = math::sub(c, pts[0]);
            sel_prims(&|q| q.translated(d));
            p.line_segment([cam.to_screen(pts[0]), cam.to_screen(c)], thin);
            length_label(p, cam, pal, pts[0], c);
        }
        (Tool::Rotate, Some(c)) if !pts.is_empty() => {
            let pivot = pts[0];
            p.circle_stroke(cam.to_screen(pivot), 4.0, thin);
            p.line_segment([cam.to_screen(pivot), cam.to_screen(c)], thin);
            if pts.len() >= 2 {
                p.line_segment([cam.to_screen(pivot), cam.to_screen(pts[1])], thin);
                let a = math::angle_deg(math::sub(c, pivot)) - math::angle_deg(math::sub(pts[1], pivot));
                sel_prims(&|q| q.rotated(pivot, a));
                let label = format!("{a:.1}°");
                p.text(
                    cam.to_screen(c) + vec2(12.0, -12.0),
                    Align2::LEFT_BOTTOM,
                    label,
                    crate::theme::proportional(12.0),
                    pal.preview,
                );
            }
        }
        (Tool::Fixture, Some(c)) => {
            let placement = Placement { position: [c[0], c[1], 0.0], rotation: [0.0; 3] };
            let kind = ObjectKind::Fixture { fid: None, cid: None, gdtf: None, mode: None, address: None };
            ghosts(ghost(kind, placement, style));
        }
        _ => {}
    }

    if let Some(c) = g.cursor
        && s.ui.tool != Tool::Select
    {
        snap_marker(p, cam.to_screen(c.point), c.kind, pal);
    }

    if let Some(dim) = &g.dim
        && let Some(pos) = ui.input(|i| i.pointer.hover_pos()).or(cursor.map(|c| cam.to_screen(c)))
    {
        dim_box(ui, pos, dim, s.ui.tool == Tool::Rotate);
    }
}

fn length_label(p: &egui::Painter, cam: &crate::state::Camera, pal: &Palette, a: P2, b: P2) {
    let len = math::dist(a, b);
    if len < 1.0 {
        return;
    }
    let mid = cam.to_screen(math::midpoint(a, b));
    let text = format!("{:.2} m", len / 1000.0);
    let font = crate::theme::proportional(12.0);
    let galley = p.layout_no_wrap(text, font, pal.preview);
    let r = Align2::CENTER_BOTTOM.anchor_size(mid - vec2(0.0, 4.0), galley.size()).expand(2.0);
    p.rect_filled(r, 3.0, pal.canvas.gamma_multiply(0.85));
    p.galley(r.min + vec2(2.0, 2.0), galley, pal.preview);
}

fn snap_marker(p: &egui::Painter, at: Pos2, kind: SnapKind, pal: &Palette) {
    let st = Stroke::new(1.5, pal.snap);
    match kind {
        SnapKind::Endpoint => {
            p.rect_stroke(egui::Rect::from_center_size(at, vec2(9.0, 9.0)), 0.0, st, egui::StrokeKind::Middle);
        }
        SnapKind::Midpoint => {
            let pts = vec![at + vec2(0.0, -6.0), at + vec2(5.5, 4.0), at + vec2(-5.5, 4.0), at + vec2(0.0, -6.0)];
            p.add(Shape::line(pts, st));
        }
        SnapKind::Truss => {
            p.circle_stroke(at, 5.0, st);
        }
        SnapKind::Orthogonal => {
            p.line_segment([at + vec2(-6.0, 0.0), at + vec2(6.0, 0.0)], st);
            p.line_segment([at + vec2(0.0, -6.0), at + vec2(0.0, 6.0)], st);
        }
        SnapKind::Grid => {
            let c = Color32::from_gray(140);
            p.line_segment([at + vec2(-4.0, -4.0), at + vec2(4.0, 4.0)], Stroke::new(1.0, c));
            p.line_segment([at + vec2(-4.0, 4.0), at + vec2(4.0, -4.0)], Stroke::new(1.0, c));
        }
        SnapKind::Free => {}
    }
}

/// The small box next to the pointer showing what is being typed.
fn dim_box(ui: &Ui, at: Pos2, dim: &DimInput, angle_only: bool) {
    egui::Area::new(egui::Id::new("dim_input"))
        .fixed_pos(at + vec2(18.0, 18.0))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if angle_only {
                        ui.label(format!("{}: {}°", t("dim.angle"), dim.length));
                        return;
                    }
                    let mark = |on: bool| if on { "▸ " } else { "" };
                    ui.label(format!("{}{}: {}", mark(!dim.on_angle), t("dim.length"), dim.length));
                    ui.separator();
                    ui.label(format!("{}{}: {}", mark(dim.on_angle), t("dim.angle"), dim.angle));
                });
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_lengths_are_metres_unless_suffixed() {
        assert_eq!(parse_length("4.5"), Some(4500.0));
        assert_eq!(parse_length("4,5"), Some(4500.0));
        assert_eq!(parse_length("450mm"), Some(450.0));
        assert_eq!(parse_length("45 cm"), Some(450.0));
        assert_eq!(parse_length("2m"), Some(2000.0));
        assert_eq!(parse_length("abc"), None);
        assert_eq!(parse_angle("-45°"), Some(-45.0));
    }

    #[test]
    fn typed_point_uses_the_pointer_direction_or_the_angle() {
        let dim = DimInput { length: "2".into(), ..Default::default() };
        let p = typed_point([0.0, 0.0], Some([0.0, 10.0]), &dim).unwrap();
        assert!((p[0]).abs() < 1e-9 && (p[1] - 2000.0).abs() < 1e-9);
        let p = typed_point([0.0, 0.0], None, &dim).unwrap();
        assert_eq!(p, [2000.0, 0.0]);
        let dim = DimInput { length: "1".into(), angle: "90".into(), on_angle: true };
        let p = typed_point([100.0, 0.0], Some([5000.0, 0.0]), &dim).unwrap();
        assert!((p[0] - 100.0).abs() < 1e-9 && (p[1] - 1000.0).abs() < 1e-9);
    }
}
