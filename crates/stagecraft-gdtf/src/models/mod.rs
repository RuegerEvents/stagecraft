//! Model meshes: 3DS and GLB files from the archive, primitive fallbacks, and the
//! spec's rule that every mesh is scaled to the model's Length × Width × Height.

pub mod gltf;
pub mod obj;
pub mod primitives;
pub mod three_ds;

use glam::{Mat3, Mat4, Vec3};

use crate::mesh::{Lod, Mesh};
use crate::{Archive, Model, PrimitiveType, Warning};

/// Triangles as read from a file, before scaling. Groups are (name, first index,
/// index count).
#[derive(Debug, Clone, Default)]
pub struct RawMesh {
    pub positions: Vec<Vec3>,
    pub normals: Option<Vec<Vec3>>,
    pub indices: Vec<u32>,
    pub groups: Vec<(String, u32, u32)>,
}

impl RawMesh {
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut it = self.positions.iter();
        let first = *it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))))
    }

    pub fn transform(&mut self, m: Mat4) {
        let nm = Mat3::from_mat4(m).inverse().transpose();
        for p in &mut self.positions {
            *p = m.transform_point3(*p);
        }
        if let Some(ns) = &mut self.normals {
            for n in ns {
                *n = (nm * *n).normalize_or_zero();
            }
        }
        if m.determinant() < 0.0 {
            for t in self.indices.as_chunks_mut::<3>().0 {
                t.swap(1, 2);
            }
        }
    }

    /// To a [`Mesh`]; faces without normals get flat normals (vertices unwelded).
    pub fn into_mesh(self, name: &str) -> Mesh {
        let mut m = Mesh::default();
        match self.normals {
            Some(n) if n.len() == self.positions.len() => {
                m.positions = self.positions.iter().map(|p| p.to_array()).collect();
                m.normals = n.iter().map(|n| n.to_array()).collect();
                m.indices = self.indices;
            }
            _ => {
                for t in self.indices.as_chunks::<3>().0 {
                    let [a, b, c] = [0, 1, 2].map(|i| self.positions[t[i] as usize]);
                    let n = (b - a).cross(c - a).normalize_or_zero();
                    let base = m.positions.len() as u32;
                    for p in [a, b, c] {
                        m.positions.push(p.to_array());
                        m.normals.push(n.to_array());
                    }
                    m.indices.extend([base, base + 1, base + 2]);
                }
            }
        }
        if !m.indices.is_empty() {
            m.groups.push(crate::mesh::MeshGroup { name: name.to_string(), start: 0, count: m.indices.len() as u32 });
        }
        m
    }
}

/// Where a model mesh came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeshSource {
    /// Path inside the archive.
    File(String),
    /// Generated from the model's `PrimitiveType` (or a box when undefined).
    Primitive,
}

/// The mesh of `model` in millimetres, GDTF axes, scaled to its dimensions.
/// Falls back to the primitive (with a warning) when the file is missing or broken.
pub fn load_model(archive: &Archive, model: &Model, lod: Lod, warnings: &mut Vec<Warning>) -> (Mesh, MeshSource) {
    let ctx = format!("Model {}", model.name);
    let dims = Vec3::new(model.length_m, model.width_m, model.height_m) * 1000.0;
    if let Some(file) = &model.file {
        let mut tried = false;
        for (folder, ext) in lod.folders() {
            let Some((path, bytes)) = archive.resource(folder, file, &[ext]) else {
                continue;
            };
            tried = true;
            let loaded = match *ext {
                "glb" | "gltf" => gltf::load(bytes),
                _ => three_ds::load(bytes),
            };
            match loaded {
                Ok(mut raw) => {
                    orient(&mut raw, dims, ext, &ctx, warnings);
                    fit(&mut raw, dims, ext, &ctx, warnings);
                    return (raw.into_mesh(&model.name), MeshSource::File(path.to_string()));
                }
                Err(e) => warnings.push(Warning::new(&ctx, format!("{path}: {e}; trying next"))),
            }
        }
        if !tried {
            warnings.push(Warning::new(&ctx, format!("file \"{file}\" not in the archive; using primitive")));
        }
    }
    if model.file.is_none() && model.primitive == PrimitiveType::Undefined {
        warnings.push(Warning::new(&ctx, "no file and PrimitiveType Undefined; using a box"));
    }
    (primitives::primitive(&model.primitive, dims), MeshSource::Primitive)
}

/// Sometimes a file uses the other up axis than its format implies. When the
/// model's dimensions fit the other reading clearly better, use that.
fn orient(raw: &mut RawMesh, dims: Vec3, ext: &str, ctx: &str, warnings: &mut Vec<Warning>) {
    let Some((lo, hi)) = raw.bounds() else { return };
    let ext_now = hi - lo;
    // Alternative: undo (glb) or apply (3ds) the y-up → z-up turn.
    let alt_m = if ext == "3ds" { gltf::y_up_to_z_up() } else { gltf::y_up_to_z_up().inverse() };
    let ext_alt = alt_m.transform_vector3(ext_now).abs();
    let (e_now, e_alt) = (shape_error(ext_now, dims), shape_error(ext_alt, dims));
    if let (Some(a), Some(b)) = (e_now, e_alt)
        && b < a * 0.5
        && a > 0.3
    {
        raw.transform(alt_m);
        warnings.push(Warning::new(ctx, "mesh fits the model dimensions only with the other up axis; turned"));
    }
}

