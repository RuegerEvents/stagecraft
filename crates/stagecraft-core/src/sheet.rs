//! Sheets: paper with viewports, a title block and labels. Plain data; drawing them is
//! `stagecraft-sheet`'s job.
//!
//! Paper coordinates are mm from the sheet's lower left corner, y up. Viewport crops
//! are in the view's own model mm (plan: world x/y; elevations and sections: the view
//! plane's u right, v up).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::{Id, P2, P3};

/// A drawing sheet.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Sheet {
    pub id: Id,
    pub name: String,
    /// Sheet number as printed in the title block, e.g. `L-101`.
    pub number: String,
    pub paper: Paper,
    pub orientation: Orientation,
    /// Title block template; `None` = the built-in generic one.
    #[serde(default)]
    pub title_block: Option<Id>,
    #[serde(default = "yes")]
    pub show_title_block: bool,
    /// A legend of fixture types with counts above the title block.
    #[serde(default = "yes")]
    pub show_legend: bool,
    /// Position in the sheet list.
    pub order: i32,
    /// Back to front.
    pub viewports: Vec<Viewport>,
}

fn yes() -> bool {
    true
}

impl Sheet {
    /// Paper size in mm as oriented.
    pub fn size(&self) -> P2 {
        let [a, b] = self.paper.portrait_size();
        match self.orientation {
            Orientation::Portrait => [a.min(b), a.max(b)],
            Orientation::Landscape => [a.max(b), a.min(b)],
        }
    }

    pub fn viewport(&self, id: Id) -> Option<&Viewport> {
        self.viewports.iter().find(|v| v.id == id)
    }
}

/// ISO 216 sizes or a custom one.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "size")]
pub enum Paper {
    A4,
    A3,
    A2,
    A1,
    A0,
    /// Any size, mm.
    Custom {
        width: f64,
        height: f64,
    },
}

impl Paper {
    /// Width × height in portrait, mm.
    pub fn portrait_size(&self) -> P2 {
        match self {
            Paper::A4 => [210.0, 297.0],
            Paper::A3 => [297.0, 420.0],
            Paper::A2 => [420.0, 594.0],
            Paper::A1 => [594.0, 841.0],
            Paper::A0 => [841.0, 1189.0],
            Paper::Custom { width, height } => [*width, *height],
        }
    }

    pub fn name(&self) -> String {
        match self {
            Paper::A4 => "A4".into(),
            Paper::A3 => "A3".into(),
            Paper::A2 => "A2".into(),
            Paper::A1 => "A1".into(),
            Paper::A0 => "A0".into(),
            Paper::Custom { width, height } => format!("{width:.0} × {height:.0}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    Portrait,
    #[default]
    Landscape,
}

/// Which side an elevation looks at, named by where the viewer stands.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// From the audience (−y) looking upstage.
    #[default]
    Front,
    /// From upstage (+y) looking at the audience.
    Back,
    /// From stage left as the audience sees it (−x), looking towards +x.
    Left,
    /// From +x looking towards −x.
    Right,
}

/// What a viewport shows.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ViewKind {
    /// Top view (the plan), vector.
    Plan,
    /// An elevation without a cut, vector.
    Elevation { side: Side },
    /// A section: a plane hides the side its normal points to; the viewer stands on
    /// that side looking back. Cut faces are hatched. Vector.
    Section {
        /// A point on the plane, world mm.
        origin: P3,
        /// Towards the viewer (and the hidden side).
        normal: P3,
    },
    /// A top view computed from the 3D model with hidden lines removed, optionally cut
    /// at a height like an architectural plan. View coordinates are world x/y, so crops
    /// and labels work as in `plan`. Vector.
    #[serde(rename = "plan_3d")]
    PlanFrom3d {
        /// Cut height, world mm: everything above is removed and cut faces are
        /// hatched. `None` = no cut.
        #[serde(default)]
        cut: Option<f64>,
    },
    /// A raster image of the 3D view (the GUI renders it; headless export draws a
    /// placeholder box).
    Rendered {
        /// Camera eye, world mm. Default: iso from front right above the model.
        #[serde(default)]
        eye: Option<P3>,
        #[serde(default)]
        target: Option<P3>,
    },
    /// A table of the fixtures (FID, type, address, mode, gel …) of the whole document
    /// or of the tags shown.
    FixtureList,
}

impl ViewKind {
    pub fn name(&self) -> &'static str {
        match self {
            ViewKind::Plan => "plan",
            ViewKind::Elevation { .. } => "elevation",
            ViewKind::Section { .. } => "section",
            ViewKind::PlanFrom3d { .. } => "plan_3d",
            ViewKind::Rendered { .. } => "rendered",
            ViewKind::FixtureList => "fixture_list",
        }
    }
}

