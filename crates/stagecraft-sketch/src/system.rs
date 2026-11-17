//! Translating a sketch into equations: our own residuals (for satisfaction checks and
//! the Jacobian analysis) and the matching ezpz constraint requests (for solving).

use std::collections::HashMap;
use std::ops::{Add, Div, Mul, Neg, Sub};

use ezpz::datatypes::inputs::{DatumCircularArc, DatumLineSegment, DatumPoint};
use ezpz::datatypes::{Angle, AngleKind};
use ezpz::{Constraint as EzConstraint, ConstraintRequest};

use crate::error::SolveError;
use crate::model::{ConstraintId, ConstraintKind, EdgeGeom, EdgeId, ItemRef, Measure, PointId, Sketch, Vec2};

/// Who an equation belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    Constraint(ConstraintId),
    /// Implicit equation of an arc edge (start and end on one circle).
    Edge(EdgeId),
}

/// One scalar equation `residual(points) = 0`. Point slots `p0..p3` index into the
/// system's point table.
#[derive(Clone, Copy, Debug)]
pub(crate) enum EqKind {
    /// p0.x = v
    FixX(f64),
    /// p0.y = v
    FixY(f64),
    /// p0.x = p1.x
    SameX,
    /// p0.y = p1.y
    SameY,
    /// |p1 − p0| = d
    Dist(f64),
    /// p1.x − p0.x = d
    Dx(f64),
    /// p1.y − p0.y = d
    Dy(f64),
    /// direction (p3 − p2) is direction (p1 − p0) rotated by θ (mod 180°)
    Angle {
        theta: f64,
    },
    /// |p1 − p0| = |p3 − p2|
    EqualLen,
    /// p0.x = (p1.x + p2.x) / 2
    MidX,
    /// p0.y = (p1.y + p2.y) / 2
    MidY,
    /// signed distance of p0 from line p1→p2 (left positive) = d
    PointLine(f64),
    /// |p1 − p0| = |p2 − p0|
    EqualDist,
    /// mirror of p0 across line p2→p3 equals p1 (x and y component)
    SymX,
    SymY,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Eq {
    pub owner: Owner,
    pub kind: EqKind,
    pub pts: [usize; 4],
}

impl Eq {
    pub fn arity(&self) -> usize {
        use EqKind::*;
        match self.kind {
            FixX(_) | FixY(_) => 1,
            SameX | SameY | Dist(_) | Dx(_) | Dy(_) => 2,
            MidX | MidY | PointLine(_) | EqualDist => 3,
            Angle { .. } | EqualLen | SymX | SymY => 4,
        }
    }
}

/// A driving dimension's equation, with the value it starts from (current geometry) and
/// the value it must reach. Used to walk large changes in steps.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DimRamp {
    pub eq: usize,
    pub start: f64,
    pub target: f64,
}

/// The equation system of a sketch.
pub(crate) struct System {
    /// Point ids in system order; variable `2i` is x, `2i + 1` is y of `points[i]`.
    pub points: Vec<PointId>,
    pub index: HashMap<PointId, usize>,
    pub eqs: Vec<Eq>,
    pub ramps: Vec<DimRamp>,
}

impl System {
    pub fn num_vars(&self) -> usize {
        self.points.len() * 2
    }

    /// Initial values from the sketch's current positions.
    pub fn values(&self, sketch_pos: &HashMap<PointId, Vec2>) -> Vec<f64> {
        let mut v = Vec::with_capacity(self.num_vars());
        for id in &self.points {
            let p = sketch_pos[id];
            v.push(p.x);
            v.push(p.y);
        }
        v
    }

    /// Sets every ramped dimension to `start + (target − start)·t`.
    pub fn set_ramp(&mut self, t: f64) {
        for r in &self.ramps {
            let v = r.start + (r.target - r.start) * t;
            match &mut self.eqs[r.eq].kind {
                EqKind::Dist(d) | EqKind::Dx(d) | EqKind::Dy(d) | EqKind::PointLine(d) => *d = v,
                EqKind::Angle { theta } => *theta = v,
                _ => unreachable!("ramp on a valueless equation"),
            }
        }
    }

