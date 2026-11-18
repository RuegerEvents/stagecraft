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
use crate::state::{DimField, DimInput, Drag, Gesture, SegHint, Tool};
use crate::theme::Palette;

/// A typed angle in degrees ("30", "-45°").
pub fn parse_angle(s: &str) -> Option<f64> {
    let s = s.trim().trim_end_matches('°').replace(',', ".");
    s.trim().parse().ok().filter(|v: &f64| v.is_finite())
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
        // A dragged sketch point must not snap to what follows it.
        (Some(Drag::SketchPoint { layer, .. }), _) => stagecraft_core::sketch::linked_on(s.engine.doc(), *layer),
        (_, Tool::Move | Tool::Rotate) if !s.ui.gesture.points.is_empty() => s.engine.selection().to_vec(),
        _ => Vec::new(),
    };
    let mut features = Features::collect(s.engine.doc(), ids.iter().copied(), &moving);
    if !matches!(s.ui.gesture.drag, Some(Drag::SketchPoint { .. })) {
        features.endpoints.extend(crate::sketch_ui::snap_points(s.engine.doc()));
    }
    let from = match &s.ui.gesture.drag {
        Some(Drag::Move { from, .. }) => Some(*from),
        _ => s.ui.gesture.points.last().copied(),
    };
    let ortho = shift && (s.ui.tool.draws() || s.ui.gesture.drag.is_some());
    s.ui.gesture.cursor = raw.map(|p| snap(p, from, ortho, &features, &s.ui.snap, tol));
    s.ui.gesture.shift = shift;

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
    // What is typed in the heads-up boxes holds for a click too: a locked length or
    // angle wins over the pointer (Shift still squares its direction). The boxes empty
    // for the next segment.
    let typed = match crate::hud::kind(s) {
        Some(crate::hud::Kind::LengthAngle) => s.ui.gesture.dim.take(),
        _ => None,
    };
    let p = match (s.ui.tool, s.ui.gesture.points.as_slice()) {
        (Tool::Truss | Tool::Curtain | Tool::Move, [first, ..]) => {
            crate::hud::constrained(*first, cursor.point, typed.as_ref(), 0.0)
        }
        (_, [.., last]) => {
            crate::hud::constrained(*last, cursor.point, typed.as_ref(), crate::hud::prev_dir(&s.ui.gesture.points))
        }
        _ => cursor.point,
    };
    let typed_len = typed.as_ref().and_then(|d| d.length_mm());
    match s.ui.tool {
        Tool::Select => unreachable!(),
        Tool::Wall | Tool::Floor | Tool::Ceiling | Tool::Run => {
            let pts = &s.ui.gesture.points;
            if pts.len() >= 3 && math::dist(pts[0], p) <= tol {
                // A run or wall clicked shut closes into a ring.
                if matches!(s.ui.tool, Tool::Run | Tool::Wall) {
                    let first = pts[0];
                    s.ui.gesture.points.push(first);
                    s.ui.gesture.segs.push(SegHint { axis_lock: shift, length: None });
                }
                finish(s);
            } else {
                if !s.ui.gesture.points.is_empty() {
                    s.ui.gesture.segs.push(SegHint { axis_lock: shift, length: typed_len });
                }
                s.ui.gesture.points.push(p);
            }
        }
        Tool::Dimension => {
            let pick = crate::sketch_ui::pick(s, raw, &cursor, tol);
            let picks = s.ui.gesture.picks.clone();
            match picks.as_slice() {
                [a, b] => {
                    let value = s.ui.gesture.dim.as_ref().and_then(|d| d.length_mm());
                    s.ui.gesture = Gesture::default();
                    commit_dimension(s, *a, *b, raw, value);
                }
                [a] if math::dist(a.at(), pick.at()) < 1e-6 => {}
                _ => s.ui.gesture.picks.push(pick),
            }
        }
        Tool::Curtain => {
            if s.ui.gesture.points.is_empty() {
                s.ui.gesture.points.push(p);
            } else {
                let a = s.ui.gesture.points[0];
                create_curtain(s, ids, a, p, tol);
            }
        }
        Tool::Calibrate => {
            if s.ui.gesture.points.is_empty() && calibration_target(s, ids, p).is_none() {
                s.ui.error(t("status.no_underlay"));
                return;
            }
            if s.ui.gesture.points.len() < 2 {
                s.ui.gesture.points.push(p);
            }
            if s.ui.gesture.points.len() == 2 {
                s.ui.info(t("status.calibrate_type"));
            }
        }
        Tool::Batten | Tool::Proscenium | Tool::Wings | Tool::Platform | Tool::Support | Tool::Hoist => {
            place_stage(s, ids, raw, p, tol);
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
            // A typed angle turns by that much on the click that sets the new direction.
            if s.ui.tool == Tool::Rotate
                && s.ui.gesture.points.len() >= 2
                && let Some(a) = s.ui.gesture.dim.as_ref().and_then(|d| d.angle_deg())
            {
                let pivot = s.ui.gesture.points[0];
                commit_rotate(s, pivot, a);
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
        if s.ui.sketch.show {
            let press_screen = cam.to_screen(press);
            if s.ui.sketch.dimensions
                && let Some(d) = crate::sketch_ui::dim_at(s.engine.doc(), &cam, press_screen)
            {
                s.ui.sketch.selected = Some(d.source);
                s.ui.gesture.drag = Some(Drag::Label { source: d.source, from: press });
                return;
            }
            if let Some((layer, point, _)) = crate::sketch_ui::point_at(s.engine.doc(), press, tol) {
                s.ui.gesture.drag = Some(Drag::SketchPoint { layer, point });
                return;
            }
        }
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
                crate::sketch_ui::clear_picks(&mut s.ui.sketch);
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
            Some(Drag::SketchPoint { layer, point }) => {
                let to = s.ui.gesture.cursor.map_or(raw, |c| c.point);
                let args = json!({ "layer": layer, "targets": [{ "point": point, "to": { "x": to[0], "y": to[1] } }] });
                let _ = s.run("sketch.drag", args);
            }
            Some(Drag::Label { source, from }) => {
                let d = math::sub(raw, from);
                if math::length(d) > 1e-6 {
                    move_label(s, source, d);
                }
            }
            None => {}
        }
        return;
    }
    // Double (or quicker triple) click on a dimension edits its value; a click
    // selects it. A double click on a sketch corner locks or unlocks it.
    let double = resp.double_clicked() || resp.triple_clicked();
    if double
        && s.ui.sketch.show
        && let Some(raw) = raw
        && crate::sketch_ui::dim_at(s.engine.doc(), &cam, cam.to_screen(raw)).is_none()
        && let Some((layer, point, at)) = crate::sketch_ui::point_at(s.engine.doc(), raw, tol)
    {
        crate::sketch_ui::toggle_lock(s, layer, point, at);
        return;
    }
    if s.ui.sketch.show
        && s.ui.sketch.dimensions
        && (resp.clicked_by(PointerButton::Primary) || double)
        && let Some(pos) = resp.interact_pointer_pos().or(resp.hover_pos())
        && let Some(d) = crate::sketch_ui::dim_at(s.engine.doc(), &cam, pos)
    {
        if double {
            crate::sketch_ui::open_editor(s, d.source);
        } else {
            s.ui.sketch.selected = Some(d.source);
            if !s.engine.selection().is_empty() {
                let _ = s.run("selection.clear", Value::Null);
            }
        }
        return;
    }
    if resp.clicked_by(PointerButton::Primary) {
        let Some(raw) = raw else { return };
        // Sketch points and lines are drawn over the objects, so they are picked first.
        if crate::sketch_ui::click_pick(s, raw, tol, shift) {
            return;
        }
        crate::sketch_ui::clear_picks(&mut s.ui.sketch);
        s.ui.sketch.selected = None;
        s.ui.sketch.editing = None;
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

/// Moves a dimension's label (and its line) by `d` world mm.
fn move_label(s: &mut Studio, src: stagecraft_geom::dims::DimSource, d: P2) {
    use stagecraft_geom::dims::DimSource;
    let doc = s.engine.doc();
    let old = match src {
        DimSource::Sketch { layer, id } => {
            doc.sketches.get(&layer).and_then(|k| k.dimension(id)).map(|x| [x.label_offset.x, x.label_offset.y])
        }
        DimSource::Reference(id) => doc.dimensions.get(&id).map(|x| x.label_offset),
    };
    let Some(old) = old else { return };
    let mut args = crate::sketch_ui::dim_args(src);
    args["offset"] = json!(math::add(old, d));
    let _ = s.run("dimension.move_label", args);
}

fn add_to(sel: &[Id], id: Id) -> Vec<Id> {
    let mut v = sel.to_vec();
    if !v.contains(&id) {
        v.push(id);
    }
    v
}

/// Text events this frame that would start or continue a typed dimension. The window's
/// tool shortcuts look first, so a digit and a unit letter in one frame stay typing.
pub fn typing_now(ui: &Ui, s: &Studio) -> bool {
    if s.ui.gesture.dim.is_some() {
        return true;
    }
    crate::hud::takes_keys(s)
        && ui.input(|i| i.events.iter().any(|e| matches!(e, Event::Text(t) if crate::hud::accepts_text(t, false))))
}

/// Typed dimensions (the heads-up boxes), Tab, Enter, Escape, Backspace while a tool
/// draws. Only when no text field has focus, so typing in the inspector is left alone.
fn keyboard(ui: &mut Ui, s: &mut Studio) {
    if ui.ctx().memory(|m| m.focused().is_some()) || s.ui.palette.open || s.ui.table.focused {
        return;
    }
    let kind = crate::hud::kind(s);
    if crate::hud::takes_keys(s)
        && let Some(kind) = kind
    {
        let started = s.ui.gesture.dim.is_some();
        let text: String = ui.input_mut(|i| {
            let mut out = String::new();
            i.events.retain(|e| match e {
                Event::Text(t) if crate::hud::accepts_text(t, started || !out.is_empty()) => {
                    out.push_str(t);
                    false
                }
                _ => true,
            });
            out
        });
        let g = &mut s.ui.gesture;
        if !text.is_empty() {
            // The first key opens the box with that key in it.
            let dim = g.dim.get_or_insert_with(|| DimInput { field: kind.first_field(), ..Default::default() });
            if std::mem::take(&mut dim.replace) {
                dim.text_mut().clear();
            }
            dim.text_mut().push_str(&text);
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab)) {
            match g.dim.as_mut() {
                // Tab before typing goes into the first box.
                None => g.dim = Some(DimInput { field: kind.first_field(), ..Default::default() }),
                Some(d) if kind == crate::hud::Kind::LengthAngle => {
                    d.field = match d.field {
                        DimField::Length => DimField::Angle,
                        DimField::Angle => DimField::Length,
                    };
                    d.replace = !d.text_mut().is_empty();
                }
                Some(_) => {}
            }
        }
        if let Some(d) = g.dim.as_mut()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Backspace))
        {
            if std::mem::take(&mut d.replace) {
                d.text_mut().clear();
            } else {
                d.text_mut().pop();
            }
        }
    }
    let units = crate::lengths::of(s);
    if let Some(d) = s.ui.gesture.dim.as_mut() {
        d.units = units;
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter)) {
        enter(s);
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
        let g = &mut s.ui.gesture;
        if g.dim.is_some() {
            // First Esc: drop what was typed, keep drawing.
            g.dim = None;
        } else if !g.points.is_empty() || g.drag.is_some() || !g.picks.is_empty() {
            end_drawing(s);
        } else if s.ui.tool != Tool::Select {
            s.ui.set_tool(Tool::Select);
        } else {
            crate::sketch_ui::clear_picks(&mut s.ui.sketch);
            let _ = s.run("selection.clear", Value::Null);
        }
    }
}

