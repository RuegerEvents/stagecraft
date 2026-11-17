//! Solving a sketch after an edit.

use std::collections::{BTreeSet, HashMap};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::analysis::analyse;
use crate::error::SolveError;
use crate::model::{
    ConstraintId, ConstraintKind, EdgeGeom, EdgeId, ItemRef, Measure, PointId, Sketch, Vec2, constraint_edges,
    constraint_points, measure_edges, measure_points,
};
use crate::system::{Owner, System, build};

/// A residual below this (mm, or mm-equivalent for angles) counts as satisfied.
pub const SATISFIED_TOL: f64 = 1e-6;

/// Tuning for a solve.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SolveOptions {
    /// Run the DOF / redundancy analysis (dense QR, a few ms on a room). Drag steps can
    /// switch it off and analyse once on release.
    pub analyse: bool,
    /// Levenberg–Marquardt iteration limit per ezpz call.
    pub max_iterations: usize,
    /// Weight of the soft drag target relative to the hard constraints.
    pub drag_weight: f64,
}

impl Default for SolveOptions {
    fn default() -> Self {
        Self { analyse: true, max_iterations: 100, drag_weight: 1.0 }
    }
}

impl SolveOptions {
    /// Options for interactive drag steps: no analysis.
    pub fn drag() -> Self {
        Self { analyse: false, ..Self::default() }
    }
}

/// A point the user drags, soft-fixed to the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DragTarget {
    pub point: PointId,
    pub to: Vec2,
}

/// FreeCAD-style state of a point or edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntityState {
    /// No degrees of freedom left (green).
    FullyConstrained,
    /// Can still move (normal colour).
    UnderConstrained,
}

/// State of a constraint or dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintState {
    Ok,
    /// Satisfied, but implied by other constraints; removing it changes nothing.
    Redundant,
    /// Contradicts other constraints (red).
    Conflicting,
}

/// Degrees of freedom of one point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PointDof {
    pub id: PointId,
    /// 0 = fully constrained, 1 = can slide along a curve, 2 = free.
    pub dof: u8,
}

/// Analysis of a solved (or unsolvable) sketch.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Status {
    /// Degrees of freedom of the whole sketch. 0 = fully constrained.
    pub dof: usize,
    /// Per-point DOF, one entry per sketch point.
    pub points: Vec<PointDof>,
    /// Constraints/dimensions implied by others.
    pub redundant: Vec<ConstraintId>,
    /// Constraints/dimensions that cannot be satisfied together.
    pub conflicting: Vec<ConstraintId>,
}

impl Status {
    pub fn point_dof(&self, id: PointId) -> Option<u8> {
        self.points.iter().find(|p| p.id == id).map(|p| p.dof)
    }

    pub fn point_state(&self, id: PointId) -> EntityState {
        match self.point_dof(id) {
            Some(0) => EntityState::FullyConstrained,
            _ => EntityState::UnderConstrained,
        }
    }

    /// An edge is fully constrained when all its points (an arc's centre included) are.
    pub fn edge_state(&self, sketch: &Sketch, id: EdgeId) -> EntityState {
        match sketch.edge(id) {
            Some(e) if e.geom.points().iter().all(|&p| self.point_state(p) == EntityState::FullyConstrained) => {
                EntityState::FullyConstrained
            }
            _ => EntityState::UnderConstrained,
        }
    }

    pub fn constraint_state(&self, id: ConstraintId) -> ConstraintState {
        if self.conflicting.contains(&id) {
            ConstraintState::Conflicting
        } else if self.redundant.contains(&id) {
            ConstraintState::Redundant
        } else {
            ConstraintState::Ok
        }
    }

    pub fn is_fully_constrained(&self) -> bool {
        self.dof == 0
    }
}