    /// ezpz requests for the current equations plus soft drag targets.
    pub fn requests(&self, drags: &[(usize, Vec2)], drag_weight: f64) -> Vec<ConstraintRequest> {
        let dp = |i: usize| DatumPoint::new_xy((2 * i) as u32, (2 * i + 1) as u32);
        let line = |a: usize, b: usize| DatumLineSegment::new(dp(a), dp(b));
        let mut out = Vec::with_capacity(self.eqs.len() + drags.len() * 2);
        for eq in &self.eqs {
            let [p0, p1, p2, p3] = eq.pts;
            let c = match eq.kind {
                EqKind::FixX(v) => EzConstraint::Fixed(dp(p0).id_x(), v),
                EqKind::FixY(v) => EzConstraint::Fixed(dp(p0).id_y(), v),
                EqKind::SameX => EzConstraint::Vertical(line(p0, p1)),
                EqKind::SameY => EzConstraint::Horizontal(line(p0, p1)),
                EqKind::Dist(d) => EzConstraint::Distance(dp(p0), dp(p1), d),
                // ezpz: p.x − q.x = d
                EqKind::Dx(d) => EzConstraint::HorizontalDistance(dp(p1), dp(p0), d),
                EqKind::Dy(d) => EzConstraint::VerticalDistance(dp(p1), dp(p0), d),
                EqKind::Angle { theta } => {
                    let kind = if theta == 0.0 {
                        AngleKind::Parallel
                    } else if theta == std::f64::consts::FRAC_PI_2 {
                        AngleKind::Perpendicular
                    } else {
                        AngleKind::Other(Angle::from_radians(theta))
                    };
                    EzConstraint::LinesAtAngle(line(p0, p1), line(p2, p3), kind)
                }
                EqKind::EqualLen => EzConstraint::LinesEqualLength(line(p0, p1), line(p2, p3)),
                EqKind::MidX => EzConstraint::Midpoint(line(p1, p2), dp(p0)),
                EqKind::MidY => continue, // MidX's ezpz request covers both rows
                EqKind::PointLine(d) => EzConstraint::PointLineDistance(dp(p0), line(p1, p2), d),
                EqKind::EqualDist => EzConstraint::Arc(DatumCircularArc { center: dp(p0), start: dp(p1), end: dp(p2) }),
                EqKind::SymX => EzConstraint::Symmetric(line(p2, p3), dp(p0), dp(p1)),
                EqKind::SymY => continue,
            };
            out.push(ConstraintRequest::highest_priority(c));
        }
        for &(i, to) in drags {
            out.push(
                ConstraintRequest::highest_priority(EzConstraint::Fixed(dp(i).id_x(), to.x)).with_weight(drag_weight),
            );
            out.push(
                ConstraintRequest::highest_priority(EzConstraint::Fixed(dp(i).id_y(), to.y)).with_weight(drag_weight),
            );
        }
        out
    }

    /// Residual of one equation for the variable vector `vals`.
    pub fn residual(&self, eq: &Eq, vals: &[f64]) -> f64 {
        let mut p = [[0.0; 2]; 4];
        for (k, slot) in p.iter_mut().enumerate().take(eq.arity()) {
            let i = eq.pts[k];
            *slot = [vals[2 * i], vals[2 * i + 1]];
        }
        eval(eq.kind, &p)
    }

    /// Residual and gradient (with respect to the equation's own point slots).
    pub fn residual_grad(&self, eq: &Eq, vals: &[f64]) -> (f64, [f64; 8]) {
        let mut p = [[Dual::c(0.0); 2]; 4];
        for (k, slot) in p.iter_mut().enumerate().take(eq.arity()) {
            let i = eq.pts[k];
            slot[0] = Dual::var(vals[2 * i], 2 * k);
            slot[1] = Dual::var(vals[2 * i + 1], 2 * k + 1);
        }
        let r = eval(eq.kind, &p);
        (r.v, r.g)
    }
}

// ---- residual functions, generic over f64 and a forward-mode dual number -------------

pub(crate) trait Num:
    Copy + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self> + Div<Output = Self> + Neg<Output = Self>
{
    fn c(v: f64) -> Self;
    fn sqrt(self) -> Self;
}

impl Num for f64 {
    fn c(v: f64) -> Self {
        v
    }
    fn sqrt(self) -> Self {
        f64::sqrt(self)
    }
}

/// Value plus gradient over the (at most) 8 coordinates of an equation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Dual {
    v: f64,
    g: [f64; 8],
}

impl Dual {
    fn var(v: f64, k: usize) -> Self {
        let mut g = [0.0; 8];
        g[k] = 1.0;
        Self { v, g }
    }
    fn map(self, v: f64, d: f64) -> Self {
        let mut g = self.g;
        for x in &mut g {
            *x *= d;
        }
        Self { v, g }
    }
}

