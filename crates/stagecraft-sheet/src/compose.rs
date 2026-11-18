//! A sheet of the document as primitives in paper mm: frame, viewports (plan with
//! labels, elevations and sections through a [`VectorViewSource`], rendered images,
//! fixture lists), viewport titles with scale bars, the legend and the title block.

use std::collections::{BTreeMap, HashMap};

use glam::DVec3;
use stagecraft_core::model::{Document, Object, ObjectKind};
use stagecraft_core::sheet::{
    LabelTemplate, Sheet, Side, TextAlign, TitleBlock, TitleItem, ViewKind, Viewport, base64_decode,
};
use stagecraft_core::{Id, geom};
use stagecraft_geom::section::Plane;
use stagecraft_geom::{Detail, FillClass, LineClass, PlanPrim, PlanStyle, TextAnchor, math};

use crate::fonts::text_width;
use crate::labels::{self, Label};
use crate::prim::{HAlign, ImageData, LabelHit, P2, Page, Pen, Prim, Rgb, VAlign, ViewportHit, hatch};
use crate::view::{EdgeClass, VectorViewSource, ViewSpec};

/// Distance of the frame from the paper edge, mm.
pub const MARGIN: f64 = 10.0;
/// Size of a rendered viewport without a crop, paper mm.
pub const RENDERED_SIZE: P2 = [180.0, 120.0];

/// What composing needs besides the document.
pub struct Inputs<'a> {
    pub source: &'a dyn VectorViewSource,
    /// Images for rendered viewports, by viewport id. Missing ones get a placeholder.
    pub rasters: &'a HashMap<Id, ImageData>,
}

/// Pen width for a line class, mm (ISO 128 pens).
pub fn weight(vp: &Viewport, class: &str) -> f64 {
    if let Some(w) = vp.line_weights.get(class) {
        return *w;
    }
    match class {
        "thin" => 0.18,
        "medium" => 0.35,
        "thick" => 0.5,
        "hidden" => 0.25,
        "centre" => 0.18,
        "cut" => 0.7,
        _ => 0.25,
    }
}

fn class_name(c: LineClass) -> &'static str {
    match c {
        LineClass::Thin => "thin",
        LineClass::Medium => "medium",
        LineClass::Thick => "thick",
        LineClass::Hidden => "hidden",
        LineClass::Centre => "centre",
    }
}

fn pen_for(vp: &Viewport, c: LineClass) -> Pen {
    let mut p = Pen::solid(weight(vp, class_name(c)));
    p.dash = match c {
        LineClass::Hidden => Some(vec![1.6, 0.8]),
        LineClass::Centre => Some(vec![6.0, 0.8, 0.8, 0.8]),
        _ => None,
    };
    p
}

fn fill_for(f: FillClass) -> Rgb {
    match f {
        FillClass::Floor => Rgb(244, 244, 244),
        FillClass::Wall => Rgb(120, 120, 120),
        FillClass::Body => Rgb::WHITE,
        FillClass::Masking => Rgb(70, 70, 70),
        FillClass::Deck => Rgb(230, 230, 230),
    }
}

/// Objects a viewport shows: visible layer, not hidden by the viewport, back to front.
pub fn shown_ids(doc: &Document, vp: &Viewport) -> Vec<Id> {
    let mut v: Vec<(u8, Id)> = doc
        .objects
        .values()
        .filter(|o| doc.layers.get(&o.layer).is_none_or(|l| l.visible))
        .filter(|o| !vp.hidden_layers.contains(&o.layer))
        .filter(|o| !o.tags.iter().any(|t| vp.hidden_tags.contains(t)))
        .map(|o| (stagecraft_geom::pick::rank(&o.kind), o.id))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    v.into_iter().map(|x| x.1).collect()
}

/// The plan style a viewport draws with.
pub fn plan_style(vp: &Viewport) -> PlanStyle {
    PlanStyle {
        detail: if vp.scale >= 200.0 { Detail::Low } else { Detail::High },
        centre_lines: false,
        labels: vp.label_template.is_none(),
    }
}