/// Result of a successful solve.
#[derive(Clone, Debug, PartialEq)]
pub struct Solved {
    /// The sketch with solved positions and updated reference dimensions.
    pub sketch: Sketch,
    /// Present when [`SolveOptions::analyse`] was set.
    pub status: Option<Status>,
    /// Whether every drag target was reached exactly (false when the dragged point is
    /// held by constraints; it then moves as close as the constraints allow).
    pub drag_reached: bool,
    /// Total Levenberg–Marquardt iterations.
    pub iterations: usize,
    /// Items created by the edit, in order.
    pub created: Vec<ItemRef>,
}

fn positions(s: &Sketch) -> HashMap<PointId, Vec2> {
    s.points.iter().map(|p| (p.id, p.pos)).collect()
}

struct Run {
    vals: Vec<f64>,
    iterations: usize,
}

fn ezpz_solve(sys: &System, vals: Vec<f64>, drags: &[(usize, Vec2)], opts: &SolveOptions) -> Result<Run, SolveError> {
    let reqs = sys.requests(drags, opts.drag_weight);
    if reqs.is_empty() {
        return Ok(Run { vals, iterations: 0 });
    }
    let guesses: Vec<(u32, f64)> = vals.iter().enumerate().map(|(i, &v)| (i as u32, v)).collect();
    let config = ezpz::Config::default().with_max_iterations(opts.max_iterations);
    match ezpz::solve(&reqs, guesses, config) {
        Ok(out) => Ok(Run { vals: out.final_values().to_vec(), iterations: out.iterations() }),
        Err(e) => Err(SolveError::Solver(e.error.to_string())),
    }
}

fn unsatisfied(sys: &System, vals: &[f64]) -> Vec<usize> {
    sys.eqs
        .iter()
        .enumerate()
        .filter(|(_, eq)| {
            let r = sys.residual(eq, vals);
            r.is_nan() || r.abs() > SATISFIED_TOL
        })
        .map(|(i, _)| i)
        .collect()
}

/// One full attempt: solve with drags, then, if the drag pulled the hard constraints
/// off, project back onto them without the drag.
fn attempt(
    sys: &mut System,
    vals0: &[f64],
    drags: &[(usize, Vec2)],
    steps: usize,
    opts: &SolveOptions,
) -> Result<Run, SolveError> {
    let mut vals = vals0.to_vec();
    let mut iterations = 0;
    for k in 1..=steps {
        sys.set_ramp(k as f64 / steps as f64);
        let run = ezpz_solve(sys, vals, drags, opts)?;
        iterations += run.iterations;
        vals = run.vals;
        if !drags.is_empty() && !unsatisfied(sys, &vals).is_empty() {
            let run = ezpz_solve(sys, vals, &[], opts)?;
            iterations += run.iterations;
            vals = run.vals;
        }
    }
    Ok(Run { vals, iterations })
}

/// Describes a constraint or dimension for error messages.
pub fn describe(sketch: &Sketch, id: ConstraintId) -> String {
    if let Some(d) = sketch.dimension(id) {
        let what = match d.measure {
            Measure::Distance { .. } => "distance",
            Measure::Length { .. } => "length",
            Measure::HorizontalDistance { .. } => "horizontal distance",
            Measure::VerticalDistance { .. } => "vertical distance",
            Measure::PointLineDistance { .. } => "point-line distance",
            Measure::Angle { .. } => "angle",
            Measure::Radius { .. } => "radius",
        };
        let unit = if d.measure.is_angle() { "°" } else { " mm" };
        return match &d.name {
            Some(n) => format!("dimension {id} \"{n}\" ({what} {}{unit})", d.value),
            None => format!("dimension {id} ({what} {}{unit})", d.value),
        };
    }
    if let Some(c) = sketch.constraint(id) {
        let what = match c.kind {
            ConstraintKind::Fixed { .. } => "fixed",
            ConstraintKind::Coincident { .. } => "coincident",
            ConstraintKind::Horizontal { .. } => "horizontal",
            ConstraintKind::Vertical { .. } => "vertical",
            ConstraintKind::Parallel { .. } => "parallel",
            ConstraintKind::Perpendicular { .. } => "perpendicular",
            ConstraintKind::EqualLength { .. } => "equal length",
            ConstraintKind::Midpoint { .. } => "midpoint",
            ConstraintKind::PointOnLine { .. } => "point on line",
            ConstraintKind::PointOnArc { .. } => "point on arc",
            ConstraintKind::Tangent { .. } => "tangent",
            ConstraintKind::Symmetric { .. } => "symmetric",
        };
        return format!("constraint {id} ({what})");
    }
    format!("constraint {id}")
}