impl Add for Dual {
    type Output = Dual;
    fn add(self, o: Dual) -> Dual {
        let mut g = self.g;
        for (a, b) in g.iter_mut().zip(o.g) {
            *a += b;
        }
        Dual { v: self.v + o.v, g }
    }
}
impl Sub for Dual {
    type Output = Dual;
    fn sub(self, o: Dual) -> Dual {
        self + (-o)
    }
}
impl Neg for Dual {
    type Output = Dual;
    fn neg(self) -> Dual {
        self.map(-self.v, -1.0)
    }
}
impl Mul for Dual {
    type Output = Dual;
    fn mul(self, o: Dual) -> Dual {
        let mut g = [0.0; 8];
        for (k, x) in g.iter_mut().enumerate() {
            *x = self.g[k] * o.v + o.g[k] * self.v;
        }
        Dual { v: self.v * o.v, g }
    }
}
impl Div for Dual {
    type Output = Dual;
    fn div(self, o: Dual) -> Dual {
        let mut g = [0.0; 8];
        let inv = 1.0 / o.v;
        for (k, x) in g.iter_mut().enumerate() {
            *x = (self.g[k] * o.v - o.g[k] * self.v) * inv * inv;
        }
        Dual { v: self.v * inv, g }
    }
}
impl Num for Dual {
    fn c(v: f64) -> Self {
        Dual { v, g: [0.0; 8] }
    }
    fn sqrt(self) -> Self {
        let s = self.v.sqrt();
        // d sqrt(x) = 1 / (2 sqrt(x)); guard the (degenerate) zero case.
        let d = if s > 0.0 { 0.5 / s } else { 0.0 };
        self.map(s, d)
    }
}

fn len<T: Num>(dx: T, dy: T) -> T {
    (dx * dx + dy * dy).sqrt()
}

pub(crate) fn eval<T: Num>(kind: EqKind, p: &[[T; 2]; 4]) -> T {
    let [p0, p1, p2, p3] = *p;
    let half = T::c(0.5);
    match kind {
        EqKind::FixX(v) => p0[0] - T::c(v),
        EqKind::FixY(v) => p0[1] - T::c(v),
        EqKind::SameX => p0[0] - p1[0],
        EqKind::SameY => p0[1] - p1[1],
        EqKind::Dist(d) => len(p1[0] - p0[0], p1[1] - p0[1]) - T::c(d),
        EqKind::Dx(d) => p1[0] - p0[0] - T::c(d),
        EqKind::Dy(d) => p1[1] - p0[1] - T::c(d),
        EqKind::Angle { theta } => {
            let (s, c) = theta.sin_cos();
            let (ux, uy) = (p1[0] - p0[0], p1[1] - p0[1]);
            let (vx, vy) = (p3[0] - p2[0], p3[1] - p2[1]);
            // v rotated back by θ must be parallel to u
            let rx = T::c(c) * vx + T::c(s) * vy;
            let ry = T::c(-s) * vx + T::c(c) * vy;
            let cross = ux * ry - uy * rx;
            cross / ((len(ux, uy) + len(vx, vy)) * half)
        }
        EqKind::EqualLen => len(p1[0] - p0[0], p1[1] - p0[1]) - len(p3[0] - p2[0], p3[1] - p2[1]),
        EqKind::MidX => p0[0] - (p1[0] + p2[0]) * half,
        EqKind::MidY => p0[1] - (p1[1] + p2[1]) * half,
        EqKind::PointLine(d) => {
            let (dx, dy) = (p2[0] - p1[0], p2[1] - p1[1]);
            let (wx, wy) = (p0[0] - p1[0], p0[1] - p1[1]);
            (dx * wy - dy * wx) / len(dx, dy) - T::c(d)
        }
        EqKind::EqualDist => len(p1[0] - p0[0], p1[1] - p0[1]) - len(p2[0] - p0[0], p2[1] - p0[1]),
        EqKind::SymX | EqKind::SymY => {
            let (dx, dy) = (p3[0] - p2[0], p3[1] - p2[1]);
            let l2 = dx * dx + dy * dy;
            let (wx, wy) = (p0[0] - p2[0], p0[1] - p2[1]);
            let t = (wx * dx + wy * dy) / l2;
            // mirror = p2 + 2·proj − w
            let mx = p2[0] + T::c(2.0) * t * dx - wx;
            let my = p2[1] + T::c(2.0) * t * dy - wy;
            if matches!(kind, EqKind::SymX) { mx - p1[0] } else { my - p1[1] }
        }
    }
}

// ---- building ------------------------------------------------------------------------

struct Builder<'a> {
    sketch: &'a Sketch,
    pos: &'a HashMap<PointId, Vec2>,
    sys: System,
}

