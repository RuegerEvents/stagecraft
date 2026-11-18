//! Vector views with hidden lines removed: document meshes go into a
//! [`stagecraft_hlr::Scene`] and come back as lines and hatched cut regions.
//!
//! Results are cached per view (shown objects, view, options) and document revision.
//! The revision is the identity of the document's object, layer, fixture type and
//! asset maps: `imbl` maps share their root until something in them changes, so
//! editing a sheet or a label leaves the cache warm while moving an object
//! recomputes. Maps that were rebuilt but are equal also count as unchanged.

use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use glam::{DVec3, Vec3};
use imbl::OrdMap;
use stagecraft_core::fixture_type::FixtureType;
use stagecraft_core::model::{Layer, Object};
use stagecraft_core::{Document, Id};
use stagecraft_geom::MaterialClass;
use stagecraft_hlr as hlr;

use crate::view::{CutRegion, EdgeClass, VectorView, VectorViewSource, ViewLine, ViewOptions, ViewSpec, lod_for};

/// Views kept per source; old ones go first.
const CACHE_SIZE: usize = 48;

/// The document parts a view depends on, kept to compare against later documents.
#[derive(Clone)]
struct Revision {
    objects: OrdMap<Id, Object>,
    layers: OrdMap<Id, Layer>,
    types: OrdMap<String, FixtureType>,
    assets: OrdMap<String, stagecraft_core::asset::Asset>,
}

impl Revision {
    fn of(doc: &Document) -> Self {
        Self {
            objects: doc.objects.clone(),
            layers: doc.layers.clone(),
            types: doc.fixture_types.clone(),
            assets: doc.assets.clone(),
        }
    }

    fn matches(&self, doc: &Document) -> bool {
        (self.objects.ptr_eq(&doc.objects) || self.objects == doc.objects)
            && (self.layers.ptr_eq(&doc.layers) || self.layers == doc.layers)
            && (self.types.ptr_eq(&doc.fixture_types) || self.types == doc.fixture_types)
            && (self.assets.ptr_eq(&doc.assets) || self.assets == doc.assets)
    }

    /// Make later comparisons cheap again after an equal-but-rebuilt match.
    fn refresh(&mut self, doc: &Document) {
        *self = Self::of(doc);
    }
}

struct Entry {
    key: u64,
    rev: Revision,
    view: Arc<VectorView>,
    used: u64,
}

#[derive(Default)]
struct Cache {
    entries: Vec<Entry>,
    clock: u64,
    hits: u64,
    misses: u64,
}

/// Hidden-line vector views through `stagecraft-hlr`, cached.
pub struct HlrSource {
    cache: Mutex<Cache>,
    pub options: hlr::Options,
}

impl Default for HlrSource {
    fn default() -> Self {
        Self::new()
    }
}

fn view_key(ids: &[Id], spec: &ViewSpec, opts: ViewOptions) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ids.hash(&mut h);
    let mut f = |v: f64| v.to_bits().hash(&mut h);
    for v in [spec.dir, spec.up] {
        v.to_array().into_iter().for_each(&mut f);
    }
    if let Some(p) = &spec.section {
        p.o().to_array().into_iter().chain(p.n().to_array()).for_each(&mut f);
    }
    f(spec.scale);
    opts.hash(&mut h);
    h.finish()
}

impl HlrSource {
    pub fn new() -> Self {
        Self { cache: Mutex::new(Cache::default()), options: hlr::Options::default() }
    }

    /// `(hits, misses)` since start, for tests and timing.
    pub fn stats(&self) -> (u64, u64) {
        let c = self.cache.lock().expect("hlr cache");
        (c.hits, c.misses)
    }

    /// Forget every cached view.
    pub fn clear(&self) {
        self.cache.lock().expect("hlr cache").entries.clear();
    }

    /// The cached view if there is a current one, without computing.
    pub fn cached(&self, doc: &Document, ids: &[Id], spec: &ViewSpec, opts: ViewOptions) -> Option<Arc<VectorView>> {
        let key = view_key(ids, spec, opts);
        let mut c = self.cache.lock().expect("hlr cache");
        c.clock += 1;
        let now = c.clock;
        let e = c.entries.iter_mut().find(|e| e.key == key && e.rev.matches(doc))?;
        if !e.rev.objects.ptr_eq(&doc.objects) {
            e.rev.refresh(doc);
        }
        e.used = now;
        Some(e.view.clone())
    }