/// Esc while drawing: keep the segments drawn so far when they make an object (as in
/// Fusion, where drawn lines stay), else drop the gesture.
fn end_drawing(s: &mut Studio) {
    let n = s.ui.gesture.points.len();
    let keeps = match s.ui.tool {
        Tool::Wall | Tool::Run => n >= 2,
        Tool::Floor | Tool::Ceiling => n >= 3,
        _ => false,
    };
    if keeps && s.ui.gesture.drag.is_none() {
        finish(s);
    } else {
        s.ui.gesture = Gesture::default();
    }
}

/// Enter: take the typed value, or finish the polyline/polygon.
fn enter(s: &mut Studio) {
    let g = &s.ui.gesture;
    let Some(dim) = g.dim.clone().filter(|d| !d.is_empty()) else {
        finish(s);
        return;
    };
    let towards = g.cursor.map(|c| c.point);
    match s.ui.tool {
        Tool::Calibrate => {
            let pts = g.points.clone();
            match (pts.as_slice(), dim.length_mm()) {
                ([a, b], Some(len)) => commit_calibrate(s, *a, *b, len),
                ([_, _], None) => s.ui.error(tf("status.bad_length", &[("text", &dim.length)])),
                _ => s.ui.error(t("status.calibrate_second")),
            }
            return;
        }
        Tool::Rotate => {
            // After the pivot, the typed number is the angle.
            let text = if dim.angle.trim().is_empty() { &dim.length } else { &dim.angle };
            match parse_angle(text) {
                Some(a) => commit_rotate(s, g.points[0], a),
                None => s.ui.error(tf("status.bad_angle", &[("text", text)])),
            }
            return;
        }
        Tool::Dimension => {
            let picks = g.picks.clone();
            let label = towards.unwrap_or_default();
            match (picks.as_slice(), dim.length_mm()) {
                ([a, b], Some(v)) => {
                    s.ui.gesture = Gesture::default();
                    commit_dimension(s, *a, *b, label, Some(v));
                }
                _ => s.ui.error(tf("status.bad_length", &[("text", &dim.length)])),
            }
            return;
        }
        _ => {}
    }
    let Some(&last) = g.points.last() else { return };
    let from = match s.ui.tool {
        Tool::Truss | Tool::Curtain | Tool::Move => g.points[0],
        _ => last,
    };
    let bad = || {
        let text = if dim.length_mm().is_none() && !dim.length.trim().is_empty() { &dim.length } else { &dim.angle };
        tf("status.bad_length", &[("text", text)])
    };
    if dim.length_mm().is_none() && dim.angle_deg().is_none() {
        s.ui.error(bad());
        return;
    }
    let p = crate::hud::constrained(from, towards.unwrap_or(from), Some(&dim), crate::hud::prev_dir(&g.points));
    if math::dist(p, from) < 1e-6 {
        s.ui.error(t("status.type_length"));
        return;
    }
    s.ui.gesture.dim = None;
    match s.ui.tool {
        Tool::Wall | Tool::Floor => {
            let hint = SegHint { axis_lock: s.ui.gesture.shift, length: dim.length_mm() };
            s.ui.gesture.segs.push(hint);
            s.ui.gesture.points.push(p);
        }
        Tool::Run | Tool::Ceiling => s.ui.gesture.points.push(p),
        Tool::Truss => create_truss(s, from, p),
        Tool::Curtain => {
            let ids = crate::canvas::visible_ids(s.engine.doc(), &s.ui);
            let tol = tolerance(&s.ui.camera);
            create_curtain(s, &ids, from, p, tol);
        }
        Tool::Move => commit_move(s, math::sub(p, from)),
        _ => {}
    }
}

