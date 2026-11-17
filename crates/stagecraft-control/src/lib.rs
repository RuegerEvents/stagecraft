//! Driving StageCraft from outside.
//!
//! - [`server`]: JSON-RPC 2.0, one JSON object per line, over TCP on 127.0.0.1. Every
//!   request carries the instance's `token`.
//! - [`discovery`]: a running instance announces port and token in a file, so MCP and
//!   the CLI can find it without flags.
//! - [`mcp`]: an MCP server on stdio that talks to a running instance or to an engine
//!   of its own.
//!
//! Methods (same in-process and over TCP): `ping`, `commands`, `describe`, `execute`.
//! See `docs/spec/control.md`.

pub mod client;
pub mod discovery;
pub mod mcp;
pub mod protocol;
pub mod server;

pub use client::Client;
pub use protocol::{RpcError, handle};
pub use server::{Call, Server};
