//! The document: layers, tags and objects.
//!
//! Lengths are millimetres, angles degrees. Axes as in MVR: x right as seen from the
//! audience, y upstage, z up. Collections are `imbl` persistent maps, so cloning a
//! document for undo is cheap.

use imbl::OrdMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type Id = Uuid;
/// A point in plan, mm.
pub type P2 = [f64; 2];
/// A point in space, mm.
pub type P3 = [f64; 3];

/// File format version written into every `.stagecraft`.
///
/// 1: block 1–7 documents. 2: embedded assets (`assets/` in the ZIP) and the fixture
/// type library. 3: layer sketches, sketch-backed walls/floors and reference
/// dimensions. 4: underlays (image, PDF, SVG, DXF assets) and notes. 5: the project
/// unit (`meta.units`) and the dimension format `project` (default of new documents).
/// Older files load unchanged (the new maps start empty, the unit is mm, and a missing
/// dimension format stays `m_sup_mm`).
pub const FORMAT_VERSION: u32 = 5;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Document {
    pub format: u32,
    pub meta: Meta,
    pub layers: OrdMap<Id, Layer>,
    pub tags: OrdMap<Id, Tag>,
    pub objects: OrdMap<Id, Object>,
    /// Drawing sheets (block 7).
    #[serde(default)]
    pub sheets: OrdMap<Id, crate::sheet::Sheet>,
    #[serde(default)]
    pub label_templates: OrdMap<Id, crate::sheet::LabelTemplate>,
    /// Custom title blocks; sheets without one use [`crate::sheet::TitleBlock::generic`].
    #[serde(default)]
    pub title_blocks: OrdMap<Id, crate::sheet::TitleBlock>,
    /// Embedded files by SHA-256 (GDTFs). Bytes are shared between undo versions.
    #[serde(default)]
    pub assets: OrdMap<String, crate::asset::Asset>,
    /// Fixture types by key (`Manufacturer@Name`), each backed by a GDTF asset.
    #[serde(default)]
    pub fixture_types: OrdMap<String, crate::fixture_type::FixtureType>,
    /// One parametric sketch per layer (keyed by layer id): the room outline, walls and
    /// floors drawn with constraints and dimensions (block 9).
    #[serde(default)]
    pub sketches: OrdMap<Id, crate::sketch::Sketch>,
    /// Reference dimensions between points of objects or sketches. They measure live
    /// and drive nothing.
    #[serde(default)]
    pub dimensions: OrdMap<Id, crate::sketch::RefDimension>,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    /// An empty document with one layer called "Stage" at elevation 0.
    pub fn new() -> Self {
        let mut doc = Self {
            format: FORMAT_VERSION,
            meta: Meta::default(),
            layers: OrdMap::new(),
            tags: OrdMap::new(),
            objects: OrdMap::new(),
            sheets: OrdMap::new(),
            label_templates: OrdMap::new(),
            title_blocks: OrdMap::new(),
            assets: OrdMap::new(),
            fixture_types: OrdMap::new(),
            sketches: OrdMap::new(),
            dimensions: OrdMap::new(),
        };
        let stage = Layer { id: Uuid::new_v4(), name: "Stage".into(), elevation: 0.0, order: 0, visible: true };
        doc.layers.insert(stage.id, stage);
        doc
    }

    /// Layers sorted by their `order`, then name.
    pub fn layers_sorted(&self) -> Vec<&Layer> {
        let mut v: Vec<&Layer> = self.layers.values().collect();
        v.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.name.cmp(&b.name)));
        v
    }

    /// The direct children of an object.
    pub fn children(&self, id: Id) -> Vec<Id> {
        self.objects.values().filter(|o| o.parent == Some(id)).map(|o| o.id).collect()
    }

    /// The object and all its descendants, parents before children.
    pub fn subtree(&self, id: Id) -> Vec<Id> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            let kids = self.children(out[i]);
            out.extend(kids);
            i += 1;
        }
        out
    }

    /// Sheets sorted by their `order`, then number.
    pub fn sheets_sorted(&self) -> Vec<&crate::sheet::Sheet> {
        let mut v: Vec<_> = self.sheets.values().collect();
        v.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.number.cmp(&b.number)));
        v
    }

    /// The sheet holding a viewport.
    pub fn sheet_of_viewport(&self, viewport: Id) -> Option<&crate::sheet::Sheet> {
        self.sheets.values().find(|s| s.viewport(viewport).is_some())
    }

    /// Change every viewport of every sheet.
    pub fn edit_viewports(&mut self, mut f: impl FnMut(&mut crate::sheet::Viewport)) {
        let ids: Vec<Id> = self.sheets.keys().copied().collect();
        for id in ids {
            let s = self.sheets.get_mut(&id).expect("listed");
            s.viewports.iter_mut().for_each(&mut f);
        }
    }

    /// One viewport, mutable.
    pub fn viewport_mut(&mut self, viewport: Id) -> Option<&mut crate::sheet::Viewport> {
        let sheet = self.sheet_of_viewport(viewport)?.id;
        self.sheets.get_mut(&sheet)?.viewports.iter_mut().find(|v| v.id == viewport)
    }

    /// True if `ancestor` is `id` or one of its parents.
    pub fn is_ancestor(&self, ancestor: Id, id: Id) -> bool {
        let mut cur = Some(id);
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            cur = self.objects.get(&c).and_then(|o| o.parent);
        }
        false
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Meta {
    pub project: String,
    pub venue: String,
    pub drawn_by: String,
    pub revision: String,
    /// Shown in title blocks.
    #[serde(default)]
    pub company: String,
    /// Date printed in title blocks, free text (e.g. `2026-11-18`).
    #[serde(default)]
    pub date: String,
    /// Logo for title blocks: PNG or JPEG, base64.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(skip)]
    pub logo: Option<String>,
    /// Venue address for title blocks, lines separated by `\n`.
    #[serde(default)]
    pub venue_address: String,
    /// Production dates, free text (e.g. `02.05.2025 – 05.05.2025`).
    #[serde(default)]
    pub production_dates: String,
    /// Website shown in title blocks.
    #[serde(default)]
    pub website: String,
    /// Language of sheet text (title block labels, legend, view names). Default English.
    #[serde(default)]
    pub sheet_language: crate::units::Language,
    /// How dimension values are written in plan and on sheets. New documents follow
    /// the project unit; files without the field (before format 5) meant `m_sup_mm`.
    #[serde(default = "crate::units::DimFormat::legacy")]
    pub dimension_format: crate::units::DimFormat,
    /// The project unit: how the GUI shows and takes lengths. The document and every
    /// command argument stay in mm.
    #[serde(default)]
    pub units: crate::units::UnitSettings,
    /// Decimal separator of lengths; `None` = the sheet language's (en `.`, de `,`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decimal_separator: Option<char>,
}

