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
pub const FORMAT_VERSION: u32 = 1;

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
    /// A lighting fixture. GDTF data arrives in block 5; this holds the patch.
    Fixture {
        fid: Option<u32>,
        cid: Option<String>,
        /// Name of the embedded GDTF file.
        gdtf: Option<String>,
        mode: Option<String>,
        /// Absolute DMX address, 1-based (universe 2 address 1 = 513).
        address: Option<u32>,
        /// kg, for load sums. Block 5 fills it from the GDTF.
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
            ObjectKind::TrussNode { .. } => "truss_node",
            ObjectKind::TrussRun { .. } => "truss_run",
            ObjectKind::Support { .. } => "support",
            ObjectKind::Hoist { .. } => "hoist",
            ObjectKind::Ceiling { .. } => "ceiling",
        }
    }

    /// Every type name, for docs and filters.
    pub const TYPE_NAMES: [&'static str; 15] = [
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
        "truss_node",
        "truss_run",
        "support",
        "hoist",
        "ceiling",
    ];

    /// Groups hold children and draw nothing.
    pub fn is_group(&self) -> bool {
        matches!(self, ObjectKind::WingSet { .. } | ObjectKind::TrussRun { .. })
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
