//! `GeneralSceneDescription.xml` → [`Scene`]. Tolerant: every problem becomes a
//! [`Warning`] and the reader carries on with a sensible default.

use crate::matrix::Matrix;
use crate::model::*;
use crate::values::*;
use crate::warning::Warning;
use crate::xml::{self, Element};
use std::collections::HashMap;
use uuid::Uuid;

pub(crate) struct Reader {
    pub warnings: Vec<Warning>,
    /// Context stack for warning messages (`Layer "A" / Fixture "B"`).
    path: Vec<String>,
    /// Bad UUID text → the UUID derived for it (only to warn once per text).
    bad_uuids: HashMap<String, Uuid>,
    nil_counter: u32,
}

/// Parse XML text into header and scene.
pub(crate) fn read_scene_xml(text: &str) -> (Header, Scene, Vec<Warning>) {
    let mut r = Reader { warnings: Vec::new(), path: Vec::new(), bad_uuids: HashMap::new(), nil_counter: 0 };
    let parsed = xml::parse(text);
    for p in parsed.problems {
        r.warn(p);
    }
    let Some(mut root) = parsed.root else {
        r.warn("no XML root element; scene is empty");
        return (Header::default(), Scene::default(), r.warnings);
    };
    root.strip_whitespace();
    let (header, scene) = r.document(&root);
    (header, scene, r.warnings)
}

