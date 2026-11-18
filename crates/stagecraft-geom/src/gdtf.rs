//! Geometry from embedded GDTFs: the fixture mesh per type, mode and level of
//! detail, and the plan symbol in fixture-local mm. Both are built once and cached
//! (keyed by the asset's SHA-256), so views never rebuild them per frame.
//!
//! Plan symbol, first that applies:
//! 1. the built-in symbol assigned to the type (`symbol.assign`),
//! 2. the GDTF's own top-view SVGs (`models/svg`), placed per geometry,
//! 3. a silhouette of the GDTF mesh seen from above.
//!
//! Fixtures without a type draw the generic box in `kinds::fixture`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use glam::{DVec3, Mat4, Vec3};
use stagecraft_core::fixture_type::{FixtureType, symbol_svg};
use stagecraft_core::model::P2;
use stagecraft_core::{Document, Object, ObjectKind};
use stagecraft_gdtf::{Gdtf, GeometryKind};

use crate::{FillClass, LineClass, Lod, MaterialClass, Mesh};

/// A plan primitive in fixture-local mm (x right, y towards the front/upstage).
#[derive(Clone, Debug, PartialEq)]
pub enum LocalPrim {
    Polygon { points: Vec<P2>, fill: FillClass, line: Option<LineClass> },
    Polyline { points: Vec<P2>, closed: bool, line: LineClass },
}

/// Where a symbol came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolSource {
    Assigned,
    GdtfSvg,
    Silhouette,
}

/// A fixture type's plan symbol in local mm.
#[derive(Clone, Debug)]
pub struct Symbol {
    pub prims: Vec<LocalPrim>,
    /// (min, max) of all points.
    pub bounds: (P2, P2),
    pub source: SymbolSource,
}

/// Extreme points per mesh part, see [`outline_points`].
pub type Outline = Arc<Vec<Vec<Vec3>>>;

#[derive(Default)]
struct Cache {
    gdtf: HashMap<String, Option<Arc<Gdtf>>>,
    meshes: HashMap<(String, String, Lod), Arc<Mesh>>,
    symbols: HashMap<(String, String, Option<String>), Option<Arc<Symbol>>>,
    outlines: HashMap<(String, String), Option<Outline>>,
}

fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(Default::default)
}

const MAX_ENTRIES: usize = 512;

/// The type and the resolved mode name of a fixture.
pub fn type_of<'d>(doc: &'d Document, obj: &Object) -> Option<(&'d FixtureType, String)> {
    let ObjectKind::Fixture { gdtf: Some(k), mode, .. } = &obj.kind else { return None };
    let t = doc.fixture_types.get(k)?;
    let m = t.mode_or_first(mode.as_deref()).map(|m| m.name.clone()).unwrap_or_default();
    Some((t, m))
}

/// The parsed GDTF of a type, cached by asset hash. `None` if the bytes are
/// missing or broken.
pub fn parsed(doc: &Document, t: &FixtureType) -> Option<Arc<Gdtf>> {
    if let Some(g) = cache().lock().ok()?.gdtf.get(&t.asset) {
        return g.clone();
    }
    let g = doc.assets.get(&t.asset).and_then(|a| Gdtf::from_bytes(a.data.as_slice()).ok()).map(Arc::new);
    let mut c = cache().lock().ok()?;
    if c.gdtf.len() > MAX_ENTRIES {
        c.gdtf.clear();
    }
    c.gdtf.insert(t.asset.clone(), g.clone());
    g
}

/// The fixture's mesh in its local frame (hanging point at the origin, body below,
/// pan/tilt in the neutral pose), or `None` without a usable GDTF.
pub fn fixture_mesh(doc: &Document, obj: &Object, lod: Lod) -> Option<Arc<Mesh>> {
    let (t, mode) = type_of(doc, obj)?;
    let key = (t.asset.clone(), mode.clone(), lod);
    if let Some(m) = cache().lock().ok()?.meshes.get(&key) {
        return Some(m.clone());
    }
    let g = parsed(doc, t)?;
    let m = Arc::new(build_mesh(&g, &mode, lod));
    let mut c = cache().lock().ok()?;
    if c.meshes.len() > MAX_ENTRIES {
        c.meshes.clear();
    }
    c.meshes.insert(key, m.clone());
    Some(m)
}

