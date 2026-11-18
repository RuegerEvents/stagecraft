//! `stagecraft`: the GUI (no subcommand), headless server, MCP server and
//! command-line access.

use std::io::{self, BufReader};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use stagecraft_control::mcp::{self, Backend};
use stagecraft_control::{Client, Server, discovery, handle};
use stagecraft_core::Engine;

#[derive(Parser)]
#[command(name = "stagecraft", version, about = "Stage and lighting planning", args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    /// Without a subcommand: open the GUI, with this .stagecraft file.
    file: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a headless instance that other tools control over the local channel.
    Serve {
        /// .stagecraft file to open.
        file: Option<PathBuf>,
        /// TCP port on 127.0.0.1 (0 = any free port).
        #[arg(long, default_value_t = 0)]
        port: u16,
    },
    /// MCP server on stdio. Talks to the newest running instance, or runs its own
    /// engine if none is running (or with --standalone).
    Mcp {
        /// Open this file in the MCP's own engine (implies --standalone).
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        standalone: bool,
    },
    /// Show running instances.
    Instances,
    /// List commands.
    Commands { prefix: Option<String> },
    /// Print the argument schema of a command.
    Describe { name: String },
    /// Run one command. Against a file with --file (saved afterwards if it changed),
    /// otherwise against the newest running instance.
    Exec {
        command: String,
        /// Arguments as JSON.
        args: Option<String>,
        #[arg(long)]
        file: Option<PathBuf>,
    },
}

/// User catalogues (truss profiles, platform systems, supports): every `*.json` in
/// `<data dir>/StageCraft/catalog/`, merged over the built-in ones.
fn load_user_catalogues() {
    let dir = discovery::base_dir().join("catalog");
    for (path, err) in stagecraft_core::catalog::load_dir(&dir) {
        eprintln!("catalogue {}: {err}", path.display());
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    load_user_catalogues();
    stagecraft_sheet::install();
    let Some(cmd) = cli.cmd else {
        let engine = match cli.file {
            Some(f) => Engine::open(&f).map_err(|e| anyhow!("{e}"))?,
            None => Engine::new(),
        };
        return stagecraft_ui::run(engine).map_err(|e| anyhow!("{e}"));
    };
    match cmd {
        Cmd::Serve { file, port } => serve(file, port),
        Cmd::Mcp { file, standalone } => run_mcp(file, standalone),
        Cmd::Instances => {
            for (inst, info) in discovery::running() {
                println!(
                    "{}  pid {}  {}  {}",
                    inst.addr,
                    inst.pid,
                    inst.mode,
                    info["path"].as_str().unwrap_or("(unsaved)")
                );
            }
            Ok(())
        }
        Cmd::Commands { prefix } => {
            let list = local_call("commands", json!({ "prefix": prefix }))?;
            for c in list.as_array().into_iter().flatten() {
                println!(
                    "{:<24} {:<6} {}",
                    c["name"].as_str().unwrap_or(""),
                    c["kind"].as_str().unwrap_or(""),
                    c["summary"].as_str().unwrap_or("")
                );
            }
            Ok(())
        }
        Cmd::Describe { name } => {
            print_json(&local_call("describe", json!({ "name": name }))?);
            Ok(())
        }
        Cmd::Exec { command, args, file } => exec(&command, args.as_deref(), file),
    }
}

fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).expect("json"));
}

fn local_call(method: &str, params: Value) -> Result<Value> {
    handle(&mut Engine::new(), method, params).map_err(|e| anyhow!("{e}"))
}

fn open_engine(file: Option<PathBuf>) -> Result<Engine> {
    match file {
        Some(f) if f.exists() => Engine::open(&f).map_err(|e| anyhow!("{e}")),
        Some(f) => {
            // A new file: remember the path so doc.save works without arguments.
            let mut e = Engine::new();
            e.execute("doc.save", json!({ "path": f })).map_err(|e| anyhow!("{e}"))?;
            Ok(e)
        }
        None => Ok(Engine::new()),
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn serve(file: Option<PathBuf>, port: u16) -> Result<()> {
    let mut engine = open_engine(file)?;
    let token = std::env::var("STAGECRAFT_TOKEN").unwrap_or_else(|_| discovery::new_token());
    let server = Server::spawn(token.clone(), port, || {}).context("starting the control server")?;
    let _reg = discovery::register(&discovery::Instance {
        pid: std::process::id(),
        addr: server.addr,
        token,
        mode: "headless".into(),
        started: now(),
    })
    .context("writing the instance file")?;
    eprintln!(
        "stagecraft listening on {} (token in {})",
        server.addr,
        discovery::base_dir().join("instances").display()
    );
    for call in server.calls {
        call.answer(&mut engine);
    }
    Ok(())
}

fn run_mcp(file: Option<PathBuf>, standalone: bool) -> Result<()> {
    let mut backend = match (&file, standalone) {
        (None, false) => match discovery::connect_newest() {
            Some((inst, client)) => {
                eprintln!("stagecraft mcp: connected to {} (pid {})", inst.addr, inst.pid);
                Backend::Remote(client)
            }
            None => Backend::Local(Box::new(Engine::new())),
        },
        _ => Backend::Local(Box::new(open_engine(file)?)),
    };
    mcp::serve(&mut backend, BufReader::new(io::stdin()), io::stdout())?;
    Ok(())
}

fn exec(command: &str, args: Option<&str>, file: Option<PathBuf>) -> Result<()> {
    let args: Value = match args {
        Some(a) => serde_json::from_str(a).context("args must be JSON")?,
        None => Value::Null,
    };
    match file {
        Some(f) => {
            let mut engine = open_engine(Some(f))?;
            if matches!(command, "sheet.export_pdf" | "sheet.export_svg") {
                render_sheet_images(&engine);
            }
            let out = engine.execute(command, args).map_err(|e| anyhow!("{e}"))?;
            if engine.dirty() {
                engine.execute("doc.save", Value::Null).map_err(|e| anyhow!("{e}"))?;
            }
            print_json(&out);
        }
        None => {
            let Some((_, mut client)) = discovery::connect_newest() else {
                bail!("no running instance; start one with `stagecraft serve` or pass --file");
            };
            print_json(&run_remote(&mut client, command, args)?);
        }
    }
    Ok(())
}

/// Rendered (3D) sheet viewports need a GPU; without a window one is created here.
/// No GPU (or `STAGECRAFT_NO_GPU` set): the export shows placeholders.
fn render_sheet_images(engine: &Engine) {
    let doc = engine.doc();
    let any = doc.sheets.values().flat_map(|s| &s.viewports).any(|v| v.view.name() == "rendered");
    if !any || std::env::var_os("STAGECRAFT_NO_GPU").is_some() {
        return;
    }
    match stagecraft_ui::view3d::offscreen::Offscreen::headless() {
        Some(mut off) => {
            let ids: Vec<_> = doc.sheets.keys().copied().collect();
            off.render_sheets(doc, &ids);
        }
        None => eprintln!("stagecraft: no GPU, 3D viewports stay placeholders"),
    }
}

fn run_remote(client: &mut Client, command: &str, args: Value) -> Result<Value> {
    client.execute(command, args).map_err(|e| anyhow!("{e}"))
}
