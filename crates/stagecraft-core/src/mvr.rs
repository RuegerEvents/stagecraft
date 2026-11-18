//! MVR import and export: `stagecraft-mvr`'s scene tree mapped to the document.
//!
//! Import: MVR layers become layers (elevation from the layer matrix), fixtures
//! become fixtures with their GDTF in the library, trusses become truss pieces
//! (catalogue profile by name, else the closest cross-section), objects with
//! geometry become `model` objects holding the mesh files as assets, groups become
//! `group` objects. The hierarchy is kept; every object lands at its MVR world
//! transform. MVR classes become tags.
//!
//! Export: layers, a class per tag, fixtures with their GDTFs, and everything else as
//! scene objects (trusses as `Truss`, hoists as `Support`) whose geometry is a GLB of
//! the object's mesh. Core has no meshes, so the mesh comes from a hook that
//! `stagecraft-geom` installs ([`install_mesher`]); without it those objects are
//! written without geometry.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use glam::{DAffine3, DMat3, DVec3};
use serde_json::{Value, json};
use stagecraft_mvr as mvr;
use uuid::Uuid;

use crate::asset::Asset;
use crate::command::CmdError;
use crate::engine::Ctx;
use crate::geom::{affine_to_placement, local_for_world, placement_to_affine};
use crate::model::{Document, Id, Layer, ModelPart, Object, ObjectKind, P3, Placement};
use crate::model_file;

/// Triangles of an object in its local frame, mm, for export.
pub struct ExportMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

pub type Mesher = dyn Fn(&Document, &Object) -> Option<ExportMesh> + Send + Sync;

static MESHER: OnceLock<Box<Mesher>> = OnceLock::new();

/// Give export a way to mesh objects (`stagecraft_geom::install` does). Later calls
/// are ignored.
pub fn install_mesher(f: Box<Mesher>) {
    let _ = MESHER.set(f);
}

pub fn affine_from(m: &mvr::Matrix) -> DAffine3 {
    DAffine3::from_cols_array(&m.to_array())
}

pub fn matrix_from(a: &DAffine3) -> mvr::Matrix {
    mvr::Matrix::from_array(a.to_cols_array())
}

/// Rotation and translation of `a`, with scale and shear taken out (Gram–Schmidt on
/// the axes, keeping a mirror as a flipped z).
fn rigid(a: &DAffine3) -> DAffine3 {
    let x = a.matrix3.x_axis.try_normalize().unwrap_or(DVec3::X);
    let y = (a.matrix3.y_axis - x * a.matrix3.y_axis.dot(x)).try_normalize().unwrap_or(DVec3::Y);
    let z = x.cross(y);
    DAffine3 { matrix3: DMat3::from_cols(x, y, z), translation: a.translation }
}

fn has_scale(a: &DAffine3) -> bool {
    let m = a.matrix3;
    [m.x_axis.length(), m.y_axis.length(), m.z_axis.length()].iter().any(|l| (l - 1.0).abs() > 1e-4)
        || m.determinant() < 0.0
}

// ---------------------------------------------------------------- import

/// Import options.
#[derive(Default)]
pub struct ImportOptions {
    /// MVR layer name → existing layer (id or name).
    pub layer_mapping: BTreeMap<String, String>,
}

#[derive(Default)]
struct Report {
    layers_created: usize,
    fixtures: usize,
    trusses: usize,
    models: usize,
    groups: usize,
    supports: usize,
    skipped: BTreeMap<String, usize>,
    gdtf_types: Vec<String>,
    warnings: Vec<String>,
}

struct Importer<'m> {
    mvr: &'m mvr::Mvr,
    gdtf_keys: HashMap<String, Option<String>>,
    class_tags: HashMap<Uuid, Id>,
    /// Model files already embedded: archive name → (asset hash, scale).
    files: HashMap<String, Option<(String, f64)>>,
    report: Report,
}

fn cie_to_hex(c: &mvr::CieColor) -> Option<String> {
    if c.y <= 0.0 {
        return None;
    }
    let big_y = if c.big_y > 0.0 { c.big_y / 100.0 } else { 1.0 };
    let x = c.x * big_y / c.y;
    let z = (1.0 - c.x - c.y) * big_y / c.y;
    let lin = [
        3.2406 * x - 1.5372 * big_y - 0.4986 * z,
        -0.9689 * x + 1.8758 * big_y + 0.0415 * z,
        0.0557 * x - 0.2040 * big_y + 1.0570 * z,
    ];
    let m = lin.iter().cloned().fold(1.0f64, f64::max);
    let g = |v: f64| {
        let v = (v / m).clamp(0.0, 1.0);
        let s = if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round() as u8
    };
    Some(format!("#{:02X}{:02X}{:02X}", g(lin[0]), g(lin[1]), g(lin[2])))
}

