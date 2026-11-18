//! Dimensions as plan geometry, shared by the canvas and sheets: extension lines,
//! the dimension line with 45° ticks, and where the value goes. Sizes that should be
//! constant on screen or paper (tick length, gaps) come in [`DimStyle`] in world mm.

use stagecraft_core::model::P2;
use stagecraft_core::sketch::{ConstraintId, DimAxis, EdgeGeom, Measure, RefDimension, Sketch, p2};
use stagecraft_core::units::LengthFormat;
use stagecraft_core::{Document, Id};

use crate::math;

/// Where a dimension comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DimSource {
    /// A dimension of a layer's sketch.
    Sketch { layer: Id, id: ConstraintId },
    /// A reference dimension between objects (`Document::dimensions`).
    Reference(Id),
}

/// One dimension, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct DimDraw {
    pub source: DimSource,
    pub driving: bool,
    /// Current value, mm or degrees.
    pub value: f64,
    pub angle: bool,
    /// Shown text: `6.00` (metres), `(6.00)` for reference dimensions, `90.0°`.
    pub text: String,
    pub name: Option<String>,
    /// Point on the dimension line where the value sits, world mm.
    pub label: P2,
    /// Unit vector from the line towards the side the text is written on.
    pub up: P2,
    /// Text angle in degrees, kept readable (between −90 and 90).
    pub rotation: f64,
    /// Extension lines, dimension line and ticks, world mm.
    pub lines: Vec<Vec<P2>>,
}

/// Sizes in world mm (divide screen or paper sizes by the zoom).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DimStyle {
    /// Half the length of a tick.
    pub tick: f64,
    /// Gap between the measured point and its extension line.
    pub gap: f64,
    /// How far extension lines and the dimension line run past each other.
    pub overshoot: f64,
    /// How values are written (the document's `LengthFormat::of(&doc.meta)`).
    pub format: LengthFormat,
}

impl DimStyle {
    /// Sizes for `px` screen points or paper mm per world mm.
    pub fn for_scale(units_per_mm: f64, tick: f64) -> Self {
        let k = 1.0 / units_per_mm.max(1e-9);
        DimStyle { tick: tick * k, gap: tick * 0.6 * k, overshoot: tick * k, format: LengthFormat::default() }
    }

    /// The same sizes with the document's value format.
    pub fn with_format(self, format: LengthFormat) -> Self {
        DimStyle { format, ..self }
    }
}

/// Metres with two decimals, three when there are odd millimetres; degrees with one.
pub fn format_value(v: f64, angle: bool) -> String {
    if angle {
        return format!("{v:.1}°");
    }
    let mm = v.round();
    if (mm % 10.0).abs() < 1e-9 { format!("{:.2}", v / 1000.0) } else { format!("{:.3}", v / 1000.0) }
}

/// A value as drawings show it, in the given format.
pub fn format_shown(v: f64, angle: bool, f: &LengthFormat) -> String {
    if angle { f.angle(v) } else { f.length(v) }
}

fn text_for(v: f64, angle: bool, driving: bool, f: &LengthFormat) -> String {
    let t = format_shown(v, angle, f);
    if driving { t } else { format!("({t})") }
}

/// Readable text angle and the matching "up" normal for a line direction.
fn orient(u: P2) -> (f64, P2) {
    let mut ang = math::angle_deg(u);
    let mut dir = u;
    if ang > 90.0 + 1e-9 || ang <= -90.0 + 1e-9 {
        ang = if ang > 0.0 { ang - 180.0 } else { ang + 180.0 };
        dir = [-u[0], -u[1]];
    }
    (ang, [-dir[1], dir[0]])
}

/// A linear dimension between `a` and `b` measured along `u` (unit), with its line
/// through `through`.
fn linear(a: P2, b: P2, u: P2, through: P2, st: &DimStyle) -> (Vec<Vec<P2>>, P2, P2, f64) {
    let n = [-u[1], u[0]];
    let d = math::dot(math::sub(through, a), n);
    let foot = |p: P2| math::add(p, math::scale(n, d - math::dot(math::sub(p, a), n)));
    let (ea, eb) = (foot(a), foot(b));
    let mut lines = Vec::new();
    for (p, e) in [(a, ea), (b, eb)] {
        let v = math::sub(e, p);
        let len = math::length(v);
        if len > st.gap {
            let dir = math::scale(v, 1.0 / len);
            lines.push(vec![math::add(p, math::scale(dir, st.gap)), math::add(e, math::scale(dir, st.overshoot))]);
        }
    }
    let along = math::dot(math::sub(eb, ea), u);
    let s = if along >= 0.0 { 1.0 } else { -1.0 };
    lines.push(vec![math::sub(ea, math::scale(u, s * st.overshoot)), math::add(eb, math::scale(u, s * st.overshoot))]);
    let tick = math::scale(math::add(u, n), st.tick / std::f64::consts::SQRT_2);
    for e in [ea, eb] {
        lines.push(vec![math::sub(e, tick), math::add(e, tick)]);
    }
    // The value sits where the label was dragged along the line, else in the middle.
    let mid = math::midpoint(ea, eb);
    let t = math::dot(math::sub(through, mid), u);
    let label = math::add(mid, math::scale(u, t));
    let (rot, up) = orient(u);
    (lines, label, up, rot)
}