/// The view of an elevation or section viewport.
pub fn view_spec(vp: &Viewport) -> Option<ViewSpec> {
    let (dir, section) = match vp.view {
        ViewKind::Elevation { side } => (
            match side {
                Side::Front => DVec3::Y,
                Side::Back => -DVec3::Y,
                Side::Left => DVec3::X,
                Side::Right => -DVec3::X,
            },
            None,
        ),
        ViewKind::Section { origin, normal } => {
            let p = Plane::new(origin, normal);
            (-p.n(), Some(p))
        }
        _ => return None,
    };
    let up = if dir.z.abs() > 0.99 { DVec3::Y } else { DVec3::Z };
    Some(ViewSpec { dir, up, section, scale: vp.scale })
}

/// Maps view mm to paper mm for a viewport whose crop is `crop`.
#[derive(Clone, Copy, Debug)]
pub struct Mapping {
    pub crop: [P2; 2],
    pub position: P2,
    pub scale: f64,
}

impl Mapping {
    pub fn to_paper(&self, p: P2) -> P2 {
        let c = [(self.crop[0][0] + self.crop[1][0]) / 2.0, (self.crop[0][1] + self.crop[1][1]) / 2.0];
        [self.position[0] + (p[0] - c[0]) / self.scale, self.position[1] + (p[1] - c[1]) / self.scale]
    }

    pub fn to_view(&self, q: P2) -> P2 {
        let c = [(self.crop[0][0] + self.crop[1][0]) / 2.0, (self.crop[0][1] + self.crop[1][1]) / 2.0];
        [c[0] + (q[0] - self.position[0]) * self.scale, c[1] + (q[1] - self.position[1]) * self.scale]
    }

    /// The viewport's rectangle on the paper.
    pub fn rect(&self) -> [P2; 2] {
        [self.to_paper(self.crop[0]), self.to_paper(self.crop[1])]
    }
}

fn grow(b: [P2; 2], f: f64) -> [P2; 2] {
    let d = [(b[1][0] - b[0][0]) * f + 200.0, (b[1][1] - b[0][1]) * f + 200.0];
    [[b[0][0] - d[0], b[0][1] - d[1]], [b[1][0] + d[0], b[1][1] + d[1]]]
}

fn plan_prims(doc: &Document, vp: &Viewport, ids: &[Id]) -> Vec<(Id, Vec<PlanPrim>)> {
    let style = plan_style(vp);
    ids.iter().filter_map(|id| doc.objects.get(id)).map(|o| (o.id, stagecraft_geom::plan(doc, o, &style))).collect()
}

fn plan_to_prims(vp: &Viewport, m: &Mapping, prims: &[PlanPrim], out: &mut Vec<Prim>) {
    let tp = |v: &[P2]| v.iter().map(|p| m.to_paper(*p)).collect::<Vec<_>>();
    for p in prims {
        match p {
            PlanPrim::Polyline { points, closed, line } => out.push(Prim::Path {
                rings: vec![tp(points)],
                closed: *closed,
                pen: Some(pen_for(vp, *line)),
                fill: None,
            }),
            PlanPrim::Polygon { points, fill, line } => out.push(Prim::Path {
                rings: vec![tp(points)],
                closed: true,
                pen: line.map(|l| pen_for(vp, l)),
                fill: Some(fill_for(*fill)),
            }),
            PlanPrim::Circle { center, radius, fill, line } => out.push(Prim::Circle {
                center: m.to_paper(*center),
                radius: radius / m.scale,
                pen: line.map(|l| pen_for(vp, l)),
                fill: fill.map(fill_for),
            }),
            PlanPrim::Arc { center, radius, start, sweep, line } => {
                let n = ((sweep.abs() / 5.0).ceil() as usize).max(2);
                let pts: Vec<P2> = (0..=n)
                    .map(|i| m.to_paper(math::polar(*center, *radius, start + sweep * i as f64 / n as f64)))
                    .collect();
                out.push(Prim::polyline(pts, false, pen_for(vp, *line)));
            }
            PlanPrim::Text { pos, text, height, rotation, anchor } => {
                let size = height / m.scale;
                if size < 0.7 {
                    continue;
                }
                out.push(Prim::Text {
                    pos: m.to_paper(*pos),
                    text: text.clone(),
                    size,
                    angle: *rotation,
                    h: match anchor {
                        TextAnchor::Center => HAlign::Center,
                        TextAnchor::Left => HAlign::Left,
                    },
                    v: VAlign::Middle,
                    bold: false,
                    color: Rgb::BLACK,
                })
            }
        }
    }
}

