//! DXF to plain linework: everything flattened to polylines and texts in drawing units. The file
//! is read with craft-dxf (ASCII and binary, bounded, damaged files read up to the damage).

use std::collections::HashMap;
use std::f64::consts::PI;

use craft_dxf::model::{Document, Entity, EntityKind, Point};

use crate::{Error, Linework, Path, Text};

/// Arc segments per full turn when flattening.
const ARC_STEPS: f64 = 72.0;
/// Inserts nested deeper than this are ignored (guards against recursive blocks).
const MAX_DEPTH: usize = 8;
/// Most columns and rows of one array insert.
const MAX_ARRAY: i64 = 1000;

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
fn unit_mm(insunits: i64) -> f64 {
    match insunits {
        1 => 25.4,
        2 => 304.8,
        4 => 1.0,
        5 => 10.0,
        6 => 1000.0,
        7 => 1_000_000.0,
        10 => 914.4,
        13 => 0.001,
        14 => 100.0,
        _ => 1.0,
    }
}

/// The drawing's entities and blocks, typed once.
struct Drawing {
    entities: Vec<Entity>,
    blocks: HashMap<String, ([f64; 2], Vec<Entity>)>,
}

/// Read a DXF into linework (model space only), the mm per drawing unit and notes on what was
/// left out (a file that stops early).
pub fn read_dxf(bytes: &[u8]) -> Result<(Linework, f64, Vec<String>), Error> {
    let (tags, stopped) = craft_dxf::parse_recovering(bytes).map_err(|e| Error::Dxf(e.to_string()))?;
    let doc = Document::read(&tags);
    if doc.sections == 0 {
        return Err(Error::Dxf("no sections".into()));
    }
    let mut notes = Vec::new();
    if let Some(line) = stopped {
        notes.push(format!("the DXF is damaged at line {line}; read what came before"));
    }
    let d = Drawing {
        entities: doc.entities.iter().filter(|e| !e.paper_space()).map(Entity::read).collect(),
        blocks: doc
            .blocks
            .iter()
            .map(|b| {
                let base = b.base();
                (b.name.to_ascii_uppercase(), ([base[0], base[1]], b.entities.iter().map(Entity::read).collect()))
            })
            .collect(),
    };
    let mut out = Linework::default();
    for e in &d.entities {
        entity(e, &Xf::ID, &d, 0, &mut out);
    }
    Ok((out, unit_mm(doc.header.insunits()), notes))
}

