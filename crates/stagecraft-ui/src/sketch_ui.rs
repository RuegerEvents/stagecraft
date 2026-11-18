//! The layer sketches in the plan: lines and points in FreeCAD colours (fully
//! constrained green, free in the normal colour, conflicting red), dimensions whose
//! values open an editor on double-click, live dragging of sketch points (solved for
//! the preview every frame, one `sketch.drag` on release) and the sketch section of
//! the inspector. Every change goes through `sketch.*` / `dimension.*` commands.

use std::collections::HashMap;

use egui::epaint::TextShape;
use egui::{Color32, Pos2, RichText, Stroke, Ui, vec2};
use serde_json::{Value, json};
use stagecraft_core::model::P2;
use stagecraft_core::sketch::{
    self as sk, Anchor, ConstraintState, EntityState, PointId, Sketch, SolveOptions, Status, constraint_edges,
    constraint_points, measure_edges, measure_points,
};
use stagecraft_core::units::{LengthFormat, SUP_SCALE, superscript_runs};
use stagecraft_core::{Document, Id};
use stagecraft_geom::dims::{self, DimDraw, DimSource, DimStyle};

use crate::app::Studio;
use crate::i18n::{t, tf};
use crate::state::Camera;
use crate::theme::{self, Palette};
use crate::widgets;

/// What the dimension tool clicked: a sketch point, a point of an object, or a free
/// position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    Sketch { layer: Id, point: PointId, at: P2 },
    Object { id: Id, local: [f64; 3], at: P2 },
    Free { at: P2, z: f64 },
}

impl Pick {
    pub fn at(&self) -> P2 {
        match *self {
            Pick::Sketch { at, .. } | Pick::Object { at, .. } | Pick::Free { at, .. } => at,
        }
    }

    pub fn anchor(&self) -> Anchor {
        match *self {
            Pick::Sketch { layer, point, .. } => Anchor::SketchPoint { layer, point },
            Pick::Object { id, local, .. } => Anchor::Object { object: id, local },
            Pick::Free { at, z } => Anchor::Point { at: [at[0], at[1], z] },
        }
    }
}

/// The value editor opened by double-clicking a dimension.
#[derive(Clone, Debug, PartialEq)]
pub struct DimEdit {
    pub source: DimSource,
    pub text: String,
    /// Focus the field on the next frame.
    pub focus: bool,
}

struct Preview {
    key: (Id, PointId, [u64; 2]),
    doc: Document,
}

/// A picked sketch point or line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SketchItem {
    Point(PointId),
    Edge(sk::EdgeId),
}

impl SketchItem {
    pub fn item_ref(self) -> sk::ItemRef {
        match self {
            SketchItem::Point(p) => sk::ItemRef::Point(p),
            SketchItem::Edge(e) => sk::ItemRef::Edge(e),
        }
    }
}

/// Sketch view state.
pub struct SketchUi {
    /// Draw sketch lines, points and dimensions over the plan.
    pub show: bool,
    /// FreeCAD colours by constraint state.
    pub colours: bool,
    /// Draw dimensions.
    pub dimensions: bool,
    /// The dimension selected in the plan (shown in the inspector).
    pub selected: Option<DimSource>,
    /// Further dimensions selected with Shift (the inspector then edits their tags).
    pub also: Vec<DimSource>,
    pub editing: Option<DimEdit>,
    /// Sketch points and lines picked in the plan (one layer), in pick order: the
    /// inspector's constraint buttons work on them, Delete removes them.
    pub picked: Vec<SketchItem>,
    pub picked_layer: Option<Id>,
    cache: HashMap<Id, (Sketch, Option<Status>)>,
    preview: Option<Preview>,
}

impl Default for SketchUi {
    fn default() -> Self {
        Self {
            show: true,
            colours: true,
            dimensions: true,
            selected: None,
            also: Vec::new(),
            editing: None,
            picked: Vec::new(),
            picked_layer: None,
            cache: HashMap::new(),
            preview: None,
        }
    }
}

impl SketchUi {
    /// The analysis of a layer's sketch, cached until the sketch changes. With `live`
    /// (a drag preview), only a change of structure (points, edges, constraints,
    /// dimensions; not positions) runs it again: degrees of freedom do not change
    /// while dragging, and the analysis costs tens of milliseconds on big sketches.
    pub fn status(&mut self, layer: Id, s: &Sketch, live: bool) -> Option<&Status> {
        let same = |c: &Sketch| {
            if !live {
                return c == s;
            }
            c.edges == s.edges
                && c.constraints == s.constraints
                && c.points.len() == s.points.len()
                && c.points.iter().zip(&s.points).all(|(a, b)| a.id == b.id)
                && c.dimensions.len() == s.dimensions.len()
                && c.dimensions.iter().zip(&s.dimensions).all(|(a, b)| a.id == b.id && a.driving == b.driving)
        };
        let fresh = self.cache.get(&layer).is_some_and(|(c, _)| same(c));
        if !fresh {
            self.cache.insert(layer, (s.clone(), s.analyse().ok()));
        }
        self.cache.get(&layer).and_then(|(_, st)| st.as_ref())
    }

