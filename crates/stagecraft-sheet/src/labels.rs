//! Fixture labels: where each field of a template goes for a fixture, in which of the
//! three rotation modes, and how labels move out of each other's way.
//!
//! Everything here works in paper mm (y up). The fixture comes in as its centre and
//! its local x/y axes on the paper (scaled: paper mm per model mm), so a mirrored
//! fixture (turned over, rotation x 180°) simply has a flipped y axis. Text is only
//! ever rotated, never mirrored, and the angle is kept in (−90°, 90°], so nothing
//! reads upside down.
//!
//! Placement is greedy, field by field: hand-dragged labels first (as a whole), then
//! the fields centred on their fixtures (FIDs), then the rest, top-left to
//! bottom-right. Each field tries its template position, then small displacements on
//! rings around it, and takes the first one that touches no placed field and no
//! fixture; failing that, the least overlap. Fields moved further than
//! [`LEADER_DISTANCE`] get a leader line back to their fixture.

use stagecraft_core::model::{Document, Object, ObjectKind};
use stagecraft_core::sheet::{LabelRotation, LabelTemplate};
use stagecraft_core::{Id, geom};

use crate::prim::P2;

/// A label displaced further than this (paper mm) gets a leader line.
pub const LEADER_DISTANCE: f64 = 3.0;

/// Text width in mm for (text, cap height mm, bold).
pub type Measure<'a> = &'a dyn Fn(&str, f64, bool) -> f64;

/// A fixture as the label layout sees it, in paper mm.
#[derive(Clone, Debug, PartialEq)]
pub struct FixtureInput {
    pub id: Id,
    pub center: P2,
    /// Fixture-local +x on the paper, length = paper mm per model mm.
    pub x_axis: P2,
    /// Fixture-local +y (towards the lens) on the paper, same length.
    pub y_axis: P2,
    /// The fixture's footprint on the paper, `[min, max]`; other labels keep off it.
    pub body: [P2; 2],
    /// One text per template field; empty fields are left out.
    pub values: Vec<String>,
    /// A hand-dragged offset (paper mm) that replaces automatic placement.
    pub manual: Option<P2>,
}

/// One placed field: a line of text, centred at `center`, turned by `angle`.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub text: String,
    pub center: P2,
    /// Degrees counter-clockwise, always in (−90, 90].
    pub angle: f64,
    /// Cap height, mm.
    pub size: f64,
    pub bold: bool,
    pub boxed: bool,
    /// Half width and half height of the box around the text (with padding).
    pub half: P2,
}

impl Field {
    /// The box corners, counter-clockwise.
    pub fn corners(&self) -> [P2; 4] {
        let (s, c) = self.angle.to_radians().sin_cos();
        let [hw, hh] = self.half;
        let p = |x: f64, y: f64| [self.center[0] + x * c - y * s, self.center[1] + x * s + y * c];
        [p(-hw, -hh), p(hw, -hh), p(hw, hh), p(-hw, hh)]
    }

    pub fn aabb(&self) -> [P2; 2] {
        bounds_of(&self.corners())
    }

    fn shifted(&self, d: P2) -> Field {
        Field { center: [self.center[0] + d[0], self.center[1] + d[1]], ..self.clone() }
    }
}

/// A fixture's label after placement.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub fixture: Id,
    pub fields: Vec<Field>,
    /// The largest move of a field from its template position, paper mm.
    pub offset: P2,
    /// From the fixture's edge to fields that moved far.
    pub leaders: Vec<[P2; 2]>,
    pub manual: bool,
}

impl Label {
    pub fn bounds(&self) -> [P2; 2] {
        let pts: Vec<P2> = self.fields.iter().flat_map(|f| f.corners()).collect();
        bounds_of(&pts)
    }
}

fn bounds_of(pts: &[P2]) -> [P2; 2] {
    let mut b = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    for p in pts {
        b[0] = [b[0][0].min(p[0]), b[0][1].min(p[1])];
        b[1] = [b[1][0].max(p[0]), b[1][1].max(p[1])];
    }
    b
}