/// Labels of a plan viewport, placed. `m` maps world plan mm to paper.
pub fn plan_labels(doc: &Document, vp: &Viewport, t: &LabelTemplate, ids: &[Id], m: &Mapping) -> Vec<Label> {
    let rect = m.rect();
    let inputs: Vec<_> = ids
        .iter()
        .filter_map(|id| doc.objects.get(id))
        .filter_map(|o| {
            labels::fixture_input(doc, o, t, &|p| m.to_paper(p), 1.0 / m.scale, vp.label_offsets.get(&o.id).copied())
        })
        .filter(|f| {
            f.center[0] >= rect[0][0]
                && f.center[0] <= rect[1][0]
                && f.center[1] >= rect[0][1]
                && f.center[1] <= rect[1][1]
        })
        .collect();
    // Text the plan draws itself (batten numbers, platform heights) stays readable.
    let obstacles: Vec<[P2; 2]> = plan_prims(doc, vp, ids)
        .into_iter()
        .flat_map(|x| x.1)
        .filter(|p| matches!(p, PlanPrim::Text { .. }))
        .map(|p| {
            let b = p.extent();
            [m.to_paper(b.min), m.to_paper(b.max)]
        })
        .collect();
    labels::place(t, &inputs, &text_width, &obstacles)
}

/// Primitives of placed labels: boxes, text, leaders.
pub fn label_prims(placed: &[Label], out: &mut Vec<Prim>) {
    for l in placed {
        for &[a, b] in &l.leaders {
            out.push(Prim::line(a, b, Pen::solid(0.13)));
            out.push(Prim::Circle { center: a, radius: 0.25, pen: None, fill: Some(Rgb::BLACK) });
        }
        for f in &l.fields {
            if f.boxed {
                out.push(Prim::Path {
                    rings: vec![f.corners().to_vec()],
                    closed: true,
                    pen: Some(Pen::solid(0.18)),
                    fill: Some(Rgb::WHITE),
                });
            }
            out.push(Prim::Text {
                pos: f.center,
                text: f.text.clone(),
                size: f.size,
                angle: f.angle,
                h: HAlign::Center,
                v: VAlign::Middle,
                bold: f.bold,
                color: Rgb::BLACK,
            });
        }
    }
}

/// The crop a viewport uses: its own, or everything it shows with a margin. `None`
/// for an empty view.
pub fn effective_crop(doc: &Document, vp: &Viewport, source: &dyn VectorViewSource) -> Option<[P2; 2]> {
    if let Some(c) = vp.crop {
        return Some(c);
    }
    let ids = shown_ids(doc, vp);
    match vp.view {
        ViewKind::Plan => {
            let prims: Vec<PlanPrim> = plan_prims(doc, vp, &ids).into_iter().flat_map(|x| x.1).collect();
            let b = stagecraft_geom::pick::bounds(&prims);
            (!b.is_empty()).then(|| grow([b.min, b.max], 0.02))
        }
        ViewKind::Elevation { .. } | ViewKind::Section { .. } => {
            let spec = view_spec(vp)?;
            source.view(doc, &ids, &spec).bounds().map(|b| grow(b, 0.02))
        }
        ViewKind::Rendered { .. } => {
            let [w, h] = RENDERED_SIZE;
            let (w, h) = (w * vp.scale, h * vp.scale);
            Some([[-w / 2.0, -h / 2.0], [w / 2.0, h / 2.0]])
        }
        ViewKind::FixtureList => None,
    }
}

struct Composed {
    prims: Vec<Prim>,
    rect: [P2; 2],
    labels: Vec<LabelHit>,
}

