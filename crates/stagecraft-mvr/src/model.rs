//! The neutral scene model. Plain data: StageCraft (or any other tool) maps it to
//! its own document. Lengths are mm, angles degrees, axes as in MVR (x right from
//! the audience, y upstage, z up).

use crate::matrix::Matrix;
use crate::xml::Element;
use uuid::Uuid;

/// MVR file format version (`verMajor`.`verMinor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
}

impl Version {
    /// The version this crate writes.
    pub const CURRENT: Version = Version { major: 1, minor: 6 };
}

impl Default for Version {
    fn default() -> Self {
        Self::CURRENT
    }
}

/// The `GeneralSceneDescription` attributes. Import fills it from the file; export
/// always writes version 1.6 and `provider="StageCraft"`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Header {
    pub version: Version,
    pub provider: String,
    pub provider_version: String,
}

/// Everything in `GeneralSceneDescription.xml` except the header.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    /// `UserData/Data` nodes, kept raw (vendor data, e.g. Vectorworks' lit files).
    pub user_data: Vec<Element>,
    pub layers: Vec<Layer>,
    pub aux: AuxData,
    /// Unknown children of `Scene` (kept raw and written back).
    pub extra: Vec<Element>,
}

/// `AUXData`: definitions that objects refer to by UUID.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AuxData {
    pub symdefs: Vec<Symdef>,
    pub positions: Vec<Position>,
    pub mapping_definitions: Vec<MappingDefinition>,
    pub classes: Vec<Class>,
    pub extra: Vec<Element>,
}

/// A layer: a container whose matrix may only lift it (elevation).
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub uuid: Uuid,
    pub name: String,
    /// Local transform (the spec allows only a z offset; others are kept as read).
    pub matrix: Matrix,
    pub children: Vec<Object>,
    pub extra: Vec<Element>,
}

impl Layer {
    pub fn new(uuid: Uuid, name: impl Into<String>) -> Self {
        Layer { uuid, name: name.into(), matrix: Matrix::IDENTITY, children: Vec::new(), extra: Vec::new() }
    }
}

/// The node type of a scene object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ObjectKind {
    SceneObject,
    GroupObject,
    FocusPoint,
    Fixture,
    Truss,
    Support,
    VideoScreen,
    Projector,
}

impl ObjectKind {
    pub const ALL: [ObjectKind; 8] = [
        ObjectKind::SceneObject,
        ObjectKind::GroupObject,
        ObjectKind::FocusPoint,
        ObjectKind::Fixture,
        ObjectKind::Truss,
        ObjectKind::Support,
        ObjectKind::VideoScreen,
        ObjectKind::Projector,
    ];

    /// The XML node name.
    pub fn tag(self) -> &'static str {
        match self {
            ObjectKind::SceneObject => "SceneObject",
            ObjectKind::GroupObject => "GroupObject",
            ObjectKind::FocusPoint => "FocusPoint",
            ObjectKind::Fixture => "Fixture",
            ObjectKind::Truss => "Truss",
            ObjectKind::Support => "Support",
            ObjectKind::VideoScreen => "VideoScreen",
            ObjectKind::Projector => "Projector",
        }
    }

    pub fn from_tag(tag: &str) -> Option<ObjectKind> {
        ObjectKind::ALL.into_iter().find(|k| k.tag() == tag)
    }

    /// Kinds whose spec lists a mandatory `Geometries` child.
    pub fn requires_geometries(self) -> bool {
        !matches!(self, ObjectKind::GroupObject | ObjectKind::Fixture)
    }

    /// Kinds that carry FixtureID / FixtureIDNumeric (mandatory there).
    pub fn has_fixture_ids(self) -> bool {
        !matches!(self, ObjectKind::GroupObject | ObjectKind::FocusPoint)
    }
}

