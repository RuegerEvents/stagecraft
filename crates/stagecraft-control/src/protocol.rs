//! The four methods, run against an engine.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use stagecraft_core::{CmdError, Engine, registry};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub const PARSE: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
    /// A command ran and failed; `data.code` holds the command's error code.
    pub const COMMAND_FAILED: i64 = -32000;
    pub const UNAUTHORIZED: i64 = -32001;

    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), data: None }
    }
}

impl From<CmdError> for RpcError {
    fn from(e: CmdError) -> Self {
        Self { code: Self::COMMAND_FAILED, message: e.message, data: Some(json!({ "code": e.code })) }
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.data.as_ref().and_then(|d| d.get("code")) {
            Some(c) => write!(f, "{}: {}", c.as_str().unwrap_or("error"), self.message),
            None => write!(f, "{} ({})", self.message, self.code),
        }
    }
}

impl std::error::Error for RpcError {}

#[derive(Deserialize)]
struct CommandsParams {
    prefix: Option<String>,
}

#[derive(Deserialize)]
struct DescribeParams {
    name: String,
}

#[derive(Deserialize)]
struct ExecuteParams {
    command: String,
    #[serde(default)]
    args: Value,
}

fn params<T: for<'de> Deserialize<'de>>(p: Value) -> Result<T, RpcError> {
    let p = if p.is_null() { json!({}) } else { p };
    serde_json::from_value(p).map_err(|e| RpcError::new(RpcError::INVALID_PARAMS, e.to_string()))
}

/// A second command registry answered next to the core one: the GUI's `ui.*`
/// commands. `commands` and `describe` merge both; `execute` asks the extension first.
pub trait Extension {
    /// `[{name, kind, summary}]` of the extension's commands.
    fn commands(&self) -> Vec<Value>;
    /// `{name, kind, summary, args}` for one of its commands, `None` if not its own.
    fn describe(&self, name: &str) -> Option<Value>;
    /// Run one of its commands; `None` if the name is not its own.
    fn execute(&mut self, engine: &mut Engine, name: &str, args: Value) -> Option<Result<Value, CmdError>>;
}

struct NoExtension;

impl Extension for NoExtension {
    fn commands(&self) -> Vec<Value> {
        Vec::new()
    }
    fn describe(&self, _: &str) -> Option<Value> {
        None
    }
    fn execute(&mut self, _: &mut Engine, _: &str, _: Value) -> Option<Result<Value, CmdError>> {
        None
    }
}

/// Run one method against the engine.
pub fn handle(engine: &mut Engine, method: &str, p: Value) -> Result<Value, RpcError> {
    handle_with(engine, &mut NoExtension, method, p)
}

/// Run one method against the engine and an extension registry.
pub fn handle_with(engine: &mut Engine, ext: &mut dyn Extension, method: &str, p: Value) -> Result<Value, RpcError> {
    match method {
        "ping" => Ok(json!({
            "app": "stagecraft",
            "version": env!("CARGO_PKG_VERSION"),
            "path": engine.path(),
            "dirty": engine.dirty(),
        })),
        "commands" => {
            let p: CommandsParams = params(p)?;
            let mut list: Vec<Value> = registry().list().into_iter().map(|c| json!(c)).collect();
            list.extend(ext.commands());
            list.retain(|c| {
                p.prefix.as_deref().is_none_or(|pre| c["name"].as_str().is_some_and(|n| n.starts_with(pre)))
            });
            list.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
            Ok(json!(list))
        }
        "describe" => {
            let p: DescribeParams = params(p)?;
            if let Some(d) = ext.describe(&p.name) {
                return Ok(d);
            }
            let e = registry().get(&p.name).ok_or_else(|| RpcError::from(CmdError::unknown_command(&p.name)))?;
            Ok(json!({
                "name": e.name,
                "kind": e.kind,
                "summary": e.summary,
                "args": e.schema(),
            }))
        }
        "execute" => {
            let p: ExecuteParams = params(p)?;
            if let Some(out) = ext.execute(engine, &p.command, p.args.clone()) {
                return Ok(out?);
            }
            Ok(engine.execute(&p.command, p.args)?)
        }
        other => Err(RpcError::new(
            RpcError::METHOD_NOT_FOUND,
            format!("no method `{other}`; use ping, commands, describe or execute"),
        )),
    }
}