/// The dimension tool's third click (or Enter): the dimension, and a typed value for
/// a driving sketch dimension.
fn commit_dimension(
    s: &mut Studio,
    a: crate::sketch_ui::Pick,
    b: crate::sketch_ui::Pick,
    label: P2,
    value: Option<f64>,
) {
    crate::sketch_ui::commit(s, a, b, label);
    let Some(v) = value else { return };
    if let Some(src @ stagecraft_geom::dims::DimSource::Sketch { .. }) = s.ui.sketch.selected
        && s.ui.sketch.editing.as_ref().is_some_and(|e| e.source == src)
    {
        s.ui.sketch.editing = None;
        let mut args = crate::sketch_ui::dim_args(src);
        args["value"] = json!(v);
        if s.run("dimension.set_value", args).is_ok() {
            let text = crate::sketch_ui::value_text(&crate::lengths::of(s), v, false);
            s.ui.info(tf("sketch.value_set", &[("value", &text)]));
        }
    }
}

/// The underlay a calibration applies to: the one selected underlay, else the topmost
/// unlocked underlay under `p`.
fn calibration_target(s: &Studio, ids: &[Id], p: P2) -> Option<Id> {
    let doc = s.engine.doc();
    let is_underlay =
        |id: &Id| matches!(doc.objects.get(id).map(|o| &o.kind), Some(ObjectKind::Underlay { locked: false, .. }));
    let sel: Vec<Id> = s.engine.selection().iter().copied().filter(is_underlay).collect();
    if sel.len() == 1 {
        return Some(sel[0]);
    }
    let style = PlanStyle::default();
    ids.iter()
        .rev()
        .copied()
        .filter(is_underlay)
        .find(|id| stagecraft_geom::pick::distance(&stagecraft_geom::plan(doc, &doc.objects[id], &style), p) <= 0.0)
}