    /// The document with a sketch point dragged to `to` (solved, walls and floors
    /// following), for drawing while the drag lasts. `None` when nothing is dragged
    /// or the drag cannot be solved.
    pub fn preview_doc(&mut self, doc: &Document, drag: Option<(Id, PointId, P2)>) -> Option<&Document> {
        let Some((layer, point, to)) = drag else {
            self.preview = None;
            return None;
        };
        let key = (layer, point, [to[0].to_bits(), to[1].to_bits()]);
        if self.preview.as_ref().is_none_or(|p| p.key != key) {
            let s = doc.sketches.get(&layer)?;
            if let Ok(solved) = s.drag(point, sk::v2(to), &SolveOptions::drag()) {
                let mut d = doc.clone();
                sk::store(&mut d, layer, solved.sketch);
                self.preview = Some(Preview { key, doc: d });
            }
        }
        self.preview.as_ref().map(|p| &p.doc)
    }
}

/// Layers whose sketch is drawn: the visible ones.
pub fn visible_layers(doc: &Document) -> Vec<Id> {
    doc.layers.values().filter(|l| l.visible).map(|l| l.id).collect()
}

/// Every dimension drawn in the plan, in world mm, sized for the zoom.
pub fn plan_dims(doc: &Document, cam: &Camera) -> Vec<DimDraw> {
    let st = DimStyle::for_scale(cam.scale, 5.0).with_format(LengthFormat::of(&doc.meta));
    let layers = visible_layers(doc);
    let mut out = Vec::new();
    for l in &layers {
        if let Some(s) = doc.sketches.get(l) {
            out.extend(dims::sketch_dims(*l, s, &st));
        }
    }
    out.extend(dims::reference_dims(doc, &st, |d| layers.contains(&d.layer)));
    out
}

const LABEL_PX: f32 = 12.0;

/// Centre of a dimension's value on screen.
pub fn label_center(cam: &Camera, d: &DimDraw) -> Pos2 {
    cam.to_screen(d.label) + vec2(d.up[0] as f32, -d.up[1] as f32) * (LABEL_PX * 0.75)
}

/// The dimension whose value is under `pos` (screen).
pub fn dim_at(doc: &Document, cam: &Camera, pos: Pos2) -> Option<DimDraw> {
    plan_dims(doc, cam)
        .into_iter()
        .map(|d| {
            let c = label_center(cam, &d);
            let r = (d.text.chars().count() as f32 * LABEL_PX * 0.3).max(LABEL_PX * 0.8);
            ((c - pos).length() / r, d)
        })
        .filter(|(k, _)| *k <= 1.0)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|x| x.1)
}

/// The sketch point near `p` (world mm) on a visible layer.
pub fn point_at(doc: &Document, p: P2, tol: f64) -> Option<(Id, PointId, P2)> {
    let mut best: Option<(Id, PointId, P2, f64)> = None;
    for l in visible_layers(doc) {
        let Some(s) = doc.sketches.get(&l) else { continue };
        for q in &s.points {
            let d = (q.pos.x - p[0]).hypot(q.pos.y - p[1]);
            if d <= tol && best.is_none_or(|b| d < b.3) {
                best = Some((l, q.id, sk::p2(q.pos), d));
            }
        }
    }
    best.map(|b| (b.0, b.1, b.2))
}

/// The sketch line (or arc) near `p` (world mm) on a visible layer.
pub fn line_at(doc: &Document, p: P2, tol: f64) -> Option<(Id, sk::EdgeId)> {
    let mut best: Option<(Id, sk::EdgeId, f64)> = None;
    for l in visible_layers(doc) {
        let Some(s) = doc.sketches.get(&l) else { continue };
        for e in &s.edges {
            let Some((pts, _)) = sk::chain(s, &[e.id]) else { continue };
            let d = stagecraft_geom::math::polyline_distance(&pts, false, p);
            if d <= tol && best.is_none_or(|b| d < b.2) {
                best = Some((l, e.id, d));
            }
        }
    }
    best.map(|b| (b.0, b.1))
}

/// A click in the plan with the select tool: picks the sketch point, else the sketch
/// line under it (Shift adds or removes). Returns false if there is none, so the
/// click selects objects instead. Lines are picked only close to them (`tol` / 2),
/// so walls stay clickable on their faces.
pub fn click_pick(s: &mut Studio, p: P2, tol: f64, shift: bool) -> bool {
    if !s.ui.sketch.show {
        return false;
    }
    let doc = s.engine.doc();
    let hit = match point_at(doc, p, tol) {
        Some((l, pt, _)) => (l, SketchItem::Point(pt)),
        None => match line_at(doc, p, tol / 2.0) {
            Some((l, e)) => (l, SketchItem::Edge(e)),
            None => return false,
        },
    };
    let sui = &mut s.ui.sketch;
    if !shift || sui.picked_layer != Some(hit.0) {
        sui.picked.clear();
    }
    sui.picked_layer = Some(hit.0);
    if let Some(i) = sui.picked.iter().position(|x| *x == hit.1) {
        sui.picked.remove(i);
    } else {
        sui.picked.push(hit.1);
    }
    sui.selected = None;
    sui.editing = None;
    if !s.engine.selection().is_empty() {
        let _ = s.run("selection.clear", Value::Null);
    }
    true
}

/// Forget the picked sketch items.
pub fn clear_picks(sui: &mut SketchUi) {
    sui.picked.clear();
    sui.picked_layer = None;
}

/// Picked items that still exist (an undo may have removed some).
fn live_picks(doc: &Document, sui: &SketchUi) -> Option<(Id, Vec<SketchItem>)> {
    let l = sui.picked_layer?;
    let s = doc.sketches.get(&l)?;
    let v: Vec<SketchItem> = sui
        .picked
        .iter()
        .copied()
        .filter(|i| match i {
            SketchItem::Point(p) => s.point(*p).is_some(),
            SketchItem::Edge(e) => s.edge(*e).is_some(),
        })
        .collect();
    (!v.is_empty()).then_some((l, v))
}

