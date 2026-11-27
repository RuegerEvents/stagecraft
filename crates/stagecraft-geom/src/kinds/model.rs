//! Model objects: meshes of embedded glTF/GLB, OBJ, STL, PLY, FBX, 3MF and 3DS files.
//!
//! A part's file is read once per asset, unit scale and up axis and cached; the
//! object's mesh (all parts in its local frame) is cached per part list. The plan is a
//! hidden-line top view of that mesh (`stagecraft-hlr`), cached in local x/y and moved
//! with the object while it only turns about z; tilted models draw their silhouette.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use glam::{DAffine3, DVec3, Mat4, Vec3};
use stagecraft_core::model::{ModelPart, P2};
use stagecraft_core::{Document, Object, ObjectKind};
use stagecraft_gdtf::models::{RawMesh, gltf};

use crate::gdtf::{MAX_ENTRIES, append, directions, world_silhouette};
use crate::{LineClass, MaterialClass, Mesh, PlanPrim, PlanStyle};

type Lines = Arc<Vec<Vec<P2>>>;

#[derive(Default)]
struct Cache {
    files: HashMap<(String, u64, bool), Option<Arc<stagecraft_gdtf::Mesh>>>,
    meshes: HashMap<String, Arc<Mesh>>,
    plans: HashMap<String, Lines>,
    outlines: HashMap<String, Arc<Vec<Vec<Vec3>>>>,
}

fn cache() -> &'static Mutex<Cache> {
    static C: OnceLock<Mutex<Cache>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// Reads a mesh file by extension (`stagecraft_core::model_file::EXTENSIONS`), in its own
/// units (glTF and FBX already turned to z-up).
pub fn read_file(ext: &str, bytes: &[u8]) -> Result<RawMesh, String> {
    stagecraft_core::model_file::read(ext, bytes)
}

/// The part's frame: `matrix` after scale and up turn.
pub fn part_transform(p: &ModelPart) -> Mat4 {
    let frame = glam::DMat4::from(DAffine3::from_cols_array(&p.matrix)).as_mat4();
    let mut t = frame * Mat4::from_scale(Vec3::splat(p.scale as f32));
    if p.y_up {
        t *= gltf::y_up_to_z_up();
    }
    t
}

/// One part's file as a mesh in mm, z-up, before its matrix. `None` if unreadable.
fn part_file(doc: &Document, p: &ModelPart) -> Option<Arc<stagecraft_gdtf::Mesh>> {
    let key = (p.asset.clone(), p.scale.to_bits(), p.y_up);
    if let Some(m) = cache().lock().ok()?.files.get(&key) {
        return m.clone();
    }
    let a = doc.assets.get(&p.asset)?;
    let m = read_file(&a.ext, a.data.as_slice()).ok().map(|raw| Arc::new(raw.into_mesh("model")));
    let mut c = cache().lock().ok()?;
    if c.files.len() > MAX_ENTRIES {
        c.files.clear();
    }
    c.files.insert(key, m.clone());
    m
}

fn parts_key(parts: &[ModelPart]) -> String {
    let mut s = String::new();
    for p in parts {
        s.push_str(&p.asset);
        for v in p.matrix.iter().chain([&p.scale]) {
            s.push_str(&format!(":{v}"));
        }
        s.push(if p.y_up { 'y' } else { 'z' });
        s.push(';');
    }
    s
}

fn parts_of(obj: &Object) -> &[ModelPart] {
    match &obj.kind {
        ObjectKind::Model { parts, .. } => parts,
        _ => &[],
    }
}

/// The model's mesh in its local frame (cached).
pub fn mesh_arc(doc: &Document, obj: &Object) -> Arc<Mesh> {
    let parts = parts_of(obj);
    let key = parts_key(parts);
    if let Some(m) = cache().lock().ok().and_then(|c| c.meshes.get(&key).cloned()) {
        return m;
    }
    let mut out = Mesh::empty();
    out.set_material(MaterialClass::Generic);
    for p in parts {
        if let Some(f) = part_file(doc, p) {
            append(&mut out, &f, part_transform(p));
        }
    }
    let m = Arc::new(out);
    if let Ok(mut c) = cache().lock() {
        if c.meshes.len() > MAX_ENTRIES {
            c.meshes.clear();
        }
        c.meshes.insert(key, m.clone());
    }
    m
}

pub fn mesh(doc: &Document, obj: &Object) -> Mesh {
    (*mesh_arc(doc, obj)).clone()
}

/// The local mesh's bounds `[min, max]`, mm; `None` when nothing could be read.
pub fn bounds(doc: &Document, parts: &[ModelPart]) -> Option<[[f64; 3]; 2]> {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for p in parts {
        let Some(f) = part_file(doc, p) else { continue };
        let t = part_transform(p);
        for q in &f.positions {
            let w = t.transform_point3(Vec3::from_array(*q)).as_dvec3();
            lo = lo.min(w);
            hi = hi.max(w);
        }
    }
    (lo.x <= hi.x).then(|| [lo.to_array(), hi.to_array()])
}