/// A spatial layer: a storey or area. Root objects on it sit at its elevation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Layer {
    pub id: Id,
    pub name: String,
    /// mm above the document origin.
    pub elevation: f64,
    pub order: i32,
    pub visible: bool,
}

/// A free tag (light, sound, rigging …). Objects carry any number of them.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Tag {
    pub id: Id,
    pub name: String,
    /// `#RRGGBB`, used to tint the tag in lists.
    pub color: Option<String>,
}

/// Position and rotation relative to the parent object, or to the layer for roots.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Placement {
    /// mm.
    #[serde(default)]
    pub position: P3,
    /// Degrees about x, y, z, applied as Rz · Ry · Rx.
    #[serde(default)]
    pub rotation: P3,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Object {
    pub id: Id,
    pub name: String,
    pub layer: Id,
    /// Sorted, no duplicates.
    pub tags: Vec<Id>,
    pub parent: Option<Id>,
    pub placement: Placement,
    pub kind: ObjectKind,
    /// Per-object overrides of how the plan draws it (hybrid model: parameters draw
    /// the symbol, this restyles it).
    #[serde(default, skip_serializing_if = "PlanOverride::is_default")]
    pub style: PlanOverride,
    /// Free attributes (key → text): `gel`, `circuit`, `focus`, `note` for fixtures, or
    /// anything else. Labels and tables can show them.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub attrs: std::collections::BTreeMap<String, String>,
    /// Walls and floors whose geometry follows the layer's sketch: the edges they are
    /// made of. `None` = free geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch: Option<crate::sketch::SketchLink>,
}