/// Delete the picked sketch items. True if there were some.
pub fn delete_picks(s: &mut Studio) -> bool {
    let Some((layer, items)) = live_picks(s.engine.doc(), &s.ui.sketch) else { return false };
    let refs: Vec<sk::ItemRef> = items.iter().map(|i| i.item_ref()).collect();
    if s.run("sketch.remove", json!({ "layer": layer, "items": refs })).is_ok() {
        clear_picks(&mut s.ui.sketch);
    }
    true
}

/// Constraints the inspector buttons add.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Equal,
    Symmetric,
    Lock,
}

impl Relation {
    pub const ALL: [Relation; 7] = [
        Relation::Horizontal,
        Relation::Vertical,
        Relation::Parallel,
        Relation::Perpendicular,
        Relation::Equal,
        Relation::Symmetric,
        Relation::Lock,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Relation::Horizontal => "sketch.horizontal",
            Relation::Vertical => "sketch.vertical",
            Relation::Parallel => "sketch.parallel",
            Relation::Perpendicular => "sketch.perpendicular",
            Relation::Equal => "sketch.equal",
            Relation::Symmetric => "sketch.symmetric",
            Relation::Lock => "sketch.lock",
        }
    }
}

/// The constraints a relation adds to the picked items (as JSON for
/// `sketch.add_constraint`), or `None` if it does not apply to them.
///
/// - horizontal / vertical: every picked line (its end points), or two points;
/// - parallel / perpendicular: two lines; equal length: two or more lines (all equal
///   to the first);
/// - symmetric: two points and a line (the axis), or two lines and a line: the first
///   two mirror each other across the last picked one, end points paired so they
///   match best;
/// - lock: every picked point is fixed where it is.
pub fn relation(s: &Sketch, items: &[SketchItem], r: Relation) -> Option<Vec<Value>> {
    let lines: Vec<(sk::EdgeId, PointId, PointId)> = items
        .iter()
        .filter_map(|i| match i {
            SketchItem::Edge(e) => match s.edge(*e)?.geom {
                sk::EdgeGeom::Line { a, b } => Some((*e, a, b)),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let points: Vec<PointId> = items
        .iter()
        .filter_map(|i| match i {
            SketchItem::Point(p) => Some(*p),
            _ => None,
        })
        .collect();
    let arcs = items.len() - lines.len() - points.len();
    let c = |ty: &str, a: Value, b: Value| json!({ "type": ty, "a": a, "b": b });
    let out: Vec<Value> = match r {
        Relation::Horizontal | Relation::Vertical => {
            let ty = if r == Relation::Horizontal { "horizontal" } else { "vertical" };
            match (lines.len(), points.len(), arcs) {
                (n, 0, 0) if n >= 1 => lines.iter().map(|l| c(ty, json!(l.1), json!(l.2))).collect(),
                (0, 2, 0) => vec![c(ty, json!(points[0]), json!(points[1]))],
                _ => return None,
            }
        }
        Relation::Parallel | Relation::Perpendicular => {
            let ty = if r == Relation::Parallel { "parallel" } else { "perpendicular" };
            match (lines.len(), points.len(), arcs) {
                (2, 0, 0) => vec![c(ty, json!(lines[0].0), json!(lines[1].0))],
                _ => return None,
            }
        }
        Relation::Equal => match (lines.len(), points.len(), arcs) {
            (n, 0, 0) if n >= 2 => {
                lines[1..].iter().map(|l| c("equal_length", json!(lines[0].0), json!(l.0))).collect()
            }
            _ => return None,
        },
        Relation::Symmetric => match (lines.len(), points.len(), arcs) {
            (1, 2, 0) => vec![json!({ "type": "symmetric", "a": points[0], "b": points[1], "axis": lines[0].0 })],
            (3, 0, 0) => {
                let (axis, l1, l2) = (lines[2].0, lines[0], lines[1]);
                let pos = |p: PointId| s.pos(p);
                // Pair the ends so mirrored points are the nearer ones across the axis.
                let straight = pos(l1.1).distance(pos(l2.1)) + pos(l1.2).distance(pos(l2.2));
                let crossed = pos(l1.1).distance(pos(l2.2)) + pos(l1.2).distance(pos(l2.1));
                let pairs =
                    if straight <= crossed { [(l1.1, l2.1), (l1.2, l2.2)] } else { [(l1.1, l2.2), (l1.2, l2.1)] };
                pairs.iter().map(|(a, b)| json!({ "type": "symmetric", "a": a, "b": b, "axis": axis })).collect()
            }
            _ => return None,
        },
        Relation::Lock => {
            if points.is_empty() || !lines.is_empty() || arcs > 0 {
                return None;
            }
            points
                .iter()
                .filter(|p| locks(s, **p).is_empty())
                .map(|p| {
                    let at = s.pos(*p);
                    json!({ "type": "fixed", "point": p, "at": { "x": at.x, "y": at.y } })
                })
                .collect()
        }
    };
    (!out.is_empty()).then_some(out)
}

/// Whether a relation applies to what is picked now.
pub fn relation_available(s: &Studio, r: Relation) -> bool {
    let Some((layer, items)) = live_picks(s.engine.doc(), &s.ui.sketch) else { return false };
    s.engine.doc().sketches.get(&layer).is_some_and(|sketch| relation(sketch, &items, r).is_some())
}

/// Add a relation to the picked items in one undo step.
pub fn apply_relation(s: &mut Studio, r: Relation) {
    let Some((layer, items)) = live_picks(s.engine.doc(), &s.ui.sketch) else { return };
    let Some(sketch) = s.engine.doc().sketches.get(&layer) else { return };
    let Some(cs) = relation(sketch, &items, r) else { return };
    let steps: Vec<Value> = cs
        .into_iter()
        .map(|c| json!({ "command": "sketch.add_constraint", "args": { "layer": layer, "constraint": c } }))
        .collect();
    if s.run("batch", json!({ "commands": steps })).is_ok() {
        s.ui.info(tf("sketch.added", &[("what", &t(r.key()))]));
    }
}

/// Inspector rows for picked sketch items: what is picked, the constraint buttons,
/// delete.
fn picked_fields(ui: &mut Ui, s: &mut Studio) {
    let Some((layer, items)) = live_picks(s.engine.doc(), &s.ui.sketch) else {
        clear_picks(&mut s.ui.sketch);
        return;
    };
    let Some(sketch) = s.engine.doc().sketches.get(&layer).cloned() else { return };
    let n_points = items.iter().filter(|i| matches!(i, SketchItem::Point(_))).count();
    let n_edges = items.len() - n_points;
    crate::widgets::section(ui, tf("sketch.picked", &[("points", &n_points), ("edges", &n_edges)]));
    ui.horizontal_wrapped(|ui| {
        for r in Relation::ALL {
            let ok = relation(&sketch, &items, r).is_some();
            let label = t(r.key());
            let resp = ui.add_enabled(ok, egui::Button::new(label));
            widgets::name(&resp, egui::Role::Button, format!("Sketch {label}"));
            if resp.clicked() {
                apply_relation(s, r);
            }
        }
    });
    if widgets::button(ui, t("sketch.delete_picked"), "Sketch delete").clicked() {
        delete_picks(s);
    }
    ui.add_space(8.0);
}

/// The box around the sketches and dimensions of the visible layers (zoom to fit),
/// with room for the dimension values.
pub fn bounds(doc: &Document, cam: &Camera) -> stagecraft_geom::Bounds {
    let mut b = stagecraft_geom::Bounds::EMPTY;
    for (p, _) in snap_points(doc) {
        b.add(p);
    }
    for d in plan_dims(doc, cam) {
        for l in &d.lines {
            l.iter().for_each(|p| b.add(*p));
        }
        let r = 2.0 * LABEL_PX as f64 / cam.scale;
        b.add([d.label[0] + d.up[0] * r, d.label[1] + d.up[1] * r]);
    }
    b
}

/// Fits the plan camera to `objects` plus the sketches and dimensions. Twice, since
/// dimension text is sized for the zoom.
pub fn fit(cam: &mut Camera, doc: &Document, objects: stagecraft_geom::Bounds) {
    for _ in 0..2 {
        let b = objects.union(&bounds(doc, cam));
        cam.fit(b);
    }
}

/// Sketch points of the visible layers, for snapping.
pub fn snap_points(doc: &Document) -> Vec<(P2, Id)> {
    visible_layers(doc)
        .into_iter()
        .filter_map(|l| doc.sketches.get(&l).map(|s| (l, s)))
        .flat_map(|(l, s)| s.points.iter().map(move |p| (sk::p2(p.pos), l)))
        .collect()
}

fn state_colour(pal: &Palette, colours: bool, conflict: bool, full: bool) -> Color32 {
    if !colours {
        return pal.sketch;
    }
    if conflict {
        pal.conflict
    } else if full {
        pal.constrained
    } else {
        pal.sketch
    }
}

/// Lines, points and dimensions of the sketches, over the objects.
pub fn draw(p: &egui::Painter, cam: &Camera, pal: &Palette, doc: &Document, sui: &mut SketchUi) {
    if !sui.show {
        return;
    }
    for l in visible_layers(doc) {
        let Some(s) = doc.sketches.get(&l) else { continue };
        let colours = sui.colours;
        let live = sui.preview.is_some();
        let status = if colours { sui.status(l, s, live).cloned() } else { None };
        // Edges and points touched by a conflicting constraint.
        let mut bad_edges = Vec::new();
        let mut bad_points = Vec::new();
        if let Some(st) = &status {
            for id in &st.conflicting {
                if let Some(c) = s.constraint(*id) {
                    bad_edges.extend(constraint_edges(&c.kind));
                    bad_points.extend(constraint_points(&c.kind));
                }
                if let Some(d) = s.dimension(*id) {
                    bad_edges.extend(measure_edges(&d.measure));
                    bad_points.extend(measure_points(&d.measure));
                }
            }
        }
        let full_point =
            |id: PointId| status.as_ref().is_some_and(|st| st.point_state(id) == EntityState::FullyConstrained);
        for e in &s.edges {
            let full = status.as_ref().is_some_and(|st| st.edge_state(s, e.id) == EntityState::FullyConstrained);
            let bad = bad_edges.contains(&e.id) || e.geom.points().iter().any(|q| bad_points.contains(q));
            let colour = state_colour(pal, colours, bad, full);
            let Some((pts, _)) = sk::chain(s, &[e.id]) else { continue };
            let sp: Vec<Pos2> = pts.iter().map(|q| cam.to_screen(*q)).collect();
            let picked = sui.picked_layer == Some(l) && sui.picked.contains(&SketchItem::Edge(e.id));
            let (colour, width) = if picked { (pal.selected, 3.0) } else { (colour, 1.4) };
            let stroke = Stroke::new(width, colour);
            if e.construction {
                p.extend(egui::Shape::dashed_line(&sp, Stroke::new(1.0, colour), 8.0, 4.0));
            } else {
                p.add(egui::Shape::line(sp, stroke));
            }
        }
        for q in &s.points {
            let bad = bad_points.contains(&q.id);
            let colour = state_colour(pal, colours, bad, full_point(q.id));
            let at = cam.to_screen(sk::p2(q.pos));
            let picked = sui.picked_layer == Some(l) && sui.picked.contains(&SketchItem::Point(q.id));
            let colour = if picked { pal.selected } else { colour };
            p.circle_filled(at, if picked { 4.5 } else { 2.6 }, colour);
            if !locks(s, q.id).is_empty() {
                let r = egui::Rect::from_center_size(at, vec2(9.0, 9.0));
                p.rect_stroke(r, 1.0, Stroke::new(1.3, colour), egui::StrokeKind::Middle);
            }
        }
    }
    if !sui.dimensions {
        return;
    }
    let conflicting: Vec<DimSource> = sui
        .cache
        .iter()
        .flat_map(|(l, (_, st))| {
            st.iter().flat_map(move |st| st.conflicting.iter().map(move |id| DimSource::Sketch { layer: *l, id: *id }))
        })
        .collect();
    for d in plan_dims(doc, cam) {
        let colour = if sui.selected == Some(d.source) {
            pal.selected
        } else if sui.colours && conflicting.contains(&d.source) {
            pal.conflict
        } else if d.driving {
            pal.dim
        } else {
            pal.dim_reference
        };
        draw_dim(p, cam, pal, &d, colour, sui.editing.as_ref().is_some_and(|e| e.source == d.source));
    }
}

/// One dimension: lines, then the value on a small backdrop so it reads over hatching.
pub fn draw_dim(p: &egui::Painter, cam: &Camera, pal: &Palette, d: &DimDraw, colour: Color32, hide_text: bool) {
    let stroke = Stroke::new(0.9, colour);
    for l in &d.lines {
        let sp: Vec<Pos2> = l.iter().map(|q| cam.to_screen(*q)).collect();
        p.add(egui::Shape::line(sp, stroke));
    }
    if hide_text {
        return;
    }
    let galley = p.fonts_mut(|f| f.layout_job(dim_text_job(&d.text, LABEL_PX, colour)));
    let c = label_center(cam, d);
    let angle = -(d.rotation.to_radians()) as f32;
    let size = galley.size();
    let (s, co) = angle.sin_cos();
    let half = vec2(-size.x / 2.0, -size.y / 2.0);
    let rotated = vec2(half.x * co - half.y * s, half.x * s + half.y * co);
    if angle.abs() < 1e-3 {
        let r = egui::Rect::from_center_size(c, size).expand(1.5);
        p.rect_filled(r, 2.0, pal.canvas.gamma_multiply(0.85));
    }
    p.add(TextShape::new(c + rotated, galley, colour).with_angle(angle));
}

/// A dimension value as a layout: superscript digits (`6.15⁵`) smaller and raised.
pub fn dim_text_job(text: &str, px: f32, colour: Color32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for (run, sup) in superscript_runs(text) {
        let size = if sup { px * SUP_SCALE as f32 } else { px };
        let format = egui::TextFormat {
            font_id: theme::proportional(size),
            color: colour,
            valign: if sup { egui::Align::TOP } else { egui::Align::BOTTOM },
            ..Default::default()
        };
        job.append(&run, if sup { px * 0.05 } else { 0.0 }, format);
    }
    job
}

/// The `fixed` constraints on a point.
pub fn locks(s: &Sketch, point: PointId) -> Vec<sk::ConstraintId> {
    s.constraints
        .iter()
        .filter(|c| matches!(c.kind, sk::ConstraintKind::Fixed { point: p, .. } if p == point))
        .map(|c| c.id)
        .collect()
}

/// Locks a sketch point where it is (a `fixed` constraint), or unlocks it.
pub fn toggle_lock(s: &mut Studio, layer: Id, point: PointId, at: P2) {
    let Some(sketch) = s.engine.doc().sketches.get(&layer) else { return };
    let locks = locks(sketch, point);
    if locks.is_empty() {
        let c = json!({ "type": "fixed", "point": point, "at": { "x": at[0], "y": at[1] } });
        if s.run("sketch.add_constraint", json!({ "layer": layer, "constraint": c })).is_ok() {
            s.ui.info(t("sketch.locked"));
        }
    } else if s.run("sketch.remove_constraint", json!({ "layer": layer, "ids": locks })).is_ok() {
        s.ui.info(t("sketch.unlocked"));
    }
}

/// The arguments naming a dimension in `dimension.*` commands.
pub fn dim_args(src: DimSource) -> Value {
    match src {
        DimSource::Sketch { layer, id } => json!({ "id": id.0, "layer": layer }),
        DimSource::Reference(id) => json!({ "id": id }),
    }
}

/// Opens the value editor of a dimension (driving sketch dimensions only).
pub fn open_editor(s: &mut Studio, src: DimSource) {
    s.ui.sketch.selected = Some(src);
    let DimSource::Sketch { layer, id } = src else {
        s.ui.info(t("sketch.reference_only"));
        return;
    };
    let Some(d) = s.engine.doc().sketches.get(&layer).and_then(|sk| sk.dimension(id)).cloned() else { return };
    if !d.driving {
        s.ui.info(t("sketch.reference_only"));
        return;
    }
    let text = if d.measure.is_angle() { format!("{:.1}", d.value) } else { crate::lengths::of(s).field(d.value) };
    s.ui.sketch.editing = Some(DimEdit { source: src, text, focus: true });
}

/// A dimension value for the status bar: a length with the project unit, an angle.
pub fn value_text(u: &stagecraft_core::units::Units, v: f64, angle: bool) -> String {
    if angle { dims::format_value(v, true) } else { u.field_with_unit(v) }
}

/// The inline editor over a dimension's value: type, Enter applies, Escape cancels.
pub fn editor(ui: &Ui, s: &mut Studio) {
    let Some(edit) = s.ui.sketch.editing.clone() else { return };
    let cam = s.ui.camera;
    let Some(d) = plan_dims(s.engine.doc(), &cam).into_iter().find(|d| d.source == edit.source) else {
        s.ui.sketch.editing = None;
        return;
    };
    let at = label_center(&cam, &d);
    let mut text = edit.text.clone();
    let mut apply = false;
    let mut cancel = false;
    let mut focused = false;
    egui::Area::new(egui::Id::unique("dimension_editor"))
        .fixed_pos(at - vec2(42.0, 11.0))
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(2.0).show(ui, |ui| {
                let r =
                    ui.add(egui::TextEdit::singleline(&mut text).desired_width(76.0).font(theme::proportional(13.0)));
                widgets::name(&r, egui::Role::TextInput, "Dimension value");
                if edit.focus && !r.has_focus() {
                    r.request_focus();
                }
                // Enter applies, Escape cancels; a click elsewhere in the plan closes it
                // (in the select tool).
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) && (r.has_focus() || r.lost_focus()) {
                    apply = true;
                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    cancel = true;
                }
                focused = r.has_focus();
            });
        });
    if apply {
        s.ui.sketch.editing = None;
        let u = crate::lengths::of(s);
        let value = if d.angle { crate::tools::parse_angle(&text) } else { u.parse(&text) };
        match value {
            Some(v) => {
                let mut args = dim_args(edit.source);
                args["value"] = json!(v);
                if s.run("dimension.set_value", args).is_ok() {
                    s.ui.info(tf("sketch.value_set", &[("value", &value_text(&u, v, d.angle))]));
                }
            }
            None => s.ui.error(tf("status.bad_length", &[("text", &text)])),
        }
    } else if cancel {
        s.ui.sketch.editing = None;
    } else if let Some(e) = &mut s.ui.sketch.editing {
        e.text = text;
        if focused {
            e.focus = false;
        }
    }
}