/// The object coordinate system of a planar entity as a 2D map into the world's x/y (the
/// identity for the usual +Z extrusion; mirrored blocks have −Z).
fn ocs(extrusion: Point) -> Xf {
    let n = extrusion;
    if n[0].abs() < 1e-12 && n[1].abs() < 1e-12 && n[2] > 0.0 {
        return Xf::ID;
    }
    let x = craft_dxf::model::ocs_to_wcs(n, [1.0, 0.0, 0.0]);
    let y = craft_dxf::model::ocs_to_wcs(n, [0.0, 1.0, 0.0]);
    Xf([x[0], x[1], y[0], y[1], 0.0, 0.0])
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

fn entity(e: &Entity, xf: &Xf, d: &Drawing, depth: usize, out: &mut Linework) {
    let p2 = |p: &Point| [p[0], p[1]];
    // Planar entities are in their object coordinate system.
    let planar = xf.then(&ocs(e.common.extrusion));
    match &e.kind {
        EntityKind::Line { start, end } => push_path(out, xf, vec![p2(start), p2(end)], false),
        EntityKind::Circle { center, radius } => {
            let mut pts = arc_points(p2(center), *radius, 0.0, 2.0 * PI);
            pts.pop();
            push_path(out, &planar, pts, true)
        }
        EntityKind::Arc { center, radius, start_angle, end_angle } => {
            let s = start_angle.to_radians();
            let mut sweep = end_angle.to_radians() - s;
            if sweep <= 0.0 {
                sweep += 2.0 * PI;
            }
            push_path(out, &planar, arc_points(p2(center), *radius, s, sweep), false)
        }
        EntityKind::Ellipse { center, major, ratio, start, end } => {
            let c = p2(center);
            let maj = [major[0], major[1]];
            let min = [-maj[1] * ratio, maj[0] * ratio];
            let (t0, mut t1) = (*start, *end);
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
        EntityKind::LwPolyline(p) => {
            let v: Vec<([f64; 2], f64)> = p.vertices.iter().map(|v| ([v.x, v.y], v.bulge)).collect();
            push_path(out, &planar, bulged(&v, p.closed), p.closed && v.iter().all(|x| x.1 == 0.0))
        }
        EntityKind::Polyline(p) if !p.is_mesh() && !p.is_polyface() => {
            let closed = p.closed();
            let v: Vec<([f64; 2], f64)> =
                p.vertices.iter().filter(|v| !v.is_spline_frame()).map(|v| (p2(&v.location), v.bulge)).collect();
            let m = if p.is_3d() { *xf } else { planar };
            push_path(out, &m, bulged(&v, closed), closed && v.iter().all(|x| x.1 == 0.0))
        }
        EntityKind::Spline(s) => {
            // Fit points if present, else the control polygon: close enough for an underlay.
            let pts: Vec<[f64; 2]> =
                if s.fit.len() >= 2 { s.fit.iter().map(p2).collect() } else { s.control.iter().map(p2).collect() };
            push_path(out, xf, pts, false)
        }
        EntityKind::Text(t) if !t.value.trim().is_empty() => out.texts.push(Text {
            pos: planar.apply(p2(&t.insert)),
            text: t.value.clone(),
            height: t.height * planar.scale(),
            rotation: t.rotation + planar.angle(),
        }),
        EntityKind::MText(t) => {
            let text = plain_mtext(&craft_dxf::mtext::unescape_codes(&t.contents));
            if !text.is_empty() {
                let rotation = match t.direction {
                    Some(dir) if dir[0] != 0.0 || dir[1] != 0.0 => dir[1].atan2(dir[0]).to_degrees(),
                    _ => t.rotation,
                };
                out.texts.push(Text {
                    pos: xf.apply(p2(&t.insert)),
                    text,
                    height: t.height * xf.scale(),
                    rotation: rotation + xf.angle(),
                });
            }
        }
        EntityKind::Insert(i) if depth < MAX_DEPTH => {
            let Some((base, entities)) = d.blocks.get(&i.block.to_ascii_uppercase()) else { return };
            let local = Xf::insert(p2(&i.insert), i.scale[0], i.scale[1], i.rotation, *base);
            let cols = i.columns.clamp(1, MAX_ARRAY);
            let rows = i.rows.clamp(1, MAX_ARRAY);
            for r in 0..rows {
                for c in 0..cols {
                    let off = Xf([1.0, 0.0, 0.0, 1.0, c as f64 * i.column_spacing, r as f64 * i.row_spacing]);
                    let m = planar.then(&off).then(&local);
                    for be in entities {
                        entity(be, &m, d, depth + 1, out);
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An R12 DXF in metres: a line, a circle, a text, a block inserted turned and doubled, an
    /// LWPOLYLINE with a bulge, and a circle mirrored by a −Z extrusion.
    pub(crate) fn sample() -> Vec<u8> {
        #[rustfmt::skip]
        let groups: &[(i32, &str)] = &[
            (0, "SECTION"), (2, "HEADER"), (9, "$INSUNITS"), (70, "6"), (0, "ENDSEC"),
            (0, "SECTION"), (2, "BLOCKS"),
            (0, "BLOCK"), (8, "0"), (2, "SQ"), (70, "0"), (10, "0"), (20, "0"), (30, "0"),
            (0, "LINE"), (8, "0"), (10, "0"), (20, "0"), (30, "0"), (11, "1"), (21, "0"), (31, "0"),
            (0, "ENDBLK"), (8, "0"),
            (0, "ENDSEC"),
            (0, "SECTION"), (2, "ENTITIES"),
            (0, "LINE"), (8, "0"), (10, "0"), (20, "0"), (30, "0"), (11, "10"), (21, "0"), (31, "0"),
            (0, "CIRCLE"), (8, "0"), (10, "5"), (20, "5"), (30, "0"), (40, "2"),
            (0, "TEXT"), (8, "0"), (10, "1"), (20, "8"), (30, "0"), (40, "0.5"), (1, "Stage"),
            (0, "INSERT"), (8, "0"), (2, "SQ"), (10, "20"), (20, "0"), (30, "0"), (41, "2"), (42, "2"), (50, "90"),
            (0, "LWPOLYLINE"), (8, "0"), (90, "2"), (70, "0"), (10, "0"), (20, "-2"), (42, "1"), (10, "2"), (20, "-2"),
            (0, "CIRCLE"), (8, "0"), (10, "30"), (20, "0"), (30, "0"), (40, "1"), (210, "0"), (220, "0"), (230, "-1"),
            (0, "ENDSEC"), (0, "EOF"),
        ];
        groups.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect::<String>().into_bytes()
    }

    #[test]
    fn dxf_lines_circles_texts_inserts() {
        let (lw, unit, notes) = read_dxf(&sample()).unwrap();
        assert!(notes.is_empty(), "{notes:?}");
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
        // the −Z circle at OCS x = 30 lies at world x = −30
        let mirrored = &lw.paths[4].points;
        assert!(mirrored.iter().all(|p| ((p[0] + 30.0).hypot(p[1]) - 1.0).abs() < 1e-9), "{mirrored:?}");
        let e = lw.extent().unwrap();
        assert_eq!(e[1][0], 20.0);
    }

    #[test]
    fn a_damaged_dxf_is_read_up_to_the_damage() {
        let mut bytes = sample();
        let cut = String::from_utf8_lossy(&bytes).find("LWPOLYLINE").unwrap();
        bytes.truncate(cut - 3);
        bytes.extend_from_slice(b"\nnot a group code\n");
        let (lw, _, notes) = read_dxf(&bytes).unwrap();
        assert!(lw.paths.len() >= 3);
        assert!(notes.iter().any(|n| n.contains("damaged")), "{notes:?}");
        assert!(read_dxf(b"hello").is_err());
    }

    #[test]
    fn mtext_codes_are_removed() {
        assert_eq!(plain_mtext(r"{\fArial|b1;Bühne}\PRechts"), "Bühne Rechts");
    }
}