/// An angle turned into (−90°, 90°], so text along it never reads upside down.
pub fn upright(deg: f64) -> f64 {
    let mut a = deg.rem_euclid(360.0);
    if a > 180.0 {
        a -= 360.0;
    }
    if a > 90.0 + 1e-9 {
        a -= 180.0;
    } else if a <= -90.0 + 1e-9 {
        a += 180.0;
    }
    if a.abs() < 1e-9 { 0.0 } else { a }
}

fn len(v: P2) -> f64 {
    v[0].hypot(v[1])
}

/// The fields of one fixture at the template position.
pub fn layout(t: &LabelTemplate, f: &FixtureInput, measure: Measure) -> Vec<Field> {
    let scale = len(f.x_axis).max(len(f.y_axis)).max(1e-12);
    let (xa, ya) = match t.rotation {
        LabelRotation::SheetFixed => ([scale, 0.0], [0.0, scale]),
        _ => (f.x_axis, f.y_axis),
    };
    let angle = match t.rotation {
        LabelRotation::Follow => upright(xa[1].atan2(xa[0]).to_degrees()),
        _ => 0.0,
    };
    let (s, c) = angle.to_radians().sin_cos();
    let (e1, e2) = ([c, s], [-s, c]);
    // How far along each anchor's direction the next stacked field starts.
    let mut stacks: Vec<(P2, f64)> = Vec::new();
    let mut out = Vec::new();
    for (field, value) in t.fields.iter().zip(&f.values) {
        if value.is_empty() {
            continue;
        }
        let text = format!("{}{}", field.prefix, value);
        let pad = if field.boxed { 0.35 * field.size } else { 0.12 * field.size };
        let half = [measure(&text, field.size, field.bold) / 2.0 + pad, field.size / 2.0 + pad];
        let a = field.anchor;
        let anchor = [f.center[0] + a[0] * xa[0] + a[1] * ya[0], f.center[1] + a[0] * xa[1] + a[1] * ya[1]];
        let d = [anchor[0] - f.center[0], anchor[1] - f.center[1]];
        let key = [(a[0] / 10.0).round(), (a[1] / 10.0).round()];
        let slot = match stacks.iter().position(|(k, _)| *k == key) {
            Some(i) => i,
            None => {
                stacks.push((key, 0.0));
                stacks.len() - 1
            }
        };
        let used = stacks[slot].1;
        let center = if len(d) < 1e-9 {
            // Centred on the fixture; further centred fields stack below (along −e2).
            let off = if used > 0.0 { used + half[1] } else { 0.0 };
            stacks[slot].1 = if used > 0.0 { used + 2.0 * half[1] } else { half[1] };
            [anchor[0] - e2[0] * off, anchor[1] - e2[1] * off]
        } else {
            let n = [d[0] / len(d), d[1] / len(d)];
            // The box's extent along n, so its near side sits `gap` beyond the anchor.
            let extent = half[0] * (n[0] * e1[0] + n[1] * e1[1]).abs() + half[1] * (n[0] * e2[0] + n[1] * e2[1]).abs();
            let dist = t.gap + used + extent;
            stacks[slot].1 = used + 2.0 * extent;
            [anchor[0] + n[0] * dist, anchor[1] + n[1] * dist]
        };
        out.push(Field { text, center, angle, size: field.size, bold: field.bold, boxed: field.boxed, half });
    }
    out
}

fn overlap(a: &[P2; 2], b: &[P2; 2]) -> f64 {
    let w = a[1][0].min(b[1][0]) - a[0][0].max(b[0][0]);
    let h = a[1][1].min(b[1][1]) - a[0][1].max(b[0][1]);
    if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
}

/// Small displacements to try, nearest first.
fn candidates() -> Vec<P2> {
    let mut v = vec![[0.0, 0.0]];
    for r in [1.0, 2.0, 3.0, 4.5, 6.0, 8.0, 10.5, 13.5, 17.0, 21.0] {
        let n = if r <= 4.5 { 8 } else { 16 };
        for i in 0..n {
            // Start upwards and to the right: labels read better above their fixture.
            let a = (90.0 - 360.0 / n as f64 * i as f64).to_radians();
            v.push([r * a.cos(), r * a.sin()]);
        }
    }
    v
}

