//! What the 3D view draws, kept between frames: one world-space mesh per visible
//! object, rebuilt only when that object's parameters, its world transform (parents,
//! layer) or its detail level change. The flattened vertex list for the GPU is rebuilt
//! when anything visible changes, and the cut faces when the section or the geometry
//! changes.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use glam::{DAffine3, DVec3, Vec3};
use stagecraft_core::{Document, Id, ObjectKind};
use stagecraft_geom::section::{self, Plane};
use stagecraft_geom::{Lod, Mesh};

use super::camera::{Camera3d, Ray};

/// One vertex as the shader reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    /// Index into the colour table (`MaterialClass::index`, or [`CAP_MATERIAL`]), or
    /// [`TINT_FLAG`] with an sRGB colour.
    pub material: u32,
    /// Bit 0: selected.
    pub flags: u32,
}

/// Colour slot of cut faces.
pub const CAP_MATERIAL: u32 = 15;
/// Set on a vertex material when the low 24 bits are the object's own sRGB colour.
pub const TINT_FLAG: u32 = 0x8000_0000;
pub const FLAG_SELECTED: u32 = 1;

/// GPU-ready triangles with a version, so the renderer uploads only on change.
#[derive(Clone, Debug, Default)]
pub struct Buffers {
    pub version: u64,
    pub vertices: Arc<Vec<Vertex>>,
    pub indices: Arc<Vec<u32>>,
}

struct Entry {
    kind: ObjectKind,
    lod: Lod,
    world: DAffine3,
    mesh: Mesh,
    bounds: Option<(DVec3, DVec3)>,
}

/// Truss bigger than this many screen points across gets diagonals, above
/// [`SIMPLE_POINTS`] chords, below that a box.
pub const FULL_POINTS: f64 = 9.0;
pub const SIMPLE_POINTS: f64 = 2.5;
/// Above this many truss pieces in view, never draw diagonals; above the second, boxes.
pub const MANY_TRUSS: usize = 400;
pub const VERY_MANY_TRUSS: usize = 2000;

#[derive(Default)]
pub struct Scene {
    entries: HashMap<Id, Entry>,
    /// Drawn objects, in a stable order.
    visible: Vec<Id>,
    synced: Option<(Document, Vec<Id>, Vec<Lod>)>,
    selection: Vec<Id>,
    geometry_version: u64,
    pub buffers: Buffers,
    pub caps: Buffers,
    caps_key: Option<(u64, Vec<Id>, Vec<[f64; 6]>)>,
    /// How many object meshes were generated (tests check the cache with it).
    pub mesh_builds: u64,
}

/// The detail level for a truss of `width` mm at `centre`.
pub fn truss_lod(cam: &Camera3d, centre: DVec3, width: f64, truss_count: usize) -> Lod {
    let points = width * cam.points_per_mm_at(centre);
    let by_size = if points >= FULL_POINTS {
        Lod::Full
    } else if points >= SIMPLE_POINTS {
        Lod::Simple
    } else {
        Lod::Box
    };
    let cap = if truss_count > VERY_MANY_TRUSS {
        Lod::Box
    } else if truss_count > MANY_TRUSS {
        Lod::Simple
    } else {
        Lod::Full
    };
    // Lod orders Full < Simple < Box: the coarser wins.
    let rank = |l: Lod| l as u8;
    if rank(by_size) >= rank(cap) { by_size } else { cap }
}

impl Scene {
    /// Bring the cache up to date. `visible` are the objects to draw (layer and tag
    /// visibility already applied). Cheap when nothing changed.
    pub fn sync(&mut self, doc: &Document, visible: &[Id], selection: &[Id], cam: &Camera3d) {
        let lods = self.lods(doc, visible, cam);
        let same = self.synced.as_ref().is_some_and(|(d, v, l)| {
            d.objects.ptr_eq(&doc.objects) && d.layers.ptr_eq(&doc.layers) && v == visible && *l == lods
        });
        let mut changed = false;
        if !same {
            changed = self.update(doc, visible, &lods);
            self.synced = Some((doc.clone(), visible.to_vec(), lods));
        }
        if changed {
            self.geometry_version += 1;
        }
        if changed || self.selection != selection {
            self.selection = selection.to_vec();
            self.flatten();
        }
    }

