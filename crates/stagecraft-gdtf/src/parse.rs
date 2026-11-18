//! `description.xml` → [`FixtureType`]. Tolerant: anything odd becomes a warning.

use std::collections::BTreeMap;

use craft_xml::El;
use glam::Mat4;

/// An element of `description.xml`.
type Node<'a> = &'a El;

use crate::types::*;
use crate::{GdtfError, Warning};

/// What a `description.xml` may hold. Real ones are a few MB with tens of thousands of
/// elements (pixel bars with many emitters).
const LIMITS: craft_xml::Limits = craft_xml::Limits {
    max_input: 256 << 20,
    max_depth: 256,
    max_nodes: 8_000_000,
    max_elements: usize::MAX,
    max_attrs: 1024,
    max_attrs_total: 16_000_000,
};

pub(crate) fn parse_description(xml: &[u8], warnings: &mut Vec<Warning>) -> Result<FixtureType, GdtfError> {
    let text = decode(xml, warnings);
    // craft-xml: no DTD processing, no entity expansion, bounded. Names by local part, as
    // GDTF has no namespaces but some exporters add one. End tags must match; the read stops
    // at the end of the root, so junk after </GDTF> (some exporters pad with NUL bytes or
    // append data) is not looked at.
    let opts = craft_xml::Options { limits: LIMITS, strict_end_tags: true, ..craft_xml::Options::local() };
    let trimmed = text.trim_end_matches(|c: char| c == '\0' || c.is_whitespace());
    let doc = opts.parse(trimmed.as_bytes()).map_err(|e| GdtfError::Xml(e.to_string()))?;
    if let Some(end) = trimmed.rfind("</GDTF>")
        && end + "</GDTF>".len() < trimmed.len()
    {
        warnings.push(Warning::new("xml", "ignored data after </GDTF>"));
    }
    let root = &doc;
    if root.name.as_str() != "GDTF" {
        warnings.push(Warning::new("xml", format!("root element is <{}>, expected <GDTF>", root.name.as_str())));
    }
    let data_version = root.attr("DataVersion").unwrap_or("").to_string();
    let ft = if root.name.as_str() == "FixtureType" {
        root
    } else {
        child(root, "FixtureType").ok_or(GdtfError::NoFixtureType)?
    };

    let mut p = Parser { warnings };
    let mut out = FixtureType {
        data_version,
        name: attr(ft, "Name"),
        short_name: attr(ft, "ShortName"),
        long_name: attr(ft, "LongName"),
        manufacturer: attr(ft, "Manufacturer"),
        description: attr(ft, "Description"),
        fixture_type_id: attr(ft, "FixtureTypeID"),
        ref_ft: opt_attr(ft, "RefFT"),
        thumbnail: opt_attr(ft, "Thumbnail"),
        thumbnail_offset: (
            p.int(ft, "ThumbnailOffsetX", 0, "FixtureType"),
            p.int(ft, "ThumbnailOffsetY", 0, "FixtureType"),
        ),
        can_have_children: !ft.attr("CanHaveChildren").is_some_and(|v| v.eq_ignore_ascii_case("no")),
        ..FixtureType::default()
    };
    if out.name.is_empty() {
        p.warn("FixtureType", "no Name");
    }

    if let Some(pd) = child(ft, "PhysicalDescriptions") {
        out.physical = p.physical(pd);
    }
    if let Some(models) = child(ft, "Models") {
        for m in models.elements().filter(|n| n.name == "Model") {
            out.models.push(p.model(m));
        }
    }
    match child(ft, "Geometries") {
        Some(g) => {
            for n in g.elements() {
                if let Some(geo) = p.geometry(n, 0) {
                    out.geometries.push(geo);
                }
            }
        }
        None => p.warn("Geometries", "missing"),
    }
    match child(ft, "DMXModes") {
        Some(m) => {
            for n in m.elements().filter(|n| n.name == "DMXMode") {
                out.modes.push(p.mode(n));
            }
        }
        None => p.warn("DMXModes", "missing"),
    }
    if let Some(r) = child(ft, "Revisions") {
        for n in r.elements().filter(|n| n.name == "Revision") {
            out.revisions.push(Revision {
                text: attr(n, "Text"),
                date: attr(n, "Date"),
                user_id: n.attr("UserID").and_then(|v| v.trim().parse().ok()),
                modified_by: attr(n, "ModifiedBy"),
            });
        }
    }
    p.check_links(&out);
    Ok(out)
}