impl Builder<'_> {
    fn pt(&mut self, id: PointId) -> Result<usize, SolveError> {
        if let Some(&i) = self.sys.index.get(&id) {
            return Ok(i);
        }
        if !self.pos.contains_key(&id) {
            return Err(SolveError::Missing(ItemRef::Point(id)));
        }
        let i = self.sys.points.len();
        self.sys.points.push(id);
        self.sys.index.insert(id, i);
        Ok(i)
    }

    fn line(&mut self, owner: ConstraintId, id: EdgeId) -> Result<(PointId, PointId), SolveError> {
        match self.sketch.edge(id).map(|e| e.geom) {
            None => Err(SolveError::Missing(ItemRef::Edge(id))),
            Some(EdgeGeom::Line { a, b }) => Ok((a, b)),
            Some(EdgeGeom::Arc { .. }) => {
                Err(SolveError::WrongEdgeKind { constraint: owner, edge: id, expected: "line" })
            }
        }
    }

    fn arc(&mut self, owner: ConstraintId, id: EdgeId) -> Result<(PointId, PointId, PointId), SolveError> {
        match self.sketch.edge(id).map(|e| e.geom) {
            None => Err(SolveError::Missing(ItemRef::Edge(id))),
            Some(EdgeGeom::Arc { center, start, end }) => Ok((center, start, end)),
            Some(EdgeGeom::Line { .. }) => {
                Err(SolveError::WrongEdgeKind { constraint: owner, edge: id, expected: "arc" })
            }
        }
    }

    fn push(&mut self, owner: Owner, kind: EqKind, ids: &[PointId]) -> Result<usize, SolveError> {
        let mut pts = [0usize; 4];
        for (k, &id) in ids.iter().enumerate() {
            pts[k] = self.pt(id)?;
        }
        self.sys.eqs.push(Eq { owner, kind, pts });
        Ok(self.sys.eqs.len() - 1)
    }

    fn p(&self, id: PointId) -> Vec2 {
        self.pos[&id]
    }
}

fn sign(v: f64) -> f64 {
    if v < 0.0 { -1.0 } else { 1.0 }
}

/// Wraps an angle to (−π/2, π/2]: line directions are equal modulo 180°.
fn wrap_half(a: f64) -> f64 {
    use std::f64::consts::PI;
    let mut a = a.rem_euclid(PI);
    if a > PI / 2.0 {
        a -= PI;
    }
    a
}

