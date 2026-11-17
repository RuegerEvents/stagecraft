//! Snapping for drawing and moving: object points first, then truss axes, then the
//! grid. Orthogonal mode (Shift) locks the direction from the previous point to the
//! nearer axis and snaps the length.

use serde::{Deserialize, Serialize};
use stagecraft_core::model::P2;
use stagecraft_core::{Document, Id, ObjectKind};

use crate::kinds::snap_geometry;
use crate::math;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapKind {
    Endpoint,
    Midpoint,
    /// On the axis of a truss piece.
    Truss,
    Grid,
    /// Locked to horizontal or vertical from the previous point.
    Orthogonal,
    /// Nothing near.
    Free,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapped {
    pub point: P2,
    pub kind: SnapKind,
    /// The object that provided the point, if any.
    pub source: Option<Id>,
}

/// Which snaps are on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapSettings {
    /// Grid spacing in mm; `None` turns grid snapping off.
    pub grid: Option<f64>,
    pub endpoints: bool,
    pub midpoints: bool,
    pub truss: bool,
}

impl Default for SnapSettings {
    fn default() -> Self {
        Self { grid: Some(100.0), endpoints: true, midpoints: true, truss: true }
    }
}

/// Snap points and lines of the objects in view, gathered once per frame.
#[derive(Clone, Debug, Default)]
pub struct Features {
    pub endpoints: Vec<(P2, Id)>,
    pub midpoints: Vec<(P2, Id)>,
    /// Truss axes in plan.
    pub axes: Vec<([P2; 2], Id)>,
}

impl Features {
    /// Collect from `ids`, leaving out `exclude` (the objects being moved).
    pub fn collect(doc: &Document, ids: impl IntoIterator<Item = Id>, exclude: &[Id]) -> Self {
        let mut f = Features::default();
        for id in ids {
            if exclude.contains(&id) {
                continue;
            }
            let Some(o) = doc.objects.get(&id) else { continue };
            let (pts, segs) = snap_geometry(doc, o);
            f.endpoints.extend(pts.into_iter().map(|p| (p, id)));
            f.midpoints.extend(segs.iter().map(|s| (math::midpoint(s[0], s[1]), id)));
            if matches!(o.kind, ObjectKind::Truss { .. }) {
                f.axes.extend(segs.into_iter().map(|s| (s, id)));
            }
        }
        f
    }
}

/// Round to the nearest multiple of `step`.
pub fn round_to(v: f64, step: f64) -> f64 {
    if step <= 0.0 { v } else { (v / step).round() * step }
}

pub fn grid_snap(p: P2, step: f64) -> P2 {
    [round_to(p[0], step), round_to(p[1], step)]
}

/// Lock `p` to the horizontal or vertical through `from`, whichever is nearer.
pub fn orthogonal(from: P2, p: P2) -> P2 {
    let d = math::sub(p, from);
    if d[0].abs() >= d[1].abs() { [p[0], from[1]] } else { [from[0], p[1]] }
}

fn nearest(points: &[(P2, Id)], p: P2, tol: f64) -> Option<(P2, Id, f64)> {
    points.iter().map(|(q, id)| (*q, *id, math::dist(*q, p))).filter(|x| x.2 <= tol).min_by(|a, b| a.2.total_cmp(&b.2))
}

/// Snap `p`. `from` is the previous point of the current gesture (for orthogonal
/// mode); `tol` is the pick radius in mm (pixels divided by zoom).
pub fn snap(p: P2, from: Option<P2>, ortho: bool, f: &Features, s: &SnapSettings, tol: f64) -> Snapped {
    if ortho && let Some(from) = from {
        return snap_orthogonal(p, from, f, s, tol);
    }
    if s.endpoints
        && let Some((q, id, _)) = nearest(&f.endpoints, p, tol)
    {
        return Snapped { point: q, kind: SnapKind::Endpoint, source: Some(id) };
    }
    if s.midpoints
        && let Some((q, id, _)) = nearest(&f.midpoints, p, tol)
    {
        return Snapped { point: q, kind: SnapKind::Midpoint, source: Some(id) };
    }
    if s.truss {
        let hit = f
            .axes
            .iter()
            .map(|(seg, id)| {
                let q = math::closest_on_segment(seg[0], seg[1], p);
                (q, *id, math::dist(q, p))
            })
            .filter(|x| x.2 <= tol)
            .min_by(|a, b| a.2.total_cmp(&b.2));
        if let Some((q, id, _)) = hit {
            // Along the truss, round the distance from its start to the grid.
            let seg = f.axes.iter().find(|a| a.1 == id).expect("found above").0;
            let q = match s.grid {
                Some(g) => {
                    let len = math::dist(seg[0], seg[1]);
                    let along = round_to(math::dist(seg[0], q), g).min(len);
                    math::lerp(seg[0], seg[1], if len > 0.0 { along / len } else { 0.0 })
                }
                None => q,
            };
            return Snapped { point: q, kind: SnapKind::Truss, source: Some(id) };
        }
    }
    match s.grid {
        Some(g) => Snapped { point: grid_snap(p, g), kind: SnapKind::Grid, source: None },
        None => Snapped { point: p, kind: SnapKind::Free, source: None },
    }
}

fn snap_orthogonal(p: P2, from: P2, f: &Features, s: &SnapSettings, tol: f64) -> Snapped {
    let q = orthogonal(from, p);
    let horizontal = q[1] == from[1];
    // An object point close to the locked line: align with it.
    if s.endpoints || s.midpoints {
        let pts = s.endpoints.then_some(&f.endpoints).into_iter().chain(s.midpoints.then_some(&f.midpoints)).flatten();
        let aligned = pts
            .map(|(e, id)| {
                let on = if horizontal { [e[0], from[1]] } else { [from[0], e[1]] };
                (on, *id, math::dist(on, q))
            })
            .filter(|x| x.2 <= tol)
            .min_by(|a, b| a.2.total_cmp(&b.2));
        if let Some((on, id, _)) = aligned {
            return Snapped { point: on, kind: SnapKind::Orthogonal, source: Some(id) };
        }
    }
    let q = match s.grid {
        Some(g) => {
            let len = round_to(math::dist(from, q), g);
            let dir = math::normalize(math::sub(q, from));
            math::add(from, math::scale(dir, len))
        }
        None => q,
    };
    Snapped { point: q, kind: SnapKind::Orthogonal, source: None }
}
