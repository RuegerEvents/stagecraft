//! [`Scene`] → `GeneralSceneDescription.xml`. Deterministic: the same scene gives
//! the same bytes. Children are written in a fixed order (the order of the spec
//! tables), followed by unknown nodes kept from import.

use crate::matrix::Matrix;
use crate::model::*;
use crate::values::*;
use crate::xml::{self, Element};

/// The `provider` this crate writes.
pub const PROVIDER: &str = "StageCraft";

/// Serialize the scene. The header is always MVR 1.6 / `provider="StageCraft"`.
pub fn write_scene_xml(scene: &Scene) -> String {
    let mut root = Element::new("GeneralSceneDescription")
        .with_attr("verMajor", Version::CURRENT.major.to_string())
        .with_attr("verMinor", Version::CURRENT.minor.to_string())
        .with_attr("provider", PROVIDER)
        .with_attr("providerVersion", env!("CARGO_PKG_VERSION"));
    if !scene.user_data.is_empty() {
        let mut ud = Element::new("UserData");
        ud.children.extend(scene.user_data.iter().cloned().map(xml::Node::Element));
        root.push(ud);
    }
    let mut sc = Element::new("Scene");
    let aux = aux(&scene.aux);
    if !aux.children.is_empty() {
        sc.push(aux);
    }
    let mut layers = Element::new("Layers");
    for l in &scene.layers {
        layers.push(layer(l));
    }
    sc.push(layers);
    for e in &scene.extra {
        sc.push(e.clone());
    }
    root.push(sc);
    xml::write_document(&root)
}

fn aux(a: &AuxData) -> Element {
    let mut el = Element::new("AUXData");
    for s in &a.symdefs {
        let mut sd = Element::new("Symdef").with_attr("uuid", fmt_uuid(&s.uuid)).with_attr("name", &s.name);
        let mut cl = Element::new("ChildList");
        for g in &s.geometries {
            cl.push(geometry(g));
        }
        sd.push(cl);
        el.push(sd);
    }
    for p in &a.positions {
        el.push(Element::new("Position").with_attr("uuid", fmt_uuid(&p.uuid)).with_attr("name", &p.name));
    }
    for m in &a.mapping_definitions {
        let mut md = Element::new("MappingDefinition").with_attr("uuid", fmt_uuid(&m.uuid)).with_attr("name", &m.name);
        md.push(text_el("SizeX", m.size_x.to_string()));
        md.push(text_el("SizeY", m.size_y.to_string()));
        if let Some(s) = &m.source {
            md.push(source(s));
        }
        if let Some(s) = &m.scale_handling {
            md.push(text_el("ScaleHandeling", s));
        }
        el.push(md);
    }
    for c in &a.classes {
        el.push(Element::new("Class").with_attr("uuid", fmt_uuid(&c.uuid)).with_attr("name", &c.name));
    }
    for e in &a.extra {
        el.push(e.clone());
    }
    el
}

fn layer(l: &Layer) -> Element {
    let mut el = Element::new("Layer").with_attr("uuid", fmt_uuid(&l.uuid)).with_attr("name", &l.name);
    if !l.matrix.is_identity() {
        el.push(matrix(&l.matrix));
    }
    if !l.children.is_empty() {
        let mut cl = Element::new("ChildList");
        for o in &l.children {
            cl.push(object(o));
        }
        el.push(cl);
    }
    for e in &l.extra {
        el.push(e.clone());
    }
    el
}