fn build_mesh(g: &Gdtf, mode: &str, lod: Lod) -> Mesh {
    let mode = (!mode.is_empty()).then_some(mode);
    let glod = match lod {
        Lod::Full => stagecraft_gdtf::Lod::Default,
        Lod::Simple | Lod::Box => stagecraft_gdtf::Lod::Low,
    };
    let fm = g.mesh(mode, glod);
    let ts = fm.posed_transforms(&|_, _| Mat4::IDENTITY);
    let mut out = Mesh::empty();
    // Bodies first, then lenses, so there are two material runs.
    for lens in [false, true] {
        out.set_material(if lens { MaterialClass::Lens } else { MaterialClass::Fixture });
        for (p, t) in fm.parts.iter().zip(&ts) {
            let is_beam = matches!(p.instance.kind, GeometryKind::Beam(_));
            if is_beam != lens {
                continue;
            }
            let Some(m) = &p.mesh else { continue };
            append(&mut out, m, *t);
        }
    }
    if lod == Lod::Box
        && let Some((lo, hi)) = out.bounds()
    {
        let mut b = Mesh::empty();
        b.set_material(MaterialClass::Fixture);
        b.cuboid(Vec3::from_array(lo).as_dvec3(), Vec3::from_array(hi).as_dvec3());
        return b;
    }
    out
}

fn append(out: &mut Mesh, m: &stagecraft_gdtf::Mesh, t: Mat4) {
    let base = out.positions.len() as u32;
    let nm = glam::Mat3::from_mat4(t).inverse().transpose();
    out.positions.extend(m.positions.iter().map(|p| t.transform_point3(Vec3::from_array(*p)).to_array()));
    if m.normals.len() == m.positions.len() {
        out.normals.extend(m.normals.iter().map(|n| (nm * Vec3::from_array(*n)).normalize_or_zero().to_array()));
    } else {
        out.normals.extend(std::iter::repeat_n([0.0, 0.0, 1.0], m.positions.len()));
    }
    let flip = t.determinant() < 0.0;
    for tri in m.indices.chunks_exact(3) {
        if flip {
            out.indices.extend([base + tri[0], base + tri[2], base + tri[1]]);
        } else {
            out.indices.extend(tri.iter().map(|i| base + i));
        }
    }
    if let Some(g) = out.groups.last_mut() {
        g.count = out.indices.len() as u32 - g.start;
    }
}

/// The plan symbol of a fixture with a type, in local mm.
pub fn symbol(doc: &Document, obj: &Object) -> Option<Arc<Symbol>> {
    let (t, mode) = type_of(doc, obj)?;
    let key = (t.asset.clone(), mode.clone(), t.symbol.clone());
    if let Some(s) = cache().lock().ok()?.symbols.get(&key) {
        return s.clone();
    }
    let s = build_symbol(doc, t, &mode).map(Arc::new);
    let mut c = cache().lock().ok()?;
    if c.symbols.len() > MAX_ENTRIES {
        c.symbols.clear();
    }
    c.symbols.insert(key, s.clone());
    s
}

fn build_symbol(doc: &Document, t: &FixtureType, mode: &str) -> Option<Symbol> {
    if let Some(svg) = t.symbol.as_deref().and_then(symbol_svg)
        && let Some(prims) = library_symbol(svg, t.size_mm)
    {
        return finish(prims, SymbolSource::Assigned);
    }
    let g = parsed(doc, t)?;
    let mode = (!mode.is_empty()).then_some(mode);
    let own = gdtf_svg_symbol(&g, mode);
    if !own.is_empty() {
        return finish(own, SymbolSource::GdtfSvg);
    }
    let fm = g.mesh(mode, stagecraft_gdtf::Lod::Low);
    let sil = stagecraft_gdtf::silhouette::silhouette(&fm);
    let pts = |v: &[[f32; 2]]| v.iter().map(|p| [p[0] as f64, p[1] as f64]).collect::<Vec<P2>>();
    let mut prims = Vec::new();
    if sil.hull.len() >= 3 {
        prims.push(LocalPrim::Polygon { points: pts(&sil.hull), fill: FillClass::Body, line: Some(LineClass::Medium) });
    }
    for (_, p) in &sil.parts {
        if p.len() >= 3 {
            prims.push(LocalPrim::Polyline { points: pts(p), closed: true, line: LineClass::Thin });
        }
    }
    finish(prims, SymbolSource::Silhouette)
}

