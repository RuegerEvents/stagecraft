//! GDTF (DIN SPEC 15800, GDTF 1.2) fixture types for StageCraft.
//!
//! [`Gdtf::from_bytes`] reads a `.gdtf` archive into a [`FixtureType`] plus the
//! archive's resources. Parsing is tolerant: only a missing or broken
//! `description.xml` fails; everything else lands in [`Gdtf::warnings`].
//!
//! - [`dmx::resolve_mode`] / [`Gdtf::resolve_mode`]: absolute channel offsets and the
//!   footprint per DMX break, with `GeometryReference` instancing.
//! - [`mesh`]: the fixture as triangles in GDTF axes, millimetres, one part per
//!   geometry instance, axes exposed for pan/tilt.
//! - [`silhouette`]: a top-view outline for plan symbols when the GDTF has no SVG.
//! - `share` feature: GDTF Share client with disk cache.
//!
//! Axes everywhere: x right, y away from the viewer (upstage when the fixture is
//! placed unrotated in MVR), z up. A GDTF fixture is drawn hanging, so its beams
//! point to −z. This is the MVR fixture-local frame.

mod archive;
pub mod dmx;
pub mod mesh;
pub mod models;
mod parse;
pub mod silhouette;
pub mod types;

#[cfg(feature = "share")]
pub mod share;

use std::fmt;

pub use archive::Archive;
pub use dmx::{PatchedChannel, ResolvedMode};
pub use mesh::{FixtureMesh, Lod, Mesh, MeshPart};
pub use silhouette::Silhouette;
pub use types::*;

/// Something odd in a GDTF that did not stop parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Where: `"Model Head"`, `"DMXMode Mode 1"`, `"archive"` …
    pub context: String,
    pub message: String,
}

impl Warning {
    pub fn new(context: impl Into<String>, message: impl Into<String>) -> Self {
        Self { context: context.into(), message: message.into() }
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.context, self.message)
    }
}

/// Why a GDTF could not be read at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GdtfError {
    Zip(String),
    NoDescription,
    Xml(String),
    NoFixtureType,
}

impl fmt::Display for GdtfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zip(e) => write!(f, "not a readable zip archive: {e}"),
            Self::NoDescription => write!(f, "archive has no description.xml"),
            Self::Xml(e) => write!(f, "description.xml is not well-formed: {e}"),
            Self::NoFixtureType => write!(f, "description.xml has no <FixtureType>"),
        }
    }
}

impl std::error::Error for GdtfError {}

/// A 2D view of a model in `models/svg*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgView {
    /// Looking down −z (`models/svg`).
    Top,
    /// Looking along −x (`models/svg_side`).
    Side,
    /// Looking along +y (`models/svg_front`).
    Front,
}

impl SvgView {
    pub fn folder(self) -> &'static str {
        match self {
            Self::Top => "models/svg",
            Self::Side => "models/svg_side",
            Self::Front => "models/svg_front",
        }
    }
}

/// A parsed GDTF archive.
#[derive(Debug, Clone)]
pub struct Gdtf {
    pub fixture: FixtureType,
    pub warnings: Vec<Warning>,
    pub archive: Archive,
}

impl Gdtf {
    /// Reads a `.gdtf` file's bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, GdtfError> {
        let mut warnings = Vec::new();
        let archive = Archive::read(bytes, &mut warnings)?;
        Self::from_archive(archive, warnings)
    }

    /// Parses an archive already in memory.
    pub fn from_archive(archive: Archive, mut warnings: Vec<Warning>) -> Result<Self, GdtfError> {
        let xml = match archive.get("description.xml") {
            Some(x) => x,
            None => {
                let nested = archive
                    .paths()
                    .find(|p| p.to_lowercase().ends_with("/description.xml"))
                    .ok_or(GdtfError::NoDescription)?;
                warnings.push(Warning::new("archive", format!("description.xml found at {nested}, not at the root")));
                archive.get(nested).ok_or(GdtfError::NoDescription)?
            }
        };
        let fixture = parse::parse_description(xml, &mut warnings)?;
        Ok(Self { fixture, warnings, archive })
    }

    /// `thumbnail.png` (or whatever `Thumbnail` names) at the archive root.
    pub fn thumbnail_png(&self) -> Option<&[u8]> {
        let name = self.fixture.thumbnail.as_deref().unwrap_or("thumbnail");
        self.archive.resource("", name, &["png"]).map(|(_, b)| b)
    }

    pub fn thumbnail_svg(&self) -> Option<&[u8]> {
        let name = self.fixture.thumbnail.as_deref().unwrap_or("thumbnail");
        self.archive.resource("", name, &["svg"]).map(|(_, b)| b)
    }

    /// Raw SVG of a model's 2D view, if the archive has one.
    pub fn model_svg(&self, model: &str, view: SvgView) -> Option<&[u8]> {
        let m = self.fixture.model(model)?;
        let file = m.file.as_deref()?;
        self.archive.resource(view.folder(), file, &["svg"]).map(|(_, b)| b)
    }

    /// Top-view SVGs of all models used by the mode's (or first) geometry tree, in
    /// tree order, with each geometry's transform in mm. Empty when there are none.
    pub fn plan_svgs(&self, mode: Option<&str>) -> Vec<(String, glam::Mat4, &[u8])> {
        let mut out = Vec::new();
        for inst in mesh::instances(&self.fixture, mode) {
            if let Some(model) = &inst.model
                && let Some(svg) = self.model_svg(model, SvgView::Top)
            {
                out.push((model.clone(), inst.world, svg));
            }
        }
        out
    }

    /// See [`dmx::resolve_mode`].
    pub fn resolve_mode(&self, mode: &str) -> Option<ResolvedMode> {
        let m = self.fixture.mode(mode)?;
        Some(dmx::resolve_mode(&self.fixture, m))
    }

    /// All modes resolved, in file order.
    pub fn resolve_modes(&self) -> Vec<ResolvedMode> {
        self.fixture.modes.iter().map(|m| dmx::resolve_mode(&self.fixture, m)).collect()
    }

    /// The fixture mesh for `mode` (its root geometry), or for the first mode.
    pub fn mesh(&self, mode: Option<&str>, lod: Lod) -> FixtureMesh {
        mesh::build(self, mode, lod)
    }
}
