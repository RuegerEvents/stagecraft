//! The document's fixture type library: one entry per imported GDTF, with what
//! commands need without parsing the file again (modes, footprints, weight, size).
//! The GDTF itself is an [`Asset`]; geometry is built from it by `stagecraft-geom`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::asset::Asset;
use crate::command::CmdError;

/// Built-in plan symbols (SVG files in `crates/stagecraft-core/assets/symbols/`).
pub const SYMBOLS: [(&str, &str); 11] = [
    ("profile", include_str!("../assets/symbols/profile.svg")),
    ("fresnel", include_str!("../assets/symbols/fresnel.svg")),
    ("par", include_str!("../assets/symbols/par.svg")),
    ("mh_spot", include_str!("../assets/symbols/mh_spot.svg")),
    ("mh_wash", include_str!("../assets/symbols/mh_wash.svg")),
    ("led_wash", include_str!("../assets/symbols/led_wash.svg")),
    ("led_bar", include_str!("../assets/symbols/led_bar.svg")),
    ("beam", include_str!("../assets/symbols/beam.svg")),
    ("blinder", include_str!("../assets/symbols/blinder.svg")),
    ("strobe", include_str!("../assets/symbols/strobe.svg")),
    ("followspot", include_str!("../assets/symbols/followspot.svg")),
];

/// The SVG source of a built-in symbol.
pub fn symbol_svg(name: &str) -> Option<&'static str> {
    SYMBOLS.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// DMX slots one break of a mode takes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct BreakFootprint {
    /// GDTF DMX break number (1-based).
    pub dmx_break: u32,
    /// Highest slot used in that break.
    pub slots: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ModeInfo {
    pub name: String,
    /// One entry per break, in break order. A fixture gets one address per entry.
    pub footprint: Vec<BreakFootprint>,
    /// Number of DMX channels (after expanding geometry references).
    pub channels: usize,
}

impl ModeInfo {
    /// Slots summed over all breaks.
    pub fn total_slots(&self) -> u32 {
        self.footprint.iter().map(|b| b.slots).sum()
    }

    /// `27`, or `16+16` for two breaks.
    pub fn footprint_text(&self) -> String {
        self.footprint.iter().map(|b| b.slots.to_string()).collect::<Vec<_>>().join("+")
    }
}

/// One fixture type in the document library.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct FixtureType {
    /// The name fixtures refer to it by: `Manufacturer@Name` (GDTF Share's file
    /// naming), unique in the document.
    pub key: String,
    pub manufacturer: String,
    pub name: String,
    pub long_name: String,
    /// GDTF `FixtureTypeID` (a GUID).
    pub fixture_type_id: String,
    /// SHA-256 of the embedded GDTF ([`crate::Document::assets`]).
    pub asset: String,
    pub modes: Vec<ModeInfo>,
    /// From `<PhysicalDescriptions>`; `None` if the GDTF gives none.
    pub weight_kg: Option<f64>,
    pub power_w: Option<f64>,
    /// Size of the fixture model in the default pose (x, y, z), mm.
    pub size_mm: [f64; 3],
    /// True if the GDTF has top-view SVGs for its models.
    pub has_plan_svg: bool,
    /// Built-in symbol assigned by the user (`symbol.assign`). Wins over the GDTF's
    /// own top view and the silhouette.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// A built-in symbol that fits the type by its name and geometry; only a hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_symbol: Option<String>,
    /// Problems found while reading the GDTF, for the user.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl FixtureType {
    pub fn mode(&self, name: &str) -> Option<&ModeInfo> {
        self.modes.iter().find(|m| m.name == name)
    }

    /// The mode by name, or the first mode for `None`.
    pub fn mode_or_first(&self, name: Option<&str>) -> Option<&ModeInfo> {
        match name {
            Some(n) => self.mode(n),
            None => self.modes.first(),
        }
    }

    /// `Manufacturer Name`, as shown in lists.
    pub fn display_name(&self) -> String {
        if self.manufacturer.is_empty() { self.name.clone() } else { format!("{} {}", self.manufacturer, self.name) }
    }
}

/// `Manufacturer@Name`, the library key for a parsed GDTF.
pub fn key_for(manufacturer: &str, name: &str) -> String {
    let m = manufacturer.trim();
    let n = name.trim();
    if m.is_empty() { n.to_owned() } else { format!("{m}@{n}") }
}

