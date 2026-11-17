//! What views draw for each object.
//!
//! The representation contract (docs/architecture.md): one module per object kind in
//! [`kinds`] implements
//!
//! - `plan(doc, obj, &PlanStyle) -> Vec<PlanPrim>`: 2D primitives in **world** mm, and
//! - `mesh(doc, obj, Lod) -> Mesh`: triangles in the object's **local** space.
//!
//! Section cuts of meshes by planes are in [`section`].
//!
//! Selection support (bounds, hit tests, box selection) and snapping are pure
//! functions on top of the plan primitives, in [`pick`] and [`snap`].

pub mod catalog;
pub mod kinds;
pub mod math;
pub mod mesh;
pub mod pick;
pub mod prim;
pub mod section;
pub mod snap;

pub use catalog::{TrussProfile, truss_profile, truss_profiles};
pub use mesh::{Group, Lod, MaterialClass, Mesh};
pub use prim::{Bounds, Detail, FillClass, LineClass, PlanPrim, PlanStyle, TextAnchor};

use stagecraft_core::{Document, Object};

/// The plan primitives of one object, in world mm.
pub fn plan(doc: &Document, obj: &Object, style: &PlanStyle) -> Vec<PlanPrim> {
    kinds::plan(doc, obj, style)
}

/// The mesh of one object, in its local space.
pub fn mesh(doc: &Document, obj: &Object, lod: Lod) -> Mesh {
    kinds::mesh(doc, obj, lod)
}

/// The mesh of one object in world mm (through `stagecraft_core::geom::world`).
pub fn world_mesh(doc: &Document, obj: &Object, lod: Lod) -> Mesh {
    let w = stagecraft_core::geom::world(doc, obj.id).unwrap_or(glam::DAffine3::IDENTITY);
    kinds::mesh(doc, obj, lod).transformed(&w)
}