fn hex_to_cie(hex: &str) -> Option<mvr::CieColor> {
    let [r, g, b] = crate::model::parse_color(hex)?;
    let lin = |c: u8| {
        let s = c as f64 / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    let (r, g, b) = (lin(r), lin(g), lin(b));
    let x = 0.4124 * r + 0.3576 * g + 0.1805 * b;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = 0.0193 * r + 0.1192 * g + 0.9505 * b;
    let s = x + y + z;
    if s <= 0.0 {
        return None;
    }
    let r3 = |v: f64| (v * 1e6).round() / 1e6;
    Some(mvr::CieColor { x: r3(x / s), y: r3(y / s), big_y: r3(y * 100.0) })
}

/// Default name when the file has none.
fn kind_name(k: mvr::ObjectKind) -> &'static str {
    match k {
        mvr::ObjectKind::SceneObject => "Scene object",
        mvr::ObjectKind::GroupObject => "Group",
        mvr::ObjectKind::FocusPoint => "Focus point",
        mvr::ObjectKind::Fixture => "Fixture",
        mvr::ObjectKind::Truss => "Truss",
        mvr::ObjectKind::Support => "Support",
        mvr::ObjectKind::VideoScreen => "Video screen",
        mvr::ObjectKind::Projector => "Projector",
    }
}

impl<'m> Importer<'m> {
    fn warn(&mut self, s: String) {
        self.report.warnings.push(s);
    }

    /// The library key for a GDTFSpec, importing the embedded file once.
    fn gdtf(&mut self, ctx: &mut Ctx, spec: &str) -> Option<String> {
        if let Some(k) = self.gdtf_keys.get(spec) {
            return k.clone();
        }
        let key = match self.mvr.resources.resolve_gdtf(spec).and_then(|n| Some((n, self.mvr.resources.get(n)?))) {
            None => {
                self.warn(format!("GDTF `{spec}` is not in the file; its fixtures have no type"));
                None
            }
            Some((name, bytes)) => match crate::commands::fixture::import_bytes(ctx, name, bytes.to_vec(), None) {
                Ok(v) => {
                    let k = v["key"].as_str().map(str::to_owned);
                    if let Some(k) = &k
                        && !self.report.gdtf_types.contains(k)
                    {
                        self.report.gdtf_types.push(k.clone());
                    }
                    k
                }
                Err(e) => {
                    self.warn(format!("GDTF `{spec}`: {}", e.message));
                    None
                }
            },
        };
        self.gdtf_keys.insert(spec.to_owned(), key.clone());
        key
    }

    /// Embed a model file of the archive once: (asset hash, file units → mm).
    fn file(&mut self, ctx: &mut Ctx, name: &str) -> Option<(String, f64)> {
        if let Some(f) = self.files.get(name) {
            return f.clone();
        }
        let res = &self.mvr.resources;
        let got = res.resolve(name).and_then(|n| Some((n.to_owned(), res.get(n)?)));
        let out = match got {
            None => {
                self.warn(format!("geometry file `{name}` is not in the archive"));
                None
            }
            Some((real, bytes)) => {
                let ext = real.rsplit_once('.').map(|x| x.1.to_ascii_lowercase()).unwrap_or_default();
                match model_file::read(&ext, bytes) {
                    Err(e) => {
                        self.warn(format!("geometry file `{name}`: {e}"));
                        None
                    }
                    Ok(_) => {
                        // MVR: 3DS in mm, glTF in metres.
                        let scale = if ext == "3ds" { 1.0 } else { 1000.0 };
                        let a = Asset::new(&real, &ext, bytes.to_vec());
                        let hash = a.hash.clone();
                        ctx.doc_mut().assets.entry(hash.clone()).or_insert(a);
                        Some((hash, scale))
                    }
                }
            }
        };
        self.files.insert(name.to_owned(), out.clone());
        out
    }