/// Place labels for all fixtures. `obstacles` are further areas to keep clear (paper
/// mm boxes), e.g. other text in the viewport.
///
/// Fields move one by one: a field centred on its fixture (the FID) may cover its own
/// fixture, every other field keeps off all fixtures. A dragged label moves as a whole.
pub fn place(t: &LabelTemplate, fixtures: &[FixtureInput], measure: Measure, obstacles: &[[P2; 2]]) -> Vec<Label> {
    let mut order: Vec<usize> = (0..fixtures.len()).collect();
    order.sort_by(|&a, &b| {
        let (fa, fb) = (&fixtures[a], &fixtures[b]);
        fb.manual
            .is_some()
            .cmp(&fa.manual.is_some())
            .then(fb.center[1].total_cmp(&fa.center[1]))
            .then(fa.center[0].total_cmp(&fb.center[0]))
    });
    let cands = candidates();
    let mut taken: Vec<[P2; 2]> = Vec::new();
    let mut out: Vec<Option<Label>> = vec![None; fixtures.len()];
    // Centred fields of all fixtures go first, so FIDs keep their place on the symbol.
    let bases: Vec<Vec<Field>> = fixtures.iter().map(|f| layout(t, f, measure)).collect();
    let centred = |f: &FixtureInput, x: &Field| len([x.center[0] - f.center[0], x.center[1] - f.center[1]]) < 1e-6;
    let mut placed: Vec<Vec<(usize, Field, P2)>> = vec![Vec::new(); fixtures.len()];
    for pass in [true, false] {
        for &i in &order {
            let f = &fixtures[i];
            for (k, field) in bases[i].iter().enumerate() {
                if f.manual.is_none() && centred(f, field) != pass {
                    continue;
                }
                if f.manual.is_some() && !pass {
                    continue;
                }
                let own_ok = centred(f, field);
                let b = field.aabb();
                let cost = |d: P2| -> f64 {
                    let b = [[b[0][0] + d[0], b[0][1] + d[1]], [b[1][0] + d[0], b[1][1] + d[1]]];
                    taken.iter().map(|o| overlap(&b, o)).sum::<f64>() * 4.0
                        + obstacles.iter().map(|o| overlap(&b, o)).sum::<f64>() * 2.0
                        + fixtures
                            .iter()
                            .filter(|g| !(own_ok && g.id == f.id))
                            .map(|g| overlap(&b, &g.body))
                            .sum::<f64>()
                };
                let d = match f.manual {
                    Some(m) => m,
                    None => {
                        let mut best = ([0.0, 0.0], f64::INFINITY);
                        for d in &cands {
                            let c = cost(*d);
                            if c <= 1e-9 {
                                best = (*d, 0.0);
                                break;
                            }
                            let score = c + len(*d) * 0.02;
                            if score < best.1 {
                                best = (*d, score);
                            }
                        }
                        best.0
                    }
                };
                let moved = field.shifted(d);
                taken.push(moved.aabb());
                placed[i].push((k, moved, d));
            }
        }
    }
    for &i in &order {
        let f = &fixtures[i];
        let mut fields = std::mem::take(&mut placed[i]);
        if fields.is_empty() {
            continue;
        }
        fields.sort_by_key(|x| x.0);
        let mut leaders = Vec::new();
        let manual = f.manual.is_some();
        if manual {
            let label_box = bounds_of(&fields.iter().flat_map(|x| x.1.corners()).collect::<Vec<_>>());
            if len(f.manual.unwrap_or_default()) > LEADER_DISTANCE {
                leaders.extend(leader(f, &label_box));
            }
        } else {
            for (_, x, d) in &fields {
                if len(*d) > LEADER_DISTANCE {
                    leaders.extend(leader(f, &x.aabb()));
                }
            }
        }
        let offset = fields.iter().map(|x| x.2).fold([0.0, 0.0], |a, b| if len(b) > len(a) { b } else { a });
        out[i] =
            Some(Label { fixture: f.id, fields: fields.into_iter().map(|x| x.1).collect(), offset, leaders, manual });
    }
    out.into_iter().flatten().collect()
}