/// The sketch section of the inspector: the selected dimension, or the active layer's
/// sketch status (degrees of freedom, conflicts).
pub fn inspector(ui: &mut Ui, s: &mut Studio) {
    if !s.ui.sketch.picked.is_empty() {
        picked_fields(ui, s);
    }
    if let Some(src) = s.ui.sketch.selected {
        if s.ui.sketch.also.is_empty() {
            dimension_fields(ui, s, src);
        } else {
            let n = 1 + s.ui.sketch.also.len();
            crate::widgets::section(ui, tf("sketch.dimensions_selected", &[("n", &n.to_string())]));
        }
        let mut all = vec![src];
        all.extend(s.ui.sketch.also.iter().copied());
        dimension_tags(ui, s, &all);
        ui.add_space(8.0);
    }
    let doc = s.engine.doc();
    let layer =
        s.ui.active_layer.filter(|l| doc.layers.contains_key(l)).or_else(|| doc.layers_sorted().first().map(|l| l.id));
    let Some(layer) = layer else { return };
    let Some(sketch) = doc.sketches.get(&layer).cloned() else { return };
    let name = doc.layers[&layer].name.clone();
    crate::widgets::section(ui, tf("sketch.title", &[("layer", &name)]));
    let st = s.ui.sketch.status(layer, &sketch, false).cloned();
    let Some(st) = st else {
        ui.label(RichText::new(t("sketch.no_status")).weak());
        return;
    };
    let (text, colour) = if !st.conflicting.is_empty() {
        let key = if st.conflicting.len() == 1 { "sketch.conflicts_one" } else { "sketch.conflicts" };
        (tf(key, &[("n", &st.conflicting.len())]), theme::of(ui).warning)
    } else if st.dof == 0 {
        (t("sketch.fully_constrained").to_owned(), theme::palette(ui.visuals().dark_mode).constrained)
    } else if st.dof == 1 {
        (t("sketch.dof_one").to_owned(), ui.visuals().text_color())
    } else {
        (tf("sketch.dof", &[("n", &st.dof)]), ui.visuals().text_color())
    };
    ui.label(RichText::new(text).color(colour));
    ui.label(
        RichText::new(tf(
            "sketch.counts",
            &[
                ("points", &sketch.points.len()),
                ("edges", &sketch.edges.len()),
                ("constraints", &sketch.constraints.len()),
                ("dimensions", &sketch.dimensions.len()),
            ],
        ))
        .weak(),
    );
    for id in &st.conflicting {
        ui.label(RichText::new(sk::describe(&sketch, *id)).color(theme::of(ui).warning));
    }
    if !st.redundant.is_empty() {
        let key = if st.redundant.len() == 1 { "sketch.redundant_one" } else { "sketch.redundant" };
        ui.label(RichText::new(tf(key, &[("n", &st.redundant.len())])).weak());
    }
    ui.horizontal(|ui| {
        let mut colours = s.ui.sketch.colours;
        if ui.checkbox(&mut colours, t("sketch.colours")).changed() {
            let _ = s.run("ui.sketch", json!({ "colours": colours }));
        }
        let mut show = s.ui.sketch.show;
        if ui.checkbox(&mut show, t("sketch.show")).changed() {
            let _ = s.run("ui.sketch", json!({ "show": show }));
        }
    });
}