    /// Model parts of an object's geometries (symbols expanded), relative to `frame`
    /// (the object's world transform without scale).
    fn parts(&mut self, ctx: &mut Ctx, o: &mvr::Object, frame: &DAffine3) -> Vec<ModelPart> {
        let mut insts = Vec::new();
        let symdefs: HashMap<Uuid, &mvr::Symdef> = self.mvr.scene.aux.symdefs.iter().map(|s| (s.uuid, s)).collect();
        fn expand(
            sd: &HashMap<Uuid, &mvr::Symdef>,
            g: &mvr::Geometry,
            parent: &mvr::Matrix,
            depth: usize,
            out: &mut Vec<(String, mvr::Matrix)>,
        ) {
            match g {
                mvr::Geometry::Geometry3D { file_name, matrix } => out.push((file_name.clone(), parent.then(matrix))),
                mvr::Geometry::Symbol { symdef, matrix, .. } => {
                    if depth < 32
                        && let Some(s) = sd.get(symdef)
                    {
                        let m = parent.then(matrix);
                        for c in &s.geometries {
                            expand(sd, c, &m, depth + 1, out);
                        }
                    }
                }
            }
        }
        for g in &o.geometries {
            expand(&symdefs, g, &o.world, 0, &mut insts);
        }
        let inv = frame.inverse();
        let mut parts = Vec::new();
        for (file, world) in insts {
            let Some((asset, scale)) = self.file(ctx, &file) else { continue };
            let local = inv * affine_from(&world);
            parts.push(ModelPart { asset, matrix: local.to_cols_array(), scale, y_up: false });
        }
        parts
    }

    fn bounds(&self, doc: &Document, parts: &[ModelPart]) -> Option<[P3; 2]> {
        let mut b = None;
        for p in parts {
            let Some(a) = doc.assets.get(&p.asset) else { continue };
            if let Ok(raw) = model_file::read(&a.ext, a.data.as_slice()) {
                b = model_file::union(b, model_file::part_bounds(&raw, p));
            }
        }
        b
    }

    /// Insert one object at world transform `w` (scale-free) under `parent`.
    fn insert(
        &mut self,
        ctx: &mut Ctx,
        mut obj: Object,
        w: &DAffine3,
        parent: Option<Id>,
        layer: Id,
        mo: &mvr::Object,
    ) -> Id {
        let doc = ctx.doc();
        if obj.id.is_nil() || doc.objects.contains_key(&obj.id) {
            obj.id = Uuid::new_v4();
        }
        obj.parent = parent;
        obj.layer = layer;
        obj.placement = local_for_world(doc, w, parent, layer);
        if let Some(c) = mo.classing.and_then(|c| self.class_tags.get(&c)) {
            obj.tags = vec![*c];
        }
        let id = obj.id;
        ctx.doc_mut().objects.insert(id, obj);
        id
    }

    fn object(&mut self, ctx: &mut Ctx, mo: &mvr::Object, siblings: &[mvr::Object], parent: Option<Id>, layer: Id) {
        let world = affine_from(&mo.world);
        let frame = rigid(&world);
        let name = if mo.name.trim().is_empty() { kind_name(mo.kind).to_owned() } else { mo.name.clone() };
        let blank = |kind: ObjectKind| Object {
            id: mo.uuid,
            name: name.clone(),
            layer,
            tags: Vec::new(),
            parent,
            placement: Placement::default(),
            kind,
            style: Default::default(),
            attrs: Default::default(),
        };
        let id = match mo.kind {
            mvr::ObjectKind::FocusPoint => {
                *self.report.skipped.entry("focus_point".into()).or_default() += 1;
                return;
            }
            mvr::ObjectKind::Fixture => {
                let o = self.fixture(ctx, mo, blank(ObjectKind::Group {}));
                if has_scale(&world) {
                    self.warn(format!("fixture `{name}`: matrix has scale or mirroring; only its rotation is used"));
                }
                self.report.fixtures += 1;
                self.insert(ctx, o, &frame, parent, layer, mo)
            }
            mvr::ObjectKind::Truss => {
                let parts = self.parts(ctx, mo, &frame);
                let mut b = self.bounds(ctx.doc(), &parts).or_else(|| self.gdtf_bounds(ctx, mo));
                if b.is_none() {
                    let len = sibling_spacing(mo, siblings);
                    if len.is_none() {
                        self.warn(format!("truss `{name}`: no geometry; a 1 m piece stands in"));
                    }
                    // Centred on its origin, as the-pult writes them.
                    let h = len.unwrap_or(1000.0) / 2.0;
                    b = Some([[-h, -145.0, -145.0], [h, 145.0, 145.0]]);
                }
                let (kind, local) = truss_from(mo, b);
                let mut o = blank(kind);
                o.attrs.insert("mvr_truss".into(), mo.gdtf_spec.clone().unwrap_or_else(|| mo.name.clone()));
                self.report.trusses += 1;
                self.insert(ctx, o, &(frame * local), parent, layer, mo)
            }
            mvr::ObjectKind::GroupObject => {
                self.report.groups += 1;
                self.insert(ctx, blank(ObjectKind::Group {}), &frame, parent, layer, mo)
            }
            mvr::ObjectKind::SceneObject
            | mvr::ObjectKind::Support
            | mvr::ObjectKind::VideoScreen
            | mvr::ObjectKind::Projector => {
                let parts = self.parts(ctx, mo, &frame);
                if parts.is_empty() {
                    if mo.kind == mvr::ObjectKind::Support {
                        self.report.supports += 1;
                        let o = blank(ObjectKind::Hoist { capacity_kg: 0.0, weight_kg: 0.0 });
                        self.insert(ctx, o, &frame, parent, layer, mo)
                    } else {
                        self.report.groups += 1;
                        self.insert(ctx, blank(ObjectKind::Group {}), &frame, parent, layer, mo)
                    }
                } else {
                    let bounds = self.bounds(ctx.doc(), &parts).unwrap_or([[0.0; 3]; 2]);
                    let mut o = blank(ObjectKind::Model { parts, bounds });
                    if mo.kind != mvr::ObjectKind::SceneObject {
                        o.attrs.insert("mvr_kind".into(), mo.kind.tag().to_owned());
                    }
                    self.report.models += 1;
                    self.insert(ctx, o, &frame, parent, layer, mo)
                }
            }
        };
        for c in &mo.children {
            self.object(ctx, c, &mo.children, Some(id), layer);
        }
    }

