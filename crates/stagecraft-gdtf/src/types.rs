//! The fixture type as described in `description.xml`.
//!
//! These types mirror the XML and keep its units: lengths in **metres**, angles in
//! degrees, power in watts, weight in kilograms. Matrices are in GDTF axes (x right,
//! y away from the viewer, z up; the fixture hangs, beams point to −z) with metres
//! as translation unit. The mesh module converts to millimetres.

use std::collections::BTreeMap;

use glam::Mat4;

/// A parsed `<FixtureType>`.
#[derive(Debug, Clone, Default)]
pub struct FixtureType {
    /// `DataVersion` of the `<GDTF>` node, e.g. `"1.2"`.
    pub data_version: String,
    pub name: String,
    pub short_name: String,
    pub long_name: String,
    pub manufacturer: String,
    pub description: String,
    /// `FixtureTypeID`, upper case as written (GUID).
    pub fixture_type_id: String,
    /// `RefFT`, if set.
    pub ref_ft: Option<String>,
    /// `Thumbnail` resource name (no extension); see [`crate::Gdtf::thumbnail_png`].
    pub thumbnail: Option<String>,
    pub thumbnail_offset: (i32, i32),
    pub can_have_children: bool,
    pub physical: Physical,
    pub models: Vec<Model>,
    /// Top-level geometries. The one a DMX mode names is the fixture; the others are
    /// templates instantiated by `GeometryReference`.
    pub geometries: Vec<Geometry>,
    pub modes: Vec<DmxMode>,
    pub revisions: Vec<Revision>,
}

impl FixtureType {
    /// The model with this name; names that differ only in surrounding whitespace
    /// match too (seen in manufacturer files).
    pub fn model(&self, name: &str) -> Option<&Model> {
        self.models
            .iter()
            .find(|m| m.name == name)
            .or_else(|| self.models.iter().find(|m| m.name.trim() == name.trim()))
    }

    /// The top-level geometry with this name (whitespace-tolerant like [`Self::model`]).
    pub fn top_geometry(&self, name: &str) -> Option<&Geometry> {
        self.geometries
            .iter()
            .find(|g| g.name == name)
            .or_else(|| self.geometries.iter().find(|g| g.name.trim() == name.trim()))
    }

    /// Any geometry with this name, searched depth first through all trees.
    pub fn find_geometry(&self, name: &str) -> Option<&Geometry> {
        self.geometries.iter().find_map(|g| g.find(name))
    }

    pub fn mode(&self, name: &str) -> Option<&DmxMode> {
        self.modes.iter().find(|m| m.name == name)
    }

    /// Total power in watts: the sum of `<PowerConsumption>` values if any are given,
    /// otherwise the sum of the `PowerConsumption` attributes written on the beams of
    /// the first mode's geometry tree (references expanded; the spec default of
    /// 1000 W for beams without the attribute is not counted). `None` when neither
    /// exists.
    pub fn power_w(&self) -> Option<f32> {
        let props: f32 = self.physical.power_consumption.iter().map(|p| p.value_w).sum();
        if !self.physical.power_consumption.is_empty() && props > 0.0 {
            return Some(props);
        }
        let root = self.modes.first().map(|m| m.geometry.as_str());
        let root = root.and_then(|r| self.top_geometry(r)).or(self.geometries.first())?;
        let mut sum = 0.0;
        let mut found = false;
        self.visit_beams(root, 0, &mut |g, b| {
            if g.attributes.contains_key("PowerConsumption") {
                sum += b.power_consumption_w;
                found = true;
            }
        });
        found.then_some(sum)
    }

    fn visit_beams(&self, g: &Geometry, depth: usize, f: &mut dyn FnMut(&Geometry, &BeamData)) {
        if depth > 32 {
            return;
        }
        if let GeometryKind::Beam(b) = &g.kind {
            f(g, b);
        }
        if let GeometryKind::Reference { geometry, .. } = &g.kind
            && let Some(t) = self.top_geometry(geometry)
        {
            self.visit_beams(t, depth + 1, f);
        }
        for c in &g.children {
            self.visit_beams(c, depth + 1, f);
        }
    }
}

/// `<PhysicalDescriptions>/<Properties>` and the parts of it we use.
#[derive(Debug, Clone, Default)]
pub struct Physical {
    /// `<Weight Value>` in kg; `None` when missing or 0 (the spec default).
    pub weight_kg: Option<f32>,
    /// `<LegHeight Value>` in metres; `None` when missing or 0.
    pub leg_height_m: Option<f32>,
    /// `<OperatingTemperature Low/High>` in °C.
    pub operating_temperature_c: Option<(f32, f32)>,
    pub power_consumption: Vec<PowerConsumption>,
}