fn compose_viewport(doc: &Document, vp: &Viewport, inputs: &Inputs) -> Option<Composed> {
    if vp.view == ViewKind::FixtureList {
        let (prims, rect) = fixture_table(doc, vp);
        return Some(Composed { prims, rect, labels: Vec::new() });
    }
    let crop = effective_crop(doc, vp, inputs.source).unwrap_or([[-1000.0, -1000.0], [1000.0, 1000.0]]);
    let m = Mapping { crop, position: vp.position, scale: vp.scale };
    let rect = m.rect();
    let ids = shown_ids(doc, vp);
    let mut inner = Vec::new();
    let mut hits = Vec::new();
    match vp.view {
        ViewKind::Plan => {
            for (_, prims) in plan_prims(doc, vp, &ids) {
                plan_to_prims(vp, &m, &prims, &mut inner);
            }
            if let Some(t) = vp.label_template.and_then(|t| doc.label_templates.get(&t)) {
                let placed = plan_labels(doc, vp, t, &ids, &m);
                label_prims(&placed, &mut inner);
                hits = placed
                    .iter()
                    .map(|l| {
                        let b = l.bounds();
                        LabelHit { viewport: vp.id, fixture: l.fixture, min: b[0], max: b[1] }
                    })
                    .collect();
            }
        }
        ViewKind::Elevation { .. } | ViewKind::Section { .. } => {
            let spec = view_spec(vp)?;
            let v = inputs.source.view(doc, &ids, &spec);
            for c in &v.cuts {
                let rings: Vec<Vec<P2>> = c.rings.iter().map(|r| r.iter().map(|p| m.to_paper(*p)).collect()).collect();
                inner.push(Prim::Path { rings: rings.clone(), closed: true, pen: None, fill: Some(Rgb::WHITE) });
                let lines = hatch(&rings, 45.0, 1.2);
                if !lines.is_empty() {
                    inner.push(Prim::Path {
                        rings: lines.into_iter().map(|l| l.to_vec()).collect(),
                        closed: false,
                        pen: Some(Pen::solid(0.13)),
                        fill: None,
                    });
                }
            }
            // One path per class keeps the PDF small.
            let mut by_class: BTreeMap<u8, Vec<Vec<P2>>> = BTreeMap::new();
            for l in &v.lines {
                let k = match l.class {
                    EdgeClass::Hidden => 0,
                    EdgeClass::Visible => 1,
                    EdgeClass::Cut => 2,
                };
                by_class.entry(k).or_default().push(l.points.iter().map(|p| m.to_paper(*p)).collect());
            }
            for (k, rings) in by_class {
                let pen = match k {
                    0 => pen_for(vp, LineClass::Hidden),
                    1 => Pen::solid(weight(vp, "thin")),
                    _ => Pen::solid(weight(vp, "cut")),
                };
                inner.push(Prim::Path { rings, closed: false, pen: Some(pen), fill: None });
            }
        }
        ViewKind::Rendered { .. } => match inputs.rasters.get(&vp.id) {
            Some(img) => inner.push(Prim::Image { min: rect[0], max: rect[1], data: img.clone() }),
            None => {
                let grey = Pen { width: 0.25, dash: None, color: Rgb(150, 150, 150) };
                inner.push(Prim::rect(rect[0], rect[1], Some(grey.clone()), Some(Rgb(246, 246, 246))));
                inner.push(Prim::line(rect[0], rect[1], grey.clone()));
                inner.push(Prim::line([rect[0][0], rect[1][1]], [rect[1][0], rect[0][1]], grey));
                let mut t = Prim::text(
                    [(rect[0][0] + rect[1][0]) / 2.0, (rect[0][1] + rect[1][1]) / 2.0],
                    "3D view — rendered in the app",
                    3.0,
                );
                if let Prim::Text { h, v, color, .. } = &mut t {
                    *h = HAlign::Center;
                    *v = VAlign::Middle;
                    *color = Rgb(110, 110, 110);
                }
                inner.push(t);
            }
        },
        ViewKind::FixtureList => unreachable!(),
    }
    Some(Composed { prims: vec![Prim::Clip { min: rect[0], max: rect[1], prims: inner }], rect, labels: hits })
}

