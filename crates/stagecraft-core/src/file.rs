//! `.stagecraft` files: a ZIP with `document.json` and the embedded assets under
//! `assets/<sha256>.<ext>` (GDTF fixture types; images later).
//!
//! Format 1 files have no assets and load as they are.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use zip::write::SimpleFileOptions;

use crate::asset::{Bytes, hash_hex};
use crate::command::CmdError;
use crate::model::{Document, FORMAT_VERSION};

const DOCUMENT_ENTRY: &str = "document.json";

pub fn save(doc: &Document, path: &Path) -> Result<(), CmdError> {
    let mut doc_out = doc.clone();
    doc_out.format = FORMAT_VERSION;
    let json = serde_json::to_vec_pretty(&doc_out).map_err(|e| CmdError::io(e.to_string()))?;

    // GDTFs are ZIPs already; compressing them again gains nothing.
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    write_atomic_with(path, |file| {
        let zerr = std::io::Error::other;
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(DOCUMENT_ENTRY, SimpleFileOptions::default()).map_err(zerr)?;
        zip.write_all(&json)?;
        for a in doc.assets.values() {
            zip.start_file(a.zip_path(), stored).map_err(zerr)?;
            zip.write_all(a.data.as_slice())?;
        }
        zip.finish().map_err(zerr)?;
        Ok(())
    })
}

/// Write `bytes` to `path` so that a crash, a full disk or a failed write never leaves half a
/// file: the old file stays until the new one is complete and flushed (craft-atomic: staged
/// beside the target, synced, renamed). Saves and exports go through here.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CmdError> {
    write_atomic_with(path, |f| f.write_all(bytes))
}

/// [`write_atomic`] with the content streamed into the staging file.
pub fn write_atomic_with(path: &Path, write: impl FnOnce(&mut File) -> std::io::Result<()>) -> Result<(), CmdError> {
    #[cfg(not(target_arch = "wasm32"))]
    let out = craft_atomic::atomic_write_with(path, write);
    // No files in the browser; keep the signature so callers need no cfg.
    #[cfg(target_arch = "wasm32")]
    let out = File::create(path).and_then(|mut f| write(&mut f));
    out.map_err(|e| CmdError::io(format!("{}: {e}", path.display())))
}

/// What a `.stagecraft` may hold. Every entry is read through craft-zip within these numbers,
/// whatever its headers declare.
const ZIP_LIMITS: craft_zip::Limits = craft_zip::Limits { max_entries: 100_000, max_total_bytes: 16 << 30 };
/// Largest `document.json`.
const MAX_DOCUMENT: u64 = 1 << 30;
/// Largest embedded asset.
const MAX_ASSET: u64 = 2 << 30;

pub fn load(path: &Path) -> Result<Document, CmdError> {
    let io = |e: std::io::Error| CmdError::io(format!("{}: {e}", path.display()));
    let zerr = |e: craft_zip::Error| CmdError::io(format!("{}: {e}", path.display()));
    let bytes = std::fs::read(path).map_err(io)?;
    let mut zip = craft_zip::Archive::open(&bytes, ZIP_LIMITS).map_err(zerr)?;
    if zip.has_duplicate_names() {
        return Err(CmdError::io(format!("{}: two entries share a name; refusing to guess", path.display())));
    }
    let buf = zip
        .read(DOCUMENT_ENTRY, MAX_DOCUMENT)
        .map_err(zerr)?
        .ok_or_else(|| CmdError::io(format!("{}: no {DOCUMENT_ENTRY} in the file", path.display())))?;
    let mut doc: Document =
        serde_json::from_slice(&buf).map_err(|e| CmdError::io(format!("{}: {e}", path.display())))?;
    drop(buf);
    if doc.format > FORMAT_VERSION {
        return Err(CmdError::io(format!(
            "{} was written by a newer StageCraft (format {}, this one reads up to {})",
            path.display(),
            doc.format,
            FORMAT_VERSION
        )));
    }
    let keys: Vec<String> = doc.assets.keys().cloned().collect();
    for k in keys {
        let a = doc.assets.get_mut(&k).expect("listed");
        let data = zip.read(&a.zip_path(), MAX_ASSET).map_err(zerr)?.ok_or_else(|| {
            CmdError::io(format!("{}: asset {} ({}) is missing", path.display(), a.file_name, a.hash))
        })?;
        if hash_hex(&data) != a.hash {
            return Err(CmdError::io(format!("{}: asset {} is damaged (hash mismatch)", path.display(), a.file_name)));
        }
        a.size = data.len() as u64;
        a.data = Bytes::new(data);
    }
    doc.format = FORMAT_VERSION;
    Ok(doc)
}