/// A line from the fixture's footprint edge to the nearest point of the label box.
fn leader(f: &FixtureInput, b: &[P2; 2]) -> Option<[P2; 2]> {
    let c = f.center;
    let to = [c[0].clamp(b[0][0], b[1][0]), c[1].clamp(b[0][1], b[1][1])];
    let d = [to[0] - c[0], to[1] - c[1]];
    if len(d) < 1e-6 {
        return None;
    }
    // Leave the footprint box along d.
    let hw = (f.body[1][0] - f.body[0][0]) / 2.0;
    let hh = (f.body[1][1] - f.body[0][1]) / 2.0;
    let t = [
        if d[0].abs() > 1e-12 { hw / d[0].abs() } else { f64::INFINITY },
        if d[1].abs() > 1e-12 { hh / d[1].abs() } else { f64::INFINITY },
    ];
    let t = t[0].min(t[1]).min(1.0);
    let from = [c[0] + d[0] * t, c[1] + d[1] * t];
    (len([to[0] - from[0], to[1] - from[1]]) > 0.3).then_some([from, to])
}

/// The text of a label field for an object (empty when it has none).
pub fn field_value(doc: &Document, o: &Object, field: &str) -> String {
    let ObjectKind::Fixture { fid, cid, weight_kg, .. } = &o.kind else {
        return field.strip_prefix("attr:").and_then(|k| o.attrs.get(k)).cloned().unwrap_or_default();
    };
    match field {
        "fid" => fid.map(|f| f.to_string()).unwrap_or_default(),
        "cid" => cid.clone().unwrap_or_default(),
        "address" => stagecraft_core::patch::address_text(o),
        "mode" => mode_name(doc, o),
        "type" => type_name(doc, o),
        "footprint" => footprint_text(doc, o),
        "weight" => weight_kg.map(|w| format!("{w:.1} kg")).unwrap_or_default(),
        "name" => o.name.clone(),
        "gel" | "circuit" | "focus" | "note" => o.attrs.get(field).cloned().unwrap_or_default(),
        other => other.strip_prefix("attr:").and_then(|k| o.attrs.get(k)).cloned().unwrap_or_default(),
    }
}

/// A fixture's type for labels, legends and lists: the library's `Manufacturer
/// Name`, else the stored key, else "Generic fixture".
pub fn type_name(doc: &Document, o: &Object) -> String {
    match &o.kind {
        ObjectKind::Fixture { gdtf: Some(g), .. } => match doc.fixture_types.get(g) {
            Some(t) => t.display_name(),
            None => {
                let base = g.rsplit('/').next().unwrap_or(g);
                base.strip_suffix(".gdtf").unwrap_or(base).replace(['_', '@'], " ")
            }
        },
        _ => "Generic fixture".into(),
    }
}

/// The mode as set, or the type's first mode.
pub fn mode_name(doc: &Document, o: &Object) -> String {
    match stagecraft_core::patch::mode(doc, o) {
        Some(m) => m.name.clone(),
        None => match &o.kind {
            ObjectKind::Fixture { mode, .. } => mode.clone().unwrap_or_default(),
            _ => String::new(),
        },
    }
}

/// `16 ch`, `2+24 ch`; empty without a type.
pub fn footprint_text(doc: &Document, o: &Object) -> String {
    stagecraft_core::patch::mode(doc, o).map(|m| format!("{} ch", m.footprint_text())).unwrap_or_default()
}

/// The label input for a fixture. `to_paper` maps world plan mm to paper mm;
/// `paper_per_mm` is 1 / scale denominator.
pub fn fixture_input(
    doc: &Document,
    o: &Object,
    t: &LabelTemplate,
    to_paper: &dyn Fn(P2) -> P2,
    paper_per_mm: f64,
    manual: Option<P2>,
) -> Option<FixtureInput> {
    if !matches!(o.kind, ObjectKind::Fixture { .. }) {
        return None;
    }
    let w = geom::world(doc, o.id)?;
    let origin = w.translation;
    let axis = |v: glam::DVec3, fallback: P2| -> P2 {
        let p = w.transform_vector3(v);
        let l = p.x.hypot(p.y);
        if l < 0.2 { fallback } else { [p.x / l * paper_per_mm, p.y / l * paper_per_mm] }
    };
    let x_axis = axis(glam::DVec3::X, [paper_per_mm, 0.0]);
    let y_axis = axis(glam::DVec3::Y, [-x_axis[1], x_axis[0]]);
    let center = to_paper([origin.x, origin.y]);
    let (lo, hi) = stagecraft_geom::gdtf::plan_bounds(doc, o);
    let corners: Vec<P2> = [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]]]
        .iter()
        .map(|[x, y]| [center[0] + x * x_axis[0] + y * y_axis[0], center[1] + x * x_axis[1] + y * y_axis[1]])
        .collect();
    let values = t.fields.iter().map(|f| field_value(doc, o, &f.field)).collect();
    Some(FixtureInput { id: o.id, center, x_axis, y_axis, body: bounds_of(&corners), values, manual })
}