/// A scale bar segment length in model mm: a round number giving 8–20 mm on paper.
pub fn scale_bar_step(scale: f64) -> f64 {
    let steps = [100.0, 200.0, 250.0, 500.0, 1000.0, 2000.0, 2500.0, 5000.0, 10000.0, 20000.0, 50000.0];
    steps.iter().copied().find(|s| s / scale >= 8.0).unwrap_or(100000.0)
}

fn metres(mm: f64) -> String {
    let m = mm / 1000.0;
    if (m - m.round()).abs() < 1e-9 { format!("{m:.0}") } else { format!("{m}") }
}

fn viewport_title(vp: &Viewport, rect: [P2; 2], out: &mut Vec<Prim>) {
    if !vp.show_title {
        return;
    }
    let y = rect[0][1] - 6.0;
    let x = rect[0][0];
    let mut name = Prim::text([x, y], vp.name.clone(), 3.0);
    if let Prim::Text { bold, .. } = &mut name {
        *bold = true;
    }
    let w = text_width(&vp.name, 3.0, true);
    out.push(name);
    let vector = !matches!(vp.view, ViewKind::FixtureList | ViewKind::Rendered { .. });
    let mut after = x + w + 3.0;
    if vector {
        let s = format!("1:{}", vp.scale);
        out.push(Prim::text([after, y], s.clone(), 2.5));
        after += text_width(&s, 2.5, false) + 6.0;
    }
    out.push(Prim::line([x, y - 1.5], [x + w, y - 1.5], Pen::solid(0.35)));
    if vp.scale_bar && vector {
        let step = scale_bar_step(vp.scale);
        let seg = step / vp.scale;
        let (x0, y0, h) = (after, y - 0.4, 1.4);
        for i in 0..4 {
            let a = [x0 + seg * i as f64, y0];
            let b = [a[0] + seg, y0 + h];
            let fill = if i % 2 == 0 { Rgb::BLACK } else { Rgb::WHITE };
            out.push(Prim::rect(a, b, Some(Pen::solid(0.18)), Some(fill)));
        }
        for i in [0, 2, 4] {
            let mut t = Prim::text([x0 + seg * i as f64, y0 + h + 1.0], metres(step * i as f64), 1.6);
            if let Prim::Text { h, .. } = &mut t {
                *h = HAlign::Center;
            }
            out.push(t);
        }
        out.push(Prim::text([x0 + seg * 4.0 + 1.5, y0], "m", 1.6));
    }
}

/// Fixtures a viewport shows, by FID then name.
fn fixtures_of<'a>(doc: &'a Document, vp: &Viewport) -> Vec<&'a Object> {
    let mut v: Vec<&Object> = shown_ids(doc, vp)
        .into_iter()
        .filter_map(|id| doc.objects.get(&id))
        .filter(|o| matches!(o.kind, ObjectKind::Fixture { .. }))
        .collect();
    let fid = |o: &Object| match o.kind {
        ObjectKind::Fixture { fid, .. } => fid.unwrap_or(u32::MAX),
        _ => u32::MAX,
    };
    v.sort_by(|a, b| fid(a).cmp(&fid(b)).then_with(|| a.name.cmp(&b.name)));
    v
}

/// A fixture's type for legends and lists.
pub fn fixture_type(doc: &Document, o: &Object) -> String {
    labels::type_name(doc, o)
}