fn object(o: &Object) -> Element {
    let k = o.kind;
    let mut el = Element::new(k.tag()).with_attr("uuid", fmt_uuid(&o.uuid)).with_attr("name", &o.name);
    if let Some(mp) = &o.multipatch {
        el.attrs.push(("multipatch".into(), fmt_uuid(mp)));
    }
    if !o.matrix.is_identity() {
        el.push(matrix(&o.matrix));
    }
    if let Some(c) = &o.classing {
        el.push(text_el("Classing", fmt_uuid(c)));
    }
    if k.requires_geometries() || !o.geometries.is_empty() {
        let mut g = Element::new("Geometries");
        for x in &o.geometries {
            g.push(geometry(x));
        }
        el.push(g);
    }
    if let Some(s) = &o.gdtf_spec {
        el.push(text_el("GDTFSpec", s));
    }
    if let Some(s) = &o.gdtf_mode {
        el.push(text_el("GDTFMode", s));
    }
    if let Some(u) = &o.focus {
        el.push(text_el("Focus", fmt_uuid(u)));
    }
    if let Some(b) = o.cast_shadow {
        el.push(text_el("CastShadow", b.to_string()));
    }
    if let Some(b) = o.dmx_invert_pan {
        el.push(text_el("DMXInvertPan", b.to_string()));
    }
    if let Some(b) = o.dmx_invert_tilt {
        el.push(text_el("DMXInvertTilt", b.to_string()));
    }
    if let Some(u) = &o.position {
        el.push(text_el("Position", fmt_uuid(u)));
    }
    if let Some(s) = &o.function {
        el.push(text_el("Function", s));
    }
    if let Some(v) = o.chain_length {
        el.push(text_el("ChainLength", fmt_f64(v)));
    }
    if k.has_fixture_ids() || !o.fixture_id.is_empty() || o.fixture_id_numeric != 0 {
        el.push(text_el("FixtureID", &o.fixture_id));
        el.push(text_el("FixtureIDNumeric", o.fixture_id_numeric.to_string()));
    }
    if k == ObjectKind::Fixture || o.unit_number != 0 {
        el.push(text_el("UnitNumber", o.unit_number.to_string()));
    }
    if let Some(s) = &o.child_position {
        el.push(text_el("ChildPosition", s));
    }
    if !o.addresses.is_empty() || !o.networks.is_empty() {
        let mut a = Element::new("Addresses");
        for d in &o.addresses {
            a.push(
                Element::new("Address").with_attr("break", d.dmx_break.to_string()).with_text(d.absolute.to_string()),
            );
        }
        for n in &o.networks {
            let mut ne = Element::new("Network").with_attr("geometry", &n.geometry);
            opt_attr(&mut ne, "ipv4", &n.ipv4);
            opt_attr(&mut ne, "subnetmask", &n.subnet_mask);
            opt_attr(&mut ne, "ipv6", &n.ipv6);
            if let Some(d) = n.dhcp {
                ne.attrs.push(("dhcp".into(), d.to_string()));
            }
            opt_attr(&mut ne, "hostname", &n.hostname);
            a.push(ne);
        }
        el.push(a);
    }
    if !o.protocols.is_empty() {
        let mut ps = Element::new("Protocols");
        for p in &o.protocols {
            let mut pe = Element::new("Protocol");
            opt_attr(&mut pe, "geometry", &p.geometry);
            opt_attr(&mut pe, "name", &p.name);
            opt_attr(&mut pe, "type", &p.protocol_type);
            opt_attr(&mut pe, "version", &p.version);
            opt_attr(&mut pe, "transmission", &p.transmission);
            ps.push(pe);
        }
        el.push(ps);
    }
    if !o.alignments.is_empty() {
        let mut al = Element::new("Alignments");
        for a in &o.alignments {
            let mut ae = Element::new("Alignment");
            opt_attr(&mut ae, "geometry", &a.geometry);
            if let Some(v) = a.up {
                ae.attrs.push(("up".into(), fmt_vec3(v)));
            }
            if let Some(v) = a.direction {
                ae.attrs.push(("direction".into(), fmt_vec3(v)));
            }
            al.push(ae);
        }
        el.push(al);
    }
    if !o.custom_commands.is_empty() {
        let mut cc = Element::new("CustomCommands");
        for c in &o.custom_commands {
            cc.push(text_el("CustomCommand", c));
        }
        el.push(cc);
    }
    if !o.overwrites.is_empty() {
        let mut ow = Element::new("Overwrites");
        for w in &o.overwrites {
            let mut we = Element::new("Overwrite").with_attr("universal", &w.universal);
            opt_attr(&mut we, "target", &w.target);
            ow.push(we);
        }
        el.push(ow);
    }
    if !o.connections.is_empty() {
        let mut cs = Element::new("Connections");
        for c in &o.connections {
            cs.push(
                Element::new("Connection")
                    .with_attr("own", &c.own)
                    .with_attr("other", &c.other)
                    .with_attr("toObject", fmt_uuid(&c.to_object)),
            );
        }
        el.push(cs);
    }
    if let Some(c) = &o.color {
        el.push(text_el("Color", fmt_vec3([c.x, c.y, c.big_y])));
    }
    if let Some(t) = o.custom_id_type {
        el.push(text_el("CustomIdType", t.to_string()));
    }
    if let Some(t) = o.custom_id {
        el.push(text_el("CustomId", t.to_string()));
    }
    if !o.mappings.is_empty() {
        let mut ms = Element::new("Mappings");
        for m in &o.mappings {
            let mut me = Element::new("Mapping").with_attr("linkedDef", fmt_uuid(&m.linked_def));
            for (n, v) in [("ux", m.ux), ("uy", m.uy), ("ox", m.ox), ("oy", m.oy)] {
                if let Some(v) = v {
                    me.push(text_el(n, v.to_string()));
                }
            }
            if let Some(rz) = m.rz {
                me.push(text_el("rz", fmt_f64(rz)));
            }
            ms.push(me);
        }
        el.push(ms);
    }
    if let Some(g) = &o.gobo {
        el.push(Element::new("Gobo").with_attr("rotation", fmt_f64(g.rotation)).with_text(&g.file_name));
    }
    if !o.sources.is_empty() {
        let mut ss = Element::new("Sources");
        for s in &o.sources {
            ss.push(source(s));
        }
        el.push(ss);
    }
    if !o.projections.is_empty() || k == ObjectKind::Projector {
        let mut ps = Element::new("Projections");
        for p in &o.projections {
            let mut pe = Element::new("Projection");
            if let Some(s) = &p.source {
                pe.push(source(s));
            }
            if let Some(s) = &p.scale_handling {
                pe.push(text_el("ScaleHandeling", s));
            }
            ps.push(pe);
        }
        el.push(ps);
    }
    for e in &o.extra {
        el.push(e.clone());
    }
    if !o.children.is_empty() {
        let mut cl = Element::new("ChildList");
        for c in &o.children {
            cl.push(object(c));
        }
        el.push(cl);
    }
    el
}