/// The tags of the selected dimensions, as for objects: chips with ×, a field to add
/// one, and the tags not on all of them yet as suggestions.
fn dimension_tags(ui: &mut Ui, s: &mut Studio, sel: &[DimSource]) {
    let doc = s.engine.doc().clone();
    let tags_of = |src: &DimSource| -> Vec<Id> {
        match src {
            DimSource::Sketch { layer, id } => doc
                .sketches
                .get(layer)
                .and_then(|sk| sk.dimension(*id))
                .map(|d| d.tags.iter().filter_map(|t| t.parse().ok()).collect())
                .unwrap_or_default(),
            DimSource::Reference(id) => doc.dimensions.get(id).map(|d| d.tags.clone()).unwrap_or_default(),
        }
    };
    let per: Vec<Vec<Id>> = sel.iter().map(tags_of).collect();
    let ids: Vec<Value> = sel.iter().map(|d| dim_args(*d)["id"].clone()).collect();
    let layer = sel.iter().find_map(|d| match d {
        DimSource::Sketch { layer, .. } => Some(*layer),
        DimSource::Reference(_) => None,
    });
    let run = |s: &mut Studio, key: &str, tag: &str| {
        let mut args = json!({ "ids": ids, key: [tag] });
        if let Some(l) = layer {
            args["layer"] = json!(l);
        }
        let _ = s.run("dimension.set_tags", args);
    };
    let current: Vec<(Id, bool)> = {
        let mut all: Vec<Id> = per.iter().flatten().copied().collect();
        all.sort();
        all.dedup();
        all.into_iter().map(|t| (t, per.iter().all(|p| p.contains(&t)))).collect()
    };
    crate::widgets::tag_editor(ui, s, &doc, &current, "Dimension", |s, tag, add| {
        run(s, if add { "add" } else { "remove" }, tag)
    });
}