fn fixture_table(doc: &Document, vp: &Viewport) -> (Vec<Prim>, [P2; 2]) {
    let cols = ["FID", "Name", "Type", "Mode", "Ch", "Address", "Gel", "Circuit", "Note"];
    let rows: Vec<[String; 9]> = fixtures_of(doc, vp)
        .into_iter()
        .map(|o| {
            let ObjectKind::Fixture { fid, .. } = &o.kind else { unreachable!() };
            let a = |k: &str| o.attrs.get(k).cloned().unwrap_or_default();
            [
                fid.map(|f| f.to_string()).unwrap_or_default(),
                o.name.clone(),
                fixture_type(doc, o),
                labels::mode_name(doc, o),
                stagecraft_core::patch::mode(doc, o).map(|m| m.footprint_text()).unwrap_or_default(),
                stagecraft_core::patch::address_text(o),
                a("gel"),
                a("circuit"),
                a("note"),
            ]
        })
        .collect();
    let (size, row_h, pad) = (1.8, 4.4, 1.5);
    let widths: Vec<f64> = (0..cols.len())
        .map(|c| {
            let w = rows.iter().map(|r| text_width(&r[c], size, false)).fold(text_width(cols[c], size, true), f64::max);
            w + 2.0 * pad
        })
        .collect();
    let total_w: f64 = widths.iter().sum();
    let total_h = row_h * (rows.len() + 1) as f64;
    let min = [vp.position[0] - total_w / 2.0, vp.position[1] - total_h / 2.0];
    let max = [min[0] + total_w, min[1] + total_h];
    let mut out = vec![Prim::rect([min[0], max[1] - row_h], max, None, Some(Rgb(230, 230, 230)))];
    let mut x = min[0];
    for (c, w) in widths.iter().enumerate() {
        let mut t = Prim::text([x + pad, max[1] - row_h + 1.3], cols[c], size);
        if let Prim::Text { bold, .. } = &mut t {
            *bold = true;
        }
        out.push(t);
        for (r, row) in rows.iter().enumerate() {
            out.push(Prim::text([x + pad, max[1] - row_h * (r as f64 + 2.0) + 1.3], row[c].clone(), size));
        }
        x += w;
        if c + 1 < widths.len() {
            out.push(Prim::line([x, min[1]], [x, max[1]], Pen::solid(0.13)));
        }
    }
    for r in 1..=rows.len() {
        let y = max[1] - row_h * r as f64;
        out.push(Prim::line([min[0], y], [max[0], y], Pen::solid(if r == 1 { 0.35 } else { 0.13 })));
    }
    out.push(Prim::rect(min, max, Some(Pen::solid(0.35)), None));
    (out, [min, max])
}

/// Placeholder values for title blocks.
pub fn placeholders(doc: &Document, sheet: &Sheet) -> Vec<(&'static str, String)> {
    let m = &doc.meta;
    let mut scales: Vec<String> = Vec::new();
    for v in &sheet.viewports {
        if matches!(v.view, ViewKind::Plan | ViewKind::Elevation { .. } | ViewKind::Section { .. }) {
            let s = format!("1:{}", v.scale);
            if !scales.contains(&s) {
                scales.push(s);
            }
        }
    }
    vec![
        ("project", m.project.clone()),
        ("venue", m.venue.clone()),
        ("drawn_by", m.drawn_by.clone()),
        ("date", m.date.clone()),
        ("scale", scales.join(", ")),
        ("revision", m.revision.clone()),
        ("sheet_name", sheet.name.clone()),
        ("sheet_number", sheet.number.clone()),
        ("company", m.company.clone()),
        ("paper", sheet.paper.name()),
    ]
}

