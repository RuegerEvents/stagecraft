use schemars::JsonSchema;
use serde::Serialize;

use crate::model::{ConstraintId, EdgeId, ItemRef};

/// Why an edit was rejected. The input sketch is never modified; the caller rolls back.
#[derive(Clone, Debug, PartialEq, thiserror::Error, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SolveError {
    /// The edit or a constraint refers to something that does not exist.
    #[error("{0} does not exist")]
    Missing(ItemRef),
    /// A constraint or dimension needs a line where there is an arc, or vice versa.
    #[error("constraint {constraint} needs edge {edge} to be a {expected}")]
    WrongEdgeKind { constraint: ConstraintId, edge: EdgeId, expected: &'static str },
    /// A tangent constraint between a line and an arc that share no end point.
    #[error("tangent constraint {constraint}: the line and the arc share no end point")]
    NotTouching { constraint: ConstraintId },
    /// Negative, NaN or infinite dimension value.
    #[error("dimension {dimension}: invalid value {value}")]
    BadValue { dimension: ConstraintId, value: f64 },
    /// Tried to set the value of a reference dimension.
    #[error("dimension {0} is a reference dimension; make it driving first")]
    NotDriving(ConstraintId),
    /// The constraints contradict each other. `conflicting` lists every constraint or
    /// dimension that could not be satisfied, `culprits` the ones this edit touched
    /// (the best candidates to blame in a message).
    #[error("conflicting constraints: {}", describe(.descriptions))]
    Conflict { conflicting: Vec<ConstraintId>, culprits: Vec<ConstraintId>, descriptions: Vec<String> },
    /// The solver stopped without satisfying the constraints, but no contradiction was
    /// found (usually a too large jump; try smaller steps).
    #[error("the solver did not converge")]
    NotConverged,
    /// Internal failure of the numeric solver.
    #[error("solver failure: {0}")]
    Solver(String),
}

fn describe(d: &[String]) -> String {
    d.join(", ")
}
