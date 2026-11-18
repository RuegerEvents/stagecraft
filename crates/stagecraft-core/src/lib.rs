//! StageCraft's document model and command engine.
//!
//! Everything that changes a document goes through [`Engine::execute`] with a command
//! name and JSON arguments. The UI, the control channel and MCP all use that one door.

pub mod asset;
pub mod catalog;
pub mod command;
pub mod commands;
pub mod engine;
pub mod file;
pub mod fixture_type;
pub mod geom;
pub mod load;
pub mod model;
pub mod model_file;
pub mod mvr;
pub mod patch;
pub mod sheet;
pub mod sketch;

pub use command::{CmdError, CmdResult, CommandInfo, Kind, Registry, registry};
pub use engine::{Engine, JournalEntry};
pub use model::{Document, Id, Layer, Object, ObjectKind, Placement, Tag};