    fn lods(&self, doc: &Document, visible: &[Id], cam: &Camera3d) -> Vec<Lod> {
        let truss_count = visible.iter().filter(|id| matches!(doc.objects[*id].kind, ObjectKind::Truss { .. })).count();
        visible
            .iter()
            .map(|id| {
                let o = &doc.objects[id];
                let Some(size) = stagecraft_geom::kinds::detail_size(o) else { return Lod::Full };
                let is_truss = matches!(o.kind, ObjectKind::Truss { .. } | ObjectKind::TrussNode { .. });
                let width = size.max(1.0);
                let centre = match self.entries.get(id).and_then(|e| e.bounds) {
                    Some((lo, hi)) => (lo + hi) / 2.0,
                    None => stagecraft_core::geom::world(doc, *id).map_or(DVec3::ZERO, |w| w.translation),
                };
                // The many-truss cap applies to truss only.
                truss_lod(cam, centre, width, if is_truss { truss_count } else { 0 })
            })
            .collect()
    }

    /// Returns true if any drawn geometry changed.
    fn update(&mut self, doc: &Document, visible: &[Id], lods: &[Lod]) -> bool {
        let mut changed = self.visible != visible;
        for (id, lod) in visible.iter().zip(lods) {
            let obj = &doc.objects[id];
            let world = stagecraft_core::geom::world(doc, *id).unwrap_or(DAffine3::IDENTITY);
            if let Some(e) = self.entries.get(id)
                && e.kind == obj.kind
                && e.lod == *lod
                && e.world == world
            {
                continue;
            }
            let mesh = stagecraft_geom::mesh(doc, obj, *lod).transformed(&world);
            self.mesh_builds += 1;
            let bounds =
                mesh.bounds().map(|(lo, hi)| (Vec3::from_array(lo).as_dvec3(), Vec3::from_array(hi).as_dvec3()));
            self.entries.insert(*id, Entry { kind: obj.kind.clone(), lod: *lod, world, mesh, bounds });
            changed = true;
        }
        // Forget objects that are gone from the document; hidden ones stay cached.
        self.entries.retain(|id, _| doc.objects.contains_key(id));
        self.visible = visible.to_vec();
        changed
    }

    fn flatten(&mut self) {
        let sel: BTreeSet<Id> = self.selection.iter().copied().collect();
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for id in &self.visible {
            let Some(e) = self.entries.get(id) else { continue };
            let base = vertices.len() as u32;
            let flags = if sel.contains(id) { FLAG_SELECTED } else { 0 };
            let m = &e.mesh;
            let mut material = vec![0u32; m.positions.len()];
            for g in &m.groups {
                for &i in &m.indices[g.start as usize..(g.start + g.count) as usize] {
                    material[i as usize] = match m.tint {
                        Some([r, gr, b]) if g.material.tintable() => {
                            TINT_FLAG | u32::from(r) << 16 | u32::from(gr) << 8 | u32::from(b)
                        }
                        _ => g.material.index(),
                    };
                }
            }
            vertices.extend(m.positions.iter().zip(&m.normals).zip(material).map(|((p, n), material)| Vertex {
                pos: *p,
                normal: *n,
                material,
                flags,
            }));
            indices.extend(m.indices.iter().map(|i| i + base));
        }
        self.buffers =
            Buffers { version: self.buffers.version + 1, vertices: Arc::new(vertices), indices: Arc::new(indices) };
    }

