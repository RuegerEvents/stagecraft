//! Vector views from 3D (elevations, sections) behind a hook, so the hidden-line
//! library (`stagecraft-hlr`, built in parallel) can replace the fallback here.
//!
//! [`VectorViewSource`] turns objects and a [`ViewSpec`] into 2D lines and filled cut
//! regions in view mm (x right on the sheet, y up, 1:1). The conventions match
//! `stagecraft-hlr`'s `View`: `right = dir × up`, a section plane hides the side its
//! normal points to and the viewer stands there looking along `dir = −normal`.
//!
//! [`FallbackSource`] has no hidden-line removal. It draws each mesh's outline edges
//! (silhouettes, boundaries and creases of faces turned towards the viewer) and, for
//! sections, the cut faces from `stagecraft_geom::section` plus what lies behind the
//! plane. Objects behind others still show through.

use std::collections::HashMap;

use glam::{DAffine3, DVec3, Vec3};
use stagecraft_core::{Document, Id};
use stagecraft_geom::section::{self, Plane};
use stagecraft_geom::{Lod, Mesh};

use crate::prim::P2;

/// An orthographic view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewSpec {
    /// Looking direction, from the viewer into the scene.
    pub dir: DVec3,
    pub up: DVec3,
    /// Cut here and hide the side the normal points to.
    pub section: Option<Plane>,
    /// Scale denominator, for picking the level of detail.
    pub scale: f64,
}

impl ViewSpec {
    /// `(right, up)` on the sheet.
    pub fn basis(&self) -> (DVec3, DVec3) {
        let d = self.dir.try_normalize().unwrap_or(DVec3::Y);
        let mut up = self.up.try_normalize().unwrap_or(DVec3::Z);
        if d.cross(up).length_squared() < 1e-12 {
            up = if d.z.abs() < 0.9 { DVec3::Z } else { DVec3::Y };
        }
        let right = d.cross(up).normalize();
        (right, right.cross(d).normalize())
    }