fn finish(prims: Vec<LocalPrim>, source: SymbolSource) -> Option<Symbol> {
    let mut pts = prims.iter().flat_map(|p| match p {
        LocalPrim::Polygon { points, .. } | LocalPrim::Polyline { points, .. } => points.iter(),
    });
    let f = *pts.next()?;
    let bounds =
        pts.fold((f, f), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
    Some(Symbol { prims, bounds, source })
}

fn class(c: Option<&str>, default: LineClass) -> LineClass {
    c.and_then(crate::kinds::line_class).unwrap_or(default)
}

/// SVG shapes as local prims through `map` (SVG units → local mm).
fn shapes_to_prims(svg: &crate::svg::Svg, map: &dyn Fn(P2) -> P2, default: LineClass) -> Vec<LocalPrim> {
    let mut out = Vec::new();
    for s in &svg.shapes {
        let points: Vec<P2> = s.points.iter().map(|p| map(*p)).collect();
        let line = class(s.class.as_deref(), default);
        if s.filled {
            out.push(LocalPrim::Polygon { points, fill: FillClass::Body, line: s.stroked.then_some(line) });
        } else {
            out.push(LocalPrim::Polyline { points, closed: s.closed, line });
        }
    }
    out
}

/// A built-in symbol (front at the top of the SVG) centred on the hanging point and
/// scaled to the type: its width is the fixture's width (x), capped so its length
/// does not exceed the fixture's largest dimension.
pub fn library_symbol(svg_text: &str, size_mm: [f64; 3]) -> Option<Vec<LocalPrim>> {
    let svg = crate::svg::parse(svg_text);
    let (lo, hi) = match svg.view_box {
        Some([x, y, w, h]) => ([x, y], [x + w, y + h]),
        None => svg.bounds()?,
    };
    // Fit the drawn shapes, not the view box margins.
    let (dlo, dhi) = svg.bounds()?;
    let (w, h) = (dhi[0] - dlo[0], dhi[1] - dlo[1]);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let width = if size_mm[0] > 1.0 { size_mm[0] } else { 300.0 };
    let longest = size_mm.iter().copied().fold(0.0_f64, f64::max).max(width);
    let k = (width / w).min(longest / h);
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    Some(shapes_to_prims(&svg, &|p| [(p[0] - c[0]) * k, -(p[1] - c[1]) * k], LineClass::Medium))
}

/// A built-in symbol for previews and lists, fitted to `size_mm`.
pub fn builtin_symbol(name: &str, size_mm: [f64; 3]) -> Option<Vec<LocalPrim>> {
    library_symbol(symbol_svg(name)?, size_mm)
}

/// The GDTF's own top views, each placed with its geometry's transform. SVG units
/// are mm; `SVGOffsetX/Y` is the model origin inside the SVG.
fn gdtf_svg_symbol(g: &Gdtf, mode: Option<&str>) -> Vec<LocalPrim> {
    let mut out = Vec::new();
    for (model, world, bytes) in g.plan_svgs(mode) {
        let Ok(text) = std::str::from_utf8(bytes) else { continue };
        let svg = crate::svg::parse(text);
        let off = g.fixture.model(&model).map_or((0.0, 0.0), |m| (m.svg_offset.0 as f64, m.svg_offset.1 as f64));
        let w = world;
        let map = move |p: P2| {
            let local = DVec3::new(p[0] - off.0, -(p[1] - off.1), 0.0).as_vec3();
            let q = w.transform_point3(local);
            [q.x as f64, q.y as f64]
        };
        out.extend(shapes_to_prims(&svg, &map, LineClass::Thin));
    }
    out
}

/// Extreme points per mesh part (fixture frame, neutral pose, mm): the points
/// furthest along 66 directions, which outline the part's convex hull from any
/// side. The plan projects them through the fixture's full world transform, so a
/// tilted fixture shows its real top view.
pub fn outline_points(doc: &Document, obj: &Object) -> Option<Outline> {
    let (t, mode) = type_of(doc, obj)?;
    let key = (t.asset.clone(), mode.clone());
    if let Some(o) = cache().lock().ok()?.outlines.get(&key) {
        return o.clone();
    }
    let out = parsed(doc, t).map(|g| {
        let fm = g.mesh((!mode.is_empty()).then_some(mode.as_str()), stagecraft_gdtf::Lod::Low);
        let ts = fm.posed_transforms(&|_, _| Mat4::IDENTITY);
        let dirs = directions();
        let parts: Vec<Vec<Vec3>> = fm
            .parts
            .iter()
            .zip(&ts)
            .filter_map(|(p, t)| {
                let m = p.mesh.as_ref()?;
                let pts: Vec<Vec3> = m.positions.iter().map(|q| t.transform_point3(Vec3::from_array(*q))).collect();
                if pts.is_empty() {
                    return None;
                }
                let mut ext: Vec<Vec3> = dirs
                    .iter()
                    .map(|d| *pts.iter().max_by(|a, b| a.dot(*d).total_cmp(&b.dot(*d))).expect("non-empty"))
                    .collect();
                ext.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)).then(a.z.total_cmp(&b.z)));
                ext.dedup_by(|a, b| a.distance_squared(*b) < 1e-6);
                Some(ext)
            })
            .collect();
        Arc::new(parts)
    });
    let mut c = cache().lock().ok()?;
    if c.outlines.len() > MAX_ENTRIES {
        c.outlines.clear();
    }
    c.outlines.insert(key, out.clone());
    out
}