    /// Cut faces for the section planes, recomputed only when planes or geometry change.
    pub fn update_caps(&mut self, planes: &[Plane]) {
        let key: Vec<[f64; 6]> = planes
            .iter()
            .map(|p| [p.origin[0], p.origin[1], p.origin[2], p.normal[0], p.normal[1], p.normal[2]])
            .collect();
        if self
            .caps_key
            .as_ref()
            .is_some_and(|(v, sel, k)| *v == self.geometry_version && *sel == self.selection && *k == key)
        {
            return;
        }
        self.caps_key = Some((self.geometry_version, self.selection.clone(), key));
        let sel: BTreeSet<Id> = self.selection.iter().copied().collect();
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for plane in planes {
            let n = plane.n().as_vec3().to_array();
            for id in &self.visible {
                let Some(e) = self.entries.get(id) else { continue };
                let Some((lo, hi)) = e.bounds else { continue };
                if !box_straddles(plane, lo, hi) {
                    continue;
                }
                let flags = if sel.contains(id) { FLAG_SELECTED } else { 0 };
                for poly in section::cut(&e.mesh, &DAffine3::IDENTITY, plane) {
                    let (pts, tris) = section::triangulate(&poly);
                    let base = vertices.len() as u32;
                    vertices.extend(pts.iter().map(|q| Vertex {
                        pos: plane.to_world(*q).as_vec3().to_array(),
                        normal: n,
                        material: CAP_MATERIAL,
                        flags,
                    }));
                    indices.extend(tris.iter().flat_map(|t| t.map(|i| base + i as u32)));
                }
            }
        }
        self.caps =
            Buffers { version: self.caps.version + 1, vertices: Arc::new(vertices), indices: Arc::new(indices) };
    }