/// `{key}` replaced by its value.
pub fn fill(text: &str, values: &[(&str, String)]) -> String {
    let mut s = text.to_owned();
    for (k, v) in values {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s.trim().to_owned()
}

fn title_block(doc: &Document, sheet: &Sheet, tb: &TitleBlock, origin: P2, out: &mut Vec<Prim>) {
    let values = placeholders(doc, sheet);
    let at = |p: P2| [origin[0] + p[0], origin[1] + p[1]];
    out.push(Prim::rect(origin, at([tb.width, tb.height]), None, Some(Rgb::WHITE)));
    for item in &tb.items {
        match item {
            TitleItem::Line { from, to, weight } => out.push(Prim::line(at(*from), at(*to), Pen::solid(*weight))),
            TitleItem::Rect { min, max, weight } => {
                out.push(Prim::rect(at(*min), at(*max), Some(Pen::solid(*weight)), None))
            }
            TitleItem::Text { pos, text, size, bold, align } => {
                let s = fill(text, &values);
                if s.is_empty() {
                    continue;
                }
                // Shrink text that would run into the next cell.
                let room = if *align == TextAlign::Left {
                    tb.items
                        .iter()
                        .filter_map(|i| match i {
                            TitleItem::Line { from, to, .. }
                                if (from[0] - to[0]).abs() < 1e-6
                                    && from[0] > pos[0]
                                    && pos[1] >= from[1].min(to[1])
                                    && pos[1] <= from[1].max(to[1]) =>
                            {
                                Some(from[0])
                            }
                            _ => None,
                        })
                        .fold(tb.width, f64::min)
                        - pos[0]
                        - 1.5
                } else {
                    f64::INFINITY
                };
                let w = text_width(&s, *size, *bold);
                let size = if w > room && room > 0.0 { size * room / w } else { *size };
                out.push(Prim::Text {
                    pos: at(*pos),
                    text: s,
                    size,
                    angle: 0.0,
                    h: match align {
                        TextAlign::Left => HAlign::Left,
                        TextAlign::Center => HAlign::Center,
                        TextAlign::Right => HAlign::Right,
                    },
                    v: VAlign::Baseline,
                    bold: *bold,
                    color: Rgb::BLACK,
                });
            }
            TitleItem::Logo { min, max } => {
                let Some(bytes) = doc.meta.logo.as_deref().and_then(base64_decode) else { continue };
                let data = ImageData::Encoded(std::sync::Arc::new(bytes));
                let Some((w, h)) = data.size() else { continue };
                let (bw, bh) = (max[0] - min[0], max[1] - min[1]);
                let k = (bw / w as f64).min(bh / h as f64);
                let (iw, ih) = (w as f64 * k, h as f64 * k);
                let lo = at([min[0] + (bw - iw) / 2.0, min[1] + (bh - ih) / 2.0]);
                out.push(Prim::Image { min: lo, max: [lo[0] + iw, lo[1] + ih], data });
            }
        }
    }
}

fn legend(doc: &Document, sheet: &Sheet, right: f64, bottom: f64, width: f64, out: &mut Vec<Prim>) {
    // Type name → (count, a fixture of that type for its symbol, footprint if all agree).
    let mut counts: BTreeMap<String, (usize, Id, Option<String>)> = BTreeMap::new();
    let mut seen: Vec<Id> = Vec::new();
    for vp in sheet.viewports.iter().filter(|v| v.view == ViewKind::Plan) {
        for o in fixtures_of(doc, vp) {
            if !seen.contains(&o.id) {
                seen.push(o.id);
                let fp = stagecraft_core::patch::mode(doc, o).map(|m| m.footprint_text());
                let e = counts.entry(fixture_type(doc, o)).or_insert((0, o.id, fp.clone()));
                e.0 += 1;
                if e.2 != fp {
                    e.2 = None;
                }
            }
        }
    }
    if counts.is_empty() {
        return;
    }
    let row = 8.0;
    let h = row * (counts.len() as f64 + 1.0) + 2.0;
    let min = [right - width, bottom];
    let max = [right, bottom + h];
    out.push(Prim::rect(min, max, Some(Pen::solid(0.35)), Some(Rgb::WHITE)));
    let mut t = Prim::text([min[0] + 3.0, max[1] - 5.5], "LEGEND", 2.2);
    if let Prim::Text { bold, .. } = &mut t {
        *bold = true;
    }
    out.push(t);
    let total: usize = counts.values().map(|c| c.0).sum();
    let mut tt = Prim::text([max[0] - 3.0, max[1] - 5.5], format!("{total} fixtures"), 1.8);
    if let Prim::Text { h, .. } = &mut tt {
        *h = HAlign::Right;
    }
    out.push(tt);
    for (i, (name, (n, example, footprint))) in counts.iter().enumerate() {
        let y = max[1] - row * (i as f64 + 2.0) - 0.5;
        legend_symbol(doc, *example, [min[0] + 6.5, y + 2.4], out);
        let mut label = name.clone();
        if let Some(fp) = footprint {
            label = format!("{label}  ({fp} ch)");
        }
        out.push(Prim::text([min[0] + 13.0, y + 1.6], label, 2.2));
        let mut c = Prim::text([max[0] - 3.0, y + 1.6], format!("× {n}"), 2.2);
        if let Prim::Text { h, .. } = &mut c {
            *h = HAlign::Right;
        }
        out.push(c);
    }
}

/// A fixture type's plan symbol fitted into a 9 × 7 mm box at `c` (paper mm), front up.
fn legend_symbol(doc: &Document, fixture: Id, c: P2, out: &mut Vec<Prim>) {
    let Some(o) = doc.objects.get(&fixture) else { return };
    let Some(sym) = stagecraft_geom::gdtf::symbol(doc, o) else {
        // The generic box.
        out.push(Prim::rect(
            [c[0] - 2.6, c[1] - 3.4],
            [c[0] + 2.6, c[1] + 3.4],
            Some(Pen::solid(0.35)),
            Some(Rgb::WHITE),
        ));
        out.push(Prim::line([c[0] - 2.6, c[1] + 2.6], [c[0] + 2.6, c[1] + 2.6], Pen::solid(0.5)));
        return;
    };
    let (lo, hi) = sym.bounds;
    let k = (9.0 / (hi[0] - lo[0]).max(1e-6)).min(7.0 / (hi[1] - lo[1]).max(1e-6));
    let mid = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let map = |p: &P2| [c[0] + (p[0] - mid[0]) * k, c[1] + (p[1] - mid[1]) * k];
    let pen = |l: LineClass| {
        Pen::solid(match l {
            LineClass::Thick => 0.35,
            LineClass::Medium => 0.25,
            _ => 0.13,
        })
    };
    for p in &sym.prims {
        match p {
            stagecraft_geom::gdtf::LocalPrim::Polygon { points, fill, line } => out.push(Prim::Path {
                rings: vec![points.iter().map(map).collect()],
                closed: true,
                pen: line.map(pen),
                fill: Some(fill_for(*fill)),
            }),
            stagecraft_geom::gdtf::LocalPrim::Polyline { points, closed, line } => out.push(Prim::Path {
                rings: vec![points.iter().map(map).collect()],
                closed: *closed,
                pen: Some(pen(*line)),
                fill: None,
            }),
        }
    }
}

/// Compose one sheet. `None` if it does not exist.
pub fn compose(doc: &Document, sheet: Id, inputs: &Inputs) -> Option<Page> {
    let sheet = doc.sheets.get(&sheet)?;
    let [w, h] = sheet.size();
    let mut prims = vec![Prim::rect([0.0, 0.0], [w, h], None, Some(Rgb::WHITE))];
    let mut viewports = Vec::new();
    let mut labels = Vec::new();
    for vp in &sheet.viewports {
        let Some(c) = compose_viewport(doc, vp, inputs) else { continue };
        prims.extend(c.prims);
        viewport_title(vp, c.rect, &mut prims);
        viewports.push(ViewportHit { id: vp.id, min: c.rect[0], max: c.rect[1] });
        labels.extend(c.labels);
    }
    prims.push(Prim::rect([MARGIN, MARGIN], [w - MARGIN, h - MARGIN], Some(Pen::solid(0.7)), None));
    let generic = TitleBlock::generic();
    let tb = sheet.title_block.and_then(|t| doc.title_blocks.get(&t)).unwrap_or(&generic);
    let (tb_w, tb_top) = if sheet.show_title_block {
        let origin = [w - MARGIN - tb.width, MARGIN];
        title_block(doc, sheet, tb, origin, &mut prims);
        (tb.width, MARGIN + tb.height)
    } else {
        (180.0, MARGIN)
    };
    if sheet.show_legend {
        legend(doc, sheet, w - MARGIN, tb_top, tb_w, &mut prims);
    }
    Some(Page { size: [w, h], prims, viewports, labels })
}

/// World → paper mapping of a plan viewport (for the GUI's hit tests and drags).
pub fn plan_mapping(doc: &Document, vp: &Viewport, source: &dyn VectorViewSource) -> Option<Mapping> {
    let crop = effective_crop(doc, vp, source)?;
    Some(Mapping { crop, position: vp.position, scale: vp.scale })
}

/// World position of a fixture in plan, for tests.
pub fn plan_point(doc: &Document, id: Id) -> Option<P2> {
    geom::world(doc, id).map(|w| [w.translation.x, w.translation.y])
}