/// One `<PowerConsumption>` entry (GDTF 1.2).
#[derive(Debug, Clone, Default)]
pub struct PowerConsumption {
    /// Watts.
    pub value_w: f32,
    pub power_factor: Option<f32>,
    pub connector: String,
    pub voltage_low: Option<f32>,
    pub voltage_high: Option<f32>,
}

/// A `<Model>`: dimensions and the mesh resource of one fixture part.
#[derive(Debug, Clone, Default)]
pub struct Model {
    pub name: String,
    /// Extent along x, metres.
    pub length_m: f32,
    /// Extent along y, metres.
    pub width_m: f32,
    /// Extent along z, metres.
    pub height_m: f32,
    pub primitive: PrimitiveType,
    /// `File` resource name without extension and folder; `None` when empty.
    pub file: Option<String>,
    /// SVG insertion offsets (top, side, front), in SVG units.
    pub svg_offset: (f32, f32),
    pub svg_side_offset: (f32, f32),
    pub svg_front_offset: (f32, f32),
}

/// `PrimitiveType` of a model. Unknown values are kept in `Other`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PrimitiveType {
    #[default]
    Undefined,
    Cube,
    Cylinder,
    Sphere,
    Base,
    Yoke,
    Head,
    Scanner,
    Conventional,
    Pigtail,
    Base1_1,
    Scanner1_1,
    Conventional1_1,
    Other(String),
}

impl PrimitiveType {
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "" | "Undefined" => Self::Undefined,
            "Cube" => Self::Cube,
            "Cylinder" => Self::Cylinder,
            "Sphere" => Self::Sphere,
            "Base" => Self::Base,
            "Yoke" => Self::Yoke,
            "Head" => Self::Head,
            "Scanner" => Self::Scanner,
            "Conventional" => Self::Conventional,
            "Pigtail" => Self::Pigtail,
            "Base1_1" => Self::Base1_1,
            "Scanner1_1" => Self::Scanner1_1,
            "Conventional1_1" => Self::Conventional1_1,
            other => Self::Other(other.to_string()),
        }
    }
}

/// A node of the geometry tree.
#[derive(Debug, Clone)]
pub struct Geometry {
    pub name: String,
    /// `Model` link; `None` when absent or empty.
    pub model: Option<String>,
    /// `Position`: transform relative to the parent geometry, metres.
    pub position: Mat4,
    pub kind: GeometryKind,
    pub children: Vec<Geometry>,
    /// Every XML attribute as written, for data this crate does not model yet.
    pub attributes: BTreeMap<String, String>,
}

impl Geometry {
    /// Depth-first search of this subtree, including `self`.
    pub fn find(&self, name: &str) -> Option<&Geometry> {
        if self.name == name {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(name))
    }

    /// Calls `f` for every node of this subtree, depth first, `self` first.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Geometry)) {
        f(self);
        for c in &self.children {
            c.walk(f);
        }
    }

    /// The XML node name this geometry came from (`"Axis"`, `"Beam"` …).
    pub fn node_name(&self) -> &str {
        self.kind.node_name()
    }
}

/// Geometry types of GDTF 1.2 (table 34). Types without extra data carry none.
#[derive(Debug, Clone, PartialEq)]
pub enum GeometryKind {
    Geometry,
    Axis,
    FilterBeam,
    FilterColor,
    FilterGobo,
    FilterShaper,
    Beam(BeamData),
    MediaServerLayer,
    MediaServerCamera,
    MediaServerMaster,
    Display {
        texture: Option<String>,
    },
    Laser,
    WiringObject,
    Inventory,
    Structure,
    Support,
    Magnet,
    /// `GeometryReference`: an instance of the top-level geometry `geometry`.
    Reference {
        geometry: String,
        breaks: Vec<Break>,
    },
    /// An unknown element inside `<Geometries>`, parsed like a plain geometry.
    Other(String),
}