fn commit_calibrate(s: &mut Studio, a: P2, b: P2, len: f64) {
    s.ui.gesture = Gesture::default();
    let ids = crate::canvas::visible_ids(s.engine.doc(), &s.ui);
    let Some(id) = calibration_target(s, &ids, a) else {
        s.ui.error(t("status.no_underlay"));
        return;
    };
    if s.run("underlay.calibrate", json!({ "id": id, "p1": a, "p2": b, "distance_mm": len })).is_ok() {
        let _ = s.run("selection.set", json!({ "ids": [id] }));
    }
}

/// End the polyline or polygon being drawn.
fn finish(s: &mut Studio) {
    let raw_pts = std::mem::take(&mut s.ui.gesture.points);
    let raw_segs = std::mem::take(&mut s.ui.gesture.segs);
    // Drop zero-length segments (a double click lands twice on one spot).
    let mut pts: Vec<P2> = Vec::new();
    let mut segs: Vec<SegHint> = Vec::new();
    for (i, p) in raw_pts.iter().enumerate() {
        if pts.last().is_some_and(|q| math::dist(*q, *p) < 1e-6) {
            continue;
        }
        if i > 0 {
            segs.push(raw_segs.get(i - 1).copied().unwrap_or_default());
        }
        pts.push(*p);
    }
    s.ui.gesture = Gesture::default();
    let layer = s.active_layer_arg();
    let tol = tolerance(&s.ui.camera);
    match s.ui.tool {
        Tool::Wall if pts.len() >= 2 => {
            let o = &s.ui.options;
            let closed = pts.len() >= 4 && math::dist(pts[0], pts[pts.len() - 1]) < 1e-6;
            if closed {
                pts.pop();
            }
            let args = json!({ "points": pts, "segments": segs, "closed": closed, "make": "wall",
                               "height": o.wall_height, "thickness": o.wall_thickness, "z": o.wall_base, "layer": layer,
                               "snap_radius": tol });
            created(s, "sketch.draw", args, "tool.wall");
        }
        Tool::Floor if pts.len() >= 3 => {
            let o = &s.ui.options;
            let args = json!({ "points": pts, "segments": segs, "closed": true, "make": "floor", "layer": layer,
                               "z": o.floor_z, "thickness": o.floor_thickness, "snap_radius": tol });
            created(s, "sketch.draw", args, "tool.floor");
        }
        Tool::Ceiling if pts.len() >= 3 => {
            let h = s.ui.options.ceiling_height;
            created(s, "ceiling.create", json!({ "outline": pts, "height": h, "layer": layer }), "tool.ceiling");
        }
        Tool::Run if pts.len() >= 2 => {
            let mut pts = pts;
            let closed = pts.len() >= 4 && math::dist(pts[0], pts[pts.len() - 1]) < 1e-6;
            if closed {
                pts.pop();
            }
            let o = &s.ui.options;
            let h = o.truss_height;
            let points: Vec<[f64; 3]> = pts.iter().map(|p| [p[0], p[1], h]).collect();
            let args = json!({ "points": points, "profile": o.truss_profile, "closed": closed, "layer": layer });
            created(s, "truss.run", args, "tool.run");
        }
        Tool::Wall | Tool::Floor | Tool::Ceiling | Tool::Run if !pts.is_empty() => {
            s.ui.error(t("status.too_few_points"));
        }
        _ => {}
    }
}