/// A view of the model placed on a sheet.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Viewport {
    pub id: Id,
    pub name: String,
    pub view: ViewKind,
    /// Scale denominator: 50 = 1:50.
    pub scale: f64,
    /// Part of the view shown, model mm `[min, max]` in view coordinates. `None` = fit
    /// everything shown.
    #[serde(default)]
    pub crop: Option<[P2; 2]>,
    /// Centre of the viewport on the paper, mm from the lower left corner.
    pub position: P2,
    /// Objects carrying any of these tags are left out.
    #[serde(default)]
    pub hidden_tags: Vec<Id>,
    /// Layers left out (on top of layers hidden in the document).
    #[serde(default)]
    pub hidden_layers: Vec<Id>,
    /// Line class (`thin`, `medium`, `thick`, `hidden`, `centre`, `cut`) to pen width
    /// in mm. Missing classes use the defaults (0.18, 0.35, 0.5, 0.25, 0.18, 0.7).
    #[serde(default)]
    pub line_weights: BTreeMap<String, f64>,
    /// Elevations, sections and plans from 3D: also draw hidden edges, dashed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden_lines: bool,
    /// Per tag: draw the lines of objects carrying it in another class and/or pen
    /// width. The first matching tag in the map's order wins. Applies to plan,
    /// elevations, sections and plan-from-3D.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tag_lines: BTreeMap<Id, TagLines>,
    /// Label template for fixtures (plan viewports). `None` = no labels.
    #[serde(default)]
    pub label_template: Option<Id>,
    /// Labels dragged by hand: fixture id → offset of the label from where the
    /// template puts it, paper mm. They are kept out of automatic placement.
    #[serde(default)]
    pub label_offsets: BTreeMap<Id, P2>,
    /// Draw the name and scale under the viewport.
    #[serde(default = "yes")]
    pub show_title: bool,
    #[serde(default = "yes")]
    pub scale_bar: bool,
    /// Plan viewports: draw the driving and reference dimensions of the layer
    /// sketches.
    #[serde(default = "yes")]
    pub sketch_dimensions: bool,
    /// Plan viewports: draw the reference dimensions between objects.
    #[serde(default = "yes")]
    pub reference_dimensions: bool,
    /// Plan viewports: draw underlays (images, PDFs, SVGs, DXFs under the plan).
    /// Default false: sheets show the drawn plan, not the drawing it was traced from.
    #[serde(default)]
    pub underlays: bool,
}

/// How a viewport draws the lines of objects with one tag.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct TagLines {
    /// Line class for the object's visible lines: `thin`, `medium`, `thick`,
    /// `hidden`, `centre`. Hidden (dashed) and cut lines keep their class. `None` =
    /// unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// Pen width for the object's visible lines, mm, over the class's width.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,
}

/// How a label behaves on a rotated fixture.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LabelRotation {
    /// Field anchors follow the fixture ("in front of the lens"); the text stays
    /// horizontal on the sheet.
    #[default]
    AnchoredUpright,
    /// The layout never rotates: anchors and text as for an unrotated fixture.
    SheetFixed,
    /// Anchors and text rotate with the fixture; text past 90° is turned by 180° so it
    /// never reads upside down.
    Follow,
}