/// Builds the equation system. Driving dimensions get their direction from the
/// current geometry (`pos`).
pub(crate) fn build(
    sketch: &Sketch,
    pos: &HashMap<PointId, Vec2>,
    extra_points: &[PointId],
) -> Result<System, SolveError> {
    let mut b = Builder {
        sketch,
        pos,
        sys: System { points: Vec::new(), index: HashMap::new(), eqs: Vec::new(), ramps: Vec::new() },
    };

    for e in &sketch.edges {
        for id in e.geom.points() {
            if !pos.contains_key(&id) {
                return Err(SolveError::Missing(ItemRef::Point(id)));
            }
        }
        if let EdgeGeom::Arc { center, start, end } = e.geom {
            b.push(Owner::Edge(e.id), EqKind::EqualDist, &[center, start, end])?;
        }
    }

    for c in &sketch.constraints {
        let o = Owner::Constraint(c.id);
        use ConstraintKind::*;
        match c.kind {
            Fixed { point, at } => {
                b.push(o, EqKind::FixX(at.x), &[point])?;
                b.push(o, EqKind::FixY(at.y), &[point])?;
            }
            Coincident { a, b: q } => {
                b.push(o, EqKind::SameX, &[a, q])?;
                b.push(o, EqKind::SameY, &[a, q])?;
            }
            Horizontal { a, b: q } => {
                b.push(o, EqKind::SameY, &[a, q])?;
            }
            Vertical { a, b: q } => {
                b.push(o, EqKind::SameX, &[a, q])?;
            }
            Parallel { a, b: l } | Perpendicular { a, b: l } => {
                let (a0, a1) = b.line(c.id, a)?;
                let (b0, b1) = b.line(c.id, l)?;
                let theta = if matches!(c.kind, Parallel { .. }) { 0.0 } else { std::f64::consts::FRAC_PI_2 };
                b.push(o, EqKind::Angle { theta }, &[a0, a1, b0, b1])?;
            }
            EqualLength { a, b: l } => {
                let (a0, a1) = b.line(c.id, a)?;
                let (b0, b1) = b.line(c.id, l)?;
                b.push(o, EqKind::EqualLen, &[a0, a1, b0, b1])?;
            }
            Midpoint { point, line } => {
                let (l0, l1) = b.line(c.id, line)?;
                b.push(o, EqKind::MidX, &[point, l0, l1])?;
                b.push(o, EqKind::MidY, &[point, l0, l1])?;
            }
            PointOnLine { point, line } => {
                let (l0, l1) = b.line(c.id, line)?;
                b.push(o, EqKind::PointLine(0.0), &[point, l0, l1])?;
            }
            PointOnArc { point, arc } => {
                let (center, start, _) = b.arc(c.id, arc)?;
                b.push(o, EqKind::EqualDist, &[center, start, point])?;
            }
            Tangent { line, arc } => {
                let (l0, l1) = b.line(c.id, line)?;
                let (center, start, end) = b.arc(c.id, arc)?;
                let shared = [start, end].into_iter().find(|p| *p == l0 || *p == l1);
                let Some(shared) = shared else {
                    return Err(SolveError::NotTouching { constraint: c.id });
                };
                let theta = std::f64::consts::FRAC_PI_2;
                b.push(o, EqKind::Angle { theta }, &[l0, l1, center, shared])?;
            }
            Symmetric { a, b: q, axis } => {
                let (l0, l1) = b.line(c.id, axis)?;
                b.push(o, EqKind::SymX, &[a, q, l0, l1])?;
                b.push(o, EqKind::SymY, &[a, q, l0, l1])?;
            }
        }
    }

    for d in sketch.dimensions.iter().filter(|d| d.driving) {
        let o = Owner::Constraint(d.id);
        let v = d.value;
        if !v.is_finite() || v < 0.0 {
            return Err(SolveError::BadValue { dimension: d.id, value: v });
        }
        let (eq, start, target) = match d.measure {
            Measure::Distance { a, b: q } => {
                let cur = b.p(a).distance(b.p(q));
                (b.push(o, EqKind::Dist(v), &[a, q])?, cur, v)
            }
            Measure::Length { line } => {
                let (a, q) = b.line(d.id, line)?;
                let cur = b.p(a).distance(b.p(q));
                (b.push(o, EqKind::Dist(v), &[a, q])?, cur, v)
            }
            Measure::Radius { arc } => {
                let (center, start, _) = b.arc(d.id, arc)?;
                let cur = b.p(center).distance(b.p(start));
                (b.push(o, EqKind::Dist(v), &[center, start])?, cur, v)
            }
            Measure::HorizontalDistance { a, b: q } => {
                b.pt(a)?;
                b.pt(q)?;
                let cur = b.p(q).x - b.p(a).x;
                let t = sign(cur) * v;
                (b.push(o, EqKind::Dx(t), &[a, q])?, cur, t)
            }
            Measure::VerticalDistance { a, b: q } => {
                b.pt(a)?;
                b.pt(q)?;
                let cur = b.p(q).y - b.p(a).y;
                let t = sign(cur) * v;
                (b.push(o, EqKind::Dy(t), &[a, q])?, cur, t)
            }
            Measure::PointLineDistance { point, line } => {
                let (l0, l1) = b.line(d.id, line)?;
                b.pt(point)?;
                let (p0, p1) = (b.p(l0), b.p(l1));
                let dir = p1 - p0;
                let cur = if dir.length() > 0.0 { dir.cross(b.p(point) - p0) / dir.length() } else { 0.0 };
                let t = sign(cur) * v;
                (b.push(o, EqKind::PointLine(t), &[point, l0, l1])?, cur, t)
            }
            Measure::Angle { a, b: l } => {
                let (a0, a1) = b.line(d.id, a)?;
                let (b0, b1) = b.line(d.id, l)?;
                let u = b.p(a1) - b.p(a0);
                let w = b.p(b1) - b.p(b0);
                let cur = u.cross(w).atan2(u.dot(w));
                let vr = v.to_radians();
                // θ is only defined modulo 180°: pick ±v closest to the current angle,
                // and start the ramp from the current angle unwrapped next to it.
                let cands = [vr, -vr];
                let t = cands
                    .into_iter()
                    .min_by(|x, y| wrap_half(x - cur).abs().total_cmp(&wrap_half(y - cur).abs()))
                    .unwrap_or(vr);
                let start = t + wrap_half(cur - t);
                (b.push(o, EqKind::Angle { theta: t }, &[a0, a1, b0, b1])?, start, t)
            }
        };
        b.sys.ramps.push(DimRamp { eq, start, target });
    }

    for &id in extra_points {
        b.pt(id)?;
    }
    Ok(b.sys)
}
