//! A sheet of the document as primitives in paper mm: frame, viewports (plan with
//! labels, elevations and sections through a [`VectorViewSource`], rendered images,
//! fixture lists), viewport titles with scale bars, the legend and the title block.

use std::collections::{BTreeMap, HashMap};

use glam::DVec3;
use stagecraft_core::model::{Document, Object, ObjectKind};
use stagecraft_core::sheet::text::tr;
use stagecraft_core::sheet::{
    LabelTemplate, Sheet, Side, TagLines, TextAlign, TitleBlock, TitleItem, ViewKind, Viewport, base64_decode,
};
use stagecraft_core::units::{Language, LengthFormat};
use stagecraft_core::{Id, geom};
use stagecraft_geom::section::Plane;
use stagecraft_geom::{Detail, FillClass, LineClass, MaterialClass, PlanPrim, PlanStyle, TextAnchor, math};

use crate::fonts::text_width;
use crate::labels::{self, Label};
use crate::prim::{HAlign, ImageData, LabelHit, P2, Page, Pen, Prim, Rgb, VAlign, ViewportHit, hatch};
use crate::view::{EdgeClass, VectorViewSource, ViewOptions, ViewSpec};

/// Distance of the frame from the paper edge, mm.
pub const MARGIN: f64 = 10.0;
/// Zoned borders: the outer line's distance from the paper edge, mm.
pub const ZONE_OUTER: f64 = 5.0;
/// Zoned borders: the inner frame's distance from the paper edge, mm.
pub const ZONE_INNER: f64 = 13.0;
/// Zoned borders aim at zones about this size, mm.
pub const ZONE_SIZE: f64 = 140.0;
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

/// The tag line style that applies to an object in a viewport: the first of the
/// viewport's tag entries the object carries.
pub fn tag_lines<'a>(doc: &Document, vp: &'a Viewport, object: Id) -> Option<&'a TagLines> {
    if vp.tag_lines.is_empty() {
        return None;
    }
    let o = doc.objects.get(&object)?;
    vp.tag_lines.iter().find(|(t, _)| o.tags.contains(t)).map(|x| x.1)
}

/// The pen for a line of class `c`, with a tag override for visible lines.
fn pen_with(vp: &Viewport, c: LineClass, over: Option<&TagLines>) -> Pen {
    let Some(t) = over.filter(|_| c != LineClass::Hidden) else { return pen_for(vp, c) };
    let c = t.class.as_deref().and_then(stagecraft_geom::kinds::line_class).unwrap_or(c);
    let mut p = pen_for(vp, c);
    if let Some(w) = t.weight {
        p.width = w;
    }
    p
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
    let shown = doc
        .objects
        .values()
        .filter(|o| doc.layers.get(&o.layer).is_none_or(|l| l.visible))
        .filter(|o| !vp.hidden_layers.contains(&o.layer))
        .filter(|o| !o.tags.iter().any(|t| vp.hidden_tags.contains(t)))
        .filter(|o| vp.underlays || !matches!(o.kind, ObjectKind::Underlay { .. }));
    stagecraft_geom::pick::back_to_front(doc, shown)
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
        ViewKind::PlanFrom3d { cut } => (-DVec3::Z, cut.map(|z| Plane::new([0.0, 0.0, z], [0.0, 0.0, 1.0]))),
        _ => return None,
    };
    let up = if dir.z.abs() > 0.99 { DVec3::Y } else { DVec3::Z };
    Some(ViewSpec { dir, up, section, scale: vp.scale })
}

/// What a vector viewport asks its source for.
pub fn view_options(vp: &Viewport) -> ViewOptions {
    ViewOptions { hidden_lines: vp.hidden_lines }
}

/// Elevations, sections and plans from 3D: drawn from meshes by a [`VectorViewSource`].
pub fn is_vector_3d(v: &ViewKind) -> bool {
    matches!(v, ViewKind::Elevation { .. } | ViewKind::Section { .. } | ViewKind::PlanFrom3d { .. })
}

