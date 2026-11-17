//! Vector views for StageCraft sheets: feature edges, hidden-line removal and section
//! cuts for orthographic projections of triangle meshes.
//!
//! Feed a [`Scene`] (meshes in world mm, each with an object id and a material class)
//! and a [`View`] (direction, up, optional section plane) to [`draw`]. The result is a
//! [`Drawing`]: 2D polylines in view mm with a [`LineClass`] each, plus filled cut
//! [`Region`]s. [`svg::to_svg`] writes it out for debugging.
//!
//! World axes follow MVR: x right seen from the audience, y upstage, z up. View
//! coordinates: x to the right on the sheet, y up on the sheet, both in mm at 1:1.
//! See the README for the full conventions.

mod chain;
mod features;
mod hlr;
mod intersect;
mod overlap;
mod prep;
pub mod primitives;
mod section;
pub mod svg;

pub use glam::{DVec2, DVec3};

/// Identifies the scene object a mesh or line belongs to; copied onto the output so
/// the caller can map line weights per object or tag.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(pub u64);

/// Material (hatch) class of a mesh; copied onto the cut regions it produces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Material(pub u32);

/// A triangle mesh in world mm. Winding and vertex sharing do not matter: vertices are
/// welded and triangles oriented consistently before edges are extracted.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub object: ObjectId,
    pub material: Material,
    pub positions: Vec<DVec3>,
    pub triangles: Vec<[u32; 3]>,
}

/// An explicit 3D polyline in world mm, e.g. a truss axis or a fixture centre line.
#[derive(Clone, Debug)]
pub struct Line3 {
    pub object: ObjectId,
    pub points: Vec<DVec3>,
    /// Usually [`LineClass::Centre`] or [`LineClass::Visible`].
    pub class: LineClass,
    /// Run the line through hidden-line removal. Hidden parts of a `Visible` line come
    /// out as `Hidden`; hidden parts of any other class are dropped. `false` draws the
    /// whole line on top.
    pub occluded: bool,
}

/// Everything to draw.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub meshes: Vec<Mesh>,
    pub lines: Vec<Line3>,
}

/// A section plane in world mm. Geometry on the side the normal points to
/// (`signed_distance > 0`) is removed; usually the normal points towards the viewer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionPlane {
    pub origin: DVec3,
    pub normal: DVec3,
}

impl SectionPlane {
    /// A plane through `origin`; `normal` points to the removed side (normalised here,
    /// zero becomes +z).
    pub fn new(origin: DVec3, normal: DVec3) -> Self {
        Self { origin, normal: normal.try_normalize().unwrap_or(DVec3::Z) }
    }

    pub fn signed_distance(&self, p: DVec3) -> f64 {
        (p - self.origin).dot(self.normal)
    }
}

/// An orthographic view. `dir` points from the eye into the scene; `up` is projected
/// onto the sheet's +y. An optional section plane cuts the scene first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub dir: DVec3,
    pub up: DVec3,
    pub section: Option<SectionPlane>,
}

impl View {
    /// A view looking along `dir` with `up` towards the top of the sheet. Falls back to
    /// another up vector when `up` is parallel to `dir`.
    pub fn new(dir: DVec3, up: DVec3) -> Self {
        Self { dir: dir.try_normalize().unwrap_or(DVec3::Y), up, section: None }
    }

    /// Front elevation: from the audience (−y) looking upstage, z up.
    pub fn front() -> Self {
        Self::new(DVec3::Y, DVec3::Z)
    }

    /// Back elevation: from upstage looking at the audience.
    pub fn back() -> Self {
        Self::new(-DVec3::Y, DVec3::Z)
    }

    /// Side elevation from stage right (+x), upstage to the right.
    pub fn side_right() -> Self {
        Self::new(-DVec3::X, DVec3::Z)
    }

    /// Side elevation from stage left (−x), upstage to the left.
    pub fn side_left() -> Self {
        Self::new(DVec3::X, DVec3::Z)
    }

    /// Plan (top) view: looking down, upstage (+y) to the top of the sheet. The view
    /// coordinates then equal world x/y.
    pub fn plan() -> Self {
        Self::new(-DVec3::Z, DVec3::Y)
    }

    /// Isometric view from front right above, like the 3D view's iso preset.
    pub fn iso() -> Self {
        Self::new(DVec3::new(-1.0, 1.0, -1.0), DVec3::Z)
    }

    /// This view with a section plane.
    pub fn with_section(mut self, plane: SectionPlane) -> Self {
        self.section = Some(plane);
        self
    }