#[cfg(test)]
mod tests {
    use stagecraft_core::sheet::{LabelField, default_label_fields};
    use uuid::Uuid;

    use super::*;

    fn measure(t: &str, size: f64, _: bool) -> f64 {
        t.chars().count() as f64 * size * 0.6
    }

    fn template(rotation: LabelRotation) -> LabelTemplate {
        LabelTemplate { id: Uuid::nil(), name: "t".into(), rotation, fields: default_label_fields(), gap: 0.8 }
    }

    /// A fixture at `c` turned by `deg` (counter-clockwise in plan), optionally turned
    /// over (mirrored y), at 1:50.
    fn fixture(c: P2, deg: f64, mirrored: bool) -> FixtureInput {
        let k = 1.0 / 50.0;
        let (s, co) = deg.to_radians().sin_cos();
        let x = [co * k, s * k];
        let y = if mirrored { [s * k, -co * k] } else { [-s * k, co * k] };
        FixtureInput {
            id: Uuid::new_v4(),
            center: c,
            x_axis: x,
            y_axis: y,
            body: [[c[0] - 3.4, c[1] - 3.4], [c[0] + 3.4, c[1] + 3.4]],
            values: vec!["101".into(), "L201".into(), "1.001".into()],
            manual: None,
        }
    }

    fn dot(a: P2, b: P2) -> f64 {
        a[0] * b[0] + a[1] * b[1]
    }

    #[test]
    fn upright_keeps_text_readable() {
        for (a, want) in [(0.0, 0.0), (90.0, 90.0), (91.0, -89.0), (180.0, 0.0), (270.0, 90.0), (-90.0, 90.0)] {
            assert!((upright(a) - want).abs() < 1e-9, "{a} → {}", upright(a));
        }
        for i in 0..72 {
            let u = upright(i as f64 * 5.0 - 180.0);
            assert!(u > -90.0 && u <= 90.0);
        }
    }

    #[test]
    fn rotated_fixtures_never_get_mirrored_or_upside_down_text() {
        for rotation in [LabelRotation::AnchoredUpright, LabelRotation::SheetFixed, LabelRotation::Follow] {
            let t = template(rotation);
            for deg in [0.0, 90.0, 180.0, 270.0, 45.0, 135.0, 225.0, 315.0] {
                for mirrored in [false, true] {
                    let f = fixture([100.0, 100.0], deg, mirrored);
                    let fields = layout(&t, &f, &measure);
                    assert_eq!(fields.len(), 3);
                    for x in &fields {
                        // Upside down would be beyond ±90°; mirrored text has no angle
                        // that produces it, and we only ever emit angles.
                        assert!(x.angle > -90.0 && x.angle <= 90.0, "{rotation:?} {deg} {mirrored}: {}", x.angle);
                        if rotation != LabelRotation::Follow {
                            assert_eq!(x.angle, 0.0);
                        }
                        // Reading direction: the text's x axis points right or straight up.
                        let (s, c) = x.angle.to_radians().sin_cos();
                        assert!(c > 1e-9 || (c.abs() < 1e-9 && s > 0.0));
                    }
                    let gel = &fields[1];
                    let to_gel = [gel.center[0] - f.center[0], gel.center[1] - f.center[1]];
                    match rotation {
                        // The gel sits in front of the lens, wherever the lens points.
                        LabelRotation::AnchoredUpright | LabelRotation::Follow => {
                            assert!(dot(to_gel, f.y_axis) > 0.0, "{rotation:?} {deg} {mirrored}")
                        }
                        // Sheet-fixed: always above, as for an unrotated fixture.
                        LabelRotation::SheetFixed => assert!(to_gel[1] > 0.0 && to_gel[0].abs() < 1e-9),
                    }
                    // The FID is centred on the fixture.
                    assert!(len([fields[0].center[0] - 100.0, fields[0].center[1] - 100.0]) < 1e-9);
                }
            }
        }
    }