/// Solves `sketch` from its current positions with optional drag targets. `culprits`
/// are the constraints the edit touched (for the error message).
pub(crate) fn solve_sketch(
    sketch: &Sketch,
    drags: &[DragTarget],
    culprits: &[ConstraintId],
    opts: &SolveOptions,
) -> Result<Solved, SolveError> {
    let pos = positions(sketch);
    for d in drags {
        if !pos.contains_key(&d.point) {
            return Err(SolveError::Missing(ItemRef::Point(d.point)));
        }
    }
    let drag_points: Vec<PointId> = drags.iter().map(|d| d.point).collect();
    let mut sys = build(sketch, &pos, &drag_points)?;
    let vals0 = sys.values(&pos);
    let sys_drags: Vec<(usize, Vec2)> = drags.iter().map(|d| (sys.index[&d.point], d.to)).collect();

    // Large dimension changes are walked in steps so the solution stays on the branch
    // the user sees (and Newton does not jump to a folded configuration).
    let max_jump = sys.ramps.iter().map(|r| (r.target - r.start).abs()).fold(0.0, f64::max);
    let step_plan: &[usize] = if max_jump > SATISFIED_TOL { &[1, 4, 16] } else { &[1] };

    let mut first_failure: Option<Vec<f64>> = None;
    let mut solved: Option<Run> = None;
    let mut total_iterations = 0;
    for &steps in step_plan {
        let run = attempt(&mut sys, &vals0, &sys_drags, steps, opts)?;
        total_iterations += run.iterations;
        if unsatisfied(&sys, &run.vals).is_empty() {
            solved = Some(run);
            break;
        }
        if first_failure.is_none() {
            first_failure = Some(run.vals);
        }
    }
    sys.set_ramp(1.0);

    let Some(run) = solved else {
        let vals = first_failure.unwrap_or(vals0);
        let bad = unsatisfied(&sys, &vals);
        let mut conflicting: BTreeSet<ConstraintId> = BTreeSet::new();
        for i in bad {
            if let Owner::Constraint(id) = sys.eqs[i].owner {
                conflicting.insert(id);
            }
        }
        if conflicting.is_empty() {
            return Err(SolveError::NotConverged);
        }
        let conflicting: Vec<ConstraintId> = conflicting.into_iter().collect();
        let culprits: Vec<ConstraintId> = culprits.iter().copied().filter(|c| conflicting.contains(c)).collect();
        // Blame the edit first, then the others.
        let mut order = culprits.clone();
        order.extend(conflicting.iter().filter(|c| !culprits.contains(c)));
        let descriptions = order.iter().map(|&id| describe(sketch, id)).collect();
        return Err(SolveError::Conflict { conflicting, culprits, descriptions });
    };

    let mut out = sketch.clone();
    for (i, id) in sys.points.iter().enumerate() {
        if let Some(p) = out.point_mut(*id) {
            p.pos = Vec2::new(run.vals[2 * i], run.vals[2 * i + 1]);
        }
    }
    update_reference_dimensions(&mut out);

    let drag_reached =
        sys_drags.iter().all(|&(i, to)| Vec2::new(run.vals[2 * i], run.vals[2 * i + 1]).distance(to) <= 1e-3);

    let status = opts.analyse.then(|| status_of(&out, &sys, &run.vals, &[]));
    Ok(Solved { sketch: out, status, drag_reached, iterations: total_iterations, created: Vec::new() })
}