fn geometry(g: &Geometry) -> Element {
    match g {
        Geometry::Geometry3D { file_name, matrix: m } => {
            let mut e = Element::new("Geometry3D").with_attr("fileName", file_name);
            if !m.is_identity() {
                e.push(matrix(m));
            }
            e
        }
        Geometry::Symbol { uuid, symdef, matrix: m } => {
            let mut e = Element::new("Symbol").with_attr("uuid", fmt_uuid(uuid)).with_attr("symdef", fmt_uuid(symdef));
            if !m.is_identity() {
                e.push(matrix(m));
            }
            e
        }
    }
}

fn source(s: &Source) -> Element {
    Element::new("Source")
        .with_attr("linkedGeometry", &s.linked_geometry)
        .with_attr("type", &s.source_type)
        .with_text(&s.value)
}

fn matrix(m: &Matrix) -> Element {
    text_el("Matrix", m.to_mvr_string())
}

fn text_el(name: &str, text: impl Into<String>) -> Element {
    let t: String = text.into();
    let e = Element::new(name);
    if t.is_empty() { e } else { e.with_text(t) }
}

fn opt_attr(el: &mut Element, name: &str, v: &Option<String>) {
    if let Some(v) = v {
        el.attrs.push((name.into(), v.clone()));
    }
}