fn dimension_fields(ui: &mut Ui, s: &mut Studio, src: DimSource) {
    let doc = s.engine.doc();
    let (value, angle, driving, name) = match src {
        DimSource::Sketch { layer, id } => {
            let Some(sketch) = doc.sketches.get(&layer) else { return };
            let Some(d) = sketch.dimension(id) else {
                s.ui.sketch.selected = None;
                return;
            };
            (sketch.measure(&d.measure).unwrap_or(d.value), d.measure.is_angle(), Some(d.driving), d.name.clone())
        }
        DimSource::Reference(id) => {
            let Some(d) = doc.dimensions.get(&id) else {
                s.ui.sketch.selected = None;
                return;
            };
            (d.measure(doc).map_or(0.0, |m| m.2), false, None, d.name.clone())
        }
    };
    let title = if driving == Some(true) { t("sketch.driving_dimension") } else { t("sketch.reference_dimension") };
    crate::widgets::section(ui, title);
    egui::Grid::new("inspector_dimension").num_columns(2).spacing([6.0, 3.0]).show(ui, |ui| {
        let u = crate::lengths::of(s);
        crate::widgets::prop_label(
            ui,
            if angle { t("sketch.value").to_owned() } else { crate::lengths::label(&u, t("sketch.value")) },
        );
        let shown = if angle { dims::format_value(value, true) } else { u.field(value) };
        if driving == Some(true) {
            let id = egui::Id::unique(("inspector", "dimension_value"));
            if let Some(v) = widgets::edit_field(ui, id, &shown, f32::INFINITY, "Inspector Dimension value") {
                let parsed = if angle { crate::tools::parse_angle(&v) } else { u.parse(&v) };
                match parsed {
                    Some(mm) => {
                        let mut args = dim_args(src);
                        args["value"] = json!(mm);
                        let _ = s.run("dimension.set_value", args);
                    }
                    None => s.ui.error(tf("status.bad_length", &[("text", &v)])),
                }
            }
        } else {
            ui.label(shown);
        }
        ui.end_row();
        if let Some(d) = driving {
            crate::widgets::prop_label(ui, t("sketch.driving"));
            let mut on = d;
            let r = ui.checkbox(&mut on, "");
            widgets::name(&r, egui::Role::CheckBox, "Inspector Driving");
            if r.changed() {
                let mut args = dim_args(src);
                args["driving"] = json!(on);
                let _ = s.run("dimension.set_driving", args);
            }
            ui.end_row();
        }
        crate::widgets::prop_label(ui, t("sketch.name"));
        let id = egui::Id::unique(("inspector", "dimension_name"));
        if let Some(v) =
            widgets::edit_field(ui, id, name.as_deref().unwrap_or(""), f32::INFINITY, "Inspector Dimension name")
        {
            let mut args = dim_args(src);
            args["name"] = json!(v);
            let _ = s.run("dimension.name", args);
        }
        ui.end_row();
    });
    if widgets::button(ui, t("sketch.remove_dimension"), "Remove dimension").clicked() {
        let mut args = dim_args(src);
        let id = args["id"].take();
        args["ids"] = json!([id]);
        if s.run("dimension.remove", args).is_ok() {
            s.ui.sketch.selected = None;
        }
    }
}

