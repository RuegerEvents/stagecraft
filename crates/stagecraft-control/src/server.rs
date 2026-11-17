//! TCP server on 127.0.0.1. Connection threads parse requests and hand them, as
//! [`Call`]s, to whoever owns the engine (the GUI loop or the headless loop).

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use serde::Deserialize;
use serde_json::{Value, json};
use stagecraft_core::Engine;

use crate::protocol::{Extension, RpcError, handle, handle_with};

/// One request waiting for the engine.
pub struct Call {
    pub method: String,
    pub params: Value,
    reply: Sender<Result<Value, RpcError>>,
}

impl Call {
    /// Run the call on the engine and send the answer back to the connection.
    pub fn answer(self, engine: &mut Engine) {
        let out = handle(engine, &self.method, self.params);
        let _ = self.reply.send(out);
    }

    /// Run the call on the engine and an extension registry (the GUI's `ui.*`).
    pub fn answer_with(self, engine: &mut Engine, ext: &mut dyn Extension) {
        let out = handle_with(engine, ext, &self.method, self.params.clone());
        let _ = self.reply.send(out);
    }

    /// Answer later, e.g. after a screenshot has been taken.
    pub fn reply(self, out: Result<Value, RpcError>) {
        let _ = self.reply.send(out);
    }
}

pub struct Server {
    pub addr: SocketAddr,
    pub calls: Receiver<Call>,
}

#[derive(Deserialize)]
struct Request {
    #[serde(default)]
    id: Value,
    method: Option<String>,
    #[serde(default)]
    params: Value,
    token: Option<String>,
}

impl Server {
    /// Listen on 127.0.0.1:`port` (0 picks a free port). `on_call` runs after each
    /// call is queued, e.g. to wake the GUI.
    pub fn spawn(
        token: String,
        port: u16,
        on_call: impl Fn() + Send + Sync + Clone + 'static,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let addr = listener.local_addr()?;
        let (tx, rx) = mpsc::channel();
        thread::Builder::new().name("stagecraft-control".into()).spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                let token = token.clone();
                let wake = on_call.clone();
                let _ = thread::Builder::new()
                    .name("stagecraft-conn".into())
                    .spawn(move || serve_conn(stream, &token, &tx, &wake));
            }
        })?;
        Ok(Self { addr, calls: rx })
    }
}

fn serve_conn(stream: TcpStream, token: &str, tx: &Sender<Call>, wake: &dyn Fn()) {
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(line) = line else { return };
        if line.trim().is_empty() {
            continue;
        }
        let (id, result) = match serde_json::from_str::<Request>(&line) {
            Err(e) => (Value::Null, Err(RpcError::new(RpcError::PARSE, e.to_string()))),
            Ok(req) => {
                let id = req.id.clone();
                (id, dispatch(req, token, tx, wake))
            }
        };
        let resp = match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
        };
        let mut bytes = serde_json::to_vec(&resp).expect("json");
        bytes.push(b'\n');
        if out.write_all(&bytes).is_err() {
            return;
        }
    }
}

fn dispatch(req: Request, token: &str, tx: &Sender<Call>, wake: &dyn Fn()) -> Result<Value, RpcError> {
    if req.token.as_deref() != Some(token) {
        return Err(RpcError::new(RpcError::UNAUTHORIZED, "missing or wrong token"));
    }
    let method = req.method.ok_or_else(|| RpcError::new(RpcError::INVALID_REQUEST, "no method"))?;
    let (reply, rx) = mpsc::channel();
    tx.send(Call { method, params: req.params, reply })
        .map_err(|_| RpcError::new(RpcError::INTERNAL, "the engine has shut down"))?;
    wake();
    rx.recv().map_err(|_| RpcError::new(RpcError::INTERNAL, "the engine dropped the call"))?
}