impl Reader {
    pub fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(Warning { context: self.path.join(" / "), message: msg.into() });
    }

    fn document(&mut self, root: &Element) -> (Header, Scene) {
        if root.name != "GeneralSceneDescription" {
            self.warn(format!("root element is <{}>, expected <GeneralSceneDescription>", root.name));
        }
        let mut header = Header {
            version: Version { major: 1, minor: 0 },
            provider: root.attr("provider").unwrap_or_default().to_string(),
            provider_version: root.attr("providerVersion").unwrap_or_default().to_string(),
        };
        match (root.attr("verMajor").and_then(parse_i64), root.attr("verMinor").and_then(parse_i64)) {
            (Some(ma), Some(mi)) if ma >= 0 && mi >= 0 => {
                header.version = Version { major: ma as u32, minor: mi as u32 }
            }
            _ => self.warn("missing or bad verMajor/verMinor; assuming 1.0"),
        }
        if header.version.major != 1 {
            self.warn(format!("MVR major version {} is not 1; reading anyway", header.version.major));
        }

        let mut scene = Scene::default();
        for el in root.elements() {
            match el.name.as_str() {
                "UserData" => scene.user_data.extend(el.elements().cloned()),
                "Scene" => self.scene(el, &mut scene),
                _ => {
                    self.warn(format!("unknown node <{}> under root, kept", el.name));
                    scene.extra.push(el.clone());
                }
            }
        }
        if root.child("Scene").is_none() {
            self.warn("no <Scene> node");
        }
        (header, scene)
    }

    fn scene(&mut self, el: &Element, scene: &mut Scene) {
        for c in el.elements() {
            match c.name.as_str() {
                "AUXData" => self.aux(c, &mut scene.aux),
                "Layers" => {
                    for l in c.elements() {
                        if l.name == "Layer" {
                            let layer = self.layer(l);
                            scene.layers.push(layer);
                        } else {
                            self.warn(format!("<{}> inside <Layers> ignored", l.name));
                        }
                    }
                }
                _ => scene.extra.push(c.clone()),
            }
        }
    }

    fn aux(&mut self, el: &Element, aux: &mut AuxData) {
        self.path.push("AUXData".into());
        for c in el.elements() {
            match c.name.as_str() {
                "Symdef" => {
                    let uuid = self.object_uuid(c);
                    let name = c.attr("name").unwrap_or_default().to_string();
                    self.path.push(format!("Symdef {name:?}"));
                    let mut geometries = Vec::new();
                    // Spec: Symdef > ChildList > (Geometry3D | Symbol). Some writers
                    // put them directly under Symdef or in Geometries.
                    for list in c.elements() {
                        match list.name.as_str() {
                            "ChildList" | "Geometries" => {
                                geometries.extend(self.geometries(list));
                            }
                            "Geometry3D" | "Symbol" => {
                                geometries.extend(self.geometry(list));
                            }
                            other => self.warn(format!("<{other}> in Symdef ignored")),
                        }
                    }
                    self.path.pop();
                    aux.symdefs.push(Symdef { uuid, name, geometries });
                }
                "Position" => {
                    let uuid = self.object_uuid(c);
                    aux.positions.push(Position { uuid, name: c.attr("name").unwrap_or_default().to_string() });
                }
                "Class" => {
                    let uuid = self.object_uuid(c);
                    aux.classes.push(Class { uuid, name: c.attr("name").unwrap_or_default().to_string() });
                }
                "MappingDefinition" => {
                    let uuid = self.object_uuid(c);
                    let md = MappingDefinition {
                        uuid,
                        name: c.attr("name").unwrap_or_default().to_string(),
                        size_x: c.child("SizeX").and_then(|e| parse_i64(&e.text())).unwrap_or(0),
                        size_y: c.child("SizeY").and_then(|e| parse_i64(&e.text())).unwrap_or(0),
                        source: c.child("Source").map(source),
                        scale_handling: c.child("ScaleHandeling").map(|e| e.text()),
                    };
                    aux.mapping_definitions.push(md);
                }
                _ => aux.extra.push(c.clone()),
            }
        }
        self.path.pop();
    }

    fn layer(&mut self, el: &Element) -> Layer {
        let uuid = self.object_uuid(el);
        let name = el.attr("name").unwrap_or_default().to_string();
        self.path.push(format!("Layer {name:?}"));
        let mut layer = Layer::new(uuid, name);
        for c in el.elements() {
            match c.name.as_str() {
                "Matrix" => layer.matrix = self.matrix(c),
                "ChildList" => layer.children = self.child_list(c),
                _ => layer.extra.push(c.clone()),
            }
        }
        let m = layer.matrix;
        if !Matrix::translation(0.0, 0.0, m.o[2]).approx_eq(&m, 1e-6) {
            self.warn("layer matrix has more than an elevation (spec allows only z); kept");
        }
        self.path.pop();
        layer
    }

    fn child_list(&mut self, el: &Element) -> Vec<Object> {
        let mut out = Vec::new();
        for c in el.elements() {
            match ObjectKind::from_tag(&c.name) {
                Some(kind) => out.push(self.object(kind, c)),
                None => self.warn(format!("unknown object <{}> in ChildList skipped", c.name)),
            }
        }
        out
    }

    fn object(&mut self, kind: ObjectKind, el: &Element) -> Object {
        let uuid = self.object_uuid(el);
        let name = el.attr("name").unwrap_or_default().to_string();
        self.path.push(format!("{} {name:?}", kind.tag()));
        let mut o = Object::new(kind, uuid, name);
        if let Some(mp) = el.attr("multipatch") {
            o.multipatch = self.ref_uuid(mp);
        }
        for c in el.elements() {
            let text = || c.text();
            match c.name.as_str() {
                "Matrix" => o.matrix = self.matrix(c),
                "Classing" => o.classing = self.ref_uuid(&text()),
                "Geometries" => o.geometries = self.geometries(c),
                "GDTFSpec" => o.gdtf_spec = non_empty(text()),
                "GDTFMode" => o.gdtf_mode = non_empty(text()),
                "FixtureID" => o.fixture_id = text(),
                "FixtureIDNumeric" => o.fixture_id_numeric = self.int(c).unwrap_or(0),
                "UnitNumber" => o.unit_number = self.int(c).unwrap_or(0),
                "CustomId" => o.custom_id = self.int(c),
                "CustomIdType" => o.custom_id_type = self.int(c),
                "Focus" => o.focus = self.ref_uuid(&text()),
                "Position" => o.position = self.ref_uuid(&text()),
                "Function" => o.function = non_empty(text()),
                "CastShadow" => o.cast_shadow = self.boolean(c),
                "DMXInvertPan" => o.dmx_invert_pan = self.boolean(c),
                "DMXInvertTilt" => o.dmx_invert_tilt = self.boolean(c),
                "ChildPosition" => o.child_position = non_empty(text()),
                "ChainLength" => {
                    o.chain_length = parse_f64(&text());
                    if o.chain_length.is_none() && !text().is_empty() {
                        self.warn(format!("bad ChainLength {:?}", text()));
                    }
                }
                "Color" => o.color = self.color(&text()),
                "Gobo" => {
                    if let Some(file_name) = non_empty(text()) {
                        let rotation = c.attr("rotation").and_then(parse_f64).unwrap_or(0.0);
                        o.gobo = Some(Gobo { file_name, rotation });
                    }
                }
                "Addresses" => self.addresses(c, &mut o),
                "Protocols" => {
                    o.protocols = c
                        .elements()
                        .filter(|p| p.name == "Protocol")
                        .map(|p| Protocol {
                            geometry: attr_s(p, "geometry"),
                            name: attr_s(p, "name"),
                            protocol_type: attr_s(p, "type"),
                            version: attr_s(p, "version"),
                            transmission: attr_s(p, "transmission"),
                        })
                        .collect()
                }
                "Alignments" => {
                    for a in c.elements().filter(|a| a.name == "Alignment") {
                        let up = self.vec3_attr(a, "up");
                        let direction = self.vec3_attr(a, "direction");
                        o.alignments.push(Alignment { geometry: attr_s(a, "geometry"), up, direction });
                    }
                }
                "CustomCommands" => {
                    o.custom_commands = c.elements().filter(|e| e.name == "CustomCommand").map(|e| e.text()).collect()
                }
                "Overwrites" => {
                    for w in c.elements().filter(|w| w.name == "Overwrite") {
                        match w.attr("universal") {
                            Some(u) => {
                                o.overwrites.push(Overwrite { universal: u.to_string(), target: attr_s(w, "target") })
                            }
                            None => self.warn("Overwrite without universal skipped"),
                        }
                    }
                }
                "Connections" => {
                    for k in c.elements().filter(|k| k.name == "Connection") {
                        let to = k.attr("toObject").and_then(|t| self.ref_uuid(t));
                        match to {
                            Some(to_object) => o.connections.push(Connection {
                                own: k.attr("own").unwrap_or_default().to_string(),
                                other: k.attr("other").unwrap_or_default().to_string(),
                                to_object,
                            }),
                            None => self.warn("Connection without toObject skipped"),
                        }
                    }
                }
                "Mappings" => {
                    for m in c.elements().filter(|m| m.name == "Mapping") {
                        let Some(linked_def) = m.attr("linkedDef").and_then(|t| self.ref_uuid(t)) else {
                            self.warn("Mapping without linkedDef skipped");
                            continue;
                        };
                        let i = |n: &str| m.child(n).and_then(|e| parse_i64(&e.text()));
                        o.mappings.push(Mapping {
                            linked_def,
                            ux: i("ux"),
                            uy: i("uy"),
                            ox: i("ox"),
                            oy: i("oy"),
                            rz: m.child("rz").and_then(|e| parse_f64(&e.text())),
                        });
                    }
                }
                "Sources" => o.sources = c.elements().filter(|s| s.name == "Source").map(source).collect(),
                "Projections" => {
                    o.projections = c
                        .elements()
                        .filter(|p| p.name == "Projection")
                        .map(|p| Projection {
                            source: p.child("Source").map(source),
                            scale_handling: p.child("ScaleHandeling").map(|e| e.text()),
                        })
                        .collect()
                }
                "ChildList" => o.children.extend(self.child_list(c)),
                _ => o.extra.push(c.clone()),
            }
        }
        if o.gdtf_spec.is_some() && o.gdtf_mode.is_none() {
            self.warn("GDTFSpec without GDTFMode");
        }
        self.path.pop();
        o
    }

    fn addresses(&mut self, el: &Element, o: &mut Object) {
        for a in el.elements() {
            match a.name.as_str() {
                "Address" => {
                    let brk = match a.attr("break") {
                        None => 0,
                        Some(b) => match parse_i64(b) {
                            Some(v) if v >= 0 => v as u32,
                            _ => {
                                self.warn(format!("bad DMX break {b:?}; using 0"));
                                0
                            }
                        },
                    };
                    let t = a.text();
                    if t.is_empty() {
                        self.warn("empty <Address> skipped");
                        continue;
                    }
                    if t == "0" {
                        continue; // some tools write 0 for "not patched"
                    }
                    match parse_address(&t) {
                        Some(abs) => o.addresses.push(DmxAddress { dmx_break: brk, absolute: abs }),
                        None => self.warn(format!("bad DMX address {t:?} skipped")),
                    }
                }
                "Network" => {
                    let geometry = a.attr("geometry").or_else(|| a.attr("Interface")).unwrap_or_default().to_string();
                    o.networks.push(Network {
                        geometry,
                        ipv4: attr_s(a, "ipv4"),
                        subnet_mask: attr_s(a, "subnetmask"),
                        ipv6: attr_s(a, "ipv6"),
                        dhcp: a.attr("dhcp").and_then(parse_bool),
                        hostname: attr_s(a, "hostname"),
                    });
                }
                other => self.warn(format!("<{other}> in Addresses ignored")),
            }
        }
    }

    fn geometries(&mut self, el: &Element) -> Vec<Geometry> {
        let mut out = Vec::new();
        for c in el.elements() {
            out.extend(self.geometry(c));
        }
        out
    }

    fn geometry(&mut self, c: &Element) -> Option<Geometry> {
        let matrix = c.child("Matrix").map(|m| self.matrix(m)).unwrap_or_default();
        match c.name.as_str() {
            "Geometry3D" => match c.attr("fileName").and_then(|f| non_empty(f.to_string())) {
                Some(file_name) => Some(Geometry::Geometry3D { file_name, matrix }),
                None => {
                    self.warn("Geometry3D without fileName skipped");
                    None
                }
            },
            "Symbol" => {
                let uuid = self.object_uuid(c);
                match c.attr("symdef").and_then(|s| self.ref_uuid(s)) {
                    Some(symdef) => Some(Geometry::Symbol { uuid, symdef, matrix }),
                    None => {
                        self.warn("Symbol without symdef skipped");
                        None
                    }
                }
            }
            other => {
                self.warn(format!("<{other}> in Geometries ignored"));
                None
            }
        }
    }

    fn matrix(&mut self, el: &Element) -> Matrix {
        let t = el.text();
        if t.is_empty() {
            return Matrix::IDENTITY;
        }
        match Matrix::parse(&t) {
            Ok(m) => m,
            Err(e) => {
                self.warn(format!("{e}; using identity"));
                Matrix::IDENTITY
            }
        }
    }

    fn int(&mut self, el: &Element) -> Option<i64> {
        let t = el.text();
        if t.is_empty() {
            return None;
        }
        let v = parse_i64(&t);
        if v.is_none() {
            self.warn(format!("bad integer {t:?} in <{}>", el.name));
        }
        v
    }

    fn boolean(&mut self, el: &Element) -> Option<bool> {
        let t = el.text();
        if t.is_empty() {
            return None;
        }
        let v = parse_bool(&t);
        if v.is_none() {
            self.warn(format!("bad bool {t:?} in <{}>", el.name));
        }
        v
    }

    fn color(&mut self, t: &str) -> Option<CieColor> {
        if t.is_empty() {
            return None;
        }
        match parse_vec3(t) {
            Some([x, y, big_y]) => Some(CieColor { x, y, big_y }),
            None => {
                self.warn(format!("bad CIE colour {t:?}"));
                None
            }
        }
    }

    fn vec3_attr(&mut self, el: &Element, name: &str) -> Option<[f64; 3]> {
        let t = el.attr(name)?;
        let v = parse_vec3(t);
        if v.is_none() {
            self.warn(format!("bad vector {t:?} in {name}"));
        }
        v
    }

    /// The `uuid` attribute of an identity-bearing node. Never fails: missing, nil
    /// or malformed UUIDs get a deterministic replacement and a warning.
    fn object_uuid(&mut self, el: &Element) -> Uuid {
        match el.attr("uuid") {
            None | Some("") => {
                self.nil_counter += 1;
                self.warn(format!("<{}> without uuid; generated one", el.name));
                derived_uuid(&format!("missing:{}", self.nil_counter))
            }
            Some(t) => match parse_uuid_strict(t) {
                Some(u) if !u.is_nil() => u,
                Some(_) => {
                    self.nil_counter += 1;
                    self.warn(format!("<{}> has the nil uuid; generated one", el.name));
                    derived_uuid(&format!("nil:{}", self.nil_counter))
                }
                None => self.bad_uuid(t),
            },
        }
    }

    /// A UUID reference. Empty or nil means "no reference".
    fn ref_uuid(&mut self, t: &str) -> Option<Uuid> {
        let t = t.trim();
        if t.is_empty() {
            return None;
        }
        match parse_uuid_strict(t) {
            Some(u) if u.is_nil() => None,
            Some(u) => Some(u),
            None => Some(self.bad_uuid(t)),
        }
    }

    fn bad_uuid(&mut self, t: &str) -> Uuid {
        if let Some(u) = self.bad_uuids.get(t) {
            return *u;
        }
        let u = derived_uuid(t);
        self.warn(format!("malformed uuid {t:?}; mapped to {}", fmt_uuid(&u)));
        self.bad_uuids.insert(t.to_string(), u);
        u
    }
}

