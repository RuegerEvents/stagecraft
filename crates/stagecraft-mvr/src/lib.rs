//! MVR (My Virtual Rig, DIN SPEC 15801) import and export.
//!
//! - [`read_mvr`] turns `.mvr` bytes into an [`Mvr`] (header, [`Scene`],
//!   [`Resources`]) plus [`Warning`]s. It fails only when there is no zip or no
//!   scene file; everything else is tolerated and reported.
//! - [`write_mvr`] writes an MVR 1.6 archive, deterministic byte for byte.
//! - [`Scene`] is a neutral tree of layers and [`Object`]s with local and
//!   resolved world [`Matrix`]es (mm; x right, y upstage, z up).
//!
//! GDTF files are carried as opaque bytes; parsing them is another crate's job.
//! See the README for conventions.

mod archive;
mod check;
pub mod glb;
mod matrix;
mod model;
mod read;
mod scene;
mod values;
mod warning;
mod write;
pub mod xml;

pub use archive::{
    Import, Mvr, MvrError, ResourceKind, Resources, SCENE_FILE, read_mvr, read_mvr_file, write_mvr, write_mvr_file,
    write_mvr_scene,
};
pub use check::check;
pub use matrix::Matrix;
pub use model::*;
pub use scene::{GeometryInstance, Stats};
pub use values::{fmt_uuid, parse_uuid_strict};
pub use warning::Warning;
pub use write::{PROVIDER, write_scene_xml};

/// Parse `GeneralSceneDescription.xml` text on its own (no archive, no file
/// checks). World matrices are resolved.
pub fn read_scene_xml(text: &str) -> (Header, Scene, Vec<Warning>) {
    let (h, mut s, w) = read::read_scene_xml(text);
    s.resolve_world();
    (h, s, w)
}

pub use uuid::Uuid;
