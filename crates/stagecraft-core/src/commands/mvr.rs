//! MVR import/export and 3D model import.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::create::{Common, insert};
use crate::asset::Asset;
use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::model::{ModelPart, ObjectKind, P3, Placement};
use crate::model_file;

pub(super) fn register(r: &mut Registry) {
    r.add("mvr.import", Kind::Edit, "Import an MVR file: layers, fixtures with GDTFs and patch, truss, models", import);
    r.add("mvr.export", Kind::Query, "Write the document as an MVR 1.6 file", export);
    r.add("model.import", Kind::Edit, "Import a 3D model (.glb, .gltf, .obj, .3ds) as a model object", model_import);
}

fn read_input(
    path: &Option<PathBuf>,
    data: &Option<String>,
    name: &Option<String>,
    default: &str,
) -> Result<(String, Vec<u8>), CmdError> {
    match (path, data) {
        (Some(p), None) => {
            let bytes = std::fs::read(p).map_err(|e| CmdError::io(format!("{}: {e}", p.display())))?;
            let n = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| default.into());
            Ok((n, bytes))
        }
        (None, Some(b)) => {
            let bytes =
                crate::sheet::base64_decode(b).ok_or_else(|| CmdError::invalid_args("data_base64 is not base64"))?;
            Ok((name.clone().unwrap_or_else(|| default.into()), bytes))
        }
        _ => Err(CmdError::invalid_args("give either `path` or `data_base64`")),
    }
}

#[derive(Deserialize, JsonSchema)]
struct ImportArgs {
    /// Path to an `.mvr` file.
    path: Option<PathBuf>,
    /// The file's bytes, base64, instead of `path`.
    data_base64: Option<String>,
    /// MVR layer name → existing layer id or name to put its objects on. Other MVR
    /// layers go to a layer of the same name, created if missing (elevation from the
    /// MVR layer's z offset).
    #[serde(default)]
    layer_mapping: BTreeMap<String, String>,
}

fn import(ctx: &mut Ctx, a: ImportArgs) -> CmdResult {
    let (_, bytes) = read_input(&a.path, &a.data_base64, &None, "scene.mvr")?;
    crate::mvr::import(ctx, &bytes, &crate::mvr::ImportOptions { layer_mapping: a.layer_mapping })
}

#[derive(Deserialize, JsonSchema)]
struct ExportArgs {
    /// Where to write the `.mvr` file.
    path: PathBuf,
}

fn export(ctx: &mut Ctx, a: ExportArgs) -> CmdResult {
    let (bytes, mut summary) = crate::mvr::export(ctx.doc())?;
    std::fs::write(&a.path, &bytes).map_err(|e| CmdError::io(format!("{}: {e}", a.path.display())))?;
    summary["path"] = json!(a.path);
    summary["bytes"] = json!(bytes.len());
    Ok(summary)
}

#[derive(Deserialize, JsonSchema)]
struct ModelArgs {
    #[serde(flatten)]
    common: Common,
    /// Path to a `.glb`, `.gltf`, `.obj` or `.3ds` file. A `.gltf`'s external
    /// buffers are read from next to it and embedded.
    path: Option<PathBuf>,
    /// The file's bytes, base64, instead of `path` (needs `file_name` for the format).
    data_base64: Option<String>,
    /// File name with extension for `data_base64`.
    file_name: Option<String>,
    /// File units: `m`, `cm`, `mm`, `in`, or `auto` (default: glTF metres; OBJ and 3DS
    /// mm unless smaller than 50 units across, then metres).
    #[serde(default)]
    unit: Option<String>,
    /// Up axis of the file: `y` or `z`. Default: z for 3DS, y for OBJ; glTF is y-up by
    /// definition and always turned.
    #[serde(default)]
    up: Option<String>,
    /// Where the file's origin goes, mm. Default 0, 0, 0.
    #[serde(default)]
    position: P3,
    /// Degrees about x, y, z.
    #[serde(default)]
    rotation: P3,
}

/// A `.gltf` with its external buffers turned into data URIs, so the file stands alone.
fn inline_gltf(bytes: &[u8], dir: Option<&Path>) -> Result<Vec<u8>, CmdError> {
    let mut v: Value =
        serde_json::from_slice(bytes).map_err(|e| CmdError::invalid_args(format!("not a glTF JSON file: {e}")))?;
    if let Some(bufs) = v.get_mut("buffers").and_then(|b| b.as_array_mut()) {
        for b in bufs {
            let Some(uri) = b.get("uri").and_then(|u| u.as_str()).map(str::to_owned) else { continue };
            if uri.starts_with("data:") {
                continue;
            }
            let Some(dir) = dir else {
                return Err(CmdError::invalid_args(format!("buffer `{uri}` is a separate file; import from a path")));
            };
            let file = dir.join(uri.replace("%20", " "));
            let data = std::fs::read(&file).map_err(|e| CmdError::io(format!("{}: {e}", file.display())))?;
            b["uri"] = json!(format!("data:application/octet-stream;base64,{}", crate::sheet::base64_encode(&data)));
        }
    }
    serde_json::to_vec(&v).map_err(|e| CmdError::io(e.to_string()))
}

fn model_import(ctx: &mut Ctx, a: ModelArgs) -> CmdResult {
    let (name, mut bytes) = read_input(&a.path, &a.data_base64, &a.file_name, "model.glb")?;
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_owned(), e.to_ascii_lowercase()),
        None => (name.clone(), String::new()),
    };
    if !model_file::EXTENSIONS.contains(&ext.as_str()) {
        return Err(CmdError::invalid_args(format!("`{name}`: use a .glb, .gltf, .obj or .3ds file")));
    }
    if ext == "gltf" {
        bytes = inline_gltf(&bytes, a.path.as_deref().and_then(Path::parent))?;
    }
    let raw = model_file::read(&ext, &bytes).map_err(|e| CmdError::invalid_args(format!("`{name}`: {e}")))?;
    let scale = match a.unit.as_deref().unwrap_or("auto") {
        "auto" => model_file::guess_scale(&ext, &raw),
        "m" => 1000.0,
        "cm" => 10.0,
        "mm" => 1.0,
        "in" => 25.4,
        u => return Err(CmdError::invalid_args(format!("unknown unit `{u}`; use m, cm, mm, in or auto"))),
    };
    let y_up = match (a.up.as_deref(), ext.as_str()) {
        (_, "glb" | "gltf") => false,
        (Some("y"), _) => true,
        (Some("z"), _) => false,
        (None, e) => e == "obj",
        (Some(u), _) => return Err(CmdError::invalid_args(format!("up must be `y` or `z`, not `{u}`"))),
    };
    let asset = Asset::new(&name, &ext, bytes);
    let part = ModelPart { asset: asset.hash.clone(), matrix: ModelPart::IDENTITY, scale, y_up };
    let bounds = model_file::part_bounds(&raw, &part).unwrap_or([[0.0; 3]; 2]);
    ctx.doc_mut().assets.entry(asset.hash.clone()).or_insert(asset);
    let p = Placement { position: a.position, rotation: a.rotation };
    let mut common = a.common;
    if common.name.is_none() {
        common.name = Some(stem);
    }
    let mut out = insert(ctx, common, "Model", p, ObjectKind::Model { parts: vec![part], bounds })?;
    out["triangles"] = json!(raw.indices.len() / 3);
    out["size"] = json!([bounds[1][0] - bounds[0][0], bounds[1][1] - bounds[0][1], bounds[1][2] - bounds[0][2]]);
    out["scale"] = json!(scale);
    Ok(out)
}
