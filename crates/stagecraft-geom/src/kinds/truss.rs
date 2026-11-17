//! Truss: in plan a double line (the outer chords) with diagonals, sized by the
//! profile from the catalogue. Ends get a cross line. Three-chord profiles show the top
//! chord in the middle. Low detail draws only the outline.

use glam::{DAffine3, DVec3};
use stagecraft_core::{Object, ObjectKind};

use super::to_plan;
use crate::{Detail, LineClass, Lod, MaterialClass, Mesh, PlanPrim, PlanStyle, TrussProfile, truss_profile};

/// The profile of a truss object, or the fallback box for unknown names.
pub fn profile_of(obj: &Object) -> TrussProfile {
    match &obj.kind {
        ObjectKind::Truss { profile, .. } => {
            truss_profile(profile).cloned().unwrap_or_else(|| TrussProfile::fallback(profile))
        }
        _ => TrussProfile::fallback("?"),
    }
}

pub fn plan(w: &DAffine3, obj: &Object, style: &PlanStyle) -> Vec<PlanPrim> {
    let ObjectKind::Truss { length, .. } = &obj.kind else { return Vec::new() };
    let length = *length;
    let prof = profile_of(obj);
    // Plan sees the top chords; z of the symbol is the truss axis.
    let p = |x: f64, y: f64| to_plan(w, [x, y, 0.0]);
    let hw = prof.width / 2.0;
    if prof.chords == 1 {
        return vec![PlanPrim::Polyline {
            points: vec![p(0.0, -hw), p(length, -hw), p(length, hw), p(0.0, hw)],
            closed: true,
            line: LineClass::Medium,
        }];
    }
    let mut out = vec![PlanPrim::Polyline {
        points: vec![p(0.0, -hw), p(length, -hw), p(length, hw), p(0.0, hw)],
        closed: true,
        line: LineClass::Medium,
    }];
    if style.detail == Detail::High {
        // Chord centre lines, inside the outer edge by half a tube.
        let c = hw - prof.tube / 2.0;
        out.push(PlanPrim::line(vec![p(0.0, -c), p(length, -c)], LineClass::Thin));
        out.push(PlanPrim::line(vec![p(0.0, c), p(length, c)], LineClass::Thin));
        if prof.chords == 3 {
            out.push(PlanPrim::line(vec![p(0.0, 0.0), p(length, 0.0)], LineClass::Thin));
        }
        // Diagonals at about 45°: one bay per chord spacing.
        let spacing = 2.0 * c;
        let bays = ((length / spacing).round() as usize).max(1);
        let bay = length / bays as f64;
        let mut zig = Vec::with_capacity(bays + 1);
        for i in 0..=bays {
            let y = if i % 2 == 0 { -c } else { c };
            zig.push(p(i as f64 * bay, y));
        }
        out.push(PlanPrim::line(zig, LineClass::Thin));
    }
    if style.centre_lines {
        out.push(PlanPrim::line(vec![p(0.0, 0.0), p(length, 0.0)], LineClass::Centre));
    }
    out
}

/// Sides of a chord tube at full detail.
pub const CHORD_SEGMENTS: usize = 12;
/// Sides of a chord tube at simple detail.
pub const CHORD_SEGMENTS_SIMPLE: usize = 6;
/// Sides of a diagonal or end-frame brace.
pub const BRACE_SEGMENTS: usize = 6;

/// Chord centres in the cross-section (local y, z), mm. The truss axis is local x;
/// three-chord profiles stand on two chords with the apex up.
pub fn chord_centres(prof: &TrussProfile) -> Vec<[f64; 2]> {
    let cy = (prof.width - prof.tube).max(0.0) / 2.0;
    let cz = (prof.height - prof.tube).max(0.0) / 2.0;
    match prof.chords {
        0 | 1 => vec![[0.0, 0.0]],
        2 => vec![[-cy, 0.0], [cy, 0.0]],
        3 => vec![[-cy, -cz], [cy, -cz], [0.0, cz]],
        _ => vec![[-cy, -cz], [cy, -cz], [cy, cz], [-cy, cz]],
    }
}

/// Diameter of the diagonals: about 40 % of the chord, at least 10 mm.
pub fn brace_diameter(prof: &TrussProfile) -> f64 {
    (prof.tube * 0.4).max(10.0).min(prof.tube)
}

/// Chords along local +x from 0 to the length; diagonals zigzag on every face with
/// bays about as long as the face is wide (as in plan); end frames close each end.
/// `Simple` drops the bracing, `Box` gives the outer box.
pub fn mesh(obj: &Object, lod: Lod) -> Mesh {
    let ObjectKind::Truss { length, .. } = &obj.kind else { return Mesh::empty() };
    let length = *length;
    let prof = profile_of(obj);
    let mut m = Mesh::empty();
    m.set_material(MaterialClass::Metal);
    if length <= 0.0 {
        return m;
    }
    let (hw, hh) = (prof.width / 2.0, prof.height / 2.0);
    if lod == Lod::Box {
        m.cuboid(DVec3::new(0.0, -hw, -hh), DVec3::new(length, hw, hh));
        return m;
    }
    let chords = chord_centres(&prof);
    let r = prof.tube / 2.0;
    let segs = if lod == Lod::Full { CHORD_SEGMENTS } else { CHORD_SEGMENTS_SIMPLE };
    let at = |x: f64, c: [f64; 2]| DVec3::new(x, c[0], c[1]);
    for c in &chords {
        m.tube(at(0.0, *c), at(length, *c), r, segs);
    }
    if lod != Lod::Full || chords.len() < 2 {
        return m;
    }
    // Faces: neighbouring chord pairs around the section (a ladder has one).
    let faces: Vec<(usize, usize)> = match chords.len() {
        2 => vec![(0, 1)],
        n => (0..n).map(|i| (i, (i + 1) % n)).collect(),
    };
    let br = brace_diameter(&prof) / 2.0;
    for (fi, &(a, b)) in faces.iter().enumerate() {
        let (ca, cb) = (chords[a], chords[b]);
        let width = ((ca[0] - cb[0]).hypot(ca[1] - cb[1])).max(1.0);
        let bays = ((length / width).round() as usize).max(1);
        let bay = length / bays as f64;
        // Braces end on the chord surface, so they do not poke out of the far side.
        let dir = [(cb[0] - ca[0]) / width, (cb[1] - ca[1]) / width];
        let inset = |c: [f64; 2], s: f64| [c[0] + dir[0] * s * r, c[1] + dir[1] * s * r];
        let (ia, ib) = (inset(ca, 1.0), inset(cb, -1.0));
        // Alternate the zigzag phase per face, as real truss does.
        let phase = fi % 2;
        for k in 0..bays {
            // The tilted end discs must not stick out past the truss ends.
            let (x0, x1) = ((k as f64 * bay).max(br), ((k + 1) as f64 * bay).min(length - br));
            let (p, q) = if (k + phase) % 2 == 0 { (ia, ib) } else { (ib, ia) };
            m.tube(at(x0, p), at(x1, q), br, BRACE_SEGMENTS);
        }
        // End frames, inset by half a brace from the ends.
        for x in [br, length - br] {
            m.tube(at(x, ia), at(x, ib), br, BRACE_SEGMENTS);
        }
    }
    m
}