fn created(s: &mut Studio, cmd: &str, args: Value, what: &str) {
    if let Ok(v) = s.run(cmd, args) {
        if let Some(id) = v.get("id") {
            let _ = s.run("selection.set", json!({ "ids": [id] }));
        }
        s.ui.info(created_message(what));
    }
}

/// "Floor created — …": what to do next with the new object, if there is a hint for it.
pub fn created_message(what: &str) -> String {
    let name = t(what);
    let next = format!("next.{}", what.trim_start_matches("tool."));
    if crate::i18n::has(&next) {
        tf("status.created_next", &[("what", &name), ("next", &t(&next))])
    } else {
        tf("status.created", &[("what", &name)])
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

/// How far under a batten's axis a fixture clicked onto it hangs, mm.
pub const PIPE_HANG: f64 = 60.0;

/// Click with the fixture tool: on a truss it hangs there (under the bottom chord), on
/// a batten under the pipe, on a support on top of it; elsewhere it stands on the
/// layer.
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
        None if let Some(b) = object_at(s, ids, raw, tol, is_batten) => {
            let x = local_on(s, b, raw).x;
            let x = s.ui.snap.grid.map_or(x, |g| round_to(x, g));
            json!({ "parent": b, "position": [x, 0.0, -crate::tools::PIPE_HANG] })
        }
        None if let Some(sp) = object_at(s, ids, raw, tol, |k| matches!(k, ObjectKind::Support { .. })) => {
            // On a stand or base: on top, upright (turned over like floor fixtures).
            let top = stagecraft_geom::kinds::stage::support_top(&doc.objects[&sp]);
            json!({ "parent": sp, "position": [0.0, 0.0, top], "rotation": [180.0, 0.0, 0.0] })
        }
        // Standing on the floor: turned over, GDTF style (fixtures hang from their origin).
        None => json!({
            "position": [cursor.point[0], cursor.point[1], 0.0],
            "rotation": [180.0, 0.0, 0.0],
            "layer": s.active_layer_arg()
        }),
    };
    let mut args = args;
    let o = &s.ui.options;
    if let Some(t) = doc.fixture_types.get(&o.fixture_type) {
        args["gdtf"] = json!(t.key);
        if t.mode(&o.fixture_mode).is_some() {
            args["mode"] = json!(o.fixture_mode);
        }
    }
    created(s, "fixture.create", args, "tool.fixture");
}

/// The topmost visible object under `p` whose kind passes `want`.
fn object_at(s: &Studio, ids: &[Id], p: P2, tol: f64, want: fn(&ObjectKind) -> bool) -> Option<Id> {
    let doc = s.engine.doc();
    let cands: Vec<Id> = ids.iter().copied().filter(|id| doc.objects.get(id).is_some_and(|o| want(&o.kind))).collect();
    pick(doc, cands, p, tol, &PlanStyle::default())
}

fn is_batten(k: &ObjectKind) -> bool {
    matches!(k, ObjectKind::Batten { .. })
}

