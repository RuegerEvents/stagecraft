//! The files of a GDTF zip, held in memory.

use std::collections::BTreeMap;

use crate::{GdtfError, Warning};

/// Largest entry read (decompressed).
pub const MAX_ENTRY: u64 = 256 << 20;
/// Most bytes all entries of one GDTF may inflate to.
pub const MAX_TOTAL: u64 = 1 << 30;
/// Most entries one GDTF may declare.
pub const MAX_ENTRIES: u64 = 20_000;

/// All entries of the archive by path (as stored, `/` separated, no leading `./`).
#[derive(Debug, Clone, Default)]
pub struct Archive {
    files: BTreeMap<String, Vec<u8>>,
}

impl Archive {
    /// Reads every file entry with craft-zip: the entry count is checked before indexing, and
    /// no entry inflates past [`MAX_ENTRY`] or the archive past [`MAX_TOTAL`], whatever its
    /// headers say. Entries that fail to decompress or are too large become warnings.
    pub fn read(bytes: &[u8], warnings: &mut Vec<Warning>) -> Result<Self, GdtfError> {
        let limits = craft_zip::Limits { max_entries: MAX_ENTRIES, max_total_bytes: MAX_TOTAL };
        let mut zip = craft_zip::Archive::open(bytes, limits).map_err(|e| GdtfError::Zip(e.to_string()))?;
        if zip.has_duplicate_names() {
            warnings.push(Warning::new("archive", "two entries share a name; one of each kept"));
        }
        let names: Vec<String> = zip.names().map(|n| n.into_owned()).collect();
        let mut files = BTreeMap::new();
        for raw in names {
            if raw.ends_with('/') {
                continue;
            }
            let name = normalize(&raw);
            match zip.read(&raw, MAX_ENTRY) {
                Ok(Some(data)) => {
                    files.insert(name, data);
                }
                Ok(None) => {}
                Err(e @ craft_zip::Error::BudgetSpent { .. }) => return Err(GdtfError::Zip(e.to_string())),
                Err(craft_zip::Error::EntryTooLarge { .. }) => {
                    warnings.push(Warning::new("archive", format!("{name}: larger than 256 MB; skipped")))
                }
                Err(e) => warnings.push(Warning::new("archive", format!("{name}: {e}"))),
            }
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