/// One object of the scene tree. All MVR object node types share this struct;
/// `kind` says which node it is, and fields a kind does not use stay empty.
///
/// The spec lists which children each node type may have; import reads every
/// known child on every kind (tolerance), export writes the fields that are set
/// plus the ones the spec makes mandatory for the kind.
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub kind: ObjectKind,
    pub uuid: Uuid,
    pub name: String,
    /// Transform relative to the parent object (or the layer).
    pub matrix: Matrix,
    /// Transform to scene space, computed by [`Scene::resolve_world`] (import does
    /// it). For a child with `child_position` this ignores the parent GDTF's
    /// geometry offset, which needs the GDTF to resolve.
    pub world: Matrix,
    /// UUID of a [`Class`].
    pub classing: Option<Uuid>,
    /// 3D representation (files in the archive or symbol instances).
    pub geometries: Vec<Geometry>,
    /// UUID of the multipatch parent.
    pub multipatch: Option<Uuid>,
    /// GDTF file name in the archive, as written (some exporters omit `.gdtf`;
    /// use [`crate::Resources::resolve_gdtf`] to find the file).
    pub gdtf_spec: Option<String>,
    pub gdtf_mode: Option<String>,
    /// FixtureID (text, may hold prefixes like `A12`). Empty when unset.
    pub fixture_id: String,
    /// FixtureIDNumeric, 0 when unset.
    pub fixture_id_numeric: i64,
    /// UnitNumber, 0 when unset.
    pub unit_number: i64,
    pub custom_id: Option<i64>,
    /// CustomIdType: 0 undefined, 1 fixture, 2 channel, 4 houselights, 5 non-dim,
    /// 6 media, 7 fog, 8 effect, 9 pyro, 10 marker.
    pub custom_id_type: Option<i64>,
    /// UUID of the focus point a fixture aims at.
    pub focus: Option<Uuid>,
    /// UUID of a [`Position`].
    pub position: Option<Uuid>,
    pub function: Option<String>,
    pub cast_shadow: Option<bool>,
    pub dmx_invert_pan: Option<bool>,
    pub dmx_invert_tilt: Option<bool>,
    /// Node link into the parent's GDTF geometry tree (children of fixtures).
    pub child_position: Option<String>,
    /// Support: chain length in mm.
    pub chain_length: Option<f64>,
    pub color: Option<CieColor>,
    pub gobo: Option<Gobo>,
    /// DMX addresses, one per break.
    pub addresses: Vec<DmxAddress>,
    pub networks: Vec<Network>,
    pub protocols: Vec<Protocol>,
    pub alignments: Vec<Alignment>,
    pub custom_commands: Vec<String>,
    pub overwrites: Vec<Overwrite>,
    pub connections: Vec<Connection>,
    pub mappings: Vec<Mapping>,
    /// VideoScreen: video sources.
    pub sources: Vec<Source>,
    /// Projector: projections.
    pub projections: Vec<Projection>,
    /// Nested objects (`ChildList`), placed relative to this object.
    pub children: Vec<Object>,
    /// Unknown child nodes (e.g. Vectorworks' `FixtureTypeId`), kept raw.
    pub extra: Vec<Element>,
}

impl Object {
    /// An empty object of `kind` at the parent's origin.
    pub fn new(kind: ObjectKind, uuid: Uuid, name: impl Into<String>) -> Self {
        Object {
            kind,
            uuid,
            name: name.into(),
            matrix: Matrix::IDENTITY,
            world: Matrix::IDENTITY,
            classing: None,
            geometries: Vec::new(),
            multipatch: None,
            gdtf_spec: None,
            gdtf_mode: None,
            fixture_id: String::new(),
            fixture_id_numeric: 0,
            unit_number: 0,
            custom_id: None,
            custom_id_type: None,
            focus: None,
            position: None,
            function: None,
            cast_shadow: None,
            dmx_invert_pan: None,
            dmx_invert_tilt: None,
            child_position: None,
            chain_length: None,
            color: None,
            gobo: None,
            addresses: Vec::new(),
            networks: Vec::new(),
            protocols: Vec::new(),
            alignments: Vec::new(),
            custom_commands: Vec::new(),
            overwrites: Vec::new(),
            connections: Vec::new(),
            mappings: Vec::new(),
            sources: Vec::new(),
            projections: Vec::new(),
            children: Vec::new(),
            extra: Vec::new(),
        }
    }

    /// World position of the origin, mm.
    pub fn world_position(&self) -> [f64; 3] {
        self.world.o
    }
}

/// A 3D representation entry in `Geometries` or a `Symdef`.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// A model file in the archive (`.3ds` in mm, or `.glb`/`.gltf`).
    Geometry3D { file_name: String, matrix: Matrix },
    /// An instance of a [`Symdef`].
    Symbol { uuid: Uuid, symdef: Uuid, matrix: Matrix },
}

