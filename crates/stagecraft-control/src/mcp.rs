//! MCP server over stdio (newline-delimited JSON-RPC).
//!
//! Three tools instead of one per command, so clients are not flooded:
//! `stagecraft_commands`, `stagecraft_describe`, `stagecraft_execute`.

use std::io::{BufRead, Write};

use serde_json::{Value, json};
use stagecraft_core::Engine;

use crate::client::Client;
use crate::protocol::{RpcError, handle};

const PROTOCOL_VERSION: &str = "2025-06-18";

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

fn tools() -> Value {
    json!([
        {
            "name": "stagecraft_commands",
            "description": "List StageCraft commands (name, kind, summary). kind is query (read-only), edit (one undo step) or meta (files, undo, selection, batch).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "prefix": { "type": "string", "description": "Only commands starting with this, e.g. `fixture.`" }
                }
            }
        },
        {
            "name": "stagecraft_describe",
            "description": "The JSON schema of a command's arguments, with units and defaults.",
            "inputSchema": {
                "type": "object",
                "properties": { "name": { "type": "string" } },
                "required": ["name"]
            }
        },
        {
            "name": "stagecraft_execute",
            "description": "Run a StageCraft command. Returns its JSON result.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "args": { "type": "object", "description": "Arguments as described by stagecraft_describe" }
                },
                "required": ["command"]
            }
        }
    ])
}

fn call_tool(backend: &mut Backend, params: &Value) -> Value {
    let name = params["name"].as_str().unwrap_or_default();
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    let out = match name {
        "stagecraft_commands" => backend.call("commands", args),
        "stagecraft_describe" => backend.call("describe", args),
        "stagecraft_execute" => backend.call("execute", args),
        other => Err(RpcError::new(RpcError::INVALID_PARAMS, format!("no tool `{other}`"))),
    };
    match out {
        Ok(v) => json!({
            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).expect("json") }],
            "isError": false,
        }),
        Err(e) => json!({
            "content": [{ "type": "text", "text": e.to_string() }],
            "isError": true,
        }),
    }
}

/// Serve MCP until the reader closes.
pub fn serve(backend: &mut Backend, reader: impl BufRead, mut writer: impl Write) -> std::io::Result<()> {
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write_msg(
                    &mut writer,
                    json!({ "jsonrpc": "2.0", "id": null,
                    "error": RpcError::new(RpcError::PARSE, e.to_string()) }),
                )?;
                continue;
            }
        };
        // Notifications have no id and get no answer.
        let Some(id) = msg.get("id").cloned() else {
            continue;
        };
        let method = msg["method"].as_str().unwrap_or_default();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let result: Result<Value, RpcError> = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or(PROTOCOL_VERSION),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "stagecraft", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => Ok(call_tool(backend, &params)),
            other => Err(RpcError::new(RpcError::METHOD_NOT_FOUND, format!("no method `{other}`"))),
        };
        let resp = match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
        };
        write_msg(&mut writer, resp)?;
    }
    Ok(())
}

fn write_msg(w: &mut impl Write, v: Value) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(&v).expect("json");
    bytes.push(b'\n');
    w.write_all(&bytes)?;
    w.flush()
}
