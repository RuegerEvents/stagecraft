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

/// Run one method against the engine.
pub fn handle(engine: &mut Engine, method: &str, p: Value) -> Result<Value, RpcError> {
    match method {
        "ping" => Ok(json!({
            "app": "stagecraft",
            "version": env!("CARGO_PKG_VERSION"),
            "path": engine.path(),
            "dirty": engine.dirty(),
        })),
        "commands" => {
            let p: CommandsParams = params(p)?;
            let list: Vec<_> = registry()
                .list()
                .into_iter()
                .filter(|c| p.prefix.as_deref().is_none_or(|pre| c.name.starts_with(pre)))
                .collect();
            Ok(json!(list))
        }
        "describe" => {
            let p: DescribeParams = params(p)?;
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
            Ok(engine.execute(&p.command, p.args)?)
        }
        other => Err(RpcError::new(
            RpcError::METHOD_NOT_FOUND,
            format!("no method `{other}`; use ping, commands, describe or execute"),
        )),
    }
}
