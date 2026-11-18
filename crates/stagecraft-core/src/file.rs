//! `.stagecraft` files: a ZIP with `document.json` and the embedded assets under
//! `assets/<sha256>.<ext>` (GDTF fixture types; images later).
//!
//! Format 1 files have no assets and load as they are.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;

use crate::asset::{Bytes, hash_hex};
use crate::command::CmdError;
use crate::model::{Document, FORMAT_VERSION};

const DOCUMENT_ENTRY: &str = "document.json";

pub fn save(doc: &Document, path: &Path) -> Result<(), CmdError> {
    let io = |e: std::io::Error| CmdError::io(format!("{}: {e}", path.display()));
    let zerr = |e: zip::result::ZipError| CmdError::io(format!("{}: {e}", path.display()));
    let mut doc_out = doc.clone();
    doc_out.format = FORMAT_VERSION;
    let json = serde_json::to_vec_pretty(&doc_out).map_err(|e| CmdError::io(e.to_string()))?;

    // Write next to the target, then rename, so a crash never leaves half a file.
    let tmp = path.with_extension("stagecraft.tmp");
    {
        let file = File::create(&tmp).map_err(io)?;
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(DOCUMENT_ENTRY, SimpleFileOptions::default()).map_err(zerr)?;
        zip.write_all(&json).map_err(io)?;
        // GDTFs are ZIPs already; compressing them again gains nothing.
        let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for a in doc.assets.values() {
            zip.start_file(a.zip_path(), stored).map_err(zerr)?;
            zip.write_all(a.data.as_slice()).map_err(io)?;
        }
        zip.finish().map_err(zerr)?;
    }
    std::fs::rename(&tmp, path).map_err(io)
}

pub fn load(path: &Path) -> Result<Document, CmdError> {
    let io = |e: std::io::Error| CmdError::io(format!("{}: {e}", path.display()));
    let zerr = |e: zip::result::ZipError| CmdError::io(format!("{}: {e}", path.display()));
    let file = File::open(path).map_err(io)?;
    let mut zip = zip::ZipArchive::new(file).map_err(zerr)?;
    let mut buf = Vec::new();
    zip.by_name(DOCUMENT_ENTRY).map_err(zerr)?.read_to_end(&mut buf).map_err(io)?;
    let mut doc: Document =
        serde_json::from_slice(&buf).map_err(|e| CmdError::io(format!("{}: {e}", path.display())))?;
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
        let mut data = Vec::new();
        zip.by_name(&a.zip_path())
            .map_err(|_| CmdError::io(format!("{}: asset {} ({}) is missing", path.display(), a.file_name, a.hash)))?
            .read_to_end(&mut data)
            .map_err(io)?;
        if hash_hex(&data) != a.hash {
            return Err(CmdError::io(format!("{}: asset {} is damaged (hash mismatch)", path.display(), a.file_name)));
        }
        a.size = data.len() as u64;
        a.data = Bytes::new(data);
    }
    doc.format = FORMAT_VERSION;
    Ok(doc)
}