/// Hidden-line top view of the local mesh: polylines in local x/y.
fn local_plan(doc: &Document, obj: &Object) -> Lines {
    let key = parts_key(parts_of(obj));
    if let Some(l) = cache().lock().ok().and_then(|c| c.plans.get(&key).cloned()) {
        return l;
    }
    let m = mesh_arc(doc, obj);
    let scene = stagecraft_hlr::Scene {
        meshes: vec![stagecraft_hlr::Mesh {
            object: stagecraft_hlr::ObjectId(0),
            material: stagecraft_hlr::Material(0),
            positions: m.positions.iter().map(|p| Vec3::from_array(*p).as_dvec3()).collect(),
            triangles: m.indices.as_chunks::<3>().0.iter().map(|t| [t[0], t[1], t[2]]).collect(),
        }],
        lines: Vec::new(),
    };
    let opts = stagecraft_hlr::Options { hidden_lines: false, intersections: false, ..Default::default() };
    let d = stagecraft_hlr::draw(&scene, &stagecraft_hlr::View::plan(), &opts);
    let lines: Vec<Vec<P2>> = d
        .polylines
        .into_iter()
        .map(|l| {
            let mut pts: Vec<P2> = l.points.iter().map(|p| [p.x, p.y]).collect();
            if l.closed && pts.len() > 2 {
                pts.push(pts[0]);
            }
            pts
        })
        .collect();
    let mut lines = lines;
    lines.extend(edge_on(&m));
    let lines = Arc::new(lines);
    if let Ok(mut c) = cache().lock() {
        if c.plans.len() > MAX_ENTRIES {
            c.plans.clear();
        }
        c.plans.insert(key, lines.clone());
    }
    lines
}

/// Faces seen edge-on from above (vertical sheets: walls modelled as single quads,
/// flats) are invisible to the hidden-line top view; each becomes the segment it
/// projects to. Duplicates (both sides of a sheet, two triangles of a quad) once.
fn edge_on(m: &Mesh) -> Vec<Vec<P2>> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for t in m.indices.as_chunks::<3>().0 {
        let [a, b, c] = [0, 1, 2].map(|i| Vec3::from_array(m.positions[t[i] as usize]).as_dvec3());
        let n = (b - a).cross(c - a);
        let len = n.length();
        if len < 1e-9 || (n.z / len).abs() > 1e-3 {
            continue;
        }
        // The two projected points furthest apart.
        let pts = [a, b, c].map(|p| glam::DVec2::new(p.x, p.y));
        let (mut i, mut j, mut best) = (0, 1, -1.0);
        for (p, q) in [(0, 1), (1, 2), (0, 2)] {
            let d = pts[p].distance_squared(pts[q]);
            if d > best {
                (i, j, best) = (p, q, d);
            }
        }
        if best < 1.0 {
            continue;
        }
        let (p, q) = if (pts[i].x, pts[i].y) <= (pts[j].x, pts[j].y) { (pts[i], pts[j]) } else { (pts[j], pts[i]) };
        let key = [p.x, p.y, q.x, q.y].map(|v| (v * 10.0).round() as i64);
        if seen.insert(key) {
            out.push(vec![[p.x, p.y], [q.x, q.y]]);
        }
    }
    out
}

fn outline(doc: &Document, obj: &Object) -> Arc<Vec<Vec<Vec3>>> {
    let key = parts_key(parts_of(obj));
    if let Some(o) = cache().lock().ok().and_then(|c| c.outlines.get(&key).cloned()) {
        return o;
    }
    let m = mesh_arc(doc, obj);
    let dirs = directions();
    let pts: Vec<Vec3> = m.positions.iter().map(|p| Vec3::from_array(*p)).collect();
    let o = if pts.is_empty() {
        Arc::new(Vec::new())
    } else {
        let ext: Vec<Vec3> = dirs
            .iter()
            .map(|d| *pts.iter().max_by(|a, b| a.dot(*d).total_cmp(&b.dot(*d))).expect("non-empty"))
            .collect();
        Arc::new(vec![ext])
    };
    if let Ok(mut c) = cache().lock() {
        c.outlines.insert(key, o.clone());
    }
    o
}

pub fn plan(doc: &Document, w: &DAffine3, obj: &Object, _style: &PlanStyle) -> Vec<PlanPrim> {
    // Only turned about z (or not at all): the cached top view still holds.
    let up = w.matrix3 * DVec3::Z;
    if (up - DVec3::Z).length() < 1e-6 {
        let lines = local_plan(doc, obj);
        return lines
            .iter()
            .map(|l| {
                let pts = l.iter().map(|p| super::to_plan_2d(w, *p)).collect();
                PlanPrim::line(pts, LineClass::Thin)
            })
            .collect();
    }
    world_silhouette(&outline(doc, obj), w)
}
