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
    },
}

impl ObjectKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            ObjectKind::Floor { .. } => "floor",
            ObjectKind::Wall { .. } => "wall",
            ObjectKind::Truss { .. } => "truss",
            ObjectKind::Fixture { .. } => "fixture",
        }
    }
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