/// 66 unit directions: the 26 of a cube's faces, edges and corners plus 40 on a
/// Fibonacci sphere.
fn directions() -> Vec<Vec3> {
    let mut v = Vec::new();
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                if (x, y, z) != (0, 0, 0) {
                    v.push(Vec3::new(x as f32, y as f32, z as f32).normalize());
                }
            }
        }
    }
    let n = 40;
    let golden = std::f32::consts::PI * (3.0 - 5f32.sqrt());
    for i in 0..n {
        let y = 1.0 - 2.0 * (i as f32 + 0.5) / n as f32;
        let r = (1.0 - y * y).sqrt();
        let a = golden * i as f32;
        v.push(Vec3::new(r * a.cos(), y, r * a.sin()));
    }
    v
}

/// The silhouette seen from above with the fixture's world transform: overall hull
/// (filled body) and one hull per part (thin), world plan mm.
pub fn world_silhouette(parts: &[Vec<Vec3>], w: &glam::DAffine3) -> Vec<crate::PlanPrim> {
    use glam::Vec2;
    let proj = |p: &Vec3| {
        let q = w.transform_point3(p.as_dvec3());
        Vec2::new(q.x as f32, q.y as f32)
    };
    let to_p2 = |h: Vec<Vec2>| h.into_iter().map(|p| [p.x as f64, p.y as f64]).collect::<Vec<P2>>();
    let all: Vec<Vec2> = parts.iter().flatten().map(proj).collect();
    let mut out = Vec::new();
    let hull = stagecraft_gdtf::silhouette::convex_hull(&all);
    if hull.len() >= 3 {
        out.push(crate::PlanPrim::Polygon {
            points: to_p2(hull),
            fill: FillClass::Body,
            line: Some(LineClass::Medium),
        });
    }
    for p in parts {
        let h = stagecraft_gdtf::silhouette::convex_hull(&p.iter().map(proj).collect::<Vec<_>>());
        if h.len() >= 3 {
            out.push(crate::PlanPrim::Polyline { points: to_p2(h), closed: true, line: LineClass::Thin });
        }
    }
    out
}

/// Bounds (min, max) in local mm of what the plan draws for a fixture: its symbol,
/// or the generic box. Labels keep clear of it.
pub fn plan_bounds(doc: &Document, obj: &Object) -> (P2, P2) {
    match symbol(doc, obj) {
        Some(s) => s.bounds,
        None => {
            let (hw, hd) = (crate::kinds::fixture::BODY_WIDTH / 2.0, crate::kinds::fixture::BODY_DEPTH / 2.0);
            ([-hw, -hd], [hw, hd])
        }
    }
}
