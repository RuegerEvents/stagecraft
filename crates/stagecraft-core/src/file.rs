//! `.stagecraft` files: a ZIP with `document.json` (assets follow in later blocks).

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;

use crate::command::CmdError;
use crate::model::{Document, FORMAT_VERSION};

const DOCUMENT_ENTRY: &str = "document.json";

pub fn save(doc: &Document, path: &Path) -> Result<(), CmdError> {
    let io = |e: std::io::Error| CmdError::io(format!("{}: {e}", path.display()));
    let zerr = |e: zip::result::ZipError| CmdError::io(format!("{}: {e}", path.display()));
    let json = serde_json::to_vec_pretty(doc).map_err(|e| CmdError::io(e.to_string()))?;

    // Write next to the target, then rename, so a crash never leaves half a file.
    let tmp = path.with_extension("stagecraft.tmp");
    {
        let file = File::create(&tmp).map_err(io)?;
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(DOCUMENT_ENTRY, SimpleFileOptions::default()).map_err(zerr)?;
        zip.write_all(&json).map_err(io)?;
        zip.finish().map_err(zerr)?;
    }
    std::fs::rename(&tmp, path).map_err(io)
}

pub fn load(path: &Path) -> Result<Document, CmdError> {
    let io = |e: std::io::Error| CmdError::io(format!("{}: {e}", path.display()));
    let zerr = |e: zip::result::ZipError| CmdError::io(format!("{}: {e}", path.display()));
    let file = File::open(path).map_err(io)?;
    let mut zip = zip::ZipArchive::new(file).map_err(zerr)?;
    let mut entry = zip.by_name(DOCUMENT_ENTRY).map_err(zerr)?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).map_err(io)?;
    let doc: Document = serde_json::from_slice(&buf).map_err(|e| CmdError::io(format!("{}: {e}", path.display())))?;
    if doc.format > FORMAT_VERSION {
        return Err(CmdError::io(format!(
            "{} was written by a newer StageCraft (format {}, this one reads up to {})",
            path.display(),
            doc.format,
            FORMAT_VERSION
        )));
    }
    Ok(doc)
}