/// UTF-8 with or without BOM; anything else is decoded lossily with a warning.
fn decode(xml: &[u8], warnings: &mut Vec<Warning>) -> String {
    let xml = xml.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(xml);
    match std::str::from_utf8(xml) {
        Ok(s) => s.to_string(),
        Err(_) => {
            warnings.push(Warning::new("xml", "description.xml is not valid UTF-8; decoded lossily"));
            String::from_utf8_lossy(xml).into_owned()
        }
    }
}

fn child<'a>(n: Node<'a>, name: &str) -> Option<Node<'a>> {
    n.elements().find(|c| c.name == name)
}

fn attr(n: Node, name: &str) -> String {
    n.attr(name).unwrap_or("").to_string()
}

fn opt_attr(n: Node, name: &str) -> Option<String> {
    n.attr(name).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

/// Parses a GDTF float, accepting a decimal comma as some exporters write.
pub(crate) fn parse_float(s: &str) -> Option<f32> {
    let s = s.trim();
    s.parse::<f32>().ok().or_else(|| s.replace(',', ".").parse::<f32>().ok()).filter(|v| v.is_finite())
}

/// `{a,b,c,d}{e,f,g,h}{i,j,k,l}{m,n,o,p}`: rows, translation in the 4th column.
/// A 3×3 rotation (`Rotation` type) or 3 rows are padded with identity. When the
/// translation sits in the 4th row instead (seen in old files), it is moved.
pub(crate) fn parse_matrix(s: &str) -> Option<Mat4> {
    let rows: Vec<Vec<f32>> = s
        .split('}')
        .map(|r| r.trim().trim_start_matches('{').trim())
        .filter(|r| !r.is_empty())
        .map(|r| r.split(',').map(|v| v.trim().parse::<f32>().ok()).collect::<Option<Vec<_>>>())
        .collect::<Option<Vec<_>>>()?;
    if rows.is_empty() || rows.len() > 4 || rows.iter().any(|r| r.len() < 3 || r.len() > 4) {
        return None;
    }
    let mut m = [[0.0f32; 4]; 4];
    for (i, row) in m.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for (i, r) in rows.iter().enumerate() {
        for (j, v) in r.iter().enumerate() {
            m[i][j] = *v;
        }
    }
    if m.iter().flatten().any(|v| !v.is_finite()) {
        return None;
    }
    let col_t = [m[0][3], m[1][3], m[2][3]];
    let row_t = [m[3][0], m[3][1], m[3][2]];
    if col_t.iter().all(|v| *v == 0.0) && row_t.iter().any(|v| *v != 0.0) {
        for (i, t) in row_t.iter().enumerate() {
            m[i][3] = *t;
            m[3][i] = 0.0;
        }
    }
    m[3] = [0.0, 0.0, 0.0, 1.0];
    // glam takes columns; transposing the rows gives them.
    Some(Mat4::from_cols_array_2d(&m).transpose())
}

/// DMXAddress: plain integer or `Universe.Address`.
pub(crate) fn parse_dmx_address(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some((u, a)) = s.split_once('.') {
        let u: u32 = u.trim().parse().ok()?;
        let a: u32 = a.trim().parse().ok()?;
        return Some(u.saturating_sub(1) * 512 + a);
    }
    s.parse().ok()
}

struct Parser<'w> {
    warnings: &'w mut Vec<Warning>,
}

