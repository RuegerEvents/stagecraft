//! DXF to plain linework: everything flattened to polylines and texts in drawing units.

use std::collections::HashMap;
use std::f64::consts::PI;

use dxf::entities::{Entity, EntityType};
use dxf::enums::Units;
use dxf::{Block, Drawing};

use crate::{Error, Linework, Path, Text};

/// Arc segments per full turn when flattening.
const ARC_STEPS: f64 = 72.0;
/// Inserts nested deeper than this are ignored (guards against recursive blocks).
const MAX_DEPTH: usize = 8;

/// A 2D affine map: `[a, b, c, d, e, f]` → `(a x + c y + e, b x + d y + f)`.
#[derive(Clone, Copy)]
struct Xf([f64; 6]);

impl Xf {
    const ID: Xf = Xf([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    fn apply(&self, p: [f64; 2]) -> [f64; 2] {
        let [a, b, c, d, e, f] = self.0;
        [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f]
    }

    /// `self ∘ other`: apply `other` first.
    fn then(&self, o: &Xf) -> Xf {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = o.0;
        Xf([
            a * a2 + c * b2,
            b * a2 + d * b2,
            a * c2 + c * d2,
            b * c2 + d * d2,
            a * e2 + c * f2 + e,
            b * e2 + d * f2 + f,
        ])
    }

    fn scale(&self) -> f64 {
        let [a, b, c, d, ..] = self.0;
        ((a * d - b * c).abs()).sqrt()
    }

    fn angle(&self) -> f64 {
        self.0[1].atan2(self.0[0]).to_degrees()
    }

    /// Insert of a block: translate to `loc`, rotate, scale, minus the block base.
    fn insert(loc: [f64; 2], sx: f64, sy: f64, rot_deg: f64, base: [f64; 2]) -> Xf {
        let (s, c) = rot_deg.to_radians().sin_cos();
        let m = Xf([c * sx, s * sx, -s * sy, c * sy, loc[0], loc[1]]);
        m.then(&Xf([1.0, 0.0, 0.0, 1.0, -base[0], -base[1]]))
    }
}

/// mm per drawing unit for `$INSUNITS`; unitless drawings are taken as mm.
fn unit_mm(u: Units) -> f64 {
    match u {
        Units::Inches => 25.4,
        Units::Feet => 304.8,
        Units::Millimeters => 1.0,
        Units::Centimeters => 10.0,
        Units::Meters => 1000.0,
        Units::Decimeters => 100.0,
        Units::Yards => 914.4,
        Units::Microns => 0.001,
        _ => 1.0,
    }
}

/// Read a DXF into linework (model space only) and the mm per drawing unit.
pub fn read_dxf(bytes: &[u8]) -> Result<(Linework, f64), Error> {
    let d = Drawing::load(&mut std::io::Cursor::new(bytes)).map_err(|e| Error::Dxf(e.to_string()))?;
    let blocks: HashMap<&str, &Block> = d.blocks().map(|b| (b.name.as_str(), b)).collect();
    let mut out = Linework::default();
    for e in d.entities().filter(|e| !e.common.is_in_paper_space) {
        entity(e, &Xf::ID, &blocks, 0, &mut out);
    }
    Ok((out, unit_mm(d.header.default_drawing_units)))
}

fn arc_points(c: [f64; 2], r: f64, start: f64, sweep: f64) -> Vec<[f64; 2]> {
    let n = ((sweep.abs() / (2.0 * PI) * ARC_STEPS).ceil() as usize).max(2);
    (0..=n)
        .map(|i| {
            let a = start + sweep * i as f64 / n as f64;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect()
}

/// Points of a polyline with bulges (arc segments), flattened.
fn bulged(v: &[([f64; 2], f64)], closed: bool) -> Vec<[f64; 2]> {
    let n = v.len();
    let mut out = Vec::new();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for i in 0..segs {
        let (p, bulge) = v[i];
        let q = v[(i + 1) % n].0;
        if out.is_empty() {
            out.push(p);
        }
        if bulge.abs() < 1e-9 {
            out.push(q);
            continue;
        }
        // bulge = tan(sweep / 4); centre from the chord.
        let sweep = 4.0 * bulge.atan();
        let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
        let chord = dx.hypot(dy);
        if chord < 1e-12 {
            continue;
        }
        let r = chord / (2.0 * (sweep / 2.0).sin()).abs();
        let mid = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
        let h = r * (sweep / 2.0).cos().abs();
        // the centre lies to the left of p→q for a positive bulge below half a turn
        let sgn = if (bulge > 0.0) == (sweep.abs() < PI) { 1.0 } else { -1.0 };
        let c = [mid[0] - dy / chord * h * sgn, mid[1] + dx / chord * h * sgn];
        let start = (p[1] - c[1]).atan2(p[0] - c[0]);
        out.extend(arc_points(c, r, start, sweep).into_iter().skip(1));
    }
    if n == 1 {
        out.push(v[0].0);
    }
    out
}

fn push_path(out: &mut Linework, xf: &Xf, pts: Vec<[f64; 2]>, closed: bool) {
    if pts.len() >= 2 {
        out.paths.push(Path { points: pts.into_iter().map(|p| xf.apply(p)).collect(), closed });
    }
}

/// MTEXT formatting codes removed (`\P` becomes a space, `{\fArial;…}` the text).
fn plain_mtext(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '\\' => match chars.next() {
                Some('P') | Some('~') => out.push(' '),
                Some('\\') => out.push('\\'),
                Some('{') => out.push('{'),
                Some('}') => out.push('}'),
                // codes with an argument up to `;`
                Some(_) => {
                    for d in chars.by_ref() {
                        if d == ';' {
                            break;
                        }
                    }
                }
                None => {}
            },
            _ => out.push(c),
        }
    }
    out.trim().to_owned()
}

fn entity(e: &Entity, xf: &Xf, blocks: &HashMap<&str, &Block>, depth: usize, out: &mut Linework) {
    let p2 = |p: &dxf::Point| [p.x, p.y];
    match &e.specific {
        EntityType::Line(l) => push_path(out, xf, vec![p2(&l.p1), p2(&l.p2)], false),
        EntityType::Circle(c) => {
            let mut pts = arc_points(p2(&c.center), c.radius, 0.0, 2.0 * PI);
            pts.pop();
            push_path(out, xf, pts, true)
        }
        EntityType::Arc(a) => {
            let s = a.start_angle.to_radians();
            let mut sweep = a.end_angle.to_radians() - s;
            if sweep <= 0.0 {
                sweep += 2.0 * PI;
            }
            push_path(out, xf, arc_points(p2(&a.center), a.radius, s, sweep), false)
        }
        EntityType::Ellipse(el) => {
            let c = p2(&el.center);
            let maj = [el.major_axis.x, el.major_axis.y];
            let min = [-maj[1] * el.minor_axis_ratio, maj[0] * el.minor_axis_ratio];
            let (t0, mut t1) = (el.start_parameter, el.end_parameter);
            if t1 <= t0 {
                t1 += 2.0 * PI;
            }
            let n = (((t1 - t0) / (2.0 * PI) * ARC_STEPS).ceil() as usize).max(2);
            let pts: Vec<[f64; 2]> = (0..=n)
                .map(|i| {
                    let t = t0 + (t1 - t0) * i as f64 / n as f64;
                    [c[0] + maj[0] * t.cos() + min[0] * t.sin(), c[1] + maj[1] * t.cos() + min[1] * t.sin()]
                })
                .collect();
            push_path(out, xf, pts, false)
        }
        EntityType::LwPolyline(p) => {
            let closed = p.flags & 1 != 0;
            let v: Vec<([f64; 2], f64)> = p.vertices.iter().map(|v| ([v.x, v.y], v.bulge)).collect();
            push_path(out, xf, bulged(&v, closed), closed && v.iter().all(|x| x.1 == 0.0))
        }
        EntityType::Polyline(p) => {
            let closed = p.flags & 1 != 0;
            let v: Vec<([f64; 2], f64)> = p.vertices().map(|v| (p2(&v.location), v.bulge)).collect();
            push_path(out, xf, bulged(&v, closed), closed && v.iter().all(|x| x.1 == 0.0))
        }
        EntityType::Spline(s) => {
            // Fit points if present, else the control polygon: close enough for an underlay.
            let pts: Vec<[f64; 2]> = if s.fit_points.len() >= 2 {
                s.fit_points.iter().map(p2).collect()
            } else {
                s.control_points.iter().map(p2).collect()
            };
            push_path(out, xf, pts, false)
        }
        EntityType::Text(t) if !t.value.trim().is_empty() => out.texts.push(Text {
            pos: xf.apply(p2(&t.location)),
            text: t.value.clone(),
            height: t.text_height * xf.scale(),
            rotation: t.rotation + xf.angle(),
        }),
        EntityType::MText(t) => {
            let text = plain_mtext(&format!("{}{}", t.extended_text.join(""), t.text));
            if !text.is_empty() {
                out.texts.push(Text {
                    pos: xf.apply(p2(&t.insertion_point)),
                    text,
                    height: t.initial_text_height * xf.scale(),
                    rotation: t.rotation_angle + xf.angle(),
                });
            }
        }
        EntityType::Insert(i) if depth < MAX_DEPTH => {
            let Some(b) = blocks.get(i.name.as_str()) else { return };
            let local = Xf::insert(p2(&i.location), i.x_scale_factor, i.y_scale_factor, i.rotation, p2(&b.base_point));
            let cols = i.column_count.max(1);
            let rows = i.row_count.max(1);
            for r in 0..rows {
                for c in 0..cols {
                    let off = Xf([1.0, 0.0, 0.0, 1.0, c as f64 * i.column_spacing, r as f64 * i.row_spacing]);
                    let m = xf.then(&off).then(&local);
                    for be in &b.entities {
                        entity(be, &m, blocks, depth + 1, out);
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        let mut d = Drawing::new();
        d.header.version = dxf::enums::AcadVersion::R2000;
        d.header.default_drawing_units = Units::Meters;
        d.add_entity(Entity::new(EntityType::Line(dxf::entities::Line::new(
            dxf::Point::new(0.0, 0.0, 0.0),
            dxf::Point::new(10.0, 0.0, 0.0),
        ))));
        d.add_entity(Entity::new(EntityType::Circle(dxf::entities::Circle::new(dxf::Point::new(5.0, 5.0, 0.0), 2.0))));
        let t = dxf::entities::Text {
            location: dxf::Point::new(1.0, 8.0, 0.0),
            value: "Stage".into(),
            text_height: 0.5,
            ..Default::default()
        };
        d.add_entity(Entity::new(EntityType::Text(t)));
        let mut b = Block { name: "SQ".into(), ..Default::default() };
        b.entities.push(Entity::new(EntityType::Line(dxf::entities::Line::new(
            dxf::Point::new(0.0, 0.0, 0.0),
            dxf::Point::new(1.0, 0.0, 0.0),
        ))));
        d.add_block(b);
        let ins = dxf::entities::Insert {
            name: "SQ".into(),
            location: dxf::Point::new(20.0, 0.0, 0.0),
            rotation: 90.0,
            x_scale_factor: 2.0,
            y_scale_factor: 2.0,
            ..Default::default()
        };
        d.add_entity(Entity::new(EntityType::Insert(ins)));
        let mut lw = dxf::entities::LwPolyline::default();
        lw.vertices.push(dxf::LwPolylineVertex { x: 0.0, y: -2.0, bulge: 1.0, ..Default::default() });
        lw.vertices.push(dxf::LwPolylineVertex { x: 2.0, y: -2.0, ..Default::default() });
        d.add_entity(Entity::new(EntityType::LwPolyline(lw)));
        let mut out = Vec::new();
        d.save(&mut out).unwrap();
        out
    }

    #[test]
    fn dxf_lines_circles_texts_inserts() {
        let (lw, unit) = read_dxf(&sample()).unwrap();
        assert_eq!(unit, 1000.0);
        assert_eq!(lw.paths[0].points, vec![[0.0, 0.0], [10.0, 0.0]]);
        assert!(lw.paths[1].closed);
        assert!(lw.paths[1].points.iter().all(|p| ((p[0] - 5.0).hypot(p[1] - 5.0) - 2.0).abs() < 1e-9));
        assert_eq!(lw.texts[0].text, "Stage");
        // the inserted block line, turned 90° and doubled
        let ins = &lw.paths[2].points;
        assert!((ins[0][0] - 20.0).abs() < 1e-9 && ins[0][1].abs() < 1e-9);
        assert!((ins[1][0] - 20.0).abs() < 1e-9 && (ins[1][1] - 2.0).abs() < 1e-9, "{ins:?}");
        // a half circle from a bulge of 1: radius 1 about (1, -2), through (1, -3) (clockwise below)
        let semi = &lw.paths[3].points;
        assert!(semi.iter().all(|p| ((p[0] - 1.0).hypot(p[1] + 2.0) - 1.0).abs() < 1e-9), "{semi:?}");
        assert!(semi.iter().any(|p| (p[1] + 3.0).abs() < 1e-6), "{semi:?}");
        let e = lw.extent().unwrap();
        assert_eq!(e[1][0], 20.0);
    }

    #[test]
    fn mtext_codes_are_removed() {
        assert_eq!(plain_mtext(r"{\fArial|b1;Bühne}\PRechts"), "Bühne Rechts");
    }
}
