//! The sketch document: points, edges, constraints and dimensions.
//!
//! Everything here is plain data (serde + JSON schema) so it can live in the StageCraft
//! document and travel as command arguments. Lengths are millimetres, angles degrees.

use std::ops::{Add, Mul, Neg, Sub};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(pub u32);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_type!(
    /// Stable id of a point. Never reused within a sketch.
    PointId
);
id_type!(
    /// Stable id of an edge (line or arc). Never reused within a sketch.
    EdgeId
);
id_type!(
    /// Stable id of a constraint or a dimension. Both share this id space so a conflict
    /// report can name either.
    ConstraintId
);

/// A 2D vector or position in millimetres.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    /// z component of the 3D cross product.
    pub fn cross(self, o: Vec2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    pub fn distance(self, o: Vec2) -> f64 {
        (o - self).length()
    }
    /// Rotated by +90° (counter-clockwise).
    pub fn perp(self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }
    pub fn normalized(self) -> Vec2 {
        let l = self.length();
        if l > 0.0 { self * (1.0 / l) } else { self }
    }
    /// Angle of the vector in radians, `atan2(y, x)`.
    pub fn angle(self) -> f64 {
        self.y.atan2(self.x)
    }
    pub fn rotated(self, radians: f64) -> Vec2 {
        let (s, c) = radians.sin_cos();
        Vec2::new(c * self.x - s * self.y, s * self.x + c * self.y)
    }
    pub fn lerp(self, o: Vec2, t: f64) -> Vec2 {
        self + (o - self) * t
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f64) -> Vec2 {
        Vec2::new(self.x * s, self.y * s)
    }
}
impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

/// A sketch point. Lines and arcs reference points by id, so connected edges share
/// their end point (no coincident constraint needed).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Point {
    pub id: PointId,
    pub pos: Vec2,
}

/// The geometry of an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EdgeGeom {
    /// Straight segment from `a` to `b`.
    Line { a: PointId, b: PointId },
    /// Circular arc around `center`, counter-clockwise from `start` to `end`. The solver
    /// keeps `start` and `end` at the same distance from `center`.
    Arc { center: PointId, start: PointId, end: PointId },
}

impl EdgeGeom {
    /// End points of the edge (for an arc: start and end, not the centre).
    pub fn ends(&self) -> (PointId, PointId) {
        match *self {
            EdgeGeom::Line { a, b } => (a, b),
            EdgeGeom::Arc { start, end, .. } => (start, end),
        }
    }
    /// Every point the edge uses, including an arc's centre.
    pub fn points(&self) -> Vec<PointId> {
        match *self {
            EdgeGeom::Line { a, b } => vec![a, b],
            EdgeGeom::Arc { center, start, end } => vec![center, start, end],
        }
    }
}

/// A line or arc of the sketch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Edge {
    pub id: EdgeId,
    pub geom: EdgeGeom,
    /// Construction geometry (centre lines, helper lines) takes part in constraints but
    /// not in closed-loop detection.
    #[serde(default, skip_serializing_if = "is_false")]
    pub construction: bool,
    /// Opaque tag owned by the caller (StageCraft stores wall/floor properties under it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}
fn is_true(b: &bool) -> bool {
    *b
}
fn default_true() -> bool {
    true
}

/// A geometric (valueless) constraint, except `Fixed`, which stores where it pins a point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConstraintKind {
    /// Pin a point to a position.
    Fixed { point: PointId, at: Vec2 },
    /// Two separate points coincide.
    Coincident { a: PointId, b: PointId },
    /// `a` and `b` have the same y (a horizontal line between them).
    Horizontal { a: PointId, b: PointId },
    /// `a` and `b` have the same x.
    Vertical { a: PointId, b: PointId },
    /// Two lines are parallel.
    Parallel { a: EdgeId, b: EdgeId },
    /// Two lines are perpendicular.
    Perpendicular { a: EdgeId, b: EdgeId },
    /// Two lines have the same length.
    EqualLength { a: EdgeId, b: EdgeId },
    /// `point` is the midpoint of `line`.
    Midpoint { point: PointId, line: EdgeId },
    /// `point` lies on the (infinite) line through `line`.
    PointOnLine { point: PointId, line: EdgeId },
    /// `point` lies on the circle of `arc`.
    PointOnArc { point: PointId, arc: EdgeId },
    /// `line` and `arc` share an end point and are tangent there.
    Tangent { line: EdgeId, arc: EdgeId },
    /// `a` and `b` are mirror images across the line through `axis`.
    Symmetric { a: PointId, b: PointId, axis: EdgeId },
}

