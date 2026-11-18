//! Running instances announce themselves in `<data dir>/StageCraft/instances/<pid>.json`
//! (`~/Library/Application Support/StageCraft/…` on macOS). `STAGECRAFT_HOME`
//! overrides the base directory. The file holds the token, so it is created 0600.

use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::Client;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Instance {
    pub pid: u32,
    pub addr: SocketAddr,
    pub token: String,
    /// "gui" or "headless".
    pub mode: String,
    /// Unix seconds.
    pub started: u64,
}

pub fn base_dir() -> PathBuf {
    if let Some(h) = std::env::var_os("STAGECRAFT_HOME") {
        return PathBuf::from(h);
    }
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("StageCraft")
}

fn instances_dir() -> PathBuf {
    base_dir().join("instances")
}

/// Removes the instance file when dropped.
pub struct Registration {
    path: PathBuf,
}

impl Drop for Registration {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn register(inst: &Instance) -> std::io::Result<Registration> {
    let dir = instances_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", inst.pid));
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&path)?;
    f.write_all(&serde_json::to_vec_pretty(inst).expect("json"))?;
    Ok(Registration { path })
}

fn read(path: &Path) -> Option<Instance> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// Instances that answer a ping, newest first. Files of dead instances are removed.
pub fn running() -> Vec<(Instance, Value)> {
    let Ok(entries) = fs::read_dir(instances_dir()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let Some(inst) = read(&path) else { continue };
        let alive =
            Client::connect(inst.addr, inst.token.clone()).ok().and_then(|mut c| c.call("ping", Value::Null).ok());
        match alive {
            Some(info) => out.push((inst, info)),
            None => {
                let _ = fs::remove_file(&path);
            }
        }
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.0.started));
    out
}

/// Connect to the newest running instance, if any.
pub fn connect_newest() -> Option<(Instance, Client)> {
    running().into_iter().find_map(|(inst, _)| Client::connect(inst.addr, inst.token.clone()).ok().map(|c| (inst, c)))
}

/// A per-launch token: 32 random bytes from the operating system as hex (craft-control), or a
/// UUID if the system has no randomness to give.
pub fn new_token() -> String {
    craft_control::server::new_token().unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string())
}
