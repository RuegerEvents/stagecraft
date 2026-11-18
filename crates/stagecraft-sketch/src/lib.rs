//! Parametric 2D sketch for StageCraft floor plans.

mod analysis;
mod draw;
mod edit;
mod error;
mod loops;
mod model;
mod solve;
mod system;

pub use draw::{Axis, Endpoint, InferOptions, LineGesture, LinePlan, infer_line, snap};
pub use edit::Edit;
pub use error::SolveError;
pub use loops::{Loop, LoopEdge};
pub use model::*;
pub use solve::{
    ConstraintState, DragTarget, EntityState, PointDof, SATISFIED_TOL, SolveOptions, Solved, Status, describe,
};