/// Fill and hatch of a cut face by material: (fill, hatch angle, spacing; 0 = none).
fn cut_style(m: MaterialClass) -> (Rgb, f64, f64) {
    match m {
        MaterialClass::Metal => (Rgb(40, 40, 40), 0.0, 0.0),
        MaterialClass::Fixture | MaterialClass::Lens => (Rgb(170, 170, 170), 0.0, 0.0),
        MaterialClass::Curtain | MaterialClass::Masking => (Rgb(95, 95, 95), 0.0, 0.0),
        MaterialClass::Deck => (Rgb::WHITE, 135.0, 1.6),
        MaterialClass::Floor => (Rgb::WHITE, 45.0, 2.0),
        MaterialClass::Wall | MaterialClass::Generic => (Rgb::WHITE, 45.0, 1.2),
    }
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

/// Asset bytes as image data, shared between composes.
fn asset_image(doc: &Document, hash: &str) -> Option<ImageData> {
    use std::sync::{Arc, Mutex};
    static CACHE: Mutex<Option<HashMap<String, Arc<Vec<u8>>>>> = Mutex::new(None);
    let mut guard = CACHE.lock().ok()?;
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(b) = cache.get(hash) {
        return Some(ImageData::Encoded(b.clone()));
    }
    let bytes = Arc::new(doc.assets.get(hash)?.data.as_slice().to_vec());
    if cache.len() > 16 {
        cache.clear();
    }
    cache.insert(hash.to_owned(), bytes.clone());
    Some(ImageData::Encoded(bytes))
}

/// Grey of an underlay's lines on paper: black faded towards white by opacity.
fn faded(opacity: f64) -> Rgb {
    let g = (255.0 * (1.0 - opacity.clamp(0.0, 1.0))).round() as u8;
    Rgb(g, g, g)
}

fn plan_to_prims(
    doc: &Document,
    vp: &Viewport,
    m: &Mapping,
    prims: &[PlanPrim],
    over: Option<&TagLines>,
    out: &mut Vec<Prim>,
) {
    let pen_for = |vp: &Viewport, c: LineClass| pen_with(vp, c, over);
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
            PlanPrim::Image { corners, asset, opacity } => {
                let Some(data) = asset_image(doc, asset) else { continue };
                let [a, b, _, d] = corners.map(|c| m.to_paper(c));
                let w = (b[0] - a[0]).hypot(b[1] - a[1]);
                let h = (d[0] - a[0]).hypot(d[1] - a[1]);
                let angle = (b[1] - a[1]).atan2(b[0] - a[0]).to_degrees();
                out.push(Prim::Image { min: a, max: [a[0] + w, a[1] + h], angle, opacity: *opacity, data });
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
            let mut prims: Vec<PlanPrim> = plan_prims(doc, vp, &ids).into_iter().flat_map(|x| x.1).collect();
            // Dimensions (with room for their text) belong in the fitted view too.
            for d in viewport_dims(doc, vp) {
                prims.extend(d.lines.into_iter().map(|l| PlanPrim::line(l, LineClass::Thin)));
                let r = DIM_TEXT * 1.6 * vp.scale;
                prims.push(PlanPrim::line(
                    vec![[d.label[0] - r, d.label[1] - r], [d.label[0] + r, d.label[1] + r]],
                    LineClass::Thin,
                ));
            }
            let b = stagecraft_geom::pick::bounds(&prims);
            (!b.is_empty()).then(|| grow([b.min, b.max], 0.02))
        }
        ViewKind::Elevation { .. } | ViewKind::Section { .. } | ViewKind::PlanFrom3d { .. } => {
            let spec = view_spec(vp)?;
            source.view(doc, &ids, &spec, view_options(vp)).bounds().map(|b| grow(b, 0.02))
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
            for (id, prims) in plan_prims(doc, vp, &ids) {
                let start = inner.len();
                plan_to_prims(doc, vp, &m, &prims, tag_lines(doc, vp, id), &mut inner);
                // Underlay linework is drawn faded, like on screen.
                if let Some(ObjectKind::Underlay { opacity, .. }) = doc.objects.get(&id).map(|o| &o.kind) {
                    for p in &mut inner[start..] {
                        match p {
                            Prim::Path { pen: Some(pen), .. } => pen.color = faded(*opacity),
                            Prim::Text { color, .. } => *color = faded(*opacity),
                            _ => {}
                        }
                    }
                }
            }
        }
        ViewKind::Elevation { .. } | ViewKind::Section { .. } | ViewKind::PlanFrom3d { .. } => {
            let spec = view_spec(vp)?;
            let v = inputs.source.view(doc, &ids, &spec, view_options(vp));
            for c in &v.cuts {
                let rings: Vec<Vec<P2>> = c.rings.iter().map(|r| r.iter().map(|p| m.to_paper(*p)).collect()).collect();
                let (fill, angle, spacing) = cut_style(c.material);
                inner.push(Prim::Path { rings: rings.clone(), closed: true, pen: None, fill: Some(fill) });
                let lines = if spacing > 0.0 { hatch(&rings, angle, spacing) } else { Vec::new() };
                if !lines.is_empty() {
                    inner.push(Prim::Path {
                        rings: lines.into_iter().map(|l| l.to_vec()).collect(),
                        closed: false,
                        pen: Some(Pen::solid(0.13)),
                        fill: None,
                    });
                }
            }
            // One path per pen keeps the PDF small; hidden lines first, cuts on top.
            type PenKey = (u8, u64, u8);
            let mut by_pen: BTreeMap<PenKey, (Pen, Vec<Vec<P2>>)> = BTreeMap::new();
            for l in &v.lines {
                let over = tag_lines(doc, vp, l.object);
                let (order, pen) = match l.class {
                    EdgeClass::Hidden => (0, pen_for(vp, LineClass::Hidden)),
                    EdgeClass::Visible => (1, pen_with(vp, LineClass::Thin, over)),
                    EdgeClass::Centre => (2, pen_for(vp, LineClass::Centre)),
                    EdgeClass::Cut => (3, Pen::solid(weight(vp, "cut"))),
                };
                let dash = pen.dash.as_ref().map_or(0, |d| d.len() as u8);
                by_pen
                    .entry((order, pen.width.to_bits(), dash))
                    .or_insert_with(|| (pen, Vec::new()))
                    .1
                    .push(l.points.iter().map(|p| m.to_paper(*p)).collect());
            }
            for (_, (pen, rings)) in by_pen {
                inner.push(Prim::Path { rings, closed: false, pen: Some(pen), fill: None });
            }
        }
        ViewKind::Rendered { .. } => match inputs.rasters.get(&vp.id) {
            Some(img) => {
                inner.push(Prim::Image { min: rect[0], max: rect[1], angle: 0.0, opacity: 1.0, data: img.clone() })
            }
            None => {
                let grey = Pen { width: 0.25, dash: None, color: Rgb(150, 150, 150) };
                inner.push(Prim::rect(rect[0], rect[1], Some(grey.clone()), Some(Rgb(246, 246, 246))));
                inner.push(Prim::line(rect[0], rect[1], grey.clone()));
                inner.push(Prim::line([rect[0][0], rect[1][1]], [rect[1][0], rect[0][1]], grey));
                let mut t = Prim::text(
                    [(rect[0][0] + rect[1][0]) / 2.0, (rect[0][1] + rect[1][1]) / 2.0],
                    tr(doc.meta.sheet_language, "rendered_placeholder"),
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
    if matches!(vp.view, ViewKind::Plan | ViewKind::PlanFrom3d { .. }) {
        dimension_prims(doc, vp, &m, &mut inner);
    }
    // Plans from 3D share world x/y with the plan, so labels go on top the same way.
    if matches!(vp.view, ViewKind::Plan | ViewKind::PlanFrom3d { .. })
        && let Some(t) = vp.label_template.and_then(|t| doc.label_templates.get(&t))
    {
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
    Some(Composed { prims: vec![Prim::Clip { min: rect[0], max: rect[1], prims: inner }], rect, labels: hits })
}

/// Text height of dimension values on paper, mm.
pub const DIM_TEXT: f64 = 2.0;

/// The dimensions a plan viewport shows, in world mm, sized for its scale.
pub fn viewport_dims(doc: &Document, vp: &Viewport) -> Vec<stagecraft_geom::dims::DimDraw> {
    let shown = |l: &Id| !vp.hidden_layers.contains(l) && doc.layers.get(l).is_some_and(|l| l.visible);
    let st = stagecraft_geom::dims::DimStyle::for_scale(1.0 / vp.scale, 1.2).with_format(LengthFormat::of(&doc.meta));
    let mut dims = Vec::new();
    if vp.sketch_dimensions {
        for (layer, s) in doc.sketches.iter().filter(|(l, _)| shown(l)) {
            dims.extend(stagecraft_geom::dims::sketch_dims(*layer, s, &st));
        }
    }
    if vp.reference_dimensions {
        dims.extend(stagecraft_geom::dims::reference_dims(doc, &st, |d| shown(&d.layer)));
    }
    // tags: `hidden_tags` leaves tagged dimensions out, `dimension_tags` keeps only those
    let tags_of = |src: &stagecraft_geom::dims::DimSource| -> Vec<Id> {
        match src {
            stagecraft_geom::dims::DimSource::Sketch { layer, id } => doc
                .sketches
                .get(layer)
                .and_then(|s| s.dimension(*id))
                .map(|d| d.tags.iter().filter_map(|t| t.parse().ok()).collect())
                .unwrap_or_default(),
            stagecraft_geom::dims::DimSource::Reference(id) => {
                doc.dimensions.get(id).map(|d| d.tags.clone()).unwrap_or_default()
            }
        }
    };
    dims.retain(|d| {
        let tags = tags_of(&d.source);
        !tags.iter().any(|t| vp.hidden_tags.contains(t))
            && (vp.dimension_tags.is_empty() || tags.iter().any(|t| vp.dimension_tags.contains(t)))
    });
    if vp.dimension_names {
        for d in &mut dims {
            let v = stagecraft_geom::dims::format_shown(d.value, d.angle, &st.format);
            d.text = match &d.name {
                Some(n) => format!("{n} {v}"),
                None => v,
            };
        }
    }
    dims
}

/// Dimensions of a plan viewport: the sketches' (if `sketch_dimensions`) and the
/// reference dimensions between objects (if `reference_dimensions`) of the layers it
/// shows, thin pen, values in metres.
pub fn dimension_prims(doc: &Document, vp: &Viewport, m: &Mapping, out: &mut Vec<Prim>) {
    let pen = Pen::solid(weight(vp, "thin"));
    for d in viewport_dims(doc, vp) {
        let rings: Vec<Vec<P2>> = d.lines.iter().map(|l| l.iter().map(|p| m.to_paper(*p)).collect()).collect();
        out.push(Prim::Path { rings, closed: false, pen: Some(pen.clone()), fill: None });
        let at = m.to_paper(d.label);
        let pos = [at[0] + d.up[0] * DIM_TEXT * 0.8, at[1] + d.up[1] * DIM_TEXT * 0.8];
        out.push(Prim::Text {
            pos,
            text: d.text,
            size: DIM_TEXT,
            angle: d.rotation,
            h: HAlign::Center,
            v: VAlign::Middle,
            bold: false,
            color: Rgb::BLACK,
        });
    }
}

/// A scale bar segment length in model mm: a round number giving 8–20 mm on paper.
pub fn scale_bar_step(scale: f64) -> f64 {
    let steps = [100.0, 200.0, 250.0, 500.0, 1000.0, 2000.0, 2500.0, 5000.0, 10000.0, 20000.0, 50000.0];
    steps.iter().copied().find(|s| s / scale >= 8.0).unwrap_or(100000.0)
}

fn metres(mm: f64, f: &LengthFormat) -> String {
    let m = mm / 1000.0;
    f.number(&if (m - m.round()).abs() < 1e-9 { format!("{m:.0}") } else { format!("{m}") })
}

fn viewport_title(doc: &Document, vp: &Viewport, rect: [P2; 2], out: &mut Vec<Prim>) {
    let f = LengthFormat::of(&doc.meta);
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
            let mut t = Prim::text([x0 + seg * i as f64, y0 + h + 1.0], metres(step * i as f64, &f), 1.6);
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
    let lang = doc.meta.sheet_language;
    let cols = [
        "col_fid",
        "col_name",
        "col_type",
        "col_mode",
        "col_channels",
        "col_address",
        "col_gel",
        "col_circuit",
        "col_note",
    ]
    .map(|k| tr(lang, k));
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
        if v.view == ViewKind::Plan || is_vector_3d(&v.view) {
            let s = format!("1:{}", v.scale);
            if !scales.contains(&s) {
                scales.push(s);
            }
        }
    }
    let lang = m.sheet_language;
    // Sheet number and revision (and the drawing date when production dates take
    // its place) for one small line.
    let mut sheet_rev = Vec::new();
    if !sheet.number.is_empty() {
        sheet_rev.push(format!("{} {}", tr(lang, "sheet"), sheet.number));
    }
    if !m.revision.is_empty() {
        sheet_rev.push(format!("{} {}", tr(lang, "revision"), m.revision));
    }
    let date = stagecraft_core::sheet::text::date(lang, &m.date);
    if !m.production_dates.is_empty() && !m.date.is_empty() {
        sheet_rev.push(date.clone());
    }
    vec![
        ("project", m.project.clone()),
        ("venue", m.venue.clone()),
        ("venue_address", m.venue_address.clone()),
        ("drawn_by", m.drawn_by.clone()),
        ("date", date),
        ("producer", m.producer.clone()),
        ("production_dates", m.production_dates.clone()),
        ("scale", scales.join(", ")),
        ("revision", m.revision.clone()),
        ("sheet_name", sheet.name.clone()),
        ("sheet_number", sheet.number.clone()),
        ("sheet_rev", sheet_rev.join("  ·  ")),
        ("company", m.company.clone()),
        ("website", m.website.clone()),
        ("paper", sheet.paper.name()),
    ]
}

/// `{key}` replaced by its value, `{a|b}` by `b` when `a` is empty, `{t:key}` by the
/// sheet-text catalog's word in English. Unknown keys stay as they are.
pub fn fill(text: &str, values: &[(&str, String)]) -> String {
    fill_in(text, values, Language::En)
}

/// [`fill`] with catalog words in `lang`.
pub fn fill_in(text: &str, values: &[(&str, String)], lang: Language) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let key = &after[..end];
        if let Some(k) = key.strip_prefix("t:") {
            out.push_str(tr(lang, k));
        } else {
            let mut known = false;
            let mut value = "";
            for k in key.split('|') {
                if let Some((_, v)) = values.iter().find(|(n, _)| *n == k) {
                    known = true;
                    if !v.trim().is_empty() {
                        value = v;
                        break;
                    }
                }
            }
            if known {
                out.push_str(value);
            } else {
                out.push('{');
                out.push_str(key);
                out.push('}');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out.trim().to_owned()
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
            TitleItem::Text { pos, text, size, bold, align, wrap, max_lines, upward } => {
                let s = fill_in(text, &values, doc.meta.sheet_language);
                if s.is_empty() {
                    continue;
                }
                let h = match align {
                    TextAlign::Left => HAlign::Left,
                    TextAlign::Center => HAlign::Center,
                    TextAlign::Right => HAlign::Right,
                };
                if wrap.is_some() || s.contains('\n') {
                    let width = wrap.unwrap_or(f64::INFINITY).min(tb.width - pos[0]).max(1.0);
                    let mut size = *size;
                    let mut lines = crate::fonts::wrap(&s, size, *bold, width);
                    let max = max_lines.map_or(usize::MAX, |m| m.max(1) as usize);
                    while (lines.len() > max || lines.iter().any(|l| text_width(l, size, *bold) > width)) && size > 0.6
                    {
                        size *= 0.93;
                        lines = crate::fonts::wrap(&s, size, *bold, width);
                    }
                    let lines: Vec<String> = lines.into_iter().filter(|l| !l.trim().is_empty()).take(max).collect();
                    let pitch = size * 1.42;
                    let n = lines.len();
                    for (k, line) in lines.into_iter().enumerate() {
                        let dy = if *upward { (n - 1 - k) as f64 * pitch } else { -(k as f64) * pitch };
                        out.push(Prim::Text {
                            pos: at([pos[0], pos[1] + dy]),
                            text: line,
                            size,
                            angle: 0.0,
                            h,
                            v: VAlign::Baseline,
                            bold: *bold,
                            color: Rgb::BLACK,
                        });
                    }
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
                    h,
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
                out.push(Prim::Image { min: lo, max: [lo[0] + iw, lo[1] + ih], angle: 0.0, opacity: 1.0, data });
            }
        }
    }
}

fn legend(doc: &Document, sheet: &Sheet, right: f64, bottom: f64, width: f64, out: &mut Vec<Prim>) {
    // Type name → (count, a fixture of that type for its symbol, footprint if all agree).
    let mut counts: BTreeMap<String, (usize, Id, Option<String>)> = BTreeMap::new();
    let mut seen: Vec<Id> = Vec::new();
    for vp in sheet.viewports.iter().filter(|v| matches!(v.view, ViewKind::Plan | ViewKind::PlanFrom3d { .. })) {
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
    // Seats in the plan viewports, for the header.
    let mut seat_rows: Vec<Id> = Vec::new();
    for vp in sheet.viewports.iter().filter(|v| matches!(v.view, ViewKind::Plan | ViewKind::PlanFrom3d { .. })) {
        for id in shown_ids(doc, vp) {
            if matches!(doc.objects[&id].kind, ObjectKind::SeatRow { .. }) && !seat_rows.contains(&id) {
                seat_rows.push(id);
            }
        }
    }
    let seats: u32 = seat_rows
        .iter()
        .map(|id| match doc.objects[id].kind {
            ObjectKind::SeatRow { count, .. } => count,
            _ => 0,
        })
        .sum();
    if counts.is_empty() && seats == 0 {
        return;
    }
    let lang = doc.meta.sheet_language;
    // Same thin black style as the compact title block, which it sits on.
    let (head, row, text) = (6.0, 7.0, 1.6);
    let h = head + row * counts.len() as f64;
    let min = [right - width, bottom];
    let max = [right, bottom + h];
    out.push(Prim::rect(min, max, Some(Pen::solid(0.25)), Some(Rgb::WHITE)));
    out.push(Prim::line([min[0], max[1] - head], [max[0], max[1] - head], Pen::solid(0.18)));
    let mut t = Prim::text([min[0] + 1.2, max[1] - head + 1.9], tr(lang, "legend"), 2.1);
    if let Prim::Text { bold, .. } = &mut t {
        *bold = true;
    }
    out.push(t);
    let total: usize = counts.values().map(|c| c.0).sum();
    let word = tr(lang, if total == 1 { "fixture" } else { "fixtures" });
    let mut summary = if counts.is_empty() { String::new() } else { format!("{total} {word}") };
    if seats > 0 {
        let seat_word = tr(lang, if seats == 1 { "seat" } else { "seats" });
        let sep = if summary.is_empty() { "" } else { " · " };
        summary = format!("{summary}{sep}{seats} {seat_word}");
    }
    let mut tt = Prim::text([max[0] - 1.2, max[1] - head + 1.9], summary, 1.4);
    if let Prim::Text { h, .. } = &mut tt {
        *h = HAlign::Right;
    }
    out.push(tt);
    for (i, (name, (n, example, footprint))) in counts.iter().enumerate() {
        let y = max[1] - head - row * (i as f64 + 1.0);
        legend_symbol(doc, *example, [min[0] + 6.0, y + row / 2.0], out);
        let mut label = name.clone();
        if let Some(fp) = footprint {
            label = format!("{label}  ({fp} {})", tr(lang, "channels"));
        }
        let count = format!("× {n}");
        let room = width - 12.0 - 3.0 - text_width(&count, text, false) - 3.0;
        let w = text_width(&label, text, false);
        let size = if w > room && room > 0.0 { text * room / w } else { text };
        out.push(Prim::text([min[0] + 12.0, y + row / 2.0 - text / 2.0], label, size));
        let mut c = Prim::text([max[0] - 1.2, y + row / 2.0 - text / 2.0], count, text);
        if let Prim::Text { h, .. } = &mut c {
            *h = HAlign::Right;
        }
        out.push(c);
    }
}

/// The sheet border with grid reference zones: an outer and an inner line, numbers
/// across top and bottom, letters down both sides (A at the top), as on Vectorworks
/// sheets.
fn zoned_border(w: f64, h: f64, out: &mut Vec<Prim>) {
    let (o, i) = (ZONE_OUTER, ZONE_INNER);
    let thin = Pen::solid(0.18);
    out.push(Prim::rect([o, o], [w - o, h - o], Some(thin.clone()), None));
    out.push(Prim::rect([i, i], [w - i, h - i], Some(Pen::solid(0.25)), None));
    let (iw, ih) = (w - 2.0 * i, h - 2.0 * i);
    let nx = ((iw / ZONE_SIZE).round() as usize).max(2);
    let ny = ((ih / ZONE_SIZE).round() as usize).max(2);
    let band = (o + i) / 2.0;
    let mark = |pos: P2, text: String, out: &mut Vec<Prim>| {
        out.push(Prim::Text {
            pos,
            text,
            size: 2.8,
            angle: 0.0,
            h: HAlign::Center,
            v: VAlign::Middle,
            bold: false,
            color: Rgb::BLACK,
        })
    };
    for k in 0..nx {
        let x0 = i + iw * k as f64 / nx as f64;
        if k > 0 {
            out.push(Prim::line([x0, o], [x0, i], thin.clone()));
            out.push(Prim::line([x0, h - i], [x0, h - o], thin.clone()));
        }
        let xc = x0 + iw / nx as f64 / 2.0;
        mark([xc, h - band], (k + 1).to_string(), out);
        mark([xc, band], (k + 1).to_string(), out);
    }
    for k in 0..ny {
        let y0 = h - i - ih * k as f64 / ny as f64;
        if k > 0 {
            out.push(Prim::line([o, y0], [i, y0], thin.clone()));
            out.push(Prim::line([w - i, y0], [w - o, y0], thin.clone()));
        }
        let yc = y0 - ih / ny as f64 / 2.0;
        let letter = zone_letter(k);
        mark([band, yc], letter.clone(), out);
        mark([w - band, yc], letter, out);
    }
}

/// A, B … Z, AA, AB …
fn zone_letter(k: usize) -> String {
    let a = (b'A' + (k % 26) as u8) as char;
    if k < 26 { a.to_string() } else { format!("{}{a}", zone_letter(k / 26 - 1)) }
}

/// Distance of the drawing frame from the paper edge for a title block's border.
pub fn frame_inset(tb: &TitleBlock) -> f64 {
    if tb.zones { ZONE_INNER } else { MARGIN }
}

/// A fixture type's plan symbol fitted into an 8 × 5.6 mm box at `c` (paper mm), front up.
fn legend_symbol(doc: &Document, fixture: Id, c: P2, out: &mut Vec<Prim>) {
    let Some(o) = doc.objects.get(&fixture) else { return };
    let Some(sym) = stagecraft_geom::gdtf::symbol(doc, o) else {
        // The generic box.
        out.push(Prim::rect(
            [c[0] - 2.0, c[1] - 2.6],
            [c[0] + 2.0, c[1] + 2.6],
            Some(Pen::solid(0.35)),
            Some(Rgb::WHITE),
        ));
        out.push(Prim::line([c[0] - 2.0, c[1] + 2.0], [c[0] + 2.0, c[1] + 2.0], Pen::solid(0.35)));
        return;
    };
    let (lo, hi) = sym.bounds;
    let k = (8.0 / (hi[0] - lo[0]).max(1e-6)).min(5.6 / (hi[1] - lo[1]).max(1e-6));
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
        let start = prims.len();
        prims.extend(c.prims);
        viewport_title(doc, vp, c.rect, &mut prims);
        viewports.push(ViewportHit { id: vp.id, min: c.rect[0], max: c.rect[1], prims: [start, prims.len()] });
        labels.extend(c.labels);
    }
    let tb = TitleBlock::for_sheet(doc, sheet);
    let inset = frame_inset(&tb);
    if tb.zones {
        zoned_border(w, h, &mut prims);
    } else {
        prims.push(Prim::rect([MARGIN, MARGIN], [w - MARGIN, h - MARGIN], Some(Pen::solid(0.7)), None));
    }
    // The legend sits directly on the title block, as wide as it.
    let (tb_w, tb_top) = if sheet.show_title_block {
        let origin = [w - inset - tb.width, inset];
        title_block(doc, sheet, &tb, origin, &mut prims);
        (tb.width, inset + tb.height)
    } else {
        (tb.width, inset)
    };
    if sheet.show_legend {
        legend(doc, sheet, w - inset, tb_top, tb_w, &mut prims);
    }
    status_stamp(doc, w - inset - tb_w - 6.0, inset, &mut prims);
    Some(Page { size: [w, h], prims, viewports, labels })
}

/// The plan status (`meta.status`, e.g. ENTWURF) as a stamp left of the title block:
/// large bold red letters in a red frame, right edge at `right`, bottom at `bottom`.
fn status_stamp(doc: &Document, right: f64, bottom: f64, out: &mut Vec<Prim>) {
    let text = stagecraft_core::sheet::text::status(doc.meta.sheet_language, &doc.meta.status);
    if text.is_empty() {
        return;
    }
    let red = Rgb(190, 30, 30);
    let size = 9.0;
    let w = text_width(&text, size, true) + 8.0;
    let (min, max) = ([right - w, bottom], [right, bottom + size + 6.0]);
    out.push(Prim::rect(min, max, Some(Pen { width: 0.8, dash: None, color: red }), Some(Rgb::WHITE)));
    out.push(Prim::Text {
        pos: [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0],
        text,
        size,
        angle: 0.0,
        h: HAlign::Center,
        v: VAlign::Middle,
        bold: true,
        color: red,
    });
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