/// How differently shaped two boxes are (scale-free); `None` if either is degenerate.
fn shape_error(a: Vec3, b: Vec3) -> Option<f32> {
    if a.min_element() <= 1e-9 || b.min_element() <= 1e-9 {
        return None;
    }
    let (a, b) = (a / a.max_element(), b / b.max_element());
    Some((0..3).map(|i| (a[i] / b[i]).ln().abs()).sum())
}

/// Scales the mesh about its origin so its bounding box matches `dims` (mm).
/// Axes with dimension 0 take the mean scale of the others; with no dimensions at
/// all, glTF is read as metres and 3DS as mm (metres if it is smaller than 10 units).
fn fit(raw: &mut RawMesh, dims: Vec3, ext: &str, ctx: &str, warnings: &mut Vec<Warning>) {
    let Some((lo, hi)) = raw.bounds() else { return };
    let size = hi - lo;
    let mut s = [0.0f32; 3];
    let mut known = Vec::new();
    for i in 0..3 {
        if dims[i] > 1e-6 && size[i] > 1e-9 {
            s[i] = dims[i] / size[i];
            known.push(s[i]);
        }
    }
    if known.is_empty() {
        let unit = if ext == "3ds" && size.max_element() >= 10.0 { 1.0 } else { 1000.0 };
        warnings.push(Warning::new(ctx, "no dimensions; mesh units guessed"));
        s = [unit; 3];
    } else if let Some(unit) = mismatched_dimensions(&known) {
        // Some exporters (Vectorworks "Custom@" files) write wrong Width/Height
        // while the mesh is already true to size: one axis matches exactly in m or
        // mm, another would stretch by more than 25 %. Keep the proportions.
        warnings.push(Warning::new(
            ctx,
            "model dimensions disagree with the mesh, which is already to scale on one axis; kept its proportions",
        ));
        s = [unit; 3];
    } else {
        let mean = known.iter().sum::<f32>() / known.len() as f32;
        for (i, v) in s.iter_mut().enumerate() {
            if *v == 0.0 {
                *v = if dims[i] <= 1e-6 && size[i] > 1e-9 { mean } else { 1.0 };
            }
        }
    }
    raw.transform(Mat4::from_scale(Vec3::from_array(s)));
}

/// With per-axis scales `s` (mm per mesh unit): the unit (1000 for metres, 1 for
/// mm) if one axis matches it within 1 % while the scales spread by more than 25 %.
fn mismatched_dimensions(s: &[f32]) -> Option<f32> {
    let (lo, hi) = s.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    if s.len() < 2 || hi / lo <= 1.25 {
        return None;
    }
    [1000.0, 1.0].into_iter().find(|unit| s.iter().any(|v| (v / unit - 1.0).abs() < 0.01))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube_raw(size: Vec3) -> RawMesh {
        // Unit cube (primitive sizes are clamped to ≥ 1 mm), then scaled.
        let m = primitives::primitive(&PrimitiveType::Cube, Vec3::ONE);
        RawMesh {
            positions: m.positions.iter().map(|p| Vec3::from_array(*p) * size).collect(),
            normals: None,
            indices: m.indices,
            groups: vec![],
        }
    }

    fn fitted(mesh: Vec3, dims_mm: Vec3) -> (Vec3, usize) {
        let mut raw = cube_raw(mesh);
        let mut w = Vec::new();
        fit(&mut raw, dims_mm, "glb", "t", &mut w);
        let (lo, hi) = raw.bounds().unwrap();
        (hi - lo, w.len())
    }

    #[test]
    fn scales_to_dimensions() {
        // Mesh in arbitrary units, stretched per axis.
        let (s, w) = fitted(Vec3::new(2.0, 1.0, 1.0), Vec3::new(300.0, 200.0, 100.0));
        assert!((s - Vec3::new(300.0, 200.0, 100.0)).length() < 1e-3);
        assert_eq!(w, 0);
        // Mesh in metres, slightly off: still fitted.
        let (s, _) = fitted(Vec3::new(0.3, 0.2, 0.1), Vec3::new(310.0, 190.0, 100.0));
        assert!((s - Vec3::new(310.0, 190.0, 100.0)).length() < 1e-2);
    }

    #[test]
    fn keeps_true_scale_mesh_when_dimensions_are_wrong() {
        // x matches in metres, y/z dimensions far off (Vectorworks Body case).
        let (s, w) = fitted(Vec3::new(0.2787, 0.2578, 0.5347), Vec3::new(278.7, 718.4, 718.4));
        assert!((s - Vec3::new(278.7, 257.8, 534.7)).length() < 0.1, "{s}");
        assert_eq!(w, 1);
    }

    #[test]
    fn zero_dimension_takes_mean_scale() {
        let (s, _) = fitted(Vec3::new(1.0, 1.0, 1.0), Vec3::new(100.0, 100.0, 0.0));
        assert!((s.z - 100.0).abs() < 1e-3);
    }
}