/// Inspector rows for walls and floors: follows the sketch (unlink) or free (adopt).
pub fn link_row(ui: &mut Ui, s: &mut Studio, ids: &[Id]) {
    let doc = s.engine.doc();
    let linked: Vec<Id> =
        ids.iter().copied().filter(|id| doc.objects.get(id).is_some_and(|o| o.sketch.is_some())).collect();
    let free: Vec<Id> = ids.iter().copied().filter(|id| !linked.contains(id)).collect();
    crate::widgets::prop_label(ui, t("sketch.geometry"));
    ui.horizontal(|ui| {
        if !linked.is_empty() {
            ui.label(RichText::new(t("sketch.follows")).weak());
            if widgets::button(ui, t("sketch.unlink"), "Unlink from sketch").clicked() {
                let _ = s.run("sketch.unlink", json!({ "ids": linked }));
            }
        }
        if !free.is_empty() {
            if linked.is_empty() {
                ui.label(RichText::new(t("sketch.free")).weak());
            }
            if widgets::button(ui, t("sketch.adopt"), "Adopt into sketch").clicked() {
                let _ = s.run("sketch.adopt", json!({ "ids": free }));
            }
        }
    });
    ui.end_row();
}

/// The dimension tool's preview: from the picks so far to the pointer.
pub fn tool_preview(p: &egui::Painter, cam: &Camera, pal: &Palette, picks: &[Pick], cursor: P2) {
    match picks {
        [a] => {
            p.line_segment([cam.to_screen(a.at()), cam.to_screen(cursor)], Stroke::new(1.0, pal.preview));
        }
        [a, b] => {
            let axis = dims::auto_axis(a.at(), b.at(), cursor);
            let st = DimStyle::for_scale(cam.scale, 5.0);
            if let Some(d) = dims::preview(a.at(), b.at(), axis, cursor, &st) {
                draw_dim(p, cam, pal, &d, pal.preview, false);
            }
        }
        _ => {}
    }
    for pk in picks {
        p.circle_stroke(cam.to_screen(pk.at()), 4.0, Stroke::new(1.5, pal.preview));
    }
}