/// One line of a label.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct LabelField {
    /// `fid`, `cid`, `address` (Universe.Address), `mode`, `gel` (colour), `circuit`,
    /// `focus`, `note`, `name`, `type` (GDTF), `weight`, or `attr:<key>` for any
    /// object attribute.
    pub field: String,
    /// Where the field attaches, fixture-local mm (x right, y towards the lens).
    /// `[0, 0]` centres it on the fixture; otherwise the text sits outside the anchor,
    /// on the side facing away from the fixture's centre.
    #[serde(default)]
    pub anchor: P2,
    /// Cap height on paper, mm. Default 2.0.
    #[serde(default = "two")]
    pub size: f64,
    /// Draw a box around the text.
    #[serde(default)]
    pub boxed: bool,
    /// Bold text.
    #[serde(default)]
    pub bold: bool,
    /// Text before the value, e.g. `Ch ` or `#`.
    #[serde(default)]
    pub prefix: String,
}

fn two() -> f64 {
    2.0
}

/// A label layout for fixtures.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct LabelTemplate {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub rotation: LabelRotation,
    pub fields: Vec<LabelField>,
    /// Gap between the anchor and the text, paper mm. Default 0.8.
    #[serde(default = "gap")]
    pub gap: f64,
}

fn gap() -> f64 {
    0.8
}

/// A drawn element of a title block. Positions are mm from the block's lower left.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "item")]
pub enum TitleItem {
    Line {
        from: P2,
        to: P2,
        /// Pen width, mm. Default 0.25.
        #[serde(default = "pen")]
        weight: f64,
    },
    Rect {
        min: P2,
        max: P2,
        #[serde(default = "pen")]
        weight: f64,
    },
    /// Text with placeholders: {project}, {venue}, {drawn_by}, {date}, {scale},
    /// {revision}, {sheet_name}, {sheet_number}, {company}, {paper}.
    Text {
        pos: P2,
        text: String,
        /// Cap height, mm. Default 2.5.
        #[serde(default = "two_half")]
        size: f64,
        #[serde(default)]
        bold: bool,
        /// `left`, `center` or `right` of `pos`. Default left.
        #[serde(default)]
        align: TextAlign,
    },
    /// The document's logo, fitted into the box keeping its aspect ratio.
    Logo { min: P2, max: P2 },
}

fn pen() -> f64 {
    0.25
}

fn two_half() -> f64 {
    2.5
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// A title block template. It sits in the lower right corner inside the frame.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct TitleBlock {
    pub id: Id,
    pub name: String,
    /// mm.
    pub width: f64,
    pub height: f64,
    pub items: Vec<TitleItem>,
}

impl TitleBlock {
    /// The built-in generic block: 180 × 50 mm, no company in it.
    pub fn generic() -> Self {
        let t = |pos: P2, text: &str, size: f64, bold: bool| TitleItem::Text {
            pos,
            text: text.into(),
            size,
            bold,
            align: TextAlign::Left,
        };
        let line = |from: P2, to: P2| TitleItem::Line { from, to, weight: 0.25 };
        let small = 1.6;
        TitleBlock {
            id: Id::nil(),
            name: "Generic".into(),
            width: 180.0,
            height: 50.0,
            items: vec![
                TitleItem::Rect { min: [0.0, 0.0], max: [180.0, 50.0], weight: 0.5 },
                line([0.0, 34.0], [180.0, 34.0]),
                line([0.0, 17.0], [180.0, 17.0]),
                line([120.0, 0.0], [120.0, 50.0]),
                line([55.0, 0.0], [55.0, 17.0]),
                line([95.0, 0.0], [95.0, 17.0]),
                line([150.0, 0.0], [150.0, 17.0]),
                // Project and venue.
                t([3.0, 45.5], "PROJECT", small, false),
                t([3.0, 37.5], "{project}", 4.0, true),
                t([3.0, 28.5], "VENUE", small, false),
                t([3.0, 21.0], "{venue}", 3.0, false),
                // Sheet name, drawn by, number.
                t([3.0, 12.0], "SHEET", small, false),
                t([3.0, 4.0], "{sheet_name}", 3.0, true),
                t([58.0, 12.0], "DRAWN BY", small, false),
                t([58.0, 4.0], "{drawn_by}", 2.2, false),
                t([98.0, 12.0], "No.", small, false),
                t([98.0, 4.0], "{sheet_number}", 3.5, true),
                // Right column: company/logo, scale, date, revision.
                TitleItem::Logo { min: [122.0, 35.5], max: [178.0, 48.5] },
                t([123.0, 28.5], "COMPANY", small, false),
                t([123.0, 21.0], "{company}", 2.5, false),
                t([123.0, 12.0], "SCALE", small, false),
                t([123.0, 4.0], "{scale}", 2.5, false),
                t([153.0, 12.0], "DATE / REV", small, false),
                t([153.0, 4.0], "{date} {revision}", 2.2, false),
            ],
        }
    }
}