/// A constraint with its stable id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Constraint {
    pub id: ConstraintId,
    pub kind: ConstraintKind,
}

/// What a dimension measures.
///
/// Values are magnitudes (non-negative). For horizontal/vertical distances, point-line
/// distances and angles the direction is taken from the geometry as it is when solving,
/// so a dimension never flips its geometry over.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Measure {
    /// Straight distance between two points.
    Distance { a: PointId, b: PointId },
    /// Length of a line.
    Length { line: EdgeId },
    /// |b.x − a.x|.
    HorizontalDistance { a: PointId, b: PointId },
    /// |b.y − a.y|.
    VerticalDistance { a: PointId, b: PointId },
    /// Perpendicular distance from a point to the line through `line`.
    PointLineDistance { point: PointId, line: EdgeId },
    /// Angle between the directions of two lines, 0–180°.
    Angle { a: EdgeId, b: EdgeId },
    /// Radius of an arc.
    Radius { arc: EdgeId },
}

impl Measure {
    /// True for angle measures (value in degrees), false for lengths (mm).
    pub fn is_angle(&self) -> bool {
        matches!(self, Measure::Angle { .. })
    }
}

/// A dimension. Driving dimensions constrain the geometry; reference dimensions only
/// measure it and get their `value` updated after every solve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Dimension {
    pub id: ConstraintId,
    pub measure: Measure,
    /// mm for lengths, degrees for angles.
    pub value: f64,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub driving: bool,
    /// Where the label sits, as an offset in mm from the midpoint of the measured
    /// geometry, so it follows the geometry.
    #[serde(default)]
    pub label_offset: Vec2,
    /// Optional parameter name ("portal width").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A parametric 2D sketch.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Sketch {
    #[serde(default)]
    pub points: Vec<Point>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    #[serde(default)]
    pub dimensions: Vec<Dimension>,
    /// Next free id; shared by points, edges, constraints and dimensions.
    #[serde(default)]
    pub next_id: u32,
}

/// Things an edit can refer to that may be missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum ItemRef {
    Point(PointId),
    Edge(EdgeId),
    Constraint(ConstraintId),
}

impl std::fmt::Display for ItemRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ItemRef::Point(id) => write!(f, "point {id}"),
            ItemRef::Edge(id) => write!(f, "edge {id}"),
            ItemRef::Constraint(id) => write!(f, "constraint {id}"),
        }
    }
}

impl Sketch {
    pub fn new() -> Self {
        Self::default()
    }