    /// Bounds of everything drawn, or `None`.
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        self.bounds_of(&self.visible)
    }

    pub fn bounds_of(&self, ids: &[Id]) -> Option<(DVec3, DVec3)> {
        ids.iter()
            .filter_map(|id| self.entries.get(id).and_then(|e| e.bounds))
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
    }

    /// Radius of a sphere around the origin-centred bounds, for depth ranges.
    pub fn radius(&self) -> f64 {
        self.bounds().map_or(20_000.0, |(lo, hi)| lo.abs().max(hi.abs()).length())
    }

    /// The nearest object hit by `ray`, ignoring geometry the section planes hide.
    pub fn pick(&self, ray: &Ray, planes: &[Plane]) -> Option<Id> {
        let mut best: Option<(f64, Id)> = None;
        for id in &self.visible {
            let Some(e) = self.entries.get(id) else { continue };
            let Some((lo, hi)) = e.bounds else { continue };
            let Some(t_box) = ray_box(ray, lo, hi) else { continue };
            if best.is_some_and(|(t, _)| t_box > t) {
                continue;
            }
            let m = &e.mesh;
            for tri in m.indices.chunks_exact(3) {
                let p = |i: u32| Vec3::from_array(m.positions[i as usize]).as_dvec3();
                if let Some(t) = ray_triangle(ray, p(tri[0]), p(tri[1]), p(tri[2]))
                    && best.is_none_or(|(b, _)| t < b)
                {
                    let hit = ray.origin + ray.dir * t;
                    if planes.iter().all(|pl| pl.signed_distance(hit) <= 0.5) {
                        best = Some((t, *id));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    pub fn visible(&self) -> &[Id] {
        &self.visible
    }
}

fn box_straddles(plane: &Plane, lo: DVec3, hi: DVec3) -> bool {
    let (mut neg, mut pos) = (false, false);
    for i in 0..8 {
        let c = DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        );
        if plane.signed_distance(c) > 0.0 {
            pos = true;
        } else {
            neg = true;
        }
    }
    neg && pos
}

/// Entry distance of a ray into a box (0 if it starts inside), or `None`.
fn ray_box(ray: &Ray, lo: DVec3, hi: DVec3) -> Option<f64> {
    let (mut t0, mut t1) = (0.0f64, f64::INFINITY);
    for a in 0..3 {
        let (o, d) = (ray.origin[a], ray.dir[a]);
        if d.abs() < 1e-12 {
            if o < lo[a] - 1.0 || o > hi[a] + 1.0 {
                return None;
            }
            continue;
        }
        let (mut ta, mut tb) = ((lo[a] - 1.0 - o) / d, (hi[a] + 1.0 - o) / d);
        if ta > tb {
            std::mem::swap(&mut ta, &mut tb);
        }
        t0 = t0.max(ta);
        t1 = t1.min(tb);
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

/// Möller–Trumbore, both sides.
fn ray_triangle(ray: &Ray, a: DVec3, b: DVec3, c: DVec3) -> Option<f64> {
    let e1 = b - a;
    let e2 = c - a;
    let p = ray.dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = ray.origin - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = ray.dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use stagecraft_core::Engine;

    fn ids(e: &Engine) -> Vec<Id> {
        e.doc().objects.keys().copied().collect()
    }

    #[test]
    fn meshes_are_rebuilt_only_for_what_changed() {
        let mut e = Engine::new();
        let t = e.execute("truss.create", json!({ "start": [0, 0, 6000], "end": [4000, 0, 6000] })).unwrap();
        let t: Id = t["id"].as_str().unwrap().parse().unwrap();
        e.execute("fixture.create", json!({ "parent": t, "position": [1000, 0, -150] })).unwrap();
        e.execute("floor.create", json!({ "outline": [[0, 0], [4000, 0], [4000, 3000]] })).unwrap();
        let cam = Camera3d::default();
        let mut s = Scene::default();
        s.sync(e.doc(), &ids(&e), &[], &cam);
        assert_eq!(s.mesh_builds, 3);
        let v = s.buffers.version;
        for _ in 0..5 {
            s.sync(e.doc(), &ids(&e), &[], &cam);
        }
        assert_eq!((s.mesh_builds, s.buffers.version), (3, v), "nothing changed, nothing rebuilt");
        // Selection changes the buffer flags but builds no mesh.
        s.sync(e.doc(), &ids(&e), &[t], &cam);
        assert_eq!(s.mesh_builds, 3);
        assert!(s.buffers.version > v);
        // Moving the truss carries the fixture: both re-transformed, the floor not.
        e.execute("object.move", json!({ "ids": [t], "delta": [0, 500, 0] })).unwrap();
        s.sync(e.doc(), &ids(&e), &[t], &cam);
        assert_eq!(s.mesh_builds, 5);
        let (lo, _) = s.bounds_of(&[t]).unwrap();
        assert!((lo.y - (500.0 - 145.0)).abs() < 1.0, "{lo}");
    }

    #[test]
    fn picking_hits_the_nearest_object_and_respects_sections() {
        let mut e = Engine::new();
        let low =
            e.execute("floor.create", json!({ "outline": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]] })).unwrap();
        let high = e
            .execute("floor.create", json!({ "outline": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]], "z": 2000 }))
            .unwrap();
        let id = |v: &serde_json::Value| -> Id { v["id"].as_str().unwrap().parse().unwrap() };
        let mut s = Scene::default();
        s.sync(e.doc(), &ids(&e), &[], &Camera3d::default());
        let down = Ray { origin: DVec3::new(500.0, 500.0, 10_000.0), dir: -DVec3::Z };
        assert_eq!(s.pick(&down, &[]), Some(id(&high)));
        let cut = Plane::new([0.0, 0.0, 1000.0], [0.0, 0.0, 1.0]);
        assert_eq!(s.pick(&down, &[cut]), Some(id(&low)));
        let miss = Ray { origin: DVec3::new(5000.0, 500.0, 10_000.0), dir: -DVec3::Z };
        assert_eq!(s.pick(&miss, &[]), None);
        // The cut through the high floor's slab... is none: it lies above.
        s.update_caps(&[cut]);
        assert!(s.caps.indices.is_empty());
        let x = [Plane::new([500.0, 0.0, 0.0], [1.0, 0.0, 0.0])];
        s.update_caps(&x);
        assert_eq!(s.caps.indices.len(), 2 * 2 * 3, "two slabs cut, two triangles each");
        let v = s.caps.version;
        s.update_caps(&x);
        assert_eq!(s.caps.version, v, "cached");
        // Selecting the upper floor highlights its cut face too.
        s.sync(e.doc(), &ids(&e), &[id(&high)], &Camera3d::default());
        s.update_caps(&x);
        assert_eq!(s.caps.vertices.iter().filter(|v| v.flags & FLAG_SELECTED != 0).count(), 4);
    }

    #[test]
    fn far_truss_loses_detail() {
        let mut cam = Camera3d { distance: 5000.0, ..Default::default() };
        let near = truss_lod(&cam, cam.target, 290.0, 10);
        cam.distance = 400_000.0;
        let far = truss_lod(&cam, cam.target, 290.0, 10);
        assert_eq!((near, far), (Lod::Full, Lod::Box));
        cam.distance = 5000.0;
        assert_eq!(truss_lod(&cam, cam.target, 290.0, MANY_TRUSS + 1), Lod::Simple);
    }
}