/// What a click with the dimension tool picks at `raw` (snapped to `snapped`).
pub fn pick(s: &Studio, raw: P2, snapped: &stagecraft_geom::snap::Snapped, tol: f64) -> Pick {
    let doc = s.engine.doc();
    if let Some((layer, point, at)) = point_at(doc, raw, tol) {
        return Pick::Sketch { layer, point, at };
    }
    use stagecraft_geom::snap::SnapKind;
    if matches!(snapped.kind, SnapKind::Endpoint | SnapKind::Midpoint | SnapKind::Truss)
        && let Some(id) = snapped.source.filter(|id| doc.objects.contains_key(id))
        && let Some(w) = stagecraft_core::geom::world(doc, id)
    {
        let p = snapped.point;
        let local = w.inverse().transform_point3(glam::DVec3::new(p[0], p[1], w.translation.z));
        return Pick::Object { id, local: local.to_array(), at: p };
    }
    let z = s.ui.active_layer.and_then(|l| doc.layers.get(&l)).map_or(0.0, |l| l.elevation);
    Pick::Free { at: snapped.point, z }
}

/// Creates the dimension the tool's picks and the label position describe. Between
/// two points of one sketch it is a sketch dimension (driving unless that
/// over-constrains, then the value editor opens); otherwise a reference dimension.
pub fn commit(s: &mut Studio, a: Pick, b: Pick, label: P2) {
    let axis = dims::auto_axis(a.at(), b.at(), label);
    let mid = [(a.at()[0] + b.at()[0]) / 2.0, (a.at()[1] + b.at()[1]) / 2.0];
    let offset = [label[0] - mid[0], label[1] - mid[1]];
    if let (Pick::Sketch { layer: la, point: pa, .. }, Pick::Sketch { layer: lb, point: pb, .. }) = (a, b)
        && la == lb
    {
        let ty = match axis {
            sk::DimAxis::Aligned => "distance",
            sk::DimAxis::Horizontal => "horizontal_distance",
            sk::DimAxis::Vertical => "vertical_distance",
        };
        let args = json!({ "layer": la, "measure": { "type": ty, "a": pa, "b": pb }, "label_offset": offset });
        if let Ok(v) = s.run("dimension.add", args) {
            let id = sk::ConstraintId(v["id"].as_u64().unwrap_or_default() as u32);
            let src = DimSource::Sketch { layer: la, id };
            s.ui.sketch.selected = Some(src);
            if v["driving"] == json!(true) {
                open_editor(s, src);
            } else {
                s.ui.info(t("sketch.became_reference"));
            }
        }
        return;
    }
    let args = json!({ "from": a.anchor(), "to": b.anchor(), "axis": axis, "label_offset": offset,
                       "layer": s.active_layer_arg() });
    if let Ok(v) = s.run("dimension.add_reference", args)
        && let Some(id) = v["id"].as_str().and_then(|x| x.parse().ok())
    {
        s.ui.sketch.selected = Some(DimSource::Reference(id));
    }
}

/// Constraint state of a sketch dimension, for tests and tools.
pub fn dimension_state(sui: &mut SketchUi, doc: &Document, src: DimSource) -> Option<ConstraintState> {
    let DimSource::Sketch { layer, id } = src else { return None };
    let s = doc.sketches.get(&layer)?;
    sui.status(layer, s, false).map(|st| st.constraint_state(id))
}