impl Parser<'_> {
    fn warn(&mut self, ctx: impl Into<String>, msg: impl Into<String>) {
        self.warnings.push(Warning::new(ctx, msg));
    }

    fn float(&mut self, n: Node, name: &str, ctx: &str) -> Option<f32> {
        let v = n.attr(name)?;
        if v.trim().is_empty() {
            return None;
        }
        let r = parse_float(v);
        if r.is_none() {
            self.warn(ctx, format!("{name}=\"{v}\" is not a number"));
        }
        r
    }

    fn float_or(&mut self, n: Node, name: &str, default: f32, ctx: &str) -> f32 {
        self.float(n, name, ctx).unwrap_or(default)
    }

    fn int(&mut self, n: Node, name: &str, default: i32, ctx: &str) -> i32 {
        match n.attr(name).map(str::trim) {
            None | Some("") => default,
            Some(v) => v.parse().ok().or_else(|| parse_float(v).map(|f| f.round() as i32)).unwrap_or_else(|| {
                self.warn(ctx, format!("{name}=\"{v}\" is not an integer"));
                default
            }),
        }
    }

    fn physical(&mut self, pd: Node) -> Physical {
        let mut out = Physical::default();
        let Some(props) = child(pd, "Properties") else {
            return out;
        };
        for n in props.elements() {
            match n.name.as_str() {
                "Weight" => out.weight_kg = self.float(n, "Value", "Weight").filter(|w| *w > 0.0),
                "LegHeight" => out.leg_height_m = self.float(n, "Value", "LegHeight").filter(|w| *w > 0.0),
                "OperatingTemperature" => {
                    let lo = self.float_or(n, "Low", 0.0, "OperatingTemperature");
                    let hi = self.float_or(n, "High", 40.0, "OperatingTemperature");
                    out.operating_temperature_c = Some((lo, hi));
                }
                "PowerConsumption" => out.power_consumption.push(PowerConsumption {
                    value_w: self.float_or(n, "Value", 0.0, "PowerConsumption"),
                    power_factor: self.float(n, "PowerFactor", "PowerConsumption"),
                    connector: attr(n, "Connector"),
                    voltage_low: self.float(n, "VoltageLow", "PowerConsumption"),
                    voltage_high: self.float(n, "VoltageHigh", "PowerConsumption"),
                }),
                _ => {}
            }
        }
        out
    }

    fn model(&mut self, n: Node) -> Model {
        let name = attr(n, "Name");
        let ctx = format!("Model {name}");
        let pair = |p: &mut Self, x: &str, y: &str| (p.float_or(n, x, 0.0, &ctx), p.float_or(n, y, 0.0, &ctx));
        let svg_offset = pair(self, "SVGOffsetX", "SVGOffsetY");
        let svg_side_offset = pair(self, "SVGSideOffsetX", "SVGSideOffsetY");
        let svg_front_offset = pair(self, "SVGFrontOffsetX", "SVGFrontOffsetY");
        let ctx = format!("Model {name}");
        let m = Model {
            length_m: self.float_or(n, "Length", 0.0, &ctx).abs(),
            width_m: self.float_or(n, "Width", 0.0, &ctx).abs(),
            height_m: self.float_or(n, "Height", 0.0, &ctx).abs(),
            primitive: PrimitiveType::parse(n.attr("PrimitiveType").unwrap_or("")),
            file: opt_attr(n, "File"),
            svg_offset,
            svg_side_offset,
            svg_front_offset,
            name,
        };
        if let PrimitiveType::Other(p) = &m.primitive {
            self.warn(ctx, format!("unknown PrimitiveType \"{p}\""));
        }
        m
    }

    fn geometry(&mut self, n: Node, depth: usize) -> Option<Geometry> {
        if depth > 64 {
            self.warn("Geometries", "geometry tree deeper than 64; cut off");
            return None;
        }
        let tag = n.name.as_str();
        let name = attr(n, "Name");
        let ctx = format!("{tag} {name}");
        let position = match n.attr("Position") {
            None => Mat4::IDENTITY,
            Some(s) if s.trim().is_empty() => Mat4::IDENTITY,
            Some(s) => parse_matrix(s).unwrap_or_else(|| {
                self.warn(&ctx, format!("unreadable Position \"{s}\"; using identity"));
                Mat4::IDENTITY
            }),
        };
        let kind = match tag {
            "Geometry" => GeometryKind::Geometry,
            "Axis" => GeometryKind::Axis,
            "FilterBeam" => GeometryKind::FilterBeam,
            "FilterColor" => GeometryKind::FilterColor,
            "FilterGobo" => GeometryKind::FilterGobo,
            "FilterShaper" => GeometryKind::FilterShaper,
            "Beam" => GeometryKind::Beam(self.beam(n, &ctx)),
            "MediaServerLayer" => GeometryKind::MediaServerLayer,
            "MediaServerCamera" => GeometryKind::MediaServerCamera,
            "MediaServerMaster" => GeometryKind::MediaServerMaster,
            "Display" => GeometryKind::Display { texture: opt_attr(n, "Texture") },
            "Laser" => GeometryKind::Laser,
            "WiringObject" => GeometryKind::WiringObject,
            "Inventory" => GeometryKind::Inventory,
            "Structure" => GeometryKind::Structure,
            "Support" => GeometryKind::Support,
            "Magnet" => GeometryKind::Magnet,
            "GeometryReference" => {
                let mut breaks = Vec::new();
                for b in n.elements().filter(|c| c.name == "Break") {
                    let off = b.attr("DMXOffset").unwrap_or("1");
                    let dmx_offset = parse_dmx_address(off).unwrap_or_else(|| {
                        self.warn(&ctx, format!("Break DMXOffset=\"{off}\" unreadable; using 1"));
                        1
                    });
                    let dmx_break = self.int(b, "DMXBreak", 1, &ctx).max(0) as u32;
                    breaks.push(Break { dmx_break, dmx_offset });
                }
                let geometry = attr(n, "Geometry");
                if geometry.is_empty() {
                    self.warn(&ctx, "GeometryReference without Geometry");
                }
                GeometryKind::Reference { geometry, breaks }
            }
            // Pin patches, wiring details and the like are not geometries.
            "PinPatch" | "Break" => return None,
            other => {
                self.warn("Geometries", format!("unknown element <{other}>, read as Geometry"));
                GeometryKind::Other(other.to_string())
            }
        };
        let mut children = Vec::new();
        if !matches!(kind, GeometryKind::Reference { .. }) {
            for c in n.elements() {
                if let Some(g) = self.geometry(c, depth + 1) {
                    children.push(g);
                }
            }
        }
        let attributes: BTreeMap<String, String> = n.attrs.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        Some(Geometry { name, model: opt_attr(n, "Model"), position, kind, children, attributes })
    }

    fn beam(&mut self, n: Node, ctx: &str) -> BeamData {
        let d = BeamData::default();
        BeamData {
            lamp_type: opt_attr(n, "LampType").unwrap_or(d.lamp_type),
            power_consumption_w: self.float_or(n, "PowerConsumption", d.power_consumption_w, ctx),
            luminous_flux_lm: self.float_or(n, "LuminousFlux", d.luminous_flux_lm, ctx),
            color_temperature_k: self.float_or(n, "ColorTemperature", d.color_temperature_k, ctx),
            beam_angle_deg: self.float_or(n, "BeamAngle", d.beam_angle_deg, ctx),
            field_angle_deg: self.float_or(n, "FieldAngle", d.field_angle_deg, ctx),
            throw_ratio: self.float_or(n, "ThrowRatio", d.throw_ratio, ctx),
            rectangle_ratio: self.float_or(n, "RectangleRatio", d.rectangle_ratio, ctx),
            beam_radius_m: self.float_or(n, "BeamRadius", d.beam_radius_m, ctx),
            beam_type: opt_attr(n, "BeamType").unwrap_or(d.beam_type),
            color_rendering_index: self.float_or(n, "ColorRenderingIndex", d.color_rendering_index, ctx),
        }
    }

    fn mode(&mut self, n: Node) -> DmxMode {
        let name = attr(n, "Name");
        let mut mode =
            DmxMode { description: attr(n, "Description"), geometry: attr(n, "Geometry"), channels: Vec::new(), name };
        let ctx = format!("DMXMode {}", mode.name);
        let Some(chs) = child(n, "DMXChannels") else {
            self.warn(&ctx, "no DMXChannels");
            return mode;
        };
        for c in chs.elements().filter(|c| c.name == "DMXChannel") {
            mode.channels.push(self.channel(c, &ctx));
        }
        mode
    }

    fn channel(&mut self, n: Node, ctx: &str) -> DmxChannel {
        let dmx_break = match n.attr("DMXBreak").map(str::trim) {
            None | Some("") => DmxBreak::Number(1),
            Some(v) if v.eq_ignore_ascii_case("overwrite") => DmxBreak::Overwrite,
            Some(v) => match v.parse::<u32>() {
                Ok(b) => DmxBreak::Number(b),
                Err(_) => {
                    self.warn(ctx, format!("DMXBreak=\"{v}\" unreadable; using 1"));
                    DmxBreak::Number(1)
                }
            },
        };
        let mut offsets = Vec::new();
        if let Some(o) = n.attr("Offset").map(str::trim)
            && !o.is_empty()
            && !o.eq_ignore_ascii_case("none")
        {
            for part in o.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                match part.parse::<i64>() {
                    Ok(v) if v >= 1 => offsets.push(v as u32),
                    _ => self.warn(ctx, format!("channel Offset part \"{part}\" ignored")),
                }
            }
        }
        let logical_channels = n
            .elements()
            .filter(|c| c.name == "LogicalChannel")
            .map(|l| LogicalChannel {
                attribute: attr(l, "Attribute"),
                snap: attr(l, "Snap"),
                master: attr(l, "Master"),
                functions: l
                    .elements()
                    .filter(|c| c.name == "ChannelFunction")
                    .map(|f| ChannelFunction {
                        name: attr(f, "Name"),
                        attribute: attr(f, "Attribute"),
                        original_attribute: attr(f, "OriginalAttribute"),
                        dmx_from: attr(f, "DMXFrom"),
                        default: attr(f, "Default"),
                        physical_from: f.attr("PhysicalFrom").and_then(parse_float),
                        physical_to: f.attr("PhysicalTo").and_then(parse_float),
                        wheel: opt_attr(f, "Wheel"),
                    })
                    .collect(),
            })
            .collect();
        DmxChannel {
            dmx_break,
            offsets,
            geometry: attr(n, "Geometry"),
            initial_function: opt_attr(n, "InitialFunction"),
            highlight: opt_attr(n, "Highlight"),
            logical_channels,
        }
    }

    /// Model, reference and mode links that point nowhere.
    fn check_links(&mut self, ft: &FixtureType) {
        let mut missing_models = Vec::new();
        let mut bad_refs = Vec::new();
        for g in &ft.geometries {
            g.walk(&mut |n| {
                if let Some(m) = &n.model
                    && ft.model(m).is_none()
                {
                    missing_models.push((n.name.clone(), m.clone()));
                }
                if let GeometryKind::Reference { geometry, .. } = &n.kind
                    && ft.top_geometry(geometry).is_none()
                {
                    bad_refs.push((n.name.clone(), geometry.clone()));
                }
            });
        }
        for (g, m) in missing_models {
            self.warn(format!("Geometry {g}"), format!("Model \"{m}\" not defined"));
        }
        for (g, r) in bad_refs {
            self.warn(
                format!("GeometryReference {g}"),
                format!("references \"{r}\", which is not a top-level geometry"),
            );
        }
        for m in &ft.modes {
            if ft.top_geometry(&m.geometry).is_none() {
                self.warn(
                    format!("DMXMode {}", m.name),
                    format!("Geometry \"{}\" is not a top-level geometry", m.geometry),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_rows_with_translation_column() {
        let m = parse_matrix("{1,0,0,0.5}{0,1,0,0}{0,0,1,-0.2}{0,0,0,1}").unwrap();
        let p = m.transform_point3(glam::Vec3::ZERO);
        assert_eq!(p, glam::Vec3::new(0.5, 0.0, -0.2));
    }

    #[test]
    fn matrix_rotation_is_row_major() {
        // 90° about z: x → y. Row-major: first row is (0,-1,0).
        let m = parse_matrix("{0,-1,0,0}{1,0,0,0}{0,0,1,0}{0,0,0,1}").unwrap();
        let v = m.transform_vector3(glam::Vec3::X);
        assert!((v - glam::Vec3::Y).length() < 1e-6);
    }

    #[test]
    fn matrix_translation_in_last_row_is_moved() {
        let m = parse_matrix("{1,0,0,0}{0,1,0,0}{0,0,1,0}{1,2,3,1}").unwrap();
        assert_eq!(m.transform_point3(glam::Vec3::ZERO), glam::Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn matrix_three_rows_and_garbage() {
        assert!(parse_matrix("{1,0,0}{0,1,0}{0,0,1}").is_some());
        assert!(parse_matrix("{a,b}").is_none());
        assert!(parse_matrix("").is_none());
    }

    #[test]
    fn dmx_addresses() {
        assert_eq!(parse_dmx_address("13"), Some(13));
        assert_eq!(parse_dmx_address("2.1"), Some(513));
        assert_eq!(parse_dmx_address("x"), None);
    }
}