    /// Bounds of a truss's own GDTF model, if it has one.
    fn gdtf_bounds(&mut self, ctx: &mut Ctx, mo: &mvr::Object) -> Option<[P3; 2]> {
        let spec = mo.gdtf_spec.as_deref()?;
        let name = self.mvr.resources.resolve_gdtf(spec)?;
        let g = stagecraft_gdtf::Gdtf::from_bytes(self.mvr.resources.get(name)?).ok()?;
        let (lo, hi) = g.mesh(mo.gdtf_mode.as_deref(), stagecraft_gdtf::Lod::Low).flatten().bounds()?;
        let _ = ctx;
        Some([lo.as_dvec3().to_array(), hi.as_dvec3().to_array()])
    }

    fn fixture(&mut self, ctx: &mut Ctx, mo: &mvr::Object, mut o: Object) -> Object {
        let key = mo.gdtf_spec.as_deref().filter(|s| !s.trim().is_empty()).and_then(|s| self.gdtf(ctx, s));
        let doc = ctx.doc();
        let t = key.as_ref().and_then(|k| doc.fixture_types.get(k));
        // Exporters trim mode names the GDTF has with trailing spaces, or change case.
        let find = |t: &crate::fixture_type::FixtureType, m: &str| {
            t.mode(m)
                .or_else(|| t.modes.iter().find(|x| x.name.trim() == m.trim()))
                .or_else(|| t.modes.iter().find(|x| x.name.trim().eq_ignore_ascii_case(m.trim())))
                .map(|x| x.name.clone())
        };
        let mode = match (t, mo.gdtf_mode.as_deref()) {
            (Some(t), Some(m)) if find(t, m).is_some() => find(t, m),
            (Some(t), Some(m)) => {
                let first = t.modes.first().map(|x| x.name.clone());
                self.warn(format!("fixture `{}`: mode `{m}` not in {}; using the first", o.name, t.key));
                first
            }
            (Some(t), None) => t.modes.first().map(|x| x.name.clone()),
            (None, m) => m.map(str::to_owned),
        };
        let breaks = t.and_then(|t| t.mode_or_first(mode.as_deref())).map_or(1, |m| m.footprint.len().max(1));
        let weight = t.and_then(|t| t.weight_kg);
        let mut addrs: Vec<&mvr::DmxAddress> = mo.addresses.iter().filter(|a| a.absolute > 0).collect();
        addrs.sort_by_key(|a| a.dmx_break);
        let address = addrs.first().map(|a| a.absolute);
        let mut extra: Vec<u32> = addrs.iter().skip(1).map(|a| a.absolute).collect();
        if addrs.len() > breaks && t.is_some() {
            self.warn(format!("fixture `{}`: {} addresses for {breaks} DMX breaks", o.name, addrs.len()));
            extra.truncate(breaks - 1);
        }
        let fid_text = mo.fixture_id.trim();
        let fid = fid_text
            .parse::<u32>()
            .ok()
            .or_else(|| (mo.fixture_id_numeric > 0).then_some(mo.fixture_id_numeric as u32));
        if !fid_text.is_empty() && fid_text.parse::<u32>().is_err() {
            o.attrs.insert("fixture_id".into(), fid_text.to_owned());
        }
        if mo.fixture_id_numeric > 0 && Some(mo.fixture_id_numeric as u32) != fid {
            o.attrs.insert("fixture_id_numeric".into(), mo.fixture_id_numeric.to_string());
        }
        let cid = mo.custom_id.filter(|c| *c != 0).map(|c| c.to_string());
        // grandMA3 writes D65 white for every fixture: that is no colour information.
        let white = mo.color.as_ref().is_some_and(|c| (c.x - 0.3127).abs() < 0.002 && (c.y - 0.329).abs() < 0.002);
        if !white && let Some(c) = mo.color.as_ref().and_then(cie_to_hex) {
            o.attrs.insert("colour".into(), c);
        }
        if key.is_none()
            && let Some(s) = &mo.gdtf_spec
        {
            o.attrs.insert("gdtf_spec".into(), s.clone());
        }
        if mo.unit_number != 0 {
            o.attrs.insert("unit_number".into(), mo.unit_number.to_string());
        }
        if mo.child_position.is_some() {
            self.warn(format!(
                "fixture `{}` hangs on a geometry of its parent; placed at the parent's origin frame",
                o.name
            ));
        }
        o.kind = ObjectKind::Fixture {
            fid,
            cid,
            gdtf: key,
            mode: if t.is_some() { mode } else { None },
            address,
            extra_addresses: extra,
            weight_kg: weight,
        };
        o
    }
}

