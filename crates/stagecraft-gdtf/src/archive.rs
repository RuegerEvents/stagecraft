//! The files of a GDTF zip, held in memory.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use crate::{GdtfError, Warning};

/// Largest entry read (decompressed).
const MAX_ENTRY: u64 = 256 << 20;

/// All entries of the archive by path (as stored, `/` separated, no leading `./`).
#[derive(Debug, Clone, Default)]
pub struct Archive {
    files: BTreeMap<String, Vec<u8>>,
}

impl Archive {
    /// Reads every file entry. Entries that fail to decompress become warnings.
    pub fn read(bytes: &[u8], warnings: &mut Vec<Warning>) -> Result<Self, GdtfError> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| GdtfError::Zip(e.to_string()))?;
        let mut files = BTreeMap::new();
        for i in 0..zip.len() {
            let mut entry = match zip.by_index(i) {
                Ok(e) => e,
                Err(e) => {
                    warnings.push(Warning::new("archive", format!("entry {i}: {e}")));
                    continue;
                }
            };
            if entry.is_dir() {
                continue;
            }
            let name = match entry.name() {
                Ok(n) => normalize(&n),
                Err(_) => normalize(&String::from_utf8_lossy(entry.name_raw())),
            };
            if entry.size() > MAX_ENTRY {
                warnings.push(Warning::new("archive", format!("{name}: larger than 256 MB; skipped")));
                continue;
            }
            let mut data = Vec::with_capacity(entry.size() as usize);
            // The header size can lie; never inflate more than the cap.
            if let Err(e) = (&mut entry).take(MAX_ENTRY + 1).read_to_end(&mut data) {
                warnings.push(Warning::new("archive", format!("{name}: {e}")));
                continue;
            }
            if data.len() as u64 > MAX_ENTRY {
                warnings.push(Warning::new("archive", format!("{name}: larger than 256 MB; skipped")));
                continue;
            }
            files.insert(name, data);
        }
        Ok(Self { files })
    }

    /// Builds an archive from path/bytes pairs (tests, or callers that unpacked already).
    pub fn from_files(files: impl IntoIterator<Item = (String, Vec<u8>)>) -> Self {
        Self { files: files.into_iter().map(|(k, v)| (normalize(&k), v)).collect() }
    }

    /// Exact path, then case-insensitive.
    pub fn get(&self, path: &str) -> Option<&[u8]> {
        let path = normalize(path);
        if let Some(v) = self.files.get(&path) {
            return Some(v);
        }
        let lower = path.to_lowercase();
        self.files.iter().find(|(k, _)| k.to_lowercase() == lower).map(|(_, v)| v.as_slice())
    }

    /// A resource by folder and name without extension, trying the extensions in
    /// order. Matching is case-insensitive; a name that already carries one of the
    /// extensions is accepted too.
    pub fn resource(&self, folder: &str, name: &str, extensions: &[&str]) -> Option<(&str, &[u8])> {
        let folder = folder.trim_matches('/').to_lowercase();
        let name = name.trim().to_lowercase();
        for ext in extensions {
            let ext = ext.to_lowercase();
            let stem = name.strip_suffix(&format!(".{ext}")).unwrap_or(&name);
            let want = if folder.is_empty() { format!("{stem}.{ext}") } else { format!("{folder}/{stem}.{ext}") };
            if let Some((k, v)) = self.files.iter().find(|(k, _)| k.to_lowercase() == want) {
                return Some((k.as_str(), v.as_slice()));
            }
        }
        None
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

fn normalize(p: &str) -> String {
    let p = p.replace('\\', "/");
    let p = p.trim_start_matches("./").trim_start_matches('/');
    p.to_string()
}