    /// World mm to view mm.
    pub fn project(&self, p: DVec3) -> P2 {
        let (r, u) = self.basis();
        [p.dot(r), p.dot(u)]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeClass {
    Visible,
    /// Behind something (dashed). The fallback never produces these.
    Hidden,
    /// Where a section plane cuts (thick).
    Cut,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ViewLine {
    pub object: Id,
    pub points: Vec<P2>,
    pub class: EdgeClass,
}

/// A filled cut face: outer ring then holes, view mm.
#[derive(Clone, Debug, PartialEq)]
pub struct CutRegion {
    pub object: Id,
    pub rings: Vec<Vec<P2>>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorView {
    pub lines: Vec<ViewLine>,
    pub cuts: Vec<CutRegion>,
}

impl VectorView {
    /// `[min, max]` of everything, view mm; `None` when empty.
    pub fn bounds(&self) -> Option<[P2; 2]> {
        let mut b = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
        let pts = self
            .lines
            .iter()
            .flat_map(|l| l.points.iter())
            .chain(self.cuts.iter().flat_map(|c| c.rings.iter().flatten()));
        for p in pts {
            b[0] = [b[0][0].min(p[0]), b[0][1].min(p[1])];
            b[1] = [b[1][0].max(p[0]), b[1][1].max(p[1])];
        }
        (b[0][0] <= b[1][0]).then_some(b)
    }
}

/// Where elevation and section linework comes from.
pub trait VectorViewSource: Send + Sync {
    fn view(&self, doc: &Document, ids: &[Id], spec: &ViewSpec) -> VectorView;
}

/// Outline edges without hidden-line removal; see the module docs.
pub struct FallbackSource;

/// Faces meeting at more than this are drawn as an edge.
const CREASE_DEG: f64 = 25.0;

fn lod_for(doc: &Document, id: Id, scale: f64) -> Lod {
    let Some(o) = doc.objects.get(&id) else { return Lod::Box };
    match stagecraft_geom::kinds::detail_size(o) {
        Some(d) if d / scale < 0.4 => Lod::Box,
        Some(d) if d / scale < 1.2 => Lod::Simple,
        _ => Lod::Full,
    }
}

impl VectorViewSource for FallbackSource {
    fn view(&self, doc: &Document, ids: &[Id], spec: &ViewSpec) -> VectorView {
        let mut out = VectorView::default();
        let toward_viewer = -spec.dir.normalize_or(DVec3::NEG_Y);
        for &id in ids {
            let Some(o) = doc.objects.get(&id) else { continue };
            if o.kind.is_group() {
                continue;
            }
            let Some(w) = stagecraft_core::geom::world(doc, id) else { continue };
            let mesh = stagecraft_geom::mesh(doc, o, lod_for(doc, id, spec.scale));
            if mesh.is_empty() {
                continue;
            }
            let segs = outline_edges(&mesh, &w, toward_viewer, spec.section.as_ref());
            if !segs.is_empty() {
                for s in segs {
                    out.lines.push(ViewLine {
                        object: id,
                        points: vec![spec.project(s[0]), spec.project(s[1])],
                        class: EdgeClass::Visible,
                    });
                }
            }
            if let Some(plane) = &spec.section {
                for poly in section::cut(&mesh, &w, plane) {
                    let to_view = |q: &[f64; 2]| spec.project(plane.to_world(*q));
                    let mut rings = vec![poly.outer.iter().map(to_view).collect::<Vec<_>>()];
                    rings.extend(poly.holes.iter().map(|h| h.iter().map(to_view).collect()));
                    for r in &rings {
                        let mut pts = r.clone();
                        pts.push(r[0]);
                        out.lines.push(ViewLine { object: id, points: pts, class: EdgeClass::Cut });
                    }
                    out.cuts.push(CutRegion { object: id, rings });
                }
            }
        }
        out
    }
}

fn quant(p: DVec3) -> (i64, i64, i64) {
    ((p.x * 100.0).round() as i64, (p.y * 100.0).round() as i64, (p.z * 100.0).round() as i64)
}

/// World-space edge segments worth drawing: boundaries and creases of faces turned to
/// the viewer, and silhouettes between front and back faces, clipped to the kept side
/// of a section plane.
fn outline_edges(mesh: &Mesh, w: &DAffine3, toward: DVec3, plane: Option<&Plane>) -> Vec<[DVec3; 2]> {
    let pos: Vec<DVec3> = mesh.positions.iter().map(|p| w.transform_point3(Vec3::from_array(*p).as_dvec3())).collect();
    type Key = (i64, i64, i64);
    let mut edges: HashMap<(Key, Key), (DVec3, DVec3, Vec<DVec3>)> = HashMap::new();
    for t in mesh.indices.chunks_exact(3) {
        let [a, b, c] = [pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]];
        let n = (b - a).cross(c - a);
        if n.length_squared() < 1e-12 {
            continue;
        }
        let n = n.normalize();
        if let Some(pl) = plane
            && [a, b, c].iter().all(|p| pl.signed_distance(*p) > 0.0)
        {
            continue;
        }
        for (p, q) in [(a, b), (b, c), (c, a)] {
            let (kp, kq) = (quant(p), quant(q));
            let key = if kp <= kq { (kp, kq) } else { (kq, kp) };
            edges.entry(key).or_insert_with(|| (p, q, Vec::new())).2.push(n);
        }
    }
    let cos_crease = CREASE_DEG.to_radians().cos();
    let mut out = Vec::new();
    for (_, (p, q, normals)) in edges {
        let front: Vec<bool> = normals.iter().map(|n| n.dot(toward) > 1e-6).collect();
        let any_front = front.iter().any(|f| *f);
        if !any_front {
            continue;
        }
        let draw = normals.len() == 1
            || front.iter().any(|f| !*f)
            || normals.iter().any(|a| normals.iter().any(|b| a.dot(*b) < cos_crease));
        if !draw {
            continue;
        }
        let seg = match plane {
            None => Some([p, q]),
            Some(pl) => clip(pl, p, q),
        };
        if let Some(s) = seg
            && (s[1] - s[0]).length_squared() > 1e-6
        {
            out.push(s);
        }
    }
    out
}

/// The part of a segment on the kept side (signed distance ≤ 0).
fn clip(pl: &Plane, p: DVec3, q: DVec3) -> Option<[DVec3; 2]> {
    let (dp, dq) = (pl.signed_distance(p), pl.signed_distance(q));
    match (dp <= 0.0, dq <= 0.0) {
        (true, true) => Some([p, q]),
        (false, false) => None,
        _ => {
            let t = dp / (dp - dq);
            let m = p + (q - p) * t;
            Some(if dp <= 0.0 { [p, m] } else { [m, q] })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use stagecraft_core::Engine;

    fn platform() -> (Engine, Id) {
        let mut e = Engine::new();
        let v = e
            .execute(
                "platform.create",
                json!({ "position": [0, 2000, 0], "width": 2000, "depth": 1000, "height": 400 }),
            )
            .unwrap();
        let id = v["id"].as_str().unwrap().parse().unwrap();
        (e, id)
    }

    #[test]
    fn front_elevation_of_a_platform_spans_its_width_and_height() {
        let (e, id) = platform();
        let spec = ViewSpec { dir: DVec3::Y, up: DVec3::Z, section: None, scale: 50.0 };
        let v = FallbackSource.view(e.doc(), &[id], &spec);
        let b = v.bounds().expect("lines");
        assert!((b[0][0] + 1000.0).abs() < 1.0 && (b[1][0] - 1000.0).abs() < 1.0, "{b:?}");
        assert!(b[0][1].abs() < 1.0 && (b[1][1] - 400.0).abs() < 1.0, "{b:?}");
        assert!(v.cuts.is_empty());
    }

    #[test]
    fn a_section_through_a_platform_has_cut_faces() {
        let (e, id) = platform();
        let plane = Plane::new([0.0, 2000.0, 0.0], [0.0, -1.0, 0.0]);
        let spec = ViewSpec { dir: DVec3::Y, up: DVec3::Z, section: Some(plane), scale: 50.0 };
        let v = FallbackSource.view(e.doc(), &[id], &spec);
        assert!(!v.cuts.is_empty(), "the deck is cut");
        assert!(v.lines.iter().any(|l| l.class == EdgeClass::Cut));
        // Cut faces are in the same view coordinates as the lines (x across, z up).
        let b = v.bounds().unwrap();
        assert!(b[1][1] <= 401.0 && b[0][0] >= -1001.0);
    }

    #[test]
    fn views_have_the_hlr_crate_axes() {
        let s = ViewSpec { dir: DVec3::Y, up: DVec3::Z, section: None, scale: 1.0 };
        assert_eq!(s.project(DVec3::new(1.0, 5.0, 2.0)), [1.0, 2.0]);
        let back = ViewSpec { dir: -DVec3::Y, ..s };
        assert_eq!(back.project(DVec3::new(1.0, 5.0, 2.0)), [-1.0, 2.0]);
        let left = ViewSpec { dir: DVec3::X, ..s };
        assert_eq!(left.project(DVec3::new(0.0, 3.0, 2.0)), [-3.0, 2.0]);
    }
}