/// For a truss without geometry: the distance to the nearest other geometry-less
/// truss lying on its local x axis in the same parent (pieces of a straight run),
/// 250–6000 mm.
fn sibling_spacing(mo: &mvr::Object, siblings: &[mvr::Object]) -> Option<f64> {
    let a = affine_from(&mo.matrix);
    let x = a.matrix3.x_axis.try_normalize()?;
    siblings
        .iter()
        .filter(|s| s.kind == mvr::ObjectKind::Truss && s.uuid != mo.uuid && s.geometries.is_empty())
        .filter_map(|s| {
            let d = affine_from(&s.matrix).translation - a.translation;
            let along = d.dot(x).abs();
            ((d - x * d.dot(x)).length() < 1.0 && (250.0..=6000.0).contains(&along)).then_some(along)
        })
        .min_by(f64::total_cmp)
}

/// A truss piece for an MVR truss with local geometry bounds `b`: the profile, and
/// the transform from the MVR object's frame to the piece's (its axis along the
/// longest side, starting at the low end, centred across).
fn truss_from(mo: &mvr::Object, b: Option<[P3; 2]>) -> (ObjectKind, DAffine3) {
    let [lo, hi] = b.unwrap_or([[0.0, -150.0, -150.0], [1000.0, 150.0, 150.0]]);
    let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
    // Local x unless another side is clearly longer: cube-like pieces (corner boxes)
    // keep the file's frame.
    let longest = (0..3).max_by(|a, b| size[*a].total_cmp(&size[*b])).unwrap_or(0);
    let axis = if size[0] >= 0.9 * size[longest] { 0 } else { longest };
    let (length, cross, local) = match axis {
        0 => (size[0], [size[1], size[2]], DAffine3::from_translation(DVec3::new(lo[0], c[1], c[2]))),
        // +x of the piece along +y: turn 90° about z.
        1 => (
            size[1],
            [size[0], size[2]],
            DAffine3::from_translation(DVec3::new(c[0], lo[1], c[2])) * DAffine3::from_rotation_z(90f64.to_radians()),
        ),
        // Along +z: turn −90° about y.
        _ => (
            size[2],
            [size[1], size[0]],
            DAffine3::from_translation(DVec3::new(c[0], c[1], lo[2])) * DAffine3::from_rotation_y(-90f64.to_radians()),
        ),
    };
    let profile = truss_profile(&[mo.name.as_str(), mo.gdtf_spec.as_deref().unwrap_or("")], cross);
    // Lengths come from f32 meshes: keep 0.01 mm.
    let length = (length.max(1.0) * 100.0).round() / 100.0;
    (ObjectKind::Truss { length, profile }, local)
}

