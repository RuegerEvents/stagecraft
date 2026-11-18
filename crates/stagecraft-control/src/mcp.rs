//! MCP server over stdio (newline-delimited JSON-RPC).
//!
//! Three tools instead of one per command, so clients are not flooded:
//! `stagecraft_commands`, `stagecraft_describe`, `stagecraft_execute`. The protocol around
//! them (lifecycle, framing, argument checks, image results) is craft-mcp-server's.

use std::io::{BufRead, Write};

use serde_json::{Value, json};
use stagecraft_core::Engine;

use crate::client::Client;
use crate::protocol::{RpcError, handle};

const INSTRUCTIONS: &str = "\
StageCraft is a stage and lighting planning tool. Every action is a command.
Find commands with stagecraft_commands (filter by prefix such as `object.` or `fixture.`),
read the argument schema with stagecraft_describe, run with stagecraft_execute.
Units: millimetres and degrees. Axes: x right as seen from the audience, y upstage, z up.
Objects can hang on a parent (fixture on truss); positions are then relative to it.
Use the `batch` command to make several edits as one undo step.";

/// Where the MCP server sends its calls.
pub enum Backend {
    /// A running StageCraft (GUI or headless) found through discovery.
    Remote(Client),
    /// An engine owned by the MCP process itself.
    Local(Box<Engine>),
}

impl Backend {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
        match self {
            Backend::Remote(c) => c.call(method, params),
            Backend::Local(e) => handle(e, method, params),
        }
    }
}

/// Longest MCP request line read: imports may carry files as base64.
pub const MAX_LINE: usize = 512 << 20;

/// Largest screenshot returned as an MCP image (bigger ones are answered as text only).
const MAX_IMAGE: u64 = 16 << 20;

fn tools() -> Vec<Value> {
    use craft_mcp_server::{Effect, tool};
    vec![
        tool(
            "stagecraft_commands",
            "List commands",
            "List StageCraft commands (name, kind, summary). kind is query (read-only), edit (one undo step) or meta (files, undo, selection, batch).",
            json!({
                "type": "object",
                "properties": {
                    "prefix": { "type": "string", "description": "Only commands starting with this, e.g. `fixture.`" }
                }
            }),
            Effect::ReadOnly,
        ),
        tool(
            "stagecraft_describe",
            "Describe a command",
            "The JSON schema of a command's arguments, with units and defaults.",
            json!({
                "type": "object",
                "properties": { "name": { "type": "string" } },
                "required": ["name"]
            }),
            Effect::ReadOnly,
        ),
        tool(
            "stagecraft_execute",
            "Run a command",
            "Run a StageCraft command. Returns its JSON result; `ui.screenshot` also returns the image.",
            json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "args": { "type": "object", "description": "Arguments as described by stagecraft_describe" }
                },
                "required": ["command"]
            }),
            // Commands open, save and export files.
            Effect::Files,
        ),
    ]
}

/// The three tools over a [`Backend`], for craft-mcp-server.
struct Tools<'a> {
    backend: &'a mut Backend,
}

impl craft_mcp_server::Tools for Tools<'_> {
    fn instructions(&self) -> String {
        INSTRUCTIONS.to_owned()
    }

    fn list(&mut self) -> Vec<Value> {
        tools()
    }

    fn call(&mut self, name: &str, args: &Value) -> Value {
        let out = match name {
            "stagecraft_commands" => self.backend.call("commands", args.clone()),
            "stagecraft_describe" => self.backend.call("describe", args.clone()),
            "stagecraft_execute" => self.backend.call("execute", args.clone()),
            other => Err(RpcError::new(RpcError::INVALID_PARAMS, format!("no tool `{other}`"))),
        };
        match out {
            Ok(v) => {
                let text = serde_json::to_string_pretty(&v).expect("json");
                let shot = (name == "stagecraft_execute" && args["command"] == "ui.screenshot")
                    .then(|| v["path"].as_str().map(std::path::PathBuf::from))
                    .flatten();
                let mut r = match shot.and_then(|p| read_image(&p)) {
                    Some(png) => craft_mcp_server::image_result(&png, "image/png", text),
                    None => craft_mcp_server::text_result(text, false),
                };
                r["isError"] = json!(false);
                r
            }
            Err(e) => craft_mcp_server::text_result(e.to_string(), true),
        }
    }
}

/// The PNG a screenshot wrote, if it is small enough to send.
fn read_image(path: &std::path::Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_IMAGE {
        return None;
    }
    std::fs::read(path).ok().filter(|b| b.starts_with(b"\x89PNG"))
}

/// Serve MCP until the reader closes (craft-mcp-server: lifecycle, bounded lines, argument
/// checks against the tool schemas).
pub fn serve(backend: &mut Backend, reader: impl BufRead, writer: impl Write) -> std::io::Result<()> {
    let info = craft_mcp_server::Info {
        name: "stagecraft",
        title: "StageCraft",
        version: env!("CARGO_PKG_VERSION"),
        max_line: MAX_LINE,
    };
    let options = craft_mcp_server::Options { tool_errors_as_results: true, ..Default::default() };
    craft_mcp_server::Server::new(info, Tools { backend }).with_options(options).serve(reader, writer)
}