/// The local position on a batten (or support) for a world plan point at its height.
fn local_on(s: &Studio, id: Id, p: P2) -> glam::DVec3 {
    let w = stagecraft_core::geom::world(s.engine.doc(), id).unwrap_or(glam::DAffine3::IDENTITY);
    w.inverse().transform_point3(glam::DVec3::new(p[0], p[1], w.translation.z))
}

/// Two clicks give a curtain's width and direction. Drawn along a batten, it hangs
/// there; elsewhere its top edge is at the curtain trim option.
fn create_curtain(s: &mut Studio, ids: &[Id], a: P2, b: P2, tol: f64) {
    s.ui.gesture = Gesture::default();
    let width = math::dist(a, b);
    if width < 1.0 {
        return;
    }
    let mid = math::midpoint(a, b);
    let angle = math::angle_deg(math::sub(b, a));
    let o = s.ui.options.clone();
    let args = match object_at(s, ids, mid, tol.max(150.0), is_batten) {
        Some(bid) => {
            let local = local_on(s, bid, mid);
            let yaw = stagecraft_core::geom::world_placement(s.engine.doc(), bid).map_or(0.0, |w| w.rotation[2]);
            json!({ "parent": bid, "position": [local.x, 0.0, 0.0], "rotation": angle - yaw,
                    "width": width, "height": o.curtain_height })
        }
        None => json!({ "position": [mid[0], mid[1], o.curtain_trim], "rotation": angle,
                        "width": width, "height": o.curtain_height, "layer": s.active_layer_arg() }),
    };
    created(s, "curtain.create", args, "tool.curtain");
}

/// One click places a batten, proscenium, wing set, platform, support or hoist.
fn place_stage(s: &mut Studio, ids: &[Id], raw: P2, p: P2, tol: f64) {
    let o = s.ui.options.clone();
    let layer = s.active_layer_arg();
    let (cmd, args) = match s.ui.tool {
        Tool::Batten => (
            "batten.create",
            json!({ "x": p[0], "y": p[1], "trim": o.batten_trim, "length": o.batten_length, "layer": layer }),
        ),
        Tool::Proscenium => (
            "proscenium.create",
            json!({ "position": [p[0], p[1], 0.0], "width": o.proscenium_width,
                    "height": o.proscenium_height, "layer": layer }),
        ),
        Tool::Wings => (
            "wingset.create",
            json!({ "x": p[0], "y": p[1], "count": o.wing_count, "spacing": o.wing_spacing,
                    "opening": o.wing_opening, "layer": layer }),
        ),
        Tool::Platform => {
            let mut a = json!({ "position": [p[0], p[1], 0.0], "preset": o.platform_preset,
                                "height": o.platform_height, "layer": layer });
            if !o.platform_system.is_empty() {
                a["system"] = json!(o.platform_system);
            }
            // Clicked onto a platform: stack on it.
            if let Some(under) = object_at(s, ids, raw, tol, |k| matches!(k, ObjectKind::Platform { .. })) {
                a["on"] = json!(under);
            }
            ("platform.create", a)
        }
        Tool::Support => {
            ("support.create", json!({ "position": [p[0], p[1], 0.0], "preset": o.support_preset, "layer": layer }))
        }
        Tool::Hoist => ("hoist.create", json!({ "position": [p[0], p[1], o.hoist_height], "layer": layer })),
        _ => return,
    };
    let what = s.ui.tool.key();
    created(s, cmd, args, what);
}

/// The ghost of what a one-click stage tool would place at `c`.
fn stage_ghost(tool: Tool, o: &crate::state::ToolOptions, c: P2) -> Vec<(ObjectKind, Placement)> {
    let at = |x: f64, y: f64| Placement { position: [x, y, 0.0], rotation: [0.0; 3] };
    let kind = match tool {
        Tool::Batten => ObjectKind::Batten { number: 0, length: o.batten_length, max_load: 0.0, kg_per_m: 0.0 },
        Tool::Proscenium => ObjectKind::Proscenium {
            width: o.proscenium_width,
            height: o.proscenium_height,
            depth: 600.0,
            frame: 1000.0,
            iron: 0.0,
            show_iron: false,
            chamfer: 0.0,
            outer_chamfer: 0.0,
        },
        Tool::Platform => {
            let [w, d] = parse_preset(&o.platform_preset).unwrap_or([2000.0, 1000.0]);
            ObjectKind::Platform {
                width: w,
                depth: d,
                height: o.platform_height,
                under: Default::default(),
                system: None,
            }
        }
        Tool::Support => {
            let pre = stagecraft_geom::support(&o.support_preset);
            ObjectKind::Support {
                style: pre.as_ref().map(|p| p.style).unwrap_or_default(),
                size: pre.as_ref().map_or(600.0, |p| p.size),
                height: 10.0,
                weight_kg: 0.0,
            }
        }
        Tool::Hoist => ObjectKind::Hoist { capacity_kg: 0.0, weight_kg: 0.0 },
        Tool::Wings => {
            let leg = |x: f64| {
                let k = ObjectKind::Masking {
                    role: stagecraft_core::model::MaskingRole::Leg,
                    width: 1500.0,
                    height: 1.0,
                    fullness: 0.0,
                    color: String::new(),
                };
                (k, at(c[0] + x, c[1]))
            };
            let x = o.wing_opening / 2.0 + 750.0;
            return (0..o.wing_count)
                .flat_map(|i| {
                    let dy = i as f64 * o.wing_spacing;
                    [leg(-x), leg(x)].map(|(k, mut p)| {
                        p.position[1] += dy;
                        (k, p)
                    })
                })
                .collect();
        }
        _ => return Vec::new(),
    };
    vec![(kind, at(c[0], c[1]))]
}