fn unit(v: P2) -> Option<P2> {
    let l = math::length(v);
    (l > 1e-9).then(|| math::scale(v, 1.0 / l))
}

/// The dimensions of one layer's sketch.
pub fn sketch_dims(layer: Id, s: &Sketch, st: &DimStyle) -> Vec<DimDraw> {
    let pos = |id| s.point(id).map(|p| p2(p.pos));
    let line = |id| s.line_ends(id).and_then(|(a, b)| Some((pos(a)?, pos(b)?)));
    let mut out = Vec::new();
    for d in &s.dimensions {
        let Some(anchor) = s.measure_anchor(&d.measure).map(p2) else { continue };
        let Some(value) = s.measure(&d.measure) else { continue };
        let through = math::add(anchor, [d.label_offset.x, d.label_offset.y]);
        let lin = |a: P2, b: P2, u: Option<P2>| -> Option<(Vec<Vec<P2>>, P2, P2, f64)> {
            let u = u.or_else(|| unit(math::sub(b, a)))?;
            Some(linear(a, b, u, through, st))
        };
        let r = match d.measure {
            Measure::Distance { a, b } => pos(a).zip(pos(b)).and_then(|(a, b)| lin(a, b, None)),
            Measure::Length { line: l } => line(l).and_then(|(a, b)| lin(a, b, None)),
            Measure::HorizontalDistance { a, b } => pos(a).zip(pos(b)).and_then(|(a, b)| lin(a, b, Some([1.0, 0.0]))),
            Measure::VerticalDistance { a, b } => pos(a).zip(pos(b)).and_then(|(a, b)| lin(a, b, Some([0.0, 1.0]))),
            Measure::PointLineDistance { point, line: l } => pos(point).zip(line(l)).and_then(|(p, (a, b))| {
                let f = math::closest_on_line(a, b, p);
                lin(p, f, None)
            }),
            Measure::Angle { a, b } => line(a).zip(line(b)).map(|(la, lb)| angle_geom(la, lb, through, st)),
            Measure::Radius { arc } => match s.edge(arc).map(|e| e.geom) {
                Some(EdgeGeom::Arc { center, start, .. }) => pos(center).zip(pos(start)).and_then(|(c, p)| {
                    let r = math::dist(c, p);
                    let u = unit(math::sub(through, c)).or_else(|| unit(math::sub(p, c)))?;
                    let tip = math::add(c, math::scale(u, r));
                    let (rot, up) = orient(u);
                    Some((vec![vec![c, tip]], math::midpoint(c, tip), up, rot))
                }),
                _ => None,
            },
        };
        let Some((lines, label, up, rotation)) = r else { continue };
        let angle = d.measure.is_angle();
        let text = match d.measure {
            Measure::Radius { .. } => format!("R{}", text_for(value, false, d.driving, &st.format)),
            _ => text_for(value, angle, d.driving, &st.format),
        };
        out.push(DimDraw {
            source: DimSource::Sketch { layer, id: d.id },
            driving: d.driving,
            value,
            angle,
            text,
            name: d.name.clone(),
            label,
            up,
            rotation,
            lines,
        });
    }
    out
}

/// Arc between two lines around their intersection, through `through`.
fn angle_geom(la: (P2, P2), lb: (P2, P2), through: P2, st: &DimStyle) -> (Vec<Vec<P2>>, P2, P2, f64) {
    let ua = math::sub(la.1, la.0);
    let ub = math::sub(lb.1, lb.0);
    let c = math::line_intersection(la.0, ua, lb.0, ub).unwrap_or_else(|| math::midpoint(la.0, lb.0));
    let r = math::dist(c, through).max(st.tick * 4.0);
    let (a0, a1) = (math::angle_deg(ua), math::angle_deg(ub));
    let mut sweep = (a1 - a0).rem_euclid(360.0);
    if sweep > 180.0 {
        sweep -= 360.0;
    }
    let n = ((sweep.abs() / 5.0).ceil() as usize).max(2);
    let arc: Vec<P2> = (0..=n).map(|i| math::polar(c, r, a0 + sweep * i as f64 / n as f64)).collect();
    let mid_ang = a0 + sweep / 2.0;
    let label = math::polar(c, r, mid_ang);
    let (rot, up) = orient(math::polar([0.0, 0.0], 1.0, mid_ang + 90.0));
    (vec![arc], label, up, rot)
}

