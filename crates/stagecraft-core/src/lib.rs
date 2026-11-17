//! StageCraft's document model and command engine.
//!
//! Everything that changes a document goes through [`Engine::execute`] with a command
//! name and JSON arguments. The UI, the control channel and MCP all use that one door.

pub mod command;
pub mod commands;
pub mod engine;
pub mod file;
pub mod geom;
pub mod model;

pub use command::{CmdError, CmdResult, CommandInfo, Kind, Registry, registry};
pub use engine::{Engine, JournalEntry};
pub use model::{Document, Id, Layer, Object, ObjectKind, Placement, Tag};