fn status_of(sketch: &Sketch, sys: &System, vals: &[f64], bad: &[usize]) -> Status {
    let a = analyse(sys, vals);
    let mut redundant = BTreeSet::new();
    let mut conflicting = BTreeSet::new();
    for &i in bad {
        if let Owner::Constraint(id) = sys.eqs[i].owner {
            conflicting.insert(id);
        }
    }
    for (i, eq) in sys.eqs.iter().enumerate() {
        if let Owner::Constraint(id) = eq.owner
            && a.dependent[i]
            && !conflicting.contains(&id)
        {
            redundant.insert(id);
        }
    }
    let points = sketch
        .points
        .iter()
        .map(|p| PointDof { id: p.id, dof: sys.index.get(&p.id).map_or(2, |&i| a.point_dof[i]) })
        .collect();
    // Points outside the system are free and not counted in `a.dof`.
    let outside = sketch.points.iter().filter(|p| !sys.index.contains_key(&p.id)).count();
    Status {
        dof: a.dof + 2 * outside,
        points,
        redundant: redundant.into_iter().collect(),
        conflicting: conflicting.into_iter().collect(),
    }
}

/// Re-measures every reference dimension.
pub(crate) fn update_reference_dimensions(s: &mut Sketch) {
    let values: Vec<Option<f64>> =
        s.dimensions.iter().map(|d| if d.driving { None } else { s.measure(&d.measure) }).collect();
    for (d, v) in s.dimensions.iter_mut().zip(values) {
        if let Some(v) = v {
            d.value = v;
        }
    }
}

impl Sketch {
    /// Analyses the sketch as it is, without moving anything. Never fails on
    /// conflicts: they are reported in [`Status::conflicting`] (for showing a broken
    /// sketch loaded from a file). Fails only on dangling references.
    pub fn analyse(&self) -> Result<Status, SolveError> {
        let pos = positions(self);
        let sys = build(self, &pos, &[])?;
        let vals = sys.values(&pos);
        let bad = unsatisfied(&sys, &vals);
        Ok(status_of(self, &sys, &vals, &bad))
    }

    /// Solves the sketch from its current positions.
    pub fn solve(&self, opts: &SolveOptions) -> Result<Solved, SolveError> {
        solve_sketch(self, &[], &[], opts)
    }

    /// Drags a point: it follows `to` as far as the constraints allow.
    pub fn drag(&self, point: PointId, to: Vec2, opts: &SolveOptions) -> Result<Solved, SolveError> {
        solve_sketch(self, &[DragTarget { point, to }], &[], opts)
    }

    /// Ids of everything that refers to a point (edges and constraints), for UI
    /// highlighting.
    pub fn references_to_point(&self, id: PointId) -> Vec<ItemRef> {
        let mut out = Vec::new();
        for e in &self.edges {
            if e.geom.points().contains(&id) {
                out.push(ItemRef::Edge(e.id));
            }
        }
        for c in &self.constraints {
            if constraint_points(&c.kind).contains(&id) {
                out.push(ItemRef::Constraint(c.id));
            }
        }
        for d in &self.dimensions {
            if measure_points(&d.measure).contains(&id) {
                out.push(ItemRef::Constraint(d.id));
            }
        }
        out
    }

    /// Constraints and dimensions that refer to an edge.
    pub fn references_to_edge(&self, id: EdgeId) -> Vec<ConstraintId> {
        let mut out = Vec::new();
        for c in &self.constraints {
            if constraint_edges(&c.kind).contains(&id) {
                out.push(c.id);
            }
        }
        for d in &self.dimensions {
            if measure_edges(&d.measure).contains(&id) {
                out.push(d.id);
            }
        }
        out
    }

    /// Lines (not arcs) touching a point.
    pub fn lines_at(&self, id: PointId) -> Vec<EdgeId> {
        self.edges
            .iter()
            .filter(|e| matches!(e.geom, EdgeGeom::Line { a, b } if a == id || b == id))
            .map(|e| e.id)
            .collect()
    }
}