/// Per-object plan style. Unset fields follow the view.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct PlanOverride {
    /// Draw every line of the symbol in this class: thin, medium, thick, hidden
    /// (dashed) or centre (dash-dot).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    /// Show or hide labels (FID, batten number, heights) regardless of the view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<bool>,
    /// `low` (outline only) or `high`, regardless of zoom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Leave the object out of the plan (it still shows in 3D).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

impl PlanOverride {
    pub fn is_default(&self) -> bool {
        *self == PlanOverride::default()
    }
}

/// How a curtain opens.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Gather {
    /// Sideways on a traveller track (tab / Greek curtain): the panels slide to the
    /// sides and their folds stack up there.
    #[default]
    #[serde(alias = "greek")]
    Tab,
    /// Italian: each panel is pulled up diagonally towards its upper outer corner and
    /// bunches up there.
    Italian,
}

/// Leg (vertical masking at the side) or border (horizontal masking above).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MaskingRole {
    #[default]
    Leg,
    Border,
}

/// What carries a platform deck.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Understructure {
    /// Four legs at the corners (Bütec style).
    #[default]
    Legs,
    /// Scissor frames on the long sides (Nivoflex style).
    Scissor,
    /// The deck sits directly on what is below.
    None,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupportStyle {
    /// A square steel plate; truss towers and stands stand on it.
    #[default]
    BasePlate,
    /// A tripod (wind-up) stand with a mast; things attach at its top.
    Stand,
    /// A ground support base with outriggers and a sleeve; a tower rises from it.
    GroundSupport,
}

