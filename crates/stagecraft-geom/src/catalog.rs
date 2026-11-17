//! Built-in catalogues, embedded as JSON so the crate needs no files at run time.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// A truss cross-section. Width and height are outer dimensions, mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrussProfile {
    pub name: String,
    pub maker: String,
    /// 1 (single tube), 2 (ladder), 3 (triangle) or 4 (square).
    pub chords: u8,
    pub width: f64,
    pub height: f64,
    /// Chord tube outer diameter, mm.
    pub tube: f64,
    pub kg_per_m: f64,
}

impl TrussProfile {
    /// Used for names the catalogue does not know: an F34-sized box, so the plan still
    /// shows something sensible.
    pub fn fallback(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            maker: String::new(),
            chords: 4,
            width: 290.0,
            height: 290.0,
            tube: 50.0,
            kg_per_m: 0.0,
        }
    }
}

#[derive(Deserialize)]
struct File {
    profiles: Vec<TrussProfile>,
}

/// Every built-in truss profile.
pub fn truss_profiles() -> &'static [TrussProfile] {
    static P: OnceLock<Vec<TrussProfile>> = OnceLock::new();
    P.get_or_init(|| {
        let f: File =
            serde_json::from_str(include_str!("../catalog/truss_profiles.json")).expect("truss catalogue is valid");
        f.profiles
    })
}

/// A profile by name, case-insensitive.
pub fn truss_profile(name: &str) -> Option<&'static TrussProfile> {
    truss_profiles().iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_has_the_presets_from_the_spec() {
        for name in ["F34", "F44P", "H30V", "X30D"] {
            assert!(truss_profile(name).is_some(), "{name} missing");
        }
        assert_eq!(truss_profile("f34").unwrap().width, 290.0);
        assert_eq!(truss_profile("X30D").unwrap().chords, 3);
        for p in truss_profiles() {
            assert!(p.width > 0.0 && p.height > 0.0 && p.tube > 0.0 && p.tube <= p.width, "{}", p.name);
        }
    }
}