/// `2x1` (metres) → mm, as the platform command reads it.
fn parse_preset(s: &str) -> Option<[f64; 2]> {
    let s = s.to_lowercase().replace(['×', '*'], "x").replace(',', ".");
    let (w, d) = s.split_once('x')?;
    let w: f64 = w.trim().trim_end_matches('m').parse().ok()?;
    let d: f64 = d.trim().trim_end_matches('m').parse().ok()?;
    (w > 0.0 && d > 0.0).then_some([w * 1000.0, d * 1000.0])
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
    static EMPTY: std::sync::OnceLock<stagecraft_core::Document> = std::sync::OnceLock::new();
    ghost_in(EMPTY.get_or_init(Default::default), kind, placement, style)
}

/// A ghost that may use the document's fixture types.
fn ghost_in(
    doc: &stagecraft_core::Document,
    kind: ObjectKind,
    placement: Placement,
    style: &PlanStyle,
) -> Vec<stagecraft_geom::PlanPrim> {
    let obj = Object {
        id: Id::nil(),
        name: String::new(),
        layer: Id::nil(),
        tags: Vec::new(),
        parent: None,
        placement,
        kind,
        style: Default::default(),
        attrs: Default::default(),
        sketch: None,
    };
    let w = stagecraft_core::geom::placement_to_affine(&obj.placement);
    stagecraft_geom::kinds::plan_at(doc, &w, &obj, style)
}