    #[test]
    fn follow_mode_turns_text_with_the_fixture() {
        let t = template(LabelRotation::Follow);
        let at = |deg| layout(&t, &fixture([0.0, 0.0], deg, false), &measure)[0].angle;
        assert_eq!(at(0.0), 0.0);
        assert!((at(30.0) - 30.0).abs() < 1e-9);
        assert!((at(90.0) - 90.0).abs() < 1e-9);
        // Past 90° it flips by 180° instead of reading upside down.
        assert!((at(150.0) + 30.0).abs() < 1e-9);
        assert!(at(180.0).abs() < 1e-9);
        assert!((at(270.0) - 90.0).abs() < 1e-9);
    }

    #[test]
    fn fields_on_one_anchor_stack_instead_of_overlapping() {
        let mut t = template(LabelRotation::AnchoredUpright);
        t.fields = vec![
            LabelField {
                field: "a".into(),
                anchor: [0.0, 300.0],
                size: 2.0,
                boxed: false,
                bold: false,
                prefix: "".into(),
            },
            LabelField {
                field: "b".into(),
                anchor: [0.0, 300.0],
                size: 2.0,
                boxed: false,
                bold: false,
                prefix: "".into(),
            },
        ];
        let mut f = fixture([0.0, 0.0], 0.0, false);
        f.values = vec!["one".into(), "two".into()];
        let fields = layout(&t, &f, &measure);
        assert!(fields[1].center[1] > fields[0].center[1] + 2.0);
        assert_eq!(overlap(&fields[0].aabb(), &fields[1].aabb()), 0.0);
    }

    #[test]
    fn overlapping_fixtures_get_separated_labels() {
        let t = template(LabelRotation::AnchoredUpright);
        // Five fixtures 150 mm apart at 1:50 (3 mm on paper): their labels collide.
        let fx: Vec<FixtureInput> = (0..5).map(|i| fixture([100.0 + i as f64 * 3.0, 100.0], 0.0, false)).collect();
        let raw: Vec<Vec<Field>> = fx.iter().map(|f| layout(&t, f, &measure)).collect();
        let raw_clash = (1..5).any(|i| overlap(&raw[0][0].aabb(), &raw[i][0].aabb()) > 0.0);
        assert!(raw_clash, "the test needs colliding template positions");
        let labels = place(&t, &fx, &measure, &[]);
        assert_eq!(labels.len(), 5);
        let boxes: Vec<[P2; 2]> = labels.iter().flat_map(|l| l.fields.iter().map(Field::aabb)).collect();
        for i in 0..boxes.len() {
            for j in i + 1..boxes.len() {
                assert!(overlap(&boxes[i], &boxes[j]) < 1e-6, "fields {i} and {j} overlap");
            }
        }
        // Someone had to move, and far moves have leaders.
        assert!(labels.iter().any(|l| len(l.offset) > 0.0));
        for l in &labels {
            if !l.leaders.is_empty() {
                assert!(len(l.offset) > LEADER_DISTANCE);
            }
        }
        // FIDs stay on their fixtures.
        for (l, f) in labels.iter().map(|l| (l, fx.iter().find(|f| f.id == l.fixture).unwrap())) {
            let fid = &l.fields[0];
            assert!(len([fid.center[0] - f.center[0], fid.center[1] - f.center[1]]) < 1e-6 || len(l.offset) > 0.0);
        }
    }

    #[test]
    fn dragged_labels_stay_where_they_were_put() {
        let t = template(LabelRotation::AnchoredUpright);
        let mut a = fixture([100.0, 100.0], 0.0, false);
        let b = fixture([101.0, 100.0], 0.0, false);
        a.manual = Some([0.0, 20.0]);
        let labels = place(&t, &[a.clone(), b], &measure, &[]);
        let la = labels.iter().find(|l| l.fixture == a.id).unwrap();
        assert!(la.manual);
        assert_eq!(la.offset, [0.0, 20.0]);
        assert!(!la.leaders.is_empty());
    }
}