/// Reference dimensions between objects that `keep` accepts.
pub fn reference_dims(doc: &Document, st: &DimStyle, keep: impl Fn(&RefDimension) -> bool) -> Vec<DimDraw> {
    let mut out = Vec::new();
    for d in doc.dimensions.values().filter(|d| keep(d)) {
        let Some((a, b, value)) = d.measure(doc) else { continue };
        let u = match d.axis {
            DimAxis::Aligned => match unit(math::sub(b, a)) {
                Some(u) => u,
                None => continue,
            },
            DimAxis::Horizontal => [1.0, 0.0],
            DimAxis::Vertical => [0.0, 1.0],
        };
        let through = math::add(math::midpoint(a, b), d.label_offset);
        let (lines, label, up, rotation) = linear(a, b, u, through, st);
        out.push(DimDraw {
            source: DimSource::Reference(d.id),
            driving: false,
            value,
            angle: false,
            text: text_for(value, false, false, &st.format),
            name: d.name.clone(),
            label,
            up,
            rotation,
            lines,
        });
    }
    out
}

/// The axis a dimension between `a` and `b` takes when its label is put at `label`,
/// as AutoCAD's linear dimension does: above or below the span → horizontal, beside it
/// → vertical, otherwise (or when the points line up with an axis) aligned.
pub fn auto_axis(a: P2, b: P2, label: P2) -> DimAxis {
    let (dx, dy) = ((b[0] - a[0]).abs(), (b[1] - a[1]).abs());
    if dx < 1.0 || dy < 1.0 {
        return DimAxis::Aligned;
    }
    let within = |v: f64, p: f64, q: f64| v >= p.min(q) && v <= p.max(q);
    match (within(label[0], a[0], b[0]), within(label[1], a[1], b[1])) {
        (true, false) => DimAxis::Horizontal,
        (false, true) => DimAxis::Vertical,
        _ => DimAxis::Aligned,
    }
}

/// A dimension that does not exist yet (tool preview).
pub fn preview(a: P2, b: P2, axis: DimAxis, label: P2, st: &DimStyle) -> Option<DimDraw> {
    let u = match axis {
        DimAxis::Aligned => unit(math::sub(b, a))?,
        DimAxis::Horizontal => [1.0, 0.0],
        DimAxis::Vertical => [0.0, 1.0],
    };
    let value = match axis {
        DimAxis::Aligned => math::dist(a, b),
        DimAxis::Horizontal => (b[0] - a[0]).abs(),
        DimAxis::Vertical => (b[1] - a[1]).abs(),
    };
    let (lines, label, up, rotation) = linear(a, b, u, label, st);
    Some(DimDraw {
        source: DimSource::Reference(Id::nil()),
        driving: true,
        value,
        angle: false,
        text: format_shown(value, false, &st.format),
        name: None,
        label,
        up,
        rotation,
        lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stagecraft_core::sketch::Vec2;

    #[test]
    fn values_read_in_metres() {
        assert_eq!(format_value(13000.0, false), "13.00");
        assert_eq!(format_value(2250.0, false), "2.25");
        assert_eq!(format_value(1805.0, false), "1.805");
        assert_eq!(format_value(90.0, true), "90.0°");
    }

    #[test]
    fn a_length_dimension_runs_beside_its_line() {
        let mut s = Sketch::new();
        let (_, e) = s.add_polyline(&[Vec2::new(0.0, 0.0), Vec2::new(4000.0, 0.0)], false);
        let d = s.add_dimension(Measure::Length { line: e[0] }, 4000.0);
        s.dimension_mut(d).unwrap().label_offset = Vec2::new(0.0, -600.0);
        let st = DimStyle { tick: 50.0, gap: 30.0, overshoot: 50.0, format: LengthFormat::default() };
        let v = sketch_dims(Id::nil(), &s, &st);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].text, "4.00");
        assert!((v[0].label[1] + 600.0).abs() < 1e-9 && (v[0].label[0] - 2000.0).abs() < 1e-9);
        assert_eq!(v[0].rotation, 0.0);
        // Two extension lines, the dimension line, two ticks.
        assert_eq!(v[0].lines.len(), 5);
    }
}