/// What an object is. Geometry is local to the object's placement.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ObjectKind {
    /// A floor area: closed outline in plan, top surface at the placement's z.
    Floor { outline: Vec<P2>, thickness: f64 },
    /// A wall along a polyline, standing on the placement's z.
    Wall { path: Vec<P2>, height: f64, thickness: f64 },
    /// A straight truss piece along local +x, starting at the placement.
    Truss { length: f64, profile: String },
    /// A lighting fixture. Origin: the hanging point; GDTF geometry hangs below it
    /// (−z), so a floor fixture is turned over (rotation x 180°).
    Fixture {
        fid: Option<u32>,
        cid: Option<String>,
        /// Fixture type key in the document library (`Manufacturer@Name`).
        gdtf: Option<String>,
        /// DMX mode name of the type; `None` = its first mode.
        mode: Option<String>,
        /// Absolute DMX address of the first break, 1-based (universe 2 address 1 = 513).
        address: Option<u32>,
        /// Addresses of further DMX breaks of the mode, in break order; 0 = unpatched.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        extra_addresses: Vec<u32>,
        /// kg, for load sums. Filled from the GDTF; can be overridden.
        #[serde(default)]
        weight_kg: Option<f64>,
    },
    /// A proscenium (portal). Origin: centre of the opening on the floor at its
    /// downstage face; the frame reaches upstage (+y) by `depth`.
    Proscenium {
        /// Clear opening, mm.
        width: f64,
        height: f64,
        /// Depth of the frame along y, mm.
        depth: f64,
        /// Width of the side pillars and height of the header, mm.
        frame: f64,
        /// y of the iron (fire curtain) line, local mm.
        iron: f64,
        show_iron: bool,
        /// The opening's upper corners cut at 45° by this much, mm (0 = square).
        #[serde(default)]
        chamfer: f64,
        /// The frame's outer upper corners cut at 45° by this much, mm (0 = square).
        #[serde(default)]
        outer_chamfer: f64,
    },
    /// A curtain hanging from its top edge. Origin: centre of the top edge; the fabric
    /// runs along local x and hangs down (-z).
    Curtain {
        width: f64,
        height: f64,
        /// Extra fabric in percent of the width (100 = twice the width of fabric).
        fullness: f64,
        /// Two panels meeting in the middle (true) or one.
        split: bool,
        /// 0 = closed, 100 = fully open.
        opening: f64,
        gather: Gather,
        /// `#RRGGBB`.
        color: String,
    },
    /// The parent of a set of legs and borders generated together; holds the values
    /// they were made with. Draws nothing itself.
    WingSet { count: u32, spacing: f64, opening: f64, leg_width: f64, height: f64, border_height: f64, color: String },
    /// A leg or border. Origin: centre of the top edge, hanging down (-z).
    Masking {
        role: MaskingRole,
        width: f64,
        height: f64,
        /// Percent extra fabric; 0 = flat.
        fullness: f64,
        color: String,
    },
    /// A fly bar (Zug). Origin: centre of the pipe; it runs along local x. Its trim
    /// height is the placement's z, its position the placement's y.
    Batten {
        number: u32,
        length: f64,
        /// Allowed payload, kg.
        max_load: f64,
        /// Own weight of the bar, kg per metre.
        kg_per_m: f64,
    },
    /// A stage platform (Podest). Origin: centre of the footprint at its base; the
    /// deck top is `height` above it.
    Platform {
        width: f64,
        depth: f64,
        height: f64,
        under: Understructure,
        /// Platform system from the catalogue (Bütec, Nivoflex …).
        system: Option<String>,
    },
    /// A straight row of seats (chairs or fixed seats). Origin: the middle of the row on
    /// the floor; seats sit side by side along local x and face local +y (at rotation 0
    /// the audience looks upstage).
    SeatRow {
        /// Number of seats.
        count: u32,
        /// Centre-to-centre distance of the seats, mm.
        seat_width: f64,
        /// Front to back of a seat, mm.
        depth: f64,
    },
    /// A truss corner or junction. Origin: the node centre; arms are truss stubs of
    /// one profile width.
    TrussNode {
        profile: String,
        /// Directions of the horizontal arms, degrees counter-clockwise from local +x.
        arms: Vec<f64>,
        /// An arm pointing down (towers, goal posts).
        #[serde(default)]
        down: bool,
        #[serde(default)]
        up: bool,
    },
    /// A run of truss: pieces and nodes as children. Draws nothing itself.
    TrussRun { profile: String },
    /// A base plate, stand or ground support base. Origin on the floor; attached
    /// things go at `height`.
    Support {
        style: SupportStyle,
        /// Plate edge or leg spread, mm.
        size: f64,
        /// Top of the mast or sleeve above the floor, mm.
        height: f64,
        weight_kg: f64,
    },
    /// A chain hoist or rigging point. Origin: the hook; the body is above it.
    Hoist { capacity_kg: f64, weight_kg: f64 },
    /// A ceiling: a closed outline at the placement's z (its underside), `thickness`
    /// up from there.
    Ceiling { outline: Vec<P2>, thickness: f64, show_in_plan: bool },
    /// A plain group: holds children, draws nothing (MVR group objects).
    Group {},
    /// Imported 3D geometry (glTF/GLB, OBJ, 3DS, or MVR scene objects): mesh files
    /// embedded as assets, placed in the object's local space.
    Model {
        parts: Vec<ModelPart>,
        /// Local bounding box `[min, max]`, mm, measured at import.
        bounds: [P3; 2],
    },
    /// A drawing under the plan: an image, a PDF page, an SVG or a DXF. A point
    /// `(u, v)` of the source (y up) sits at local `(u · scale, v · scale)` mm; the
    /// placement moves and turns it. Drawn below everything, never in 3D.
    Underlay {
        format: UnderlayFormat,
        /// SHA-256 of the imported file in the document's assets.
        source: String,
        /// What the plan shows for images, PDFs and SVGs: a PNG or JPEG asset (for
        /// images the source itself). The picture spans `extent`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        raster: Option<String>,
        /// DXF: the flattened linework as a JSON asset (`stagecraft_underlay::Linework`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        linework: Option<String>,
        /// PDF page, 1-based.
        #[serde(default = "one")]
        page: u32,
        /// `[min, max]` of the content in source units (pixels, PDF points, SVG units,
        /// DXF drawing units), y up.
        extent: [P2; 2],
        /// mm per source unit.
        scale: f64,
        /// 0 (invisible) to 1.
        opacity: f64,
        /// Locked underlays cannot be picked, moved, turned, calibrated or deleted.
        #[serde(default)]
        locked: bool,
    },
    /// A text note in the plan (assumptions, remarks). Origin: the upper left corner of
    /// the first line; lines run along local +x.
    Note {
        /// Text; `\n` starts a new line.
        text: String,
        /// Cap height, mm in the model (250 = 2.5 mm on paper at 1:100).
        height: f64,
        /// Wrap lines longer than this, mm; 0 = only at `\n`.
        #[serde(default)]
        width: f64,
        /// Draw a box around the text.
        #[serde(default)]
        frame: bool,
        /// End of a leader line in local mm (relative to the origin), if any.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        leader: Option<P2>,
    },
}