impl GeometryKind {
    pub fn node_name(&self) -> &str {
        match self {
            Self::Geometry => "Geometry",
            Self::Axis => "Axis",
            Self::FilterBeam => "FilterBeam",
            Self::FilterColor => "FilterColor",
            Self::FilterGobo => "FilterGobo",
            Self::FilterShaper => "FilterShaper",
            Self::Beam(_) => "Beam",
            Self::MediaServerLayer => "MediaServerLayer",
            Self::MediaServerCamera => "MediaServerCamera",
            Self::MediaServerMaster => "MediaServerMaster",
            Self::Display { .. } => "Display",
            Self::Laser => "Laser",
            Self::WiringObject => "WiringObject",
            Self::Inventory => "Inventory",
            Self::Structure => "Structure",
            Self::Support => "Support",
            Self::Magnet => "Magnet",
            Self::Reference { .. } => "GeometryReference",
            Self::Other(s) => s,
        }
    }

    pub fn is_axis(&self) -> bool {
        matches!(self, Self::Axis)
    }
}

/// `<Break>` of a `GeometryReference`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Break {
    pub dmx_break: u32,
    /// 1-based offset; 1 means "no offset".
    pub dmx_offset: u32,
}

/// Light output data of a `<Beam>`.
#[derive(Debug, Clone, PartialEq)]
pub struct BeamData {
    /// `Discharge`, `Tungsten`, `Halogen`, `LED` (as written).
    pub lamp_type: String,
    pub power_consumption_w: f32,
    pub luminous_flux_lm: f32,
    pub color_temperature_k: f32,
    pub beam_angle_deg: f32,
    pub field_angle_deg: f32,
    pub throw_ratio: f32,
    pub rectangle_ratio: f32,
    pub beam_radius_m: f32,
    /// `Wash`, `Spot`, `None`, `Rectangle`, `PC`, `Fresnel`, `Glow`.
    pub beam_type: String,
    pub color_rendering_index: f32,
}

impl Default for BeamData {
    /// The spec's defaults (table 41).
    fn default() -> Self {
        Self {
            lamp_type: "Discharge".into(),
            power_consumption_w: 1000.0,
            luminous_flux_lm: 10000.0,
            color_temperature_k: 6000.0,
            beam_angle_deg: 25.0,
            field_angle_deg: 25.0,
            throw_ratio: 1.0,
            rectangle_ratio: 1.7777,
            beam_radius_m: 0.05,
            beam_type: "Wash".into(),
            color_rendering_index: 100.0,
        }
    }
}

/// A `<DMXMode>`.
#[derive(Debug, Clone, Default)]
pub struct DmxMode {
    pub name: String,
    pub description: String,
    /// Root geometry of this mode (a top-level geometry name).
    pub geometry: String,
    pub channels: Vec<DmxChannel>,
}

/// `DMXBreak` of a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmxBreak {
    Number(u32),
    /// Taken from the `GeometryReference` that instantiates the channel.
    Overwrite,
}

/// A `<DMXChannel>` as written; see [`crate::dmx`] for absolute addresses.
#[derive(Debug, Clone)]
pub struct DmxChannel {
    pub dmx_break: DmxBreak,
    /// Relative addresses, most significant first. Empty for a virtual channel.
    pub offsets: Vec<u32>,
    pub geometry: String,
    pub initial_function: Option<String>,
    pub highlight: Option<String>,
    pub logical_channels: Vec<LogicalChannel>,
}

impl DmxChannel {
    /// A virtual channel consumes no DMX slot.
    pub fn is_virtual(&self) -> bool {
        self.offsets.is_empty()
    }

    /// Attribute of the first logical channel, or `""`.
    pub fn attribute(&self) -> &str {
        self.logical_channels.first().map(|l| l.attribute.as_str()).unwrap_or("")
    }

    /// The channel name per spec: `<Geometry>_<Attribute>`.
    pub fn name(&self) -> String {
        format!("{}_{}", self.geometry, self.attribute())
    }
}

#[derive(Debug, Clone, Default)]
pub struct LogicalChannel {
    pub attribute: String,
    pub snap: String,
    pub master: String,
    pub functions: Vec<ChannelFunction>,
}

#[derive(Debug, Clone, Default)]
pub struct ChannelFunction {
    pub name: String,
    pub attribute: String,
    pub original_attribute: String,
    /// `DMXFrom` as written, e.g. `"0/1"`.
    pub dmx_from: String,
    /// `Default` as written.
    pub default: String,
    pub physical_from: Option<f32>,
    pub physical_to: Option<f32>,
    pub wheel: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Revision {
    pub text: String,
    pub date: String,
    pub user_id: Option<u32>,
    pub modified_by: String,
}