/// A sensible first label: FID boxed on the fixture, address behind it, gel in front.
pub fn default_label_fields() -> Vec<LabelField> {
    let f = |field: &str, anchor: P2, size: f64, boxed: bool, bold: bool| LabelField {
        field: field.into(),
        anchor,
        size,
        boxed,
        bold,
        prefix: String::new(),
    };
    vec![
        f("fid", [0.0, 0.0], 2.2, true, true),
        f("gel", [0.0, 230.0], 1.8, false, false),
        f("address", [0.0, -190.0], 1.6, false, false),
    ]
}

/// Base64 (standard alphabet, padded), for the logo in the document JSON.
pub fn base64_encode(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let bytes: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for c in bytes.chunks(4) {
        let pad = c.iter().rev().take_while(|&&b| b == b'=').count();
        let mut n = 0u32;
        for &b in &c[..4 - pad] {
            n = n << 6 | val(b)?;
        }
        n <<= 6 * pad as u32;
        let got = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&got[..3 - pad]);
    }
    Some(out)
}

/// Draws sheets into files. `stagecraft-sheet` installs one (the app and the GUI call
/// its `install()`); core stays free of PDF code, and `sheet.export_*` say so when
/// nothing is installed.
pub trait SheetExporter: Send + Sync {
    /// One PDF with a page per sheet, in the order given.
    fn pdf(&self, doc: &crate::model::Document, sheets: &[Id]) -> Result<Vec<u8>, String>;
    /// One sheet as SVG.
    fn svg(&self, doc: &crate::model::Document, sheet: Id) -> Result<String, String>;
}

static EXPORTER: std::sync::OnceLock<Box<dyn SheetExporter>> = std::sync::OnceLock::new();

/// Install the exporter once; later calls are ignored.
pub fn install_exporter(e: Box<dyn SheetExporter>) {
    let _ = EXPORTER.set(e);
}

pub fn exporter() -> Option<&'static dyn SheetExporter> {
    EXPORTER.get().map(|b| b.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for data in [&b""[..], b"a", b"ab", b"abc", b"abcd", &[0, 255, 128, 7, 9]] {
            let e = base64_encode(data);
            assert_eq!(base64_decode(&e).as_deref(), Some(data), "{e}");
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
    }

    #[test]
    fn paper_sizes_turn() {
        let mut s = Sheet {
            id: Id::nil(),
            name: "x".into(),
            number: "1".into(),
            paper: Paper::A3,
            orientation: Orientation::Landscape,
            title_block: None,
            show_title_block: true,
            show_legend: true,
            order: 0,
            viewports: vec![],
        };
        assert_eq!(s.size(), [420.0, 297.0]);
        s.orientation = Orientation::Portrait;
        assert_eq!(s.size(), [297.0, 420.0]);
    }
}