    /// Front section: removes everything downstage of `y` (towards the viewer).
    pub fn section_front(y: f64) -> Self {
        Self::front().with_section(SectionPlane::new(DVec3::new(0.0, y, 0.0), -DVec3::Y))
    }

    /// Side section seen from stage right: removes everything at x > `x`.
    pub fn section_side(x: f64) -> Self {
        Self::side_right().with_section(SectionPlane::new(DVec3::new(x, 0.0, 0.0), DVec3::X))
    }

    /// Plan cut at height `z`: removes everything above, like an architectural plan.
    pub fn plan_cut(z: f64) -> Self {
        Self::plan().with_section(SectionPlane::new(DVec3::new(0.0, 0.0, z), DVec3::Z))
    }

    /// `(right, up, towards_eye)`: a right-handed orthonormal basis. View coordinates of
    /// a world point are its dot products with these.
    pub fn basis(&self) -> (DVec3, DVec3, DVec3) {
        let d = self.dir.try_normalize().unwrap_or(DVec3::Y);
        let mut up = self.up.try_normalize().unwrap_or(DVec3::Z);
        if d.cross(up).length_squared() < 1e-12 {
            up = if d.z.abs() < 0.9 { DVec3::Z } else { DVec3::Y };
        }
        let right = d.cross(up).normalize();
        let up = right.cross(d).normalize();
        (right, up, -d)
    }

    /// World point to view space: x right, y up, z towards the eye (larger = closer).
    pub fn to_view(&self, p: DVec3) -> DVec3 {
        let (r, u, w) = self.basis();
        DVec3::new(p.dot(r), p.dot(u), p.dot(w))
    }

    /// World direction to view space.
    pub(crate) fn dir_to_view(&self, v: DVec3) -> DVec3 {
        self.to_view(v)
    }
}

/// Tuning knobs. The defaults suit stage geometry in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// Edges whose faces meet at more than this angle (degrees) are drawn as creases.
    pub crease_angle_deg: f64,
    /// Vertices closer than this (mm) are welded; also the tolerance for joining output
    /// segments into polylines.
    pub weld_tolerance: f64,
    /// Emit hidden (dashed) lines. `false` drops them and skips edges that are always
    /// hidden, which is faster.
    pub hidden_lines: bool,
    /// Visible or hidden pieces shorter than this (mm) are dropped.
    pub min_length: f64,
    /// Draw the lines where separate parts penetrate each other (different meshes, or
    /// separate connected components of one mesh).
    pub intersections: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { crease_angle_deg: 50.0, weld_tolerance: 0.01, hidden_lines: true, min_length: 0.05, intersections: true }
    }
}

/// What a line represents; the caller maps classes to weights and dash patterns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LineClass {
    /// Outline of a cut face (section plane), usually the thickest line.
    Cut,
    /// A visible silhouette, crease or boundary edge.
    Visible,
    /// The occluded part of an edge, usually dashed.
    Hidden,
    /// A centre line from [`Line3`], usually dash-dot.
    Centre,
}

/// A 2D polyline in view mm.
#[derive(Clone, Debug, PartialEq)]
pub struct Polyline {
    pub object: ObjectId,
    pub class: LineClass,
    pub points: Vec<DVec2>,
    /// The last point connects back to the first (not repeated in `points`).
    pub closed: bool,
}

/// A filled cut face in view mm: outer loop counter-clockwise, holes clockwise, no
/// repeated first point.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub object: ObjectId,
    pub material: Material,
    pub outer: Vec<DVec2>,
    pub holes: Vec<Vec<DVec2>>,
}

impl Region {
    /// Area in mm², holes taken off.
    pub fn area(&self) -> f64 {
        area(&self.outer).abs() - self.holes.iter().map(|h| area(h).abs()).sum::<f64>()
    }
}

/// The result of [`draw`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawing {
    pub polylines: Vec<Polyline>,
    pub regions: Vec<Region>,
}