/// A catalogue profile whose name appears in one of `names` (longest name first),
/// else the one with the closest cross-section.
pub fn truss_profile(names: &[&str], cross: [f64; 2]) -> String {
    let mut profiles = crate::catalog::truss_profiles();
    profiles.sort_by(|a, b| b.name.len().cmp(&a.name.len()).then(a.name.cmp(&b.name)));
    let norm = |s: &str| s.to_ascii_lowercase().replace([' ', '-', '_'], "");
    for n in names {
        let n = norm(n);
        if let Some(p) = profiles.iter().find(|p| !p.name.is_empty() && n.contains(&norm(&p.name))) {
            return p.name.clone();
        }
    }
    let (w, h) = (cross[0].max(cross[1]), cross[0].min(cross[1]));
    profiles
        .iter()
        .min_by(|a, b| {
            let d = |p: &crate::catalog::TrussProfile| {
                (p.width.max(p.height) - w).abs() + (p.width.min(p.height) - h).abs()
            };
            d(a).total_cmp(&d(b)).then(a.name.cmp(&b.name))
        })
        .map_or_else(|| "F34".to_owned(), |p| p.name.clone())
}

/// `mvr.import`: read the bytes and add everything to the document.
pub fn import(ctx: &mut Ctx, bytes: &[u8], opts: &ImportOptions) -> Result<Value, CmdError> {
    let imp = mvr::read_mvr(bytes).map_err(|e| CmdError::invalid(format!("not an MVR file: {e}")))?;
    let stats = imp.mvr.scene.stats();
    let mut im = Importer {
        mvr: &imp.mvr,
        gdtf_keys: HashMap::new(),
        class_tags: HashMap::new(),
        files: HashMap::new(),
        report: Report::default(),
    };
    im.report.warnings.extend(imp.warnings.iter().map(|w| w.to_string()));
    // Classes → tags (by name).
    for c in &imp.mvr.scene.aux.classes {
        let name = c.name.trim();
        if name.is_empty() {
            continue;
        }
        let doc = ctx.doc_mut();
        let id = match doc.tags.values().find(|t| t.name == name) {
            Some(t) => t.id,
            None => {
                let id = if doc.tags.contains_key(&c.uuid) || c.uuid.is_nil() { Uuid::new_v4() } else { c.uuid };
                doc.tags.insert(id, crate::model::Tag { id, name: name.to_owned(), color: None });
                id
            }
        };
        im.class_tags.insert(c.uuid, id);
    }
    let mut layers = Vec::new();
    for l in &imp.mvr.scene.layers {
        let lm = affine_from(&l.matrix);
        let doc = ctx.doc();
        let target = match opts.layer_mapping.get(&l.name) {
            Some(r) => Some(crate::commands::resolve_layer(doc, r)?),
            None => doc.layers.values().find(|x| x.name == l.name).map(|x| x.id),
        };
        let id = match target {
            Some(id) => id,
            None => {
                let order = doc.layers.values().map(|x| x.order).max().unwrap_or(0) + 1;
                let id = if doc.layers.contains_key(&l.uuid) || l.uuid.is_nil() { Uuid::new_v4() } else { l.uuid };
                let name = if l.name.trim().is_empty() { format!("Layer {order}") } else { l.name.clone() };
                let layer = Layer { id, name, elevation: lm.translation.z, order, visible: true };
                ctx.doc_mut().layers.insert(id, layer);
                im.report.layers_created += 1;
                id
            }
        };
        if !(lm.matrix3 - DMat3::IDENTITY).to_cols_array().iter().all(|v| v.abs() < 1e-9)
            || lm.translation.x.abs() > 1e-6
            || lm.translation.y.abs() > 1e-6
        {
            im.warn(format!("layer `{}` is moved or turned; objects keep their world positions", l.name));
        }
        layers.push((l, id));
    }
    for (l, id) in layers {
        for o in &l.children {
            im.object(ctx, o, &l.children, None, id);
        }
    }
    let r = im.report;
    let warnings = collapse(r.warnings);
    Ok(json!({
        "layers_created": r.layers_created,
        "fixtures": r.fixtures,
        "trusses": r.trusses,
        "models": r.models,
        "groups": r.groups,
        "supports": r.supports,
        "skipped": r.skipped,
        "gdtf_types": r.gdtf_types,
        "file": {
            "version": format!("{}.{}", imp.mvr.header.version.major, imp.mvr.header.version.minor),
            "provider": imp.mvr.header.provider,
            "layers": stats.layers, "fixtures": stats.fixtures, "trusses": stats.trusses,
            "scene_objects": stats.scene_objects, "groups": stats.groups, "supports": stats.supports,
            "focus_points": stats.focus_points, "video_screens": stats.video_screens, "projectors": stats.projectors,
        },
        "warnings": warnings,
    }))
}