/// Previews, the snap marker, the selection box and the typed-dimension box.
pub fn draw_overlay(ui: &Ui, p: &egui::Painter, s: &Studio, pal: &Palette, style: &PlanStyle) {
    let cam = &s.ui.camera;
    let g = &s.ui.gesture;
    // Typed values move the preview to where Enter or a click will put the point.
    let typed = match (s.ui.tool, crate::hud::segment(s)) {
        (
            Tool::Wall | Tool::Floor | Tool::Truss | Tool::Move | Tool::Run | Tool::Ceiling | Tool::Curtain,
            Some((_, to)),
        ) => Some(to),
        _ => None,
    };
    let cursor = typed.or(g.cursor.map(|c| c.point));
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
            length_label(p, cam, pal, *from, c, &crate::lengths::of(s));
        }
        _ => {}
    }

    if let (Some(Drag::Label { from, .. }), Some(raw)) =
        (&g.drag, ui.input(|i| i.pointer.hover_pos()).map(|q| cam.to_world(q)))
    {
        p.line_segment([cam.to_screen(*from), cam.to_screen(raw)], thin);
    }
    if s.ui.tool == Tool::Dimension
        && let Some(c) = g.cursor
    {
        let raw = ui.input(|i| i.pointer.hover_pos()).map_or(c.point, |q| cam.to_world(q));
        let at = if g.picks.len() == 2 { raw } else { c.point };
        crate::sketch_ui::tool_preview(p, cam, pal, &g.picks, at);
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
        }
        (Tool::Run, Some(c)) if !pts.is_empty() => {
            let mut sp: Vec<Pos2> = pts.iter().map(|q| cam.to_screen(*q)).collect();
            sp.push(cam.to_screen(c));
            let w = (o.truss_profile_width() * cam.scale).max(2.0) as f32;
            p.add(Shape::line(sp, Stroke::new(w, pal.preview.gamma_multiply(0.45))));
        }
        (Tool::Curtain, Some(c)) if !pts.is_empty() => {
            let a = pts[0];
            let d = math::sub(c, a);
            let len = math::length(d);
            if len > 1.0 {
                let mid = math::midpoint(a, c);
                let placement = Placement { position: [mid[0], mid[1], 0.0], rotation: [0.0, 0.0, math::angle_deg(d)] };
                let kind = ObjectKind::Curtain {
                    width: len,
                    height: o.curtain_height,
                    fullness: 100.0,
                    split: true,
                    opening: 0.0,
                    gather: Default::default(),
                    color: String::new(),
                };
                ghosts(ghost(kind, placement, style));
            }
        }
        (Tool::Batten | Tool::Proscenium | Tool::Wings | Tool::Platform | Tool::Support | Tool::Hoist, Some(c)) => {
            for (kind, placement) in stage_ghost(s.ui.tool, o, c) {
                ghosts(ghost(kind, placement, style));
            }
        }
        (Tool::Floor | Tool::Ceiling, Some(c)) if !pts.is_empty() => {
            let mut sp: Vec<Pos2> = pts.iter().map(|q| cam.to_screen(*q)).collect();
            sp.push(cam.to_screen(c));
            p.add(Shape::line(sp.clone(), Stroke::new(1.5, pal.preview)));
            if pts.len() >= 2 {
                p.extend(Shape::dashed_line(&[sp[sp.len() - 1], sp[0]], thin, 6.0, 4.0));
            }
        }
        (Tool::Truss, Some(c)) if !pts.is_empty() => {
            let a = pts[0];
            let d = math::sub(c, a);
            let len = math::length(d);
            if len > 1.0 {
                let placement = Placement { position: [a[0], a[1], 0.0], rotation: [0.0, 0.0, math::angle_deg(d)] };
                ghosts(ghost(ObjectKind::Truss { length: len, profile: o.truss_profile.clone() }, placement, style));
            }
        }
        (Tool::Calibrate, Some(c)) if !pts.is_empty() => {
            let b = pts.get(1).copied().unwrap_or(c);
            p.line_segment([cam.to_screen(pts[0]), cam.to_screen(b)], Stroke::new(1.5, pal.preview));
            for q in [pts[0], b] {
                p.circle_stroke(cam.to_screen(q), 4.0, thin);
            }
        }
        (Tool::Move, Some(c)) if !pts.is_empty() => {
            let d = math::sub(c, pts[0]);
            sel_prims(&|q| q.translated(d));
            p.line_segment([cam.to_screen(pts[0]), cam.to_screen(c)], thin);
        }
        (Tool::Rotate, Some(c)) if !pts.is_empty() => {
            let pivot = pts[0];
            p.circle_stroke(cam.to_screen(pivot), 4.0, thin);
            p.line_segment([cam.to_screen(pivot), cam.to_screen(c)], thin);
            if pts.len() >= 2 {
                p.line_segment([cam.to_screen(pivot), cam.to_screen(pts[1])], thin);
            }
            if let Some((_, _, Some(a))) = crate::hud::rotation(s) {
                sel_prims(&|q| q.rotated(pivot, a));
            }
        }
        (Tool::Fixture, Some(c)) => {
            let placement = Placement { position: [c[0], c[1], 0.0], rotation: [0.0; 3] };
            let o = &s.ui.options;
            let typed = s.engine.doc().fixture_types.contains_key(&o.fixture_type);
            let kind = ObjectKind::Fixture {
                fid: None,
                cid: None,
                gdtf: typed.then(|| o.fixture_type.clone()),
                mode: (typed && !o.fixture_mode.is_empty()).then(|| o.fixture_mode.clone()),
                address: None,
                extra_addresses: Vec::new(),
                weight_kg: None,
            };
            ghosts(ghost_in(s.engine.doc(), kind, placement, style));
        }
        _ => {}
    }

    // Where a click closes the outline: a ring on the first corner, filled when the
    // pointer is on it.
    if matches!(s.ui.tool, Tool::Wall | Tool::Floor | Tool::Ceiling | Tool::Run) && pts.len() >= 3 {
        let at = cam.to_screen(pts[0]);
        let on = g.cursor.is_some_and(|c| math::dist(c.point, pts[0]) <= tolerance(cam));
        if on {
            p.circle_filled(at, 7.0, pal.selected.gamma_multiply(0.35));
        }
        p.circle_stroke(at, 7.0, Stroke::new(1.5, pal.selected));
    }
    if let Some(c) = g.cursor
        && s.ui.tool != Tool::Select
    {
        snap_marker(p, cam.to_screen(c.point), c.kind, pal);
    }

    crate::hud::draw(ui, p, s, pal);
}

fn length_label(
    p: &egui::Painter,
    cam: &crate::state::Camera,
    pal: &Palette,
    a: P2,
    b: P2,
    u: &stagecraft_core::units::Units,
) {
    let len = math::dist(a, b);
    if len < 1.0 {
        return;
    }
    let mid = cam.to_screen(math::midpoint(a, b));
    let text = u.with_unit(len);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_angles() {
        assert_eq!(parse_angle("-45°"), Some(-45.0));
        assert_eq!(parse_angle("22,5"), Some(22.5));
        assert_eq!(parse_angle("abc"), None);
    }
}
