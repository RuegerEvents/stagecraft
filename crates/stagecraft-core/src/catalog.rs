//! Catalogues: truss profiles, platform systems, supports. The built-in ones are JSON
//! files embedded in the crate (`catalog/*.json`), so nothing is read at run time and
//! the crate stays wasm-clean. Users extend them with JSON in the same shape through
//! [`extend`] (the app loads every `*.json` from `<data dir>/StageCraft/catalog/`, see
//! [`load_dir`]); an entry with the name of a built-in one replaces it.

use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

use crate::model::{SupportStyle, Understructure};

/// A truss cross-section. Width and height are outer dimensions, mm.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrussProfile {
    pub name: String,
    #[serde(default)]
    pub maker: String,
    /// 1 (single tube), 2 (ladder), 3 (triangle) or 4 (square).
    pub chords: u8,
    pub width: f64,
    pub height: f64,
    /// Chord tube outer diameter, mm.
    pub tube: f64,
    pub kg_per_m: f64,
    /// Weight of one corner or junction block, kg. Default: a profile-wide cube's
    /// worth of truss.
    #[serde(default)]
    pub node_kg: Option<f64>,
    /// Piece lengths a run is split into, longest first, mm.
    #[serde(default = "stock_lengths")]
    pub stock_lengths: Vec<f64>,
}

fn stock_lengths() -> Vec<f64> {
    vec![3000.0, 2500.0, 2000.0, 1500.0, 1000.0, 500.0]
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
            node_kg: None,
            stock_lengths: stock_lengths(),
        }
    }

    /// How far a corner or junction reaches from its centre along each arm, mm: one
    /// profile width.
    pub fn node_reach(&self) -> f64 {
        self.width
    }

    pub fn node_weight(&self) -> f64 {
        self.node_kg.unwrap_or(self.kg_per_m * self.width * 3.0 / 1000.0)
    }
}

/// A stage platform system (Bütec, Nivoflex …).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlatformSystem {
    pub name: String,
    #[serde(default)]
    pub maker: String,
    pub understructure: Understructure,
    /// Deck footprints, width × depth, mm.
    pub sizes: Vec<[f64; 2]>,
    /// Deck thickness, mm.
    pub deck: f64,
    pub min_height: f64,
    pub max_height: f64,
    pub kg_per_m2: f64,
}

/// A base plate, stand or ground support base.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SupportPreset {
    pub name: String,
    pub style: SupportStyle,
    pub size: f64,
    pub height: f64,
    pub weight_kg: f64,
}

/// Everything known, built-in first, user entries merged in.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub profiles: Vec<TrussProfile>,
    #[serde(default)]
    pub systems: Vec<PlatformSystem>,
    #[serde(default)]
    pub supports: Vec<SupportPreset>,
}

impl Catalog {
    fn builtin() -> Self {
        let mut c = Catalog::default();
        for text in [
            include_str!("../catalog/truss_profiles.json"),
            include_str!("../catalog/platform_systems.json"),
            include_str!("../catalog/supports.json"),
        ] {
            c.merge(serde_json::from_str(text).expect("built-in catalogue is valid"));
        }
        c
    }

    /// Add `other`'s entries; same names (case-insensitive) replace.
    pub fn merge(&mut self, other: Catalog) {
        fn put<T>(v: &mut Vec<T>, x: T, name: impl Fn(&T) -> &str) {
            match v.iter().position(|y| name(y).eq_ignore_ascii_case(name(&x))) {
                Some(i) => v[i] = x,
                None => v.push(x),
            }
        }
        for p in other.profiles {
            put(&mut self.profiles, p, |p| &p.name);
        }
        for s in other.systems {
            put(&mut self.systems, s, |s| &s.name);
        }
        for s in other.supports {
            put(&mut self.supports, s, |s| &s.name);
        }
    }
}

fn store() -> &'static RwLock<Catalog> {
    static C: OnceLock<RwLock<Catalog>> = OnceLock::new();
    C.get_or_init(|| RwLock::new(Catalog::builtin()))
}

/// A copy of the current catalogue.
pub fn current() -> Catalog {
    store().read().expect("catalogue lock").clone()
}

/// Merge user entries from JSON text with any of the keys `profiles`, `systems`,
/// `supports` (same shape as the built-in files).
pub fn extend(json: &str) -> Result<(), String> {
    let c: Catalog = serde_json::from_str(json).map_err(|e| e.to_string())?;
    store().write().expect("catalogue lock").merge(c);
    Ok(())
}

/// Merge every `*.json` in `dir`. Missing directory = nothing to do. Returns the files
/// that failed with their errors.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_dir(dir: &std::path::Path) -> Vec<(std::path::PathBuf, String)> {
    let mut errors = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return errors };
    let mut paths: Vec<_> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths.into_iter().filter(|p| p.extension().is_some_and(|e| e == "json")) {
        let r = std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| extend(&t));
        if let Err(e) = r {
            errors.push((p, e));
        }
    }
    errors
}

/// Every truss profile.
pub fn truss_profiles() -> Vec<TrussProfile> {
    store().read().expect("catalogue lock").profiles.clone()
}

/// A profile by name, case-insensitive.
pub fn truss_profile(name: &str) -> Option<TrussProfile> {
    store().read().expect("catalogue lock").profiles.iter().find(|p| p.name.eq_ignore_ascii_case(name)).cloned()
}

/// The profile, or an F34-sized fallback for unknown names.
pub fn truss_profile_or_fallback(name: &str) -> TrussProfile {
    truss_profile(name).unwrap_or_else(|| TrussProfile::fallback(name))
}

pub fn platform_systems() -> Vec<PlatformSystem> {
    store().read().expect("catalogue lock").systems.clone()
}

pub fn platform_system(name: &str) -> Option<PlatformSystem> {
    store().read().expect("catalogue lock").systems.iter().find(|s| s.name.eq_ignore_ascii_case(name)).cloned()
}

pub fn supports() -> Vec<SupportPreset> {
    store().read().expect("catalogue lock").supports.clone()
}

pub fn support(name: &str) -> Option<SupportPreset> {
    store().read().expect("catalogue lock").supports.iter().find(|s| s.name.eq_ignore_ascii_case(name)).cloned()
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
            assert!(!p.stock_lengths.is_empty());
        }
        for name in ["Bütec", "Nivoflex"] {
            assert!(platform_system(name).is_some(), "{name} missing");
        }
        assert_eq!(platform_system("nivoflex").unwrap().understructure, Understructure::Scissor);
        assert!(support("Base plate 600").is_some());
    }

    #[test]
    fn user_entries_extend_and_replace() {
        extend(r#"{ "profiles": [{ "name": "Test99", "chords": 4, "width": 500, "height": 500, "tube": 60, "kg_per_m": 12 }] }"#)
            .unwrap();
        let p = truss_profile("test99").unwrap();
        assert_eq!(p.width, 500.0);
        assert_eq!(p.stock_lengths[0], 3000.0);
        assert!(extend("{ not json").is_err());
    }
}
