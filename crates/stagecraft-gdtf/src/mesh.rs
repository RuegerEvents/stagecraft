//! The fixture as triangles, assembled through the geometry tree.
//!
//! Units mm, GDTF axes (x right, y away from the viewer, z up; fixture hanging,
//! beams along −z). This is the fixture-local frame of an MVR fixture, so a caller
//! places the result with the MVR fixture matrix only.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat4, Vec3};

use crate::models::{self, MeshSource};
use crate::{FixtureType, Gdtf, GeometryKind, Warning};

/// Indexed triangles with per-vertex normals and named index ranges.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub groups: Vec<MeshGroup>,
}

/// A named run of indices (a geometry instance after [`FixtureMesh::flatten`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshGroup {
    pub name: String,
    /// First index into [`Mesh::indices`].
    pub start: u32,
    pub count: u32,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut it = self.positions.iter().map(|p| Vec3::from_array(*p));
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    /// Appends `other` transformed by `m`; its groups are replaced by one named `name`.
    pub fn append(&mut self, other: &Mesh, m: Mat4, name: &str) {
        let base = self.positions.len() as u32;
        let start = self.indices.len() as u32;
        let nm = glam::Mat3::from_mat4(m).inverse().transpose();
        self.positions.extend(other.positions.iter().map(|p| m.transform_point3(Vec3::from_array(*p)).to_array()));
        self.normals.extend(other.normals.iter().map(|n| (nm * Vec3::from_array(*n)).normalize_or_zero().to_array()));
        let flip = m.determinant() < 0.0;
        for t in other.indices.chunks_exact(3) {
            if flip {
                self.indices.extend([base + t[0], base + t[2], base + t[1]]);
            } else {
                self.indices.extend(t.iter().map(|i| base + i));
            }
        }
        let count = self.indices.len() as u32 - start;
        if count > 0 {
            self.groups.push(MeshGroup { name: name.to_string(), start, count });
        }
    }
}

/// Mesh level of detail (GDTF folders `gltf_low`, `gltf`, `gltf_high` and the 3DS
/// equivalents). Missing levels fall back to the default one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Lod {
    Low,
    #[default]
    Default,
    High,
}

impl Lod {
    /// (folder, extension) in lookup order. glTF before 3DS, as the spec prefers.
    pub fn folders(self) -> &'static [(&'static str, &'static str)] {
        const DEFAULT: [(&str, &str); 2] = [("models/gltf", "glb"), ("models/3ds", "3ds")];
        match self {
            Lod::Default => &DEFAULT,
            Lod::Low => {
                &[("models/gltf_low", "glb"), ("models/3ds_low", "3ds"), ("models/gltf", "glb"), ("models/3ds", "3ds")]
            }
            Lod::High => &[
                ("models/gltf_high", "glb"),
                ("models/3ds_high", "3ds"),
                ("models/gltf", "glb"),
                ("models/3ds", "3ds"),
            ],
        }
    }
}

/// One node of the instanced geometry tree (references expanded).
#[derive(Debug, Clone)]
pub struct GeometryInstance {
    /// Instance name: the geometry's name, or the reference's name for the root of
    /// an instanced template.
    pub name: String,
    /// Names from the root, `/`-separated (`Base/Yoke/Head/Beam`).
    pub path: String,
    /// Name of the geometry definition (the template's for a reference root).
    pub definition: String,
    /// Kind of the definition (a reference root takes its template's kind).
    pub kind: GeometryKind,
    pub model: Option<String>,
    pub parent: Option<usize>,
    /// Relative to the parent, mm.
    pub local: Mat4,
    /// Relative to the fixture origin in the default pose, mm.
    pub world: Mat4,
    /// Name of the enclosing `GeometryReference`, if this node is instanced.
    pub reference: Option<String>,
}

/// A GDTF matrix (metres) as millimetres.
pub fn to_mm(m: Mat4) -> Mat4 {
    let mut m = m;
    m.w_axis.x *= 1000.0;
    m.w_axis.y *= 1000.0;
    m.w_axis.z *= 1000.0;
    m
}