    fn store(&self, doc: &Document, key: u64, view: Arc<VectorView>) {
        let mut c = self.cache.lock().expect("hlr cache");
        c.clock += 1;
        let used = c.clock;
        // Entries of an older revision for the same view are dead.
        c.entries.retain(|e| e.key != key);
        if c.entries.len() >= CACHE_SIZE
            && let Some(i) = c.entries.iter().enumerate().min_by_key(|(_, e)| e.used).map(|(i, _)| i)
        {
            c.entries.swap_remove(i);
        }
        c.entries.push(Entry { key, rev: Revision::of(doc), view, used });
    }
}

impl VectorViewSource for HlrSource {
    fn view(&self, doc: &Document, ids: &[Id], spec: &ViewSpec, opts: ViewOptions) -> Arc<VectorView> {
        if let Some(v) = self.cached(doc, ids, spec, opts) {
            self.cache.lock().expect("hlr cache").hits += 1;
            return v;
        }
        let v = Arc::new(compute(doc, ids, spec, opts, &self.options));
        let mut c = self.cache.lock().expect("hlr cache");
        c.misses += 1;
        drop(c);
        self.store(doc, view_key(ids, spec, opts), v.clone());
        v
    }
}

/// The HLR view the sheet crate would use, for a sheet viewport's spec.
pub fn hlr_view(spec: &ViewSpec) -> hlr::View {
    let v = hlr::View::new(spec.dir, spec.up);
    match &spec.section {
        Some(p) => v.with_section(hlr::SectionPlane::new(p.o(), p.n())),
        None => v,
    }
}

/// Meshes of the shown objects in world mm, one per object and material run.
pub fn scene(doc: &Document, ids: &[Id], scale: f64) -> hlr::Scene {
    let mut scene = hlr::Scene::default();
    for (i, &id) in ids.iter().enumerate() {
        let Some(o) = doc.objects.get(&id) else { continue };
        if o.kind.is_group() {
            continue;
        }
        let Some(w) = stagecraft_core::geom::world(doc, id) else { continue };
        let mesh = stagecraft_geom::mesh(doc, o, lod_for(doc, id, scale));
        if mesh.is_empty() {
            continue;
        }
        let world: Vec<DVec3> =
            mesh.positions.iter().map(|p| w.transform_point3(Vec3::from_array(*p).as_dvec3())).collect();
        let mut remap = vec![u32::MAX; world.len()];
        for g in &mesh.groups {
            let mut positions = Vec::new();
            let mut triangles = Vec::new();
            let range = g.start as usize..(g.start + g.count) as usize;
            for t in mesh.indices[range].chunks_exact(3) {
                let mut tri = [0u32; 3];
                for (k, &vi) in t.iter().enumerate() {
                    let r = &mut remap[vi as usize];
                    if *r == u32::MAX {
                        *r = positions.len() as u32;
                        positions.push(world[vi as usize]);
                    }
                    tri[k] = *r;
                }
                triangles.push(tri);
            }
            for t in mesh.indices[g.start as usize..(g.start + g.count) as usize].iter() {
                remap[*t as usize] = u32::MAX;
            }
            if !triangles.is_empty() {
                scene.meshes.push(hlr::Mesh {
                    object: hlr::ObjectId(i as u64),
                    material: hlr::Material(g.material.index()),
                    positions,
                    triangles,
                });
            }
        }
    }
    scene
}

fn material(m: hlr::Material) -> MaterialClass {
    MaterialClass::ALL.get(m.0 as usize).copied().unwrap_or_default()
}

/// Run hidden-line removal for a view (no cache).
pub fn compute(doc: &Document, ids: &[Id], spec: &ViewSpec, opts: ViewOptions, base: &hlr::Options) -> VectorView {
    let scene = scene(doc, ids, spec.scale);
    let o = hlr::Options { hidden_lines: opts.hidden_lines, ..*base };
    let d = hlr::draw(&scene, &hlr_view(spec), &o);
    let mut out = VectorView::default();
    for l in d.polylines {
        let mut points: Vec<[f64; 2]> = l.points.iter().map(|p| [p.x, p.y]).collect();
        if l.closed && points.len() > 2 {
            points.push(points[0]);
        }
        let class = match l.class {
            hlr::LineClass::Cut => EdgeClass::Cut,
            hlr::LineClass::Visible => EdgeClass::Visible,
            hlr::LineClass::Hidden => EdgeClass::Hidden,
            hlr::LineClass::Centre => EdgeClass::Centre,
        };
        out.lines.push(ViewLine { object: ids[l.object.0 as usize], points, class });
    }
    for r in d.regions {
        let ring = |v: &[glam::DVec2]| v.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>();
        let mut rings = vec![ring(&r.outer)];
        rings.extend(r.holes.iter().map(|h| ring(h)));
        out.cuts.push(CutRegion { object: ids[r.object.0 as usize], material: material(r.material), rings });
    }
    out
}