/// Read a GDTF into a library entry and its asset. `key` overrides `Manufacturer@Name`.
pub fn read_gdtf(file_name: &str, bytes: Vec<u8>, key: Option<&str>) -> Result<(FixtureType, Asset), CmdError> {
    let g = stagecraft_gdtf::Gdtf::from_bytes(&bytes)
        .map_err(|e| CmdError::invalid(format!("{file_name} is not a readable GDTF: {e}")))?;
    let ft = &g.fixture;
    let modes = g
        .resolve_modes()
        .into_iter()
        .zip(&ft.modes)
        .map(|(r, m)| ModeInfo {
            name: m.name.clone(),
            footprint: r.footprint.iter().map(|(b, s)| BreakFootprint { dmx_break: *b, slots: *s }).collect(),
            channels: r.channels.len(),
        })
        .collect();
    let mesh = g.mesh(None, stagecraft_gdtf::Lod::Default);
    let size_mm = mesh
        .flatten()
        .bounds()
        .map(|(lo, hi)| {
            let d = hi - lo;
            [round3(d.x), round3(d.y), round3(d.z)]
        })
        .unwrap_or([0.0; 3]);
    let has_plan_svg = !g.plan_svgs(None).is_empty();
    let suggested_symbol = suggest_symbol(&g, &mesh).map(str::to_owned);
    let mut warnings: Vec<String> = g.warnings.iter().chain(&mesh.warnings).map(|w| w.to_string()).collect();
    warnings.truncate(50);
    let key = match key {
        Some(k) if !k.trim().is_empty() => k.trim().to_owned(),
        _ => key_for(&ft.manufacturer, &ft.name),
    };
    if key.is_empty() {
        return Err(CmdError::invalid(format!("{file_name} has no fixture type name; pass `key`")));
    }
    let asset = crate::asset::Asset::new(file_name, "gdtf", bytes);
    let t = FixtureType {
        key,
        manufacturer: ft.manufacturer.clone(),
        name: ft.name.clone(),
        long_name: ft.long_name.clone(),
        fixture_type_id: ft.fixture_type_id.clone(),
        asset: asset.hash.clone(),
        modes,
        weight_kg: ft.physical.weight_kg.map(round3),
        power_w: ft.power_w().map(round3),
        size_mm,
        has_plan_svg,
        symbol: None,
        suggested_symbol,
        warnings,
    };
    Ok((t, asset))
}

/// GDTF numbers are f32; keep three decimals so 4.2 stays 4.2.
fn round3(v: f32) -> f64 {
    (f64::from(v) * 1000.0).round() / 1000.0
}

/// A built-in symbol guessed from names, beam data and pan/tilt axes.
fn suggest_symbol(g: &stagecraft_gdtf::Gdtf, mesh: &stagecraft_gdtf::FixtureMesh) -> Option<&'static str> {
    let ft = &g.fixture;
    let text = format!("{} {} {} {}", ft.name, ft.long_name, ft.short_name, ft.description).to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| text.contains(w));
    let beam_type = mesh
        .beams()
        .first()
        .and_then(|&i| match &mesh.parts[i].instance.kind {
            stagecraft_gdtf::GeometryKind::Beam(b) => Some(b.beam_type.to_lowercase()),
            _ => None,
        })
        .unwrap_or_default();
    let beams = mesh.beams().len();
    let moving = mesh.axes().len() >= 2 || has(&["moving head", "movinghead"]);
    Some(if has(&["follow"]) {
        "followspot"
    } else if has(&["strobe", "atomic"]) {
        "strobe"
    } else if has(&["blinder", "mole", "audience"]) {
        "blinder"
    } else if moving {
        if has(&["beam"]) && !has(&["wash"]) {
            "beam"
        } else if has(&["wash", "aura"]) || beam_type == "wash" {
            "mh_wash"
        } else {
            "mh_spot"
        }
    } else if has(&["bar", "batten", "strip", "pixel", "linear"]) || beams >= 6 {
        "led_bar"
    } else if has(&["fresnel", "pc "]) || beam_type == "fresnel" || beam_type == "pc" {
        "fresnel"
    } else if has(&["profile", "source 4", "source four", "ellipsoidal", "leko", "zoom"]) {
        "profile"
    } else if has(&["par"]) {
        "par"
    } else if has(&["wash", "led"]) {
        "led_wash"
    } else {
        return None;
    })
}
