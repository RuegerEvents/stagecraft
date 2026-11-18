//! Drawing helpers for the UI: constraint inference from a line-drawing gesture.
//!
//! - start or end near an existing point → the line uses that point (coincident),
//! - near an existing line → a new point with "point on line",
//! - axis lock (Shift) → horizontal or vertical, whichever is closer; without the lock a
//!   line within `auto_axis_deg` of an axis also gets it,
//! - a typed length → a driving length dimension.
//!
//! [`infer_line`] only plans (the UI can preview the inferred constraints);
//! [`Edit::DrawLine`](crate::Edit::DrawLine) applies the plan and solves.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::{EdgeGeom, EdgeId, PointId, Sketch, Vec2};

/// What the user did: press at `start`, release (or click) at `end`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LineGesture {
    pub start: Vec2,
    pub end: Vec2,
    /// Shift held: force horizontal or vertical.
    #[serde(default)]
    pub axis_lock: bool,
    /// Length typed while drawing (mm).
    #[serde(default)]
    pub length: Option<f64>,
}

/// Tolerances for inference. `snap_radius` is in mm; the UI derives it from the zoom
/// (a few pixels).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InferOptions {
    pub snap_radius: f64,
    /// Add horizontal/vertical without Shift when the line is within this many degrees
    /// of an axis. `None` = only with Shift.
    pub auto_axis_deg: Option<f64>,
    /// Snap onto existing lines (point on line).
    pub snap_to_lines: bool,
}

impl Default for InferOptions {
    fn default() -> Self {
        Self { snap_radius: 100.0, auto_axis_deg: Some(2.0), snap_to_lines: true }
    }
}

/// An end of the planned line.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Endpoint {
    /// Reuse an existing point (snapped: coincident).
    Existing { point: PointId },
    /// A new point; `on_line` adds a point-on-line constraint.
    New {
        at: Vec2,
        #[serde(default)]
        on_line: Option<EdgeId>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// The line to draw and the constraints inferred for it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LinePlan {
    pub start: Endpoint,
    pub end: Endpoint,
    #[serde(default)]
    pub axis: Option<Axis>,
    /// Driving length dimension.
    #[serde(default)]
    pub length: Option<f64>,
}

impl Endpoint {
    pub fn position(&self, sketch: &Sketch) -> Vec2 {
        match *self {
            Endpoint::Existing { point } => sketch.pos(point),
            Endpoint::New { at, .. } => at,
        }
    }
}

/// Snaps a position: nearest point within the radius, else nearest line.
pub fn snap(sketch: &Sketch, at: Vec2, opts: &InferOptions) -> Endpoint {
    let nearest = sketch
        .points
        .iter()
        .map(|p| (p.id, p.pos.distance(at)))
        .filter(|&(_, d)| d <= opts.snap_radius)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((point, _)) = nearest {
        return Endpoint::Existing { point };
    }
    if opts.snap_to_lines {
        let mut best: Option<(EdgeId, Vec2, f64)> = None;
        for e in sketch.edges.iter() {
            let EdgeGeom::Line { a, b } = e.geom else {
                continue;
            };
            let (pa, pb) = (sketch.pos(a), sketch.pos(b));
            let d = pb - pa;
            let l2 = d.dot(d);
            if l2 == 0.0 {
                continue;
            }
            let t = (at - pa).dot(d) / l2;
            if !(0.0..=1.0).contains(&t) {
                continue;
            }
            let q = pa + d * t;
            let dist = q.distance(at);
            if dist <= opts.snap_radius && best.is_none_or(|b| dist < b.2) {
                best = Some((e.id, q, dist));
            }
        }
        if let Some((line, q, _)) = best {
            return Endpoint::New { at: q, on_line: Some(line) };
        }
    }
    Endpoint::New { at, on_line: None }
}

fn axis_of(d: Vec2, lock: bool, auto_deg: Option<f64>) -> Option<Axis> {
    if d.length() == 0.0 {
        return None;
    }
    let ang = d.y.abs().atan2(d.x.abs()).to_degrees(); // 0 = horizontal, 90 = vertical
    let nearest = if ang <= 45.0 { (Axis::Horizontal, ang) } else { (Axis::Vertical, 90.0 - ang) };
    if lock || auto_deg.is_some_and(|tol| nearest.1 <= tol) { Some(nearest.0) } else { None }
}

/// Plans a line from a gesture: snapping, axis and length.
pub fn infer_line(sketch: &Sketch, g: &LineGesture, opts: &InferOptions) -> LinePlan {
    let start = snap(sketch, g.start, opts);
    let p0 = start.position(sketch);
    let raw = g.end - p0;

    if let Some(len) = g.length {
        // typed length wins over snapping the end
        let axis = axis_of(raw, g.axis_lock, opts.auto_axis_deg);
        let dir = match axis {
            Some(Axis::Horizontal) => Vec2::new(raw.x.signum(), 0.0),
            Some(Axis::Vertical) => Vec2::new(0.0, raw.y.signum()),
            None if raw.length() > 0.0 => raw.normalized(),
            None => Vec2::new(1.0, 0.0),
        };
        return LinePlan { start, end: Endpoint::New { at: p0 + dir * len, on_line: None }, axis, length: Some(len) };
    }

    let mut end = snap(sketch, g.end, opts);
    if let (Endpoint::Existing { point: a }, Endpoint::Existing { point: b }) = (start, end)
        && a == b
    {
        end = Endpoint::New { at: g.end, on_line: None };
    }
    let p1 = end.position(sketch);
    let axis = axis_of(p1 - p0, g.axis_lock, opts.auto_axis_deg);
    if let (Some(axis), Endpoint::New { at, on_line: None }) = (axis, &mut end) {
        match axis {
            Axis::Horizontal => at.y = p0.y,
            Axis::Vertical => at.x = p0.x,
        }
    }
    LinePlan { start, end, axis, length: None }
}