impl Drawing {
    /// `(min, max)` over all points, `None` when empty.
    pub fn bounds(&self) -> Option<(DVec2, DVec2)> {
        let pts =
            self.polylines.iter().flat_map(|p| p.points.iter()).chain(self.regions.iter().flat_map(|r| r.outer.iter()));
        let mut it = pts.peekable();
        it.peek()?;
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for p in it {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        Some((lo, hi))
    }

    /// Polylines of one class.
    pub fn lines(&self, class: LineClass) -> impl Iterator<Item = &Polyline> {
        self.polylines.iter().filter(move |p| p.class == class)
    }

    /// Total length (mm) of the polylines of one class.
    pub fn length(&self, class: LineClass) -> f64 {
        self.lines(class).map(polyline_length).sum()
    }
}

pub(crate) fn polyline_length(p: &Polyline) -> f64 {
    let mut l: f64 = p.points.windows(2).map(|w| w[0].distance(w[1])).sum();
    if p.closed && p.points.len() > 2 {
        l += p.points[p.points.len() - 1].distance(p.points[0]);
    }
    l
}

/// Signed area of a closed 2D loop (positive = counter-clockwise).
pub(crate) fn area(pts: &[DVec2]) -> f64 {
    let n = pts.len();
    (0..n).map(|i| pts[i].perp_dot(pts[(i + 1) % n])).sum::<f64>() / 2.0
}

/// One edge to classify, in view space.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Seg {
    pub a: DVec3,
    pub b: DVec3,
    pub object: ObjectId,
    /// Class of the visible part; see [`Line3::occluded`] for the hidden part.
    pub class: LineClass,
    pub occluded: bool,
    /// An intersection line: dropped where it lies on any visible or cut line.
    pub weak: bool,
}

/// Draw `scene` as seen in `view`.
pub fn draw(scene: &Scene, view: &View, opts: &Options) -> Drawing {
    let plane = view
        .section
        .map(|s| section::ViewPlane { origin: view.to_view(s.origin), normal: view.dir_to_view(s.normal).normalize() });

    // Per mesh: weld, orient, extract edges, cut.
    let per_mesh = |m: &Mesh| {
        let pts: Vec<DVec3> = m.positions.iter().map(|p| view.to_view(*p)).collect();
        let prepared = prep::prepare(&pts, &m.triangles, opts.weld_tolerance);
        let mut segs = features::feature_edges(&prepared, m.object, opts);
        let mut occluders = Vec::new();
        let mut regions = Vec::new();
        match &plane {
            None => occluders.extend(prepared.occluders()),
            Some(pl) => {
                segs = segs.into_iter().filter_map(|s| pl.clip_seg(s)).collect();
                occluders.extend(prepared.occluders().flat_map(|t| pl.clip_tri(t)));
                let cut = section::cut(&prepared, pl, opts.weld_tolerance);
                for l in &cut.lines {
                    for w in l.windows(2) {
                        segs.push(Seg {
                            a: w[0],
                            b: w[1],
                            object: m.object,
                            class: LineClass::Cut,
                            occluded: true,
                            weak: false,
                        });
                    }
                }
                for r in cut.regions {
                    occluders.extend(r.caps.iter().copied());
                    regions.push(Region { object: m.object, material: m.material, outer: r.outer, holes: r.holes });
                }
            }
        }
        (segs, occluders, regions, prepared)
    };

    #[cfg(feature = "parallel")]
    let results: Vec<_> = {
        use rayon::prelude::*;
        scene.meshes.par_iter().map(per_mesh).collect()
    };
    #[cfg(not(feature = "parallel"))]
    let results: Vec<_> = scene.meshes.iter().map(per_mesh).collect();

    let mut segs = Vec::new();
    let mut occluders = Vec::new();
    let mut regions = Vec::new();
    if opts.intersections {
        let prepared: Vec<_> = results.iter().zip(&scene.meshes).map(|(r, m)| (&r.3, m.object)).collect();
        let (r, u, w) = view.basis();
        let frame = |p: DVec3| r * p.x + u * p.y + w * p.z;
        for s in intersect::intersection_lines(&prepared, frame, opts.weld_tolerance) {
            match &plane {
                Some(pl) => segs.extend(pl.clip_seg(s)),
                None => segs.push(s),
            }
        }
    }
    for (s, o, r, _) in results {
        segs.extend(s);
        occluders.extend(o);
        regions.extend(r);
    }
    for l in &scene.lines {
        let pts: Vec<DVec3> = l.points.iter().map(|p| view.to_view(*p)).collect();
        for w in pts.windows(2) {
            let s = Seg { a: w[0], b: w[1], object: l.object, class: l.class, occluded: l.occluded, weak: false };
            match &plane {
                Some(pl) => segs.extend(pl.clip_seg(s)),
                None => segs.push(s),
            }
        }
    }

    let pieces = hlr::classify(&segs, &occluders, opts);
    let pieces = overlap::remove_overlaps(pieces, opts.weld_tolerance.max(0.01), opts.min_length);
    let polylines = chain::join(pieces, opts.weld_tolerance);
    regions.sort_by_key(|r| r.object);
    Drawing { polylines, regions }
}