/// The geometry tree of `mode` (or the first mode, or the first top-level geometry)
/// with references expanded, depth first, parents before children.
pub fn instances(ft: &FixtureType, mode: Option<&str>) -> Vec<GeometryInstance> {
    let root_name = mode.and_then(|m| ft.mode(m)).or(ft.modes.first()).map(|m| m.geometry.as_str());
    let root = root_name.and_then(|n| ft.top_geometry(n)).or(ft.geometries.first());
    let mut out = Vec::new();
    if let Some(root) = root {
        push(ft, root, None, None, None, "", &mut out, 0);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn push(
    ft: &FixtureType,
    g: &crate::Geometry,
    parent: Option<usize>,
    reference: Option<&str>,
    override_name: Option<(&str, Mat4, Option<String>)>,
    parent_path: &str,
    out: &mut Vec<GeometryInstance>,
    depth: usize,
) {
    if depth > 64 || out.len() > 100_000 {
        return;
    }
    if let GeometryKind::Reference { geometry, .. } = &g.kind {
        // A reference: instantiate the template with the reference's name, position
        // and (optionally) model. The template's own Position is not applied.
        if let Some(t) = ft.top_geometry(geometry)
            && !matches!(t.kind, GeometryKind::Reference { .. })
        {
            let model = g.model.clone().or_else(|| t.model.clone());
            push(ft, t, parent, Some(&g.name), Some((&g.name, g.position, model)), parent_path, out, depth + 1);
        }
        return;
    }
    let (name, pos, model) = match override_name {
        Some((n, p, m)) => (n.to_string(), p, m),
        None => (g.name.clone(), g.position, g.model.clone()),
    };
    let local = to_mm(pos);
    let world = match parent {
        Some(p) => out[p].world * local,
        None => local,
    };
    let path = if parent_path.is_empty() { name.clone() } else { format!("{parent_path}/{name}") };
    let idx = out.len();
    out.push(GeometryInstance {
        name,
        path: path.clone(),
        definition: g.name.clone(),
        kind: g.kind.clone(),
        model,
        parent,
        local,
        world,
        reference: reference.map(str::to_string),
    });
    for c in &g.children {
        push(ft, c, Some(idx), reference, None, &path, out, depth + 1);
    }
}

/// One geometry instance with its own mesh in its own frame.
#[derive(Debug, Clone)]
pub struct MeshPart {
    pub instance: GeometryInstance,
    /// The model mesh in the part's frame (mm), shared between instances of the
    /// same model. `None` for geometries without a model.
    pub mesh: Option<Arc<Mesh>>,
}

/// The fixture mesh: one part per geometry instance, plus warnings from loading.
#[derive(Debug, Clone, Default)]
pub struct FixtureMesh {
    pub parts: Vec<MeshPart>,
    /// Model name → where its mesh came from.
    pub sources: HashMap<String, MeshSource>,
    pub warnings: Vec<Warning>,
}

impl FixtureMesh {
    /// Indices of the `Axis` parts (pan, tilt …) in tree order.
    pub fn axes(&self) -> Vec<usize> {
        self.parts.iter().enumerate().filter(|(_, p)| p.instance.kind.is_axis()).map(|(i, _)| i).collect()
    }

    /// Indices of the `Beam` parts.
    pub fn beams(&self) -> Vec<usize> {
        self.parts
            .iter()
            .enumerate()
            .filter(|(_, p)| matches!(p.instance.kind, GeometryKind::Beam(_)))
            .map(|(i, _)| i)
            .collect()
    }

    /// World transforms with an extra local transform per part (e.g. a rotation of
    /// an axis): `world_i = world_parent · local_i · pose(i)`.
    pub fn posed_transforms(&self, pose: &dyn Fn(usize, &MeshPart) -> Mat4) -> Vec<Mat4> {
        let mut out: Vec<Mat4> = Vec::with_capacity(self.parts.len());
        for (i, p) in self.parts.iter().enumerate() {
            let local = p.instance.local * pose(i, p);
            let w = match p.instance.parent {
                Some(pi) if pi < out.len() => out[pi] * local,
                _ => local,
            };
            out.push(w);
        }
        out
    }

    /// All parts in the fixture frame, default pose; one group per part (named by
    /// its path).
    pub fn flatten(&self) -> Mesh {
        self.flatten_posed(&|_, _| Mat4::IDENTITY)
    }

    /// All parts with [`Self::posed_transforms`].
    pub fn flatten_posed(&self, pose: &dyn Fn(usize, &MeshPart) -> Mat4) -> Mesh {
        let ts = self.posed_transforms(pose);
        let mut out = Mesh::default();
        for (p, t) in self.parts.iter().zip(ts) {
            if let Some(m) = &p.mesh {
                out.append(m, t, &p.instance.path);
            }
        }
        out
    }
}

/// Builds the fixture mesh of `mode` (see [`instances`]).
pub fn build(gdtf: &Gdtf, mode: Option<&str>, lod: Lod) -> FixtureMesh {
    let ft = &gdtf.fixture;
    let mut out = FixtureMesh::default();
    let mut cache: HashMap<String, Arc<Mesh>> = HashMap::new();
    for inst in instances(ft, mode) {
        let mesh = inst.model.as_ref().and_then(|name| {
            if let Some(m) = cache.get(name) {
                return Some(m.clone());
            }
            let model = ft.model(name)?;
            let (m, src) = models::load_model(&gdtf.archive, model, lod, &mut out.warnings);
            let m = Arc::new(m);
            cache.insert(name.clone(), m.clone());
            out.sources.insert(name.clone(), src);
            Some(m)
        });
        out.parts.push(MeshPart { instance: inst, mesh });
    }
    out
}
