//! Truss: in plan a double line (the outer chords) with diagonals, sized by the
//! profile from the catalogue. Ends get a cross line. Three-chord profiles show the top
//! chord in the middle. Low detail draws only the outline.

use glam::DAffine3;
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

pub fn mesh(_obj: &Object, _lod: Lod) -> Mesh {
    // TODO(block 3): chords and diagonals as tubes; a box at Lod::Low.
    Mesh::empty(MaterialClass::Metal)
}