/// Identical warnings once, at most 100.
fn collapse(w: Vec<String>) -> Vec<String> {
    let mut seen: Vec<(String, usize)> = Vec::new();
    for s in w {
        match seen.iter_mut().find(|x| x.0 == s) {
            Some(x) => x.1 += 1,
            None => seen.push((s, 1)),
        }
    }
    let n = seen.len();
    let mut out: Vec<String> =
        seen.into_iter().take(100).map(|(s, c)| if c > 1 { format!("{s} ({c}×)") } else { s }).collect();
    if n > 100 {
        out.push(format!("… {} more", n - 100));
    }
    out
}

// ---------------------------------------------------------------- export

struct Exporter<'d> {
    doc: &'d Document,
    resources: mvr::Resources,
    /// Generated GLBs by content hash → file name.
    glbs: HashMap<String, String>,
    gdtf_files: HashMap<String, String>,
    warnings: Vec<String>,
    counts: BTreeMap<&'static str, usize>,
}

impl Exporter<'_> {
    fn glb(&mut self, m: &ExportMesh) -> String {
        let bytes = mvr::glb::write_glb(&m.positions, &m.normals, &m.indices);
        let hash = crate::asset::hash_hex(&bytes);
        if let Some(n) = self.glbs.get(&hash) {
            return n.clone();
        }
        let name = format!("sc_{}.glb", &hash[..16]);
        self.resources.insert(name.clone(), bytes);
        self.glbs.insert(hash, name.clone());
        name
    }

    fn mesh_geometry(&mut self, o: &Object) -> Vec<mvr::Geometry> {
        let Some(mesher) = MESHER.get() else {
            *self.counts.entry("without_geometry").or_default() += 1;
            return Vec::new();
        };
        match mesher(self.doc, o) {
            Some(m) if !m.indices.is_empty() => {
                vec![mvr::Geometry::Geometry3D { file_name: self.glb(&m), matrix: mvr::Matrix::IDENTITY }]
            }
            _ => Vec::new(),
        }
    }

    /// Geometries of a model object: its own files where MVR reads them the same way
    /// (GLB/glTF in metres, 3DS in mm), else a GLB of its mesh.
    fn model_geometry(&mut self, o: &Object, parts: &[ModelPart]) -> Vec<mvr::Geometry> {
        let native = parts.iter().all(|p| {
            self.doc.assets.get(&p.asset).is_some_and(|a| match a.ext.as_str() {
                "glb" => p.scale == 1000.0 && !p.y_up,
                "3ds" => p.scale == 1.0 && !p.y_up,
                _ => false,
            })
        });
        if !native {
            return self.mesh_geometry(o);
        }
        let mut out = Vec::new();
        for p in parts {
            let a = &self.doc.assets[&p.asset];
            let name = format!("{}.{}", &a.hash[..16], a.ext);
            if !self.resources.files.contains_key(&name) {
                self.resources.insert(name.clone(), a.data.as_slice().to_vec());
            }
            out.push(mvr::Geometry::Geometry3D { file_name: name, matrix: mvr::Matrix::from_array(p.matrix) });
        }
        out
    }

    fn gdtf_file(&mut self, key: &str) -> Option<String> {
        if let Some(n) = self.gdtf_files.get(key) {
            return Some(n.clone());
        }
        let t = self.doc.fixture_types.get(key)?;
        let a = self.doc.assets.get(&t.asset)?;
        let base: String = key.chars().map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
        let name = format!("{base}.gdtf");
        self.resources.insert(name.clone(), a.data.as_slice().to_vec());
        self.gdtf_files.insert(key.to_owned(), name.clone());
        Some(name)
    }

    fn object(&mut self, o: &Object) -> mvr::Object {
        let doc = self.doc;
        let kind = match &o.kind {
            ObjectKind::Fixture { .. } => mvr::ObjectKind::Fixture,
            ObjectKind::Truss { .. } | ObjectKind::TrussNode { .. } => mvr::ObjectKind::Truss,
            ObjectKind::Hoist { .. } => mvr::ObjectKind::Support,
            k if k.is_group() => mvr::ObjectKind::GroupObject,
            _ => mvr::ObjectKind::SceneObject,
        };
        *self.counts.entry(kind.tag()).or_default() += 1;
        let mut m = mvr::Object::new(kind, o.id, o.name.clone());
        m.matrix = matrix_from(&placement_to_affine(&o.placement));
        m.classing = o.tags.first().copied();
        match &o.kind {
            ObjectKind::Fixture { fid, cid, gdtf, mode, address, extra_addresses, .. } => {
                let t = gdtf.as_ref().and_then(|k| doc.fixture_types.get(k));
                m.gdtf_spec = gdtf.as_deref().and_then(|k| self.gdtf_file(k));
                if gdtf.is_some() && m.gdtf_spec.is_none() {
                    self.warnings.push(format!(
                        "fixture `{}`: type `{}` missing",
                        o.name,
                        gdtf.as_deref().unwrap_or("")
                    ));
                }
                m.gdtf_mode = t.and_then(|t| t.mode_or_first(mode.as_deref())).map(|x| x.name.clone());
                m.fixture_id = fid.map(|f| f.to_string()).unwrap_or_default();
                m.fixture_id_numeric = fid.unwrap_or(0) as i64;
                m.custom_id = cid.as_deref().and_then(|c| c.trim().parse::<i64>().ok());
                if cid.is_some() && m.custom_id.is_none() {
                    self.warnings.push(format!(
                        "fixture `{}`: CID `{}` is not a number; left out",
                        o.name,
                        cid.as_deref().unwrap_or("")
                    ));
                }
                if m.custom_id.is_some() {
                    m.custom_id_type = Some(0);
                }
                let mut addrs = Vec::new();
                if let Some(a) = address.filter(|a| *a > 0) {
                    addrs.push(mvr::DmxAddress { dmx_break: 0, absolute: a });
                }
                for (i, a) in extra_addresses.iter().enumerate() {
                    if *a > 0 {
                        addrs.push(mvr::DmxAddress { dmx_break: i as u32 + 1, absolute: *a });
                    }
                }
                m.addresses = addrs;
                m.color = o.attrs.get("colour").and_then(|c| hex_to_cie(c));
                m.unit_number = o.attrs.get("unit_number").and_then(|u| u.parse().ok()).unwrap_or(0);
            }
            ObjectKind::Model { parts, .. } => m.geometries = self.model_geometry(o, parts),
            k if k.is_group() => {}
            _ => m.geometries = self.mesh_geometry(o),
        }
        for c in doc.children(o.id) {
            let child = self.object(&doc.objects[&c]);
            m.children.push(child);
        }
        m
    }
}

