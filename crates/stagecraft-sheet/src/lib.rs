//! Sheets: compose a sheet of the document into vector primitives in paper mm
//! ([`compose`]), and write them as PDF or SVG ([`render`]).
//!
//! - [`labels`]: fixture label layout in the three rotation modes and collision
//!   avoidance (pure; the plan canvas uses it too).
//! - [`view`]: the [`view::VectorViewSource`] hook for elevations and sections, with a
//!   mesh-outline fallback until `stagecraft-hlr` replaces it.
//! - [`install`] gives `stagecraft-core` the exporter behind `sheet.export_pdf` and
//!   `sheet.export_svg`; the app and the GUI call it at start.
//!
//! Builds for wasm32-unknown-unknown.

pub mod compose;
pub mod fonts;
pub mod labels;
pub mod prim;
pub mod render;
pub mod view;

use std::collections::HashMap;
use std::sync::Mutex;

use stagecraft_core::{Document, Id};

pub use compose::{Inputs, compose};
pub use prim::{ImageData, Page, Prim};
pub use view::{FallbackSource, VectorViewSource};

/// Images for rendered viewports, by viewport id. The GUI fills it when it renders a
/// 3D viewport; headless exports see it empty and draw placeholders.
static RASTERS: Mutex<Option<HashMap<Id, ImageData>>> = Mutex::new(None);

pub fn set_raster(viewport: Id, image: ImageData) {
    RASTERS.lock().expect("rasters").get_or_insert_with(HashMap::new).insert(viewport, image);
}

pub fn rasters() -> HashMap<Id, ImageData> {
    RASTERS.lock().expect("rasters").clone().unwrap_or_default()
}

/// The vector view source used for exports (the fallback until HLR lands).
pub fn default_source() -> &'static dyn VectorViewSource {
    &FallbackSource
}

/// Compose sheets with the default source and the raster cache.
pub fn pages(doc: &Document, sheets: &[Id]) -> Vec<Page> {
    let rasters = rasters();
    let inputs = Inputs { source: default_source(), rasters: &rasters };
    sheets.iter().filter_map(|id| compose(doc, *id, &inputs)).collect()
}

pub fn export_pdf(doc: &Document, sheets: &[Id]) -> Result<Vec<u8>, String> {
    let pages = pages(doc, sheets);
    if pages.is_empty() {
        return Err("no such sheet".into());
    }
    let title = if doc.meta.project.is_empty() { "StageCraft".to_owned() } else { doc.meta.project.clone() };
    render::pdf(&pages, &title)
}

pub fn export_svg(doc: &Document, sheet: Id) -> Result<String, String> {
    pages(doc, &[sheet]).first().map(render::svg).ok_or_else(|| "no such sheet".into())
}

struct Exporter;

impl stagecraft_core::sheet::SheetExporter for Exporter {
    fn pdf(&self, doc: &Document, sheets: &[Id]) -> Result<Vec<u8>, String> {
        export_pdf(doc, sheets)
    }
    fn svg(&self, doc: &Document, sheet: Id) -> Result<String, String> {
        export_svg(doc, sheet)
    }
}

/// Make `sheet.export_pdf` / `sheet.export_svg` work. Safe to call more than once.
pub fn install() {
    stagecraft_core::sheet::install_exporter(Box::new(Exporter));
}