fn one() -> u32 {
    1
}

/// Where an underlay comes from.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UnderlayFormat {
    /// PNG or JPEG.
    Image,
    Pdf,
    Svg,
    Dxf,
    /// Read as DXF linework (desktop only).
    Dwg,
}

/// One mesh file of a model object.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ModelPart {
    /// SHA-256 of the file in the document's assets (`glb`, `gltf`, `obj` or `3ds`).
    pub asset: String,
    /// The file's frame in the object's local space as an MVR-style 4×3 matrix:
    /// columns u, v, w (unit axes) then the origin o, mm. Applied after `scale` and
    /// the up-axis turn.
    pub matrix: [f64; 12],
    /// File units to mm: 1000 for metres (glTF), 1 for mm.
    pub scale: f64,
    /// The file is y-up and is turned to z-up first (OBJ). glTF is always turned.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub y_up: bool,
}

impl ModelPart {
    /// The identity placement.
    pub const IDENTITY: [f64; 12] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
}

impl ObjectKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            ObjectKind::Floor { .. } => "floor",
            ObjectKind::Wall { .. } => "wall",
            ObjectKind::Truss { .. } => "truss",
            ObjectKind::Fixture { .. } => "fixture",
            ObjectKind::Proscenium { .. } => "proscenium",
            ObjectKind::Curtain { .. } => "curtain",
            ObjectKind::WingSet { .. } => "wing_set",
            ObjectKind::Masking { .. } => "masking",
            ObjectKind::Batten { .. } => "batten",
            ObjectKind::Platform { .. } => "platform",
            ObjectKind::SeatRow { .. } => "seat_row",
            ObjectKind::TrussNode { .. } => "truss_node",
            ObjectKind::TrussRun { .. } => "truss_run",
            ObjectKind::Support { .. } => "support",
            ObjectKind::Hoist { .. } => "hoist",
            ObjectKind::Ceiling { .. } => "ceiling",
            ObjectKind::Group { .. } => "group",
            ObjectKind::Model { .. } => "model",
            ObjectKind::Underlay { .. } => "underlay",
            ObjectKind::Note { .. } => "note",
        }
    }

    /// Every type name, for docs and filters.
    pub const TYPE_NAMES: [&'static str; 20] = [
        "floor",
        "wall",
        "truss",
        "fixture",
        "proscenium",
        "curtain",
        "wing_set",
        "masking",
        "batten",
        "platform",
        "seat_row",
        "truss_node",
        "truss_run",
        "support",
        "hoist",
        "ceiling",
        "group",
        "model",
        "underlay",
        "note",
    ];

    /// Underlays and notes: plan annotations without a body. They have no mesh, no
    /// weight and are not exported to MVR.
    pub fn is_annotation(&self) -> bool {
        matches!(self, ObjectKind::Underlay { .. } | ObjectKind::Note { .. })
    }

    /// Groups hold children and draw nothing.
    pub fn is_group(&self) -> bool {
        matches!(self, ObjectKind::WingSet { .. } | ObjectKind::TrussRun { .. } | ObjectKind::Group { .. })
    }
}

/// `#RRGGBB` to bytes.
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

/// `1.001` style address for an absolute 1-based DMX address.
pub fn format_address(absolute: u32) -> String {
    let u = (absolute - 1) / 512 + 1;
    let a = (absolute - 1) % 512 + 1;
    format!("{u}.{a:03}")
}

/// Parse `1.001`, `2.17` or a plain absolute number.
pub fn parse_address(s: &str) -> Option<u32> {
    match s.split_once('.') {
        Some((u, a)) => {
            let u: u32 = u.trim().parse().ok()?;
            let a: u32 = a.trim().parse().ok()?;
            (u >= 1 && (1..=512).contains(&a)).then(|| (u - 1) * 512 + a)
        }
        None => s.trim().parse().ok().filter(|&n| n >= 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_round_trip() {
        assert_eq!(format_address(1), "1.001");
        assert_eq!(format_address(512), "1.512");
        assert_eq!(format_address(513), "2.001");
        assert_eq!(parse_address("2.001"), Some(513));
        assert_eq!(parse_address("1.17"), Some(17));
        assert_eq!(parse_address("600"), Some(600));
        assert_eq!(parse_address("1.513"), None);
        assert_eq!(parse_address("0.1"), None);
    }
}