/// `mvr.export`: the document as MVR 1.6 bytes, and a summary.
pub fn export(doc: &Document) -> Result<(Vec<u8>, Value), CmdError> {
    let mut ex = Exporter {
        doc,
        resources: mvr::Resources::default(),
        glbs: HashMap::new(),
        gdtf_files: HashMap::new(),
        warnings: Vec::new(),
        counts: BTreeMap::new(),
    };
    let mut scene = mvr::Scene::default();
    for t in doc.tags.values() {
        scene.aux.classes.push(mvr::Class { uuid: t.id, name: t.name.clone() });
    }
    for l in doc.layers_sorted() {
        let mut layer = mvr::Layer::new(l.id, l.name.clone());
        layer.matrix = mvr::Matrix::translation(0.0, 0.0, l.elevation);
        for o in doc.objects.values().filter(|o| o.parent.is_none() && o.layer == l.id) {
            let mo = ex.object(o);
            layer.children.push(mo);
        }
        scene.layers.push(layer);
    }
    scene.resolve_world();
    let bytes = mvr::write_mvr_scene(&scene, &ex.resources).map_err(|e| CmdError::io(e.to_string()))?;
    let summary = json!({
        "layers": scene.layers.len(),
        "objects": ex.counts,
        "files": ex.resources.files.len(),
        "warnings": ex.warnings,
    });
    Ok((bytes, summary))
}

/// World transform of an object as an MVR matrix (for tests and checks).
pub fn world_matrix(doc: &Document, id: Id) -> Option<mvr::Matrix> {
    crate::geom::world(doc, id).map(|w| matrix_from(&w))
}

/// Placement from an MVR matrix (scale dropped).
pub fn placement_of(m: &mvr::Matrix) -> Placement {
    affine_to_placement(&rigid(&affine_from(m)))
}