    fn alloc(&mut self) -> u32 {
        // Ids are never reused; tolerate hand-written JSON with a stale counter.
        let max = self
            .points
            .iter()
            .map(|p| p.id.0)
            .chain(self.edges.iter().map(|e| e.id.0))
            .chain(self.constraints.iter().map(|c| c.id.0))
            .chain(self.dimensions.iter().map(|d| d.id.0))
            .max();
        if let Some(max) = max {
            self.next_id = self.next_id.max(max + 1);
        }
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    // ---- lookup -------------------------------------------------------------------

    pub fn point(&self, id: PointId) -> Option<&Point> {
        self.points.iter().find(|p| p.id == id)
    }
    pub fn point_mut(&mut self, id: PointId) -> Option<&mut Point> {
        self.points.iter_mut().find(|p| p.id == id)
    }
    /// Position of a point. Panics on an unknown id; use [`Sketch::point`] to check.
    pub fn pos(&self, id: PointId) -> Vec2 {
        self.point(id).unwrap_or_else(|| panic!("unknown point {id}")).pos
    }
    pub fn edge(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.iter().find(|e| e.id == id)
    }
    pub fn edge_mut(&mut self, id: EdgeId) -> Option<&mut Edge> {
        self.edges.iter_mut().find(|e| e.id == id)
    }
    pub fn constraint(&self, id: ConstraintId) -> Option<&Constraint> {
        self.constraints.iter().find(|c| c.id == id)
    }
    pub fn dimension(&self, id: ConstraintId) -> Option<&Dimension> {
        self.dimensions.iter().find(|d| d.id == id)
    }
    pub fn dimension_mut(&mut self, id: ConstraintId) -> Option<&mut Dimension> {
        self.dimensions.iter_mut().find(|d| d.id == id)
    }
    /// Dimension by parameter name.
    pub fn dimension_named(&self, name: &str) -> Option<&Dimension> {
        self.dimensions.iter().find(|d| d.name.as_deref() == Some(name))
    }

    /// End points of a line edge, `None` if the id is unknown or the edge is an arc.
    pub fn line_ends(&self, id: EdgeId) -> Option<(PointId, PointId)> {
        match self.edge(id)?.geom {
            EdgeGeom::Line { a, b } => Some((a, b)),
            EdgeGeom::Arc { .. } => None,
        }
    }

    // ---- building (no solving) ------------------------------------------------------

    pub fn add_point(&mut self, pos: Vec2) -> PointId {
        let id = PointId(self.alloc());
        self.points.push(Point { id, pos });
        id
    }

    pub fn add_line(&mut self, a: PointId, b: PointId) -> EdgeId {
        self.add_edge(EdgeGeom::Line { a, b })
    }

    pub fn add_arc(&mut self, center: PointId, start: PointId, end: PointId) -> EdgeId {
        self.add_edge(EdgeGeom::Arc { center, start, end })
    }

    pub fn add_edge(&mut self, geom: EdgeGeom) -> EdgeId {
        let id = EdgeId(self.alloc());
        self.edges.push(Edge { id, geom, construction: false, tag: None });
        id
    }

    /// Adds points and lines for a polygon (closed) or polyline (open). Returns the
    /// point ids and edge ids in order.
    pub fn add_polyline(&mut self, pts: &[Vec2], closed: bool) -> (Vec<PointId>, Vec<EdgeId>) {
        let ids: Vec<PointId> = pts.iter().map(|&p| self.add_point(p)).collect();
        let mut edges = Vec::new();
        for w in ids.windows(2) {
            edges.push(self.add_line(w[0], w[1]));
        }
        if closed && ids.len() > 2 {
            edges.push(self.add_line(ids[ids.len() - 1], ids[0]));
        }
        (ids, edges)
    }

    pub fn add_constraint(&mut self, kind: ConstraintKind) -> ConstraintId {
        let id = ConstraintId(self.alloc());
        self.constraints.push(Constraint { id, kind });
        id
    }

    /// Adds a driving dimension with the given value.
    pub fn add_dimension(&mut self, measure: Measure, value: f64) -> ConstraintId {
        self.add_dimension_full(measure, Some(value), true, None)
    }

    /// Adds a dimension. `value: None` takes the currently measured value.
    pub fn add_dimension_full(
        &mut self,
        measure: Measure,
        value: Option<f64>,
        driving: bool,
        name: Option<String>,
    ) -> ConstraintId {
        let value = value.unwrap_or_else(|| self.measure(&measure).unwrap_or(0.0));
        let id = ConstraintId(self.alloc());
        self.dimensions.push(Dimension { id, measure, value, driving, label_offset: Vec2::ZERO, name });
        id
    }

    /// Adds a named driving dimension (a parameter such as "portal width").
    pub fn add_named_dimension(&mut self, name: &str, measure: Measure, value: f64) -> ConstraintId {
        self.add_dimension_full(measure, Some(value), true, Some(name.to_owned()))
    }

    /// Adds a reference dimension (measures only).
    pub fn add_reference(&mut self, measure: Measure) -> ConstraintId {
        self.add_dimension_full(measure, None, false, None)
    }

    /// Fixes a point where it currently is.
    pub fn fix(&mut self, point: PointId) -> ConstraintId {
        let at = self.pos(point);
        self.add_constraint(ConstraintKind::Fixed { point, at })
    }

    /// Removes a constraint or dimension. Returns false if the id was unknown.
    pub fn remove_constraint(&mut self, id: ConstraintId) -> bool {
        let before = self.constraints.len() + self.dimensions.len();
        self.constraints.retain(|c| c.id != id);
        self.dimensions.retain(|d| d.id != id);
        before != self.constraints.len() + self.dimensions.len()
    }

    /// Removes an edge and every constraint/dimension referring to it.
    pub fn remove_edge(&mut self, id: EdgeId) -> bool {
        let before = self.edges.len();
        self.edges.retain(|e| e.id != id);
        if before == self.edges.len() {
            return false;
        }
        self.constraints.retain(|c| !constraint_edges(&c.kind).contains(&id));
        self.dimensions.retain(|d| !measure_edges(&d.measure).contains(&id));
        true
    }

    /// Removes a point, every edge using it and every constraint/dimension referring to
    /// either.
    pub fn remove_point(&mut self, id: PointId) -> bool {
        if self.point(id).is_none() {
            return false;
        }
        let dead: Vec<EdgeId> = self.edges.iter().filter(|e| e.geom.points().contains(&id)).map(|e| e.id).collect();
        for e in dead {
            self.remove_edge(e);
        }
        self.points.retain(|p| p.id != id);
        self.constraints.retain(|c| !constraint_points(&c.kind).contains(&id));
        self.dimensions.retain(|d| !measure_points(&d.measure).contains(&id));
        true
    }

    // ---- measuring ------------------------------------------------------------------

    /// Current value of a measure (mm or degrees, non-negative), `None` if it refers to
    /// missing or wrong-kind geometry.
    pub fn measure(&self, m: &Measure) -> Option<f64> {
        let p = |id: PointId| self.point(id).map(|p| p.pos);
        Some(match *m {
            Measure::Distance { a, b } => p(a)?.distance(p(b)?),
            Measure::Length { line } => {
                let (a, b) = self.line_ends(line)?;
                p(a)?.distance(p(b)?)
            }
            Measure::HorizontalDistance { a, b } => (p(b)?.x - p(a)?.x).abs(),
            Measure::VerticalDistance { a, b } => (p(b)?.y - p(a)?.y).abs(),
            Measure::PointLineDistance { point, line } => {
                let (a, b) = self.line_ends(line)?;
                let (a, b) = (p(a)?, p(b)?);
                let d = b - a;
                if d.length() == 0.0 {
                    return None;
                }
                (d.cross(p(point)? - a) / d.length()).abs()
            }
            Measure::Angle { a, b } => {
                let (a0, a1) = self.line_ends(a)?;
                let (b0, b1) = self.line_ends(b)?;
                let u = p(a1)? - p(a0)?;
                let v = p(b1)? - p(b0)?;
                u.cross(v).atan2(u.dot(v)).to_degrees().abs()
            }
            Measure::Radius { arc } => match self.edge(arc)?.geom {
                EdgeGeom::Arc { center, start, .. } => p(center)?.distance(p(start)?),
                EdgeGeom::Line { .. } => return None,
            },
        })
    }

    /// Midpoint of the measured geometry; label positions are relative to it.
    pub fn measure_anchor(&self, m: &Measure) -> Option<Vec2> {
        let pts = measure_points_resolved(self, m)?;
        let n = pts.len() as f64;
        let sum = pts.iter().fold(Vec2::ZERO, |acc, &p| acc + p);
        Some(sum * (1.0 / n))
    }
}

fn measure_points_resolved(s: &Sketch, m: &Measure) -> Option<Vec<Vec2>> {
    let mut ids = measure_points(m);
    for e in measure_edges(m) {
        ids.extend(s.edge(e)?.geom.points());
    }
    ids.iter().map(|&id| s.point(id).map(|p| p.pos)).collect()
}

/// Points a constraint refers to directly (not via edges).
pub fn constraint_points(k: &ConstraintKind) -> Vec<PointId> {
    use ConstraintKind::*;
    match *k {
        Fixed { point, .. } => vec![point],
        Coincident { a, b } | Horizontal { a, b } | Vertical { a, b } => vec![a, b],
        Midpoint { point, .. } | PointOnLine { point, .. } | PointOnArc { point, .. } => {
            vec![point]
        }
        Symmetric { a, b, .. } => vec![a, b],
        Parallel { .. } | Perpendicular { .. } | EqualLength { .. } | Tangent { .. } => vec![],
    }
}

/// Edges a constraint refers to.
pub fn constraint_edges(k: &ConstraintKind) -> Vec<EdgeId> {
    use ConstraintKind::*;
    match *k {
        Parallel { a, b } | Perpendicular { a, b } | EqualLength { a, b } => vec![a, b],
        Midpoint { line, .. } | PointOnLine { line, .. } => vec![line],
        PointOnArc { arc, .. } => vec![arc],
        Tangent { line, arc } => vec![line, arc],
        Symmetric { axis, .. } => vec![axis],
        Fixed { .. } | Coincident { .. } | Horizontal { .. } | Vertical { .. } => vec![],
    }
}

/// Points a measure refers to directly.
pub fn measure_points(m: &Measure) -> Vec<PointId> {
    match *m {
        Measure::Distance { a, b } | Measure::HorizontalDistance { a, b } | Measure::VerticalDistance { a, b } => {
            vec![a, b]
        }
        Measure::PointLineDistance { point, .. } => vec![point],
        Measure::Length { .. } | Measure::Angle { .. } | Measure::Radius { .. } => vec![],
    }
}

/// Edges a measure refers to.
pub fn measure_edges(m: &Measure) -> Vec<EdgeId> {
    match *m {
        Measure::Length { line } => vec![line],
        Measure::PointLineDistance { line, .. } => vec![line],
        Measure::Angle { a, b } => vec![a, b],
        Measure::Radius { arc } => vec![arc],
        Measure::Distance { .. } | Measure::HorizontalDistance { .. } | Measure::VerticalDistance { .. } => vec![],
    }
}
