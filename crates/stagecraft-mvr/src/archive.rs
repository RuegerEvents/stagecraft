//! The `.mvr` zip container: [`Resources`], [`read_mvr`] and [`write_mvr`].

use crate::model::{Header, Scene};
use crate::warning::Warning;
use crate::{check, read, write};
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

/// Name of the scene file inside the archive.
pub const SCENE_FILE: &str = "GeneralSceneDescription.xml";

/// What kind of file a resource is, from its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    /// `.gdtf` fixture type (itself a zip; kept as opaque bytes here).
    Gdtf,
    /// `.gdtt` GDTF template (`Universal.gdtt`).
    GdtfTemplate,
    /// `.3ds` (1 unit = 1 mm).
    Model3ds,
    /// `.glb` / `.gltf`.
    ModelGltf,
    /// `.png`, `.jpg`, `.jpeg`, `.svg` …
    Image,
    Other,
}

impl ResourceKind {
    pub fn of(name: &str) -> ResourceKind {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "gdtf" => ResourceKind::Gdtf,
            "gdtt" => ResourceKind::GdtfTemplate,
            "3ds" => ResourceKind::Model3ds,
            "glb" | "gltf" => ResourceKind::ModelGltf,
            "png" | "jpg" | "jpeg" | "svg" | "bmp" | "gif" | "tif" | "tiff" | "webp" => ResourceKind::Image,
            _ => ResourceKind::Other,
        }
    }
}

/// The files of an MVR archive besides the scene XML, by file name.
///
/// Sorted by name, so iteration and export are deterministic.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Resources {
    pub files: BTreeMap<String, Vec<u8>>,
}

impl Resources {
    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.files.get(name).map(Vec::as_slice)
    }

    pub fn insert(&mut self, name: impl Into<String>, bytes: Vec<u8>) {
        self.files.insert(name.into(), bytes);
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// File names of one kind.
    pub fn names_of(&self, kind: ResourceKind) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str).filter(move |n| ResourceKind::of(n) == kind)
    }

    pub fn count_of(&self, kind: ResourceKind) -> usize {
        self.names_of(kind).count()
    }

    /// Find the file a `GDTFSpec` refers to. Exporters differ: some write the
    /// name without `.gdtf`, some change case. Tries exact, `+ ".gdtf"`, then
    /// case-insensitive matches of both.
    pub fn resolve_gdtf(&self, spec: &str) -> Option<&str> {
        let spec = spec.trim();
        let with_ext = format!("{spec}.gdtf");
        for cand in [spec, with_ext.as_str()] {
            if let Some((k, _)) = self.files.get_key_value(cand) {
                return Some(k);
            }
        }
        self.files
            .keys()
            .find(|k| k.eq_ignore_ascii_case(spec) || k.eq_ignore_ascii_case(&with_ext))
            .map(String::as_str)
    }

    /// Find any referenced file: exact, then case-insensitive.
    pub fn resolve(&self, name: &str) -> Option<&str> {
        if let Some((k, _)) = self.files.get_key_value(name) {
            return Some(k);
        }
        self.files.keys().find(|k| k.eq_ignore_ascii_case(name)).map(String::as_str)
    }
}

/// An MVR file in memory: header, scene and the other files of the archive.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mvr {
    /// As read from the file. Export writes its own header.
    pub header: Header,
    pub scene: Scene,
    pub resources: Resources,
}

/// Result of an import: the file, and everything that was odd about it.
#[derive(Debug, Clone)]
pub struct Import {
    pub mvr: Mvr,
    pub warnings: Vec<Warning>,
}

/// Fatal import errors. Everything else is a [`Warning`].
#[derive(Debug, thiserror::Error)]
pub enum MvrError {
    #[error("not a zip archive: {0}")]
    NotZip(String),
    #[error("archive has no {SCENE_FILE}")]
    NoSceneFile,
    #[error("zip error: {0}")]
    Zip(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Read an `.mvr` archive from bytes. Fails only when the bytes are not a zip or
/// hold no scene file; the scene XML itself is read tolerantly.
pub fn read_mvr(bytes: &[u8]) -> Result<Import, MvrError> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|e| MvrError::NotZip(e.to_string()))?;
    let mut warnings = Vec::new();
    let mut scene_xml: Option<Vec<u8>> = None;
    let mut resources = Resources::default();

