use std::io::Cursor;
use std::thread;

use serde_json::{Value, json};
use stagecraft_control::mcp::{self, Backend};
use stagecraft_control::{Client, RpcError, Server, discovery};
use stagecraft_core::Engine;

/// A headless instance on its own thread, the way `stagecraft serve` runs it.
fn spawn_instance(token: &str) -> std::net::SocketAddr {
    let server = Server::spawn(token.into(), 0, || {}).unwrap();
    let addr = server.addr;
    thread::spawn(move || {
        let mut engine = Engine::new();
        for call in server.calls {
            call.answer(&mut engine);
        }
    });
    addr
}

#[test]
fn execute_over_tcp() {
    let addr = spawn_instance("secret");
    let mut c = Client::connect(addr, "secret").unwrap();
    assert_eq!(c.call("ping", Value::Null).unwrap()["app"], "stagecraft");

    let t = c.execute("truss.create", json!({ "start": [0, 0, 6000], "end": [8000, 0, 6000] })).unwrap();
    c.execute("fixture.create", json!({ "parent": t["id"], "position": [1000, 0, -300], "fid": 1 })).unwrap();
    let list = c.execute("object.list", json!({ "type": "fixture" })).unwrap();
    assert_eq!(list[0]["world"]["position"], json!([1000.0, 0.0, 5700.0]));

    let err = c.execute("fixture.create", json!({ "fid": 1 })).unwrap_err();
    assert_eq!(err.code, RpcError::COMMAND_FAILED);
    assert_eq!(err.data.unwrap()["code"], "invalid");

    let schema = c.call("describe", json!({ "name": "truss.create" })).unwrap();
    assert!(schema["args"]["properties"]["profile"].is_object());
}

#[test]
fn wrong_token_is_refused() {
    let addr = spawn_instance("right");
    let mut c = Client::connect(addr, "wrong").unwrap();
    let err = c.call("ping", Value::Null).unwrap_err();
    assert_eq!(err.code, RpcError::UNAUTHORIZED);
}

#[test]
fn discovery_finds_live_instances_and_drops_dead_ones() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: only this test in this binary reads STAGECRAFT_HOME.
    unsafe { std::env::set_var("STAGECRAFT_HOME", home.path()) };
    let addr = spawn_instance("tok");
    let _reg = discovery::register(&discovery::Instance {
        pid: 4242,
        addr,
        token: "tok".into(),
        mode: "headless".into(),
        started: 2,
    })
    .unwrap();
    // A stale file pointing nowhere.
    let dead = discovery::register(&discovery::Instance {
        pid: 4243,
        addr: "127.0.0.1:9".parse().unwrap(),
        token: "x".into(),
        mode: "gui".into(),
        started: 3,
    })
    .unwrap();
    std::mem::forget(dead);
    let found = discovery::running();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0.pid, 4242);
    assert!(!home.path().join("instances/4243.json").exists());
}

fn mcp_session(lines: &[Value]) -> Vec<Value> {
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let mut out = Vec::new();
    let mut backend = Backend::Local(Box::new(Engine::new()));
    mcp::serve(&mut backend, Cursor::new(input), &mut out).unwrap();
    String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn mcp_round_trip() {
    let resp = mcp_session(&[
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "0" } } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "stagecraft_execute",
            "arguments": { "command": "layer.create", "args": { "name": "Galerie", "elevation": 4500 } } } }),
        json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "stagecraft_execute",
            "arguments": { "command": "layer.create", "args": { "name": "Galerie" } } } }),
        json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": { "name": "stagecraft_commands",
            "arguments": { "prefix": "fixture." } } }),
    ]);
    assert_eq!(resp.len(), 5, "the notification gets no answer");
    assert_eq!(resp[0]["result"]["serverInfo"]["name"], "stagecraft");
    assert_eq!(resp[1]["result"]["tools"].as_array().unwrap().len(), 3);
    assert_eq!(resp[2]["result"]["isError"], false);
    assert_eq!(resp[3]["result"]["isError"], true);
    let text = resp[4]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("fixture.number"));
}

struct Echo {
    calls: usize,
}

impl stagecraft_control::Extension for Echo {
    fn commands(&self) -> Vec<Value> {
        vec![json!({ "name": "ui.echo", "kind": "ui", "summary": "Echo" })]
    }
    fn describe(&self, name: &str) -> Option<Value> {
        (name == "ui.echo").then(|| json!({ "name": "ui.echo", "kind": "ui", "summary": "Echo", "args": {} }))
    }
    fn execute(&mut self, _: &mut Engine, name: &str, args: Value) -> Option<Result<Value, stagecraft_core::CmdError>> {
        (name == "ui.echo").then(|| {
            self.calls += 1;
            Ok(args)
        })
    }
}

#[test]
fn an_extension_registry_is_listed_described_and_run() {
    let mut engine = Engine::new();
    let mut ext = Echo { calls: 0 };
    let list = stagecraft_control::handle_with(&mut engine, &mut ext, "commands", json!({ "prefix": "ui." })).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    let all = stagecraft_control::handle_with(&mut engine, &mut ext, "commands", Value::Null).unwrap();
    let names: Vec<&str> = all.as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"ui.echo") && names.contains(&"object.list"));
    assert!(names.windows(2).all(|w| w[0] <= w[1]));
    let d = stagecraft_control::handle_with(&mut engine, &mut ext, "describe", json!({ "name": "ui.echo" })).unwrap();
    assert_eq!(d["kind"], "ui");
    let out = stagecraft_control::handle_with(
        &mut engine,
        &mut ext,
        "execute",
        json!({ "command": "ui.echo", "args": { "x": 1 } }),
    )
    .unwrap();
    assert_eq!(out, json!({ "x": 1 }));
    stagecraft_control::handle_with(&mut engine, &mut ext, "execute", json!({ "command": "doc.info" })).unwrap();
    assert_eq!(ext.calls, 1);
}