impl Geometry {
    pub fn matrix(&self) -> &Matrix {
        match self {
            Geometry::Geometry3D { matrix, .. } | Geometry::Symbol { matrix, .. } => matrix,
        }
    }
}

/// A reusable graphic, instanced by [`Geometry::Symbol`].
#[derive(Debug, Clone, PartialEq)]
pub struct Symdef {
    pub uuid: Uuid,
    pub name: String,
    pub geometries: Vec<Geometry>,
}

/// A logical position (e.g. a truss name) fixtures and trusses can belong to.
#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    pub uuid: Uuid,
    pub name: String,
}

/// A class for visibility filtering across layers.
#[derive(Debug, Clone, PartialEq)]
pub struct Class {
    pub uuid: Uuid,
    pub name: String,
}

/// An input source for pixel mapping.
#[derive(Debug, Clone, PartialEq)]
pub struct MappingDefinition {
    pub uuid: Uuid,
    pub name: String,
    pub size_x: i64,
    pub size_y: i64,
    pub source: Option<Source>,
    /// `ScaleKeepRatio` (default), `ScaleIgnoreRatio` or `KeepSizeCenter`.
    pub scale_handling: Option<String>,
}

/// A CIE 1931 xyY colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CieColor {
    pub x: f64,
    pub y: f64,
    /// Luminance Y.
    pub big_y: f64,
}

/// The gobo of a fixture: an image file in the archive and its rotation.
#[derive(Debug, Clone, PartialEq)]
pub struct Gobo {
    pub file_name: String,
    /// Degrees.
    pub rotation: f64,
}

/// A DMX address. Stored absolute (`(universe - 1) * 512 + address`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DmxAddress {
    /// The DMX break of the GDTF mode this address belongs to (0 for most).
    pub dmx_break: u32,
    /// Absolute address, starting at 1.
    pub absolute: u32,
}

impl DmxAddress {
    pub fn new(dmx_break: u32, universe: u32, address: u32) -> Self {
        DmxAddress { dmx_break, absolute: universe.saturating_sub(1) * 512 + address }
    }

    /// Universe, starting at 1.
    pub fn universe(&self) -> u32 {
        self.absolute.saturating_sub(1) / 512 + 1
    }

    /// Address within the universe, 1..=512.
    pub fn address(&self) -> u32 {
        (self.absolute.saturating_sub(1)) % 512 + 1
    }
}

/// A network address of a fixture interface.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Network {
    /// Wire geometry name in the GDTF (MVR 1.5 called it `Interface`).
    pub geometry: String,
    pub ipv4: Option<String>,
    pub subnet_mask: Option<String>,
    pub ipv6: Option<String>,
    pub dhcp: Option<bool>,
    pub hostname: Option<String>,
}

/// A protocol assignment (Art-Net, sACN, NDI …).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Protocol {
    pub geometry: Option<String>,
    pub name: Option<String>,
    pub protocol_type: Option<String>,
    pub version: Option<String>,
    pub transmission: Option<String>,
}

/// A custom beam alignment.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Alignment {
    pub geometry: Option<String>,
    pub up: Option<[f64; 3]>,
    pub direction: Option<[f64; 3]>,
}

/// A wheel/emitter/filter overwrite against the universal GDTF.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Overwrite {
    pub universal: String,
    pub target: Option<String>,
}

/// A wire from one of this object's geometries to another object.
#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    pub own: String,
    pub other: String,
    pub to_object: Uuid,
}

/// A pixel mapping of a fixture into a [`MappingDefinition`].
#[derive(Debug, Clone, PartialEq)]
pub struct Mapping {
    pub linked_def: Uuid,
    pub ux: Option<i64>,
    pub uy: Option<i64>,
    pub ox: Option<i64>,
    pub oy: Option<i64>,
    /// Degrees.
    pub rz: Option<f64>,
}

/// A media source (VideoScreen, Projection, MappingDefinition).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Source {
    pub linked_geometry: String,
    /// `NDI`, `File`, `CITP`, `CaptureDevice`.
    pub source_type: String,
    /// Stream name, file name in the archive, or capture device name.
    pub value: String,
}

/// A projection of a projector.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Projection {
    pub source: Option<Source>,
    pub scale_handling: Option<String>,
}
