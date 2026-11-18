use std::io::{BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde_json::{Value, json};

use crate::protocol::RpcError;

/// Longest answer line read from an instance.
pub const MAX_RESPONSE: usize = 512 << 20;

/// A connection to a running instance.
pub struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    token: String,
    next_id: u64,
}

impl Client {
    pub fn connect(addr: SocketAddr, token: impl Into<String>) -> std::io::Result<Self> {
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
        stream.set_nodelay(true)?;
        Ok(Self { reader: BufReader::new(stream.try_clone()?), writer: stream, token: token.into(), next_id: 1 })
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
        let io = |e: std::io::Error| RpcError::new(RpcError::INTERNAL, format!("connection: {e}"));
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params, "token": self.token });
        let mut bytes = serde_json::to_vec(&req).expect("json");
        bytes.push(b'\n');
        self.writer.write_all(&bytes).map_err(io)?;
        let line = match craft_jsonrpc::read_line(&mut self.reader, MAX_RESPONSE) {
            Ok(Some(line)) => line,
            Ok(None) => return Err(RpcError::new(RpcError::INTERNAL, "the instance closed the connection")),
            Err(craft_jsonrpc::LineError::TooLong) => {
                return Err(RpcError::new(RpcError::INTERNAL, "the answer is larger than the client reads"));
            }
            Err(craft_jsonrpc::LineError::Io(e)) => return Err(io(e)),
        };
        let mut resp: Value =
            serde_json::from_slice(&line).map_err(|e| RpcError::new(RpcError::PARSE, e.to_string()))?;
        if let Some(err) = resp.get_mut("error") {
            return Err(
                serde_json::from_value(err.take()).unwrap_or_else(|e| RpcError::new(RpcError::INTERNAL, e.to_string()))
            );
        }
        Ok(resp.get_mut("result").map(Value::take).unwrap_or(Value::Null))
    }

    /// Shorthand for the `execute` method.
    pub fn execute(&mut self, command: &str, args: Value) -> Result<Value, RpcError> {
        self.call("execute", json!({ "command": command, "args": args }))
    }
}