    for i in 0..zip.len() {
        let mut f = match zip.by_index(i) {
            Ok(f) => f,
            Err(e) => {
                warnings.push(Warning::new("archive", format!("entry {i} unreadable: {e}")));
                continue;
            }
        };
        if f.is_dir() {
            continue;
        }
        // Vectorworks writes UTF-8 names without the zip UTF-8 flag, so the zip
        // crate decodes them as CP437. Prefer the raw bytes when they are UTF-8.
        let name = match std::str::from_utf8(f.name_raw()) {
            Ok(s) => s.to_string(),
            Err(_) => f.name().map(|n| n.into_owned()).unwrap_or_else(|_| format!("entry-{i}")),
        };
        let mut data = Vec::with_capacity(f.size() as usize);
        if let Err(e) = f.read_to_end(&mut data) {
            warnings.push(Warning::new("archive", format!("{name:?} unreadable: {e}")));
            continue;
        }
        drop(f);
        // Spec: files live at the root. Tolerate folders, but match the scene
        // file by its base name.
        let base = name.rsplit('/').next().unwrap_or(&name);
        if base == SCENE_FILE && (scene_xml.is_none() || name == SCENE_FILE) {
            if name != SCENE_FILE {
                warnings.push(Warning::new("archive", format!("scene file in a folder: {name:?}")));
            }
            scene_xml = Some(data);
            continue;
        }
        if name.contains('/') {
            warnings.push(Warning::new("archive", format!("{name:?} is in a folder; spec wants files at the root")));
        }
        if resources.files.insert(name.clone(), data).is_some() {
            warnings.push(Warning::new("archive", format!("duplicate entry {name:?}; last one kept")));
        }
    }
    let xml_bytes = scene_xml.ok_or(MvrError::NoSceneFile)?;
    let text = decode_xml(&xml_bytes, &mut warnings);
    let (header, mut scene, w) = read::read_scene_xml(&text);
    warnings.extend(w);
    scene.resolve_world();
    warnings.extend(check::check(&scene, &resources));
    let warnings = crate::warning::collapse(warnings, 3);
    Ok(Import { mvr: Mvr { header, scene, resources }, warnings })
}

/// Read an `.mvr` file from disk.
pub fn read_mvr_file(path: impl AsRef<std::path::Path>) -> Result<Import, MvrError> {
    read_mvr(&std::fs::read(path)?)
}

fn decode_xml(bytes: &[u8], warnings: &mut Vec<Warning>) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if bytes.starts_with(b"\xFF\xFE") || bytes.starts_with(b"\xFE\xFF") {
        let le = bytes[0] == 0xFF;
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) })
            .collect();
        warnings.push(Warning::new(SCENE_FILE, "UTF-16 encoded; spec requires UTF-8"));
        return String::from_utf16_lossy(&units);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            warnings.push(Warning::new(SCENE_FILE, "not valid UTF-8; bad bytes replaced"));
            String::from_utf8_lossy(bytes).into_owned()
        }
    }
}

/// Write an `.mvr` archive. Deterministic: entries in name order (scene file
/// first), fixed timestamps, JPEG/PNG stored, the rest deflated.
pub fn write_mvr(mvr: &Mvr) -> Result<Vec<u8>, MvrError> {
    write_mvr_scene(&mvr.scene, &mvr.resources)
}

/// [`write_mvr`] from parts.
pub fn write_mvr_scene(scene: &Scene, resources: &Resources) -> Result<Vec<u8>, MvrError> {
    let xml = write::write_scene_xml(scene);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let zerr = |e: zip::result::ZipError| MvrError::Zip(e.to_string());
    let opts = |method| {
        SimpleFileOptions::default()
            .compression_method(method)
            .last_modified_time(DateTime::default())
            .unix_permissions(0o644)
    };
    zip.start_file(SCENE_FILE, opts(CompressionMethod::Deflated)).map_err(zerr)?;
    zip.write_all(xml.as_bytes())?;
    for (name, data) in &resources.files {
        let method = match ResourceKind::of(name) {
            // JPEG and PNG are compressed already. GDTFs are zips too, but often
            // with stored entries, so deflate does pay off there.
            ResourceKind::Image if is_compressed_image(name) => CompressionMethod::Stored,
            _ => CompressionMethod::Deflated,
        };
        zip.start_file(name.as_str(), opts(method)).map_err(zerr)?;
        zip.write_all(data)?;
    }
    let cursor = zip.finish().map_err(zerr)?;
    Ok(cursor.into_inner())
}

fn is_compressed_image(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".webp", ".gif"].iter().any(|e| n.ends_with(e))
}

/// Write an `.mvr` file to disk.
pub fn write_mvr_file(mvr: &Mvr, path: impl AsRef<std::path::Path>) -> Result<(), MvrError> {
    std::fs::write(path, write_mvr(mvr)?)?;
    Ok(())
}
