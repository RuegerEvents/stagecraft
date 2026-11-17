//! Parametric 2D sketch for StageCraft floor plans.

mod analysis;
mod edit;
mod error;
mod model;
mod solve;
mod system;

pub use edit::Edit;
pub use error::SolveError;
pub use model::*;
pub use solve::{
    ConstraintState, DragTarget, EntityState, PointDof, SATISFIED_TOL, SolveOptions, Solved, Status, describe,
};