fn source(el: &Element) -> Source {
    Source {
        linked_geometry: el.attr("linkedGeometry").unwrap_or_default().to_string(),
        source_type: el.attr("type").unwrap_or_default().to_string(),
        value: el.text(),
    }
}

fn attr_s(el: &Element, name: &str) -> Option<String> {
    el.attr(name).map(str::to_string)
}

fn non_empty(s: String) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Absolute address from `"45"` or `"Universe.Address"` (`"2.17"` → 529).
pub(crate) fn parse_address(t: &str) -> Option<u32> {
    let t = t.trim();
    if let Some((u, a)) = t.split_once('.') {
        let u: u32 = u.trim().parse().ok()?;
        let a: u32 = a.trim().parse().ok()?;
        if u == 0 || a == 0 || a > 512 {
            return None;
        }
        return u.checked_sub(1)?.checked_mul(512)?.checked_add(a);
    }
    let v: i64 = t.parse().ok()?;
    (v >= 1 && v <= u32::MAX as i64).then_some(v as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_forms() {
        assert_eq!(parse_address("45"), Some(45));
        assert_eq!(parse_address("2.17"), Some(529));
        assert_eq!(parse_address("0"), None);
        assert_eq!(parse_address("1.513"), None);
        assert_eq!(parse_address("x"), None);
    }
}
