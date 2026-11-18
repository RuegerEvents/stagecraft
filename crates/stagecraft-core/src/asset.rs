//! Files embedded in a document (GDTF fixture types now, images later).
//!
//! The bytes are shared (`Arc`), so cloning a document for undo never copies them.
//! `document.json` lists each asset; the bytes live in the `.stagecraft` ZIP at
//! `assets/<hash>.<ext>`, keyed by the SHA-256 of their content.

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Shared, immutable bytes. Equal if they are the same allocation or hold the same
/// bytes.
#[derive(Clone, Default)]
pub struct Bytes(pub Arc<[u8]>);

impl Bytes {
    pub fn new(data: Vec<u8>) -> Self {
        Self(data.into())
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// True if both point at the same allocation (an undo copy of the same asset).
    pub fn shares(&self, other: &Bytes) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl PartialEq for Bytes {
    fn eq(&self, other: &Self) -> bool {
        self.shares(other) || self.0 == other.0
    }
}

impl fmt::Debug for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bytes({})", self.0.len())
    }
}

/// One embedded file.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Asset {
    /// SHA-256 of the bytes, lowercase hex. The key in [`crate::Document::assets`].
    pub hash: String,
    /// The file name it was imported from (`Robe Lighting@Robin MegaPointe.gdtf`).
    pub file_name: String,
    /// File extension without the dot: `gdtf`, later `png`, `svg` …
    pub ext: String,
    pub size: u64,
    /// The content; stored next to `document.json`, not inside it.
    #[serde(skip)]
    pub data: Bytes,
}

impl Asset {
    pub fn new(file_name: &str, ext: &str, data: Vec<u8>) -> Self {
        Self {
            hash: hash_hex(&data),
            file_name: file_name.to_owned(),
            ext: ext.to_owned(),
            size: data.len() as u64,
            data: Bytes::new(data),
        }
    }

    /// Path of the bytes inside the `.stagecraft` ZIP.
    pub fn zip_path(&self) -> String {
        format!("assets/{}.{}", self.hash, self.ext)
    }
}

/// SHA-256 as lowercase hex.
pub fn hash_hex(data: &[u8]) -> String {
    let d = Sha256::digest(data);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable() {
        assert_eq!(hash_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let a = Asset::new("x.gdtf", "gdtf", b"abc".to_vec());
        assert_eq!(a.zip_path(), format!("assets/{}.gdtf", a.hash));
        let b = a.clone();
        assert!(a.data.shares(&b.data));
        assert_eq!(a, b);
    }
}
