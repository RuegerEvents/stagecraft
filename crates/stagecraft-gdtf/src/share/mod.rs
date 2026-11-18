//! GDTF Share client (feature `share`, native only).
//!
//! Blocking HTTP via `ureq`. The session cookie from `login.php` is kept in the
//! client's agent. The fixture list and downloaded files are cached under a
//! directory the caller passes in:
//!
//! ```text
//! <cache>/list.json          raw getList.php response
//! <cache>/files/<rid>.gdtf   downloads (a rid is one revision, so never stale)
//! ```
//!
//! Credentials are never logged, cached or included in errors.

#[cfg(all(feature = "keychain", target_os = "macos"))]
pub mod keychain;

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;

pub const BASE_URL: &str = "https://gdtf-share.com/apis/public";
/// Keychain service holding the GDTF Share login (account = user name).
pub const KEYCHAIN_SERVICE: &str = "StageCraft GDTF Share";

/// One revision on GDTF Share.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShareEntry {
    /// Revision id, used for downloads.
    pub rid: u64,
    pub manufacturer: String,
    pub fixture: String,
    pub revision: String,
    /// Fixture type GUID.
    pub uuid: String,
    /// Unix seconds.
    pub last_modified: i64,
    pub creation_date: i64,
    pub uploader: String,
    pub creator: String,
    /// GDTF version of the file, e.g. `"1.2"`.
    pub version: String,
    pub rating: f32,
    pub filesize: u64,
    pub modes: Vec<ShareMode>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShareMode {
    pub name: String,
    pub dmx_footprint: u32,
}

#[derive(Debug)]
pub enum ShareError {
    /// Login refused, or a request needed a login first.
    Auth(String),
    Http(String),
    /// The server answered with something we could not read.
    Format(String),
    Io(std::io::Error),
    /// No credentials found (keychain).
    NoCredentials(String),
}

impl fmt::Display for ShareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auth(e) => write!(f, "GDTF Share login: {e}"),
            Self::Http(e) => write!(f, "GDTF Share request failed: {e}"),
            Self::Format(e) => write!(f, "GDTF Share answer unreadable: {e}"),
            Self::Io(e) => write!(f, "GDTF Share cache: {e}"),
            Self::NoCredentials(e) => write!(f, "no GDTF Share credentials: {e}"),
        }
    }
}

impl std::error::Error for ShareError {}

impl From<std::io::Error> for ShareError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// A GDTF Share session with a disk cache.
pub struct ShareClient {
    agent: ureq::Agent,
    base: String,
    cache: PathBuf,
    logged_in: bool,
}

impl ShareClient {
    /// A client caching under `cache_dir` (created when needed).
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        Self::with_base_url(cache_dir, BASE_URL)
    }

    /// For tests against another server.
    pub fn with_base_url(cache_dir: impl Into<PathBuf>, base: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(120)))
            .http_status_as_error(false)
            .user_agent("StageCraft-GDTF/0.1")
            .build()
            .into();
        Self { agent, base: base.trim_end_matches('/').to_string(), cache: cache_dir.into(), logged_in: false }
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache
    }

    pub fn is_logged_in(&self) -> bool {
        self.logged_in
    }

    /// `POST login.php {"user","password"}`; the session cookie stays in the agent.
    pub fn login(&mut self, user: &str, password: &str) -> Result<(), ShareError> {
        let body = serde_json::json!({ "user": user, "password": password }).to_string();
        let mut resp = self
            .agent
            .post(format!("{}/login.php", self.base))
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(|e| ShareError::Http(e.to_string()))?;
        let status = resp.status();
        let text = resp.body_mut().read_to_string().map_err(|e| ShareError::Http(e.to_string()))?;
        let v: Value = serde_json::from_str(&text)
            .map_err(|_| ShareError::Format(format!("login answered HTTP {status} with non-JSON")))?;
        if v.get("result").and_then(Value::as_bool) == Some(true) {
            self.logged_in = true;
            Ok(())
        } else {
            let msg = v.get("error").and_then(Value::as_str).unwrap_or("refused").to_string();
            Err(ShareError::Auth(msg))
        }
    }

    /// Logs in with the keychain entry [`KEYCHAIN_SERVICE`].
    #[cfg(all(feature = "keychain", target_os = "macos"))]
    pub fn login_from_keychain(&mut self) -> Result<(), ShareError> {
        let (user, password) = keychain::credentials(KEYCHAIN_SERVICE)?;
        self.login(&user, &password)
    }

    /// The fixture list. Uses `<cache>/list.json` when it is younger than
    /// `max_age`; otherwise fetches (needs a login) and rewrites the cache. When
    /// fetching fails and a cache exists, the stale cache is returned.
    pub fn list(&mut self, max_age: Duration) -> Result<Vec<ShareEntry>, ShareError> {
        let path = self.cache.join("list.json");
        if let Ok(meta) = std::fs::metadata(&path)
            && let Ok(modified) = meta.modified()
            && SystemTime::now().duration_since(modified).unwrap_or_default() < max_age
            && let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(list) = parse_list(&text)
        {
            return Ok(list);
        }
        match self.fetch_list() {
            Ok((text, list)) => {
                std::fs::create_dir_all(&self.cache)?;
                write_atomic(&path, text.as_bytes())?;
                Ok(list)
            }
            Err(e) => match std::fs::read_to_string(&path).ok().and_then(|t| parse_list(&t).ok()) {
                Some(list) => Ok(list),
                None => Err(e),
            },
        }
    }

    /// The cached list only, without network.
    pub fn cached_list(&self) -> Option<Vec<ShareEntry>> {
        let text = std::fs::read_to_string(self.cache.join("list.json")).ok()?;
        parse_list(&text).ok()
    }

    fn fetch_list(&mut self) -> Result<(String, Vec<ShareEntry>), ShareError> {
        if !self.logged_in {
            return Err(ShareError::Auth("not logged in".into()));
        }
        let mut resp =
            self.agent.get(format!("{}/getList.php", self.base)).call().map_err(|e| ShareError::Http(e.to_string()))?;
        let status = resp.status();
        let text = resp
            .body_mut()
            .with_config()
            .limit(512 << 20)
            .read_to_string()
            .map_err(|e| ShareError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(ShareError::Http(format!("getList.php: HTTP {status}")));
        }
        let list = parse_list(&text)?;
        Ok((text, list))
    }

    /// Path a download of `rid` is cached at.
    pub fn file_path(&self, rid: u64) -> PathBuf {
        self.cache.join("files").join(format!("{rid}.gdtf"))
    }

    /// The `.gdtf` bytes of revision `rid`, from the cache or `downloadFile.php`.
    pub fn download(&mut self, rid: u64) -> Result<Vec<u8>, ShareError> {
        let path = self.file_path(rid);
        if let Ok(b) = std::fs::read(&path)
            && b.starts_with(b"PK")
        {
            return Ok(b);
        }
        if !self.logged_in {
            return Err(ShareError::Auth("not logged in".into()));
        }
        let mut resp = self
            .agent
            .get(format!("{}/downloadFile.php", self.base))
            .query("rid", rid.to_string())
            .call()
            .map_err(|e| ShareError::Http(e.to_string()))?;
        let status = resp.status();
        let bytes = resp
            .body_mut()
            .with_config()
            .limit(512 << 20)
            .read_to_vec()
            .map_err(|e| ShareError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(ShareError::Http(format!("downloadFile.php?rid={rid}: HTTP {status}")));
        }
        if !bytes.starts_with(b"PK") {
            // Errors come back as JSON with HTTP 200.
            let msg = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_else(|| "answer is not a zip".into());
            return Err(ShareError::Format(format!("rid {rid}: {msg}")));
        }
        std::fs::create_dir_all(path.parent().unwrap())?;
        write_atomic(&path, &bytes)?;
        Ok(bytes)
    }
}

/// A crash or a full disk never leaves half a cached GDTF (craft-atomic).
fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    craft_atomic::atomic_write(path, data)
}

/// Parses a `getList.php` answer. Field types vary in practice (numbers as strings
/// and vice versa), so every field is read leniently; entries without a rid are
/// dropped.
pub fn parse_list(text: &str) -> Result<Vec<ShareEntry>, ShareError> {
    let v: Value = serde_json::from_str(text).map_err(|e| ShareError::Format(e.to_string()))?;
    if v.get("result").and_then(Value::as_bool) == Some(false) {
        let msg = v.get("error").and_then(Value::as_str).unwrap_or("result false");
        return Err(ShareError::Auth(msg.to_string()));
    }
    let list = v.get("list").or(Some(&v)).and_then(Value::as_array).ok_or(ShareError::Format("no list".into()))?;
    Ok(list.iter().filter_map(entry).collect())
}

fn s(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn n(v: &Value, k: &str) -> Option<f64> {
    match v.get(k)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn entry(v: &Value) -> Option<ShareEntry> {
    let rid = n(v, "rid")? as u64;
    let modes = v
        .get("modes")
        .and_then(Value::as_array)
        .map(|ms| {
            ms.iter()
                .map(|m| ShareMode {
                    name: s(m, "name"),
                    dmx_footprint: n(m, "dmxfootprint").or(n(m, "dmxFootprint")).unwrap_or(0.0) as u32,
                })
                .collect()
        })
        .unwrap_or_default();
    Some(ShareEntry {
        rid,
        manufacturer: s(v, "manufacturer"),
        fixture: s(v, "fixture"),
        revision: s(v, "revision"),
        uuid: s(v, "uuid"),
        last_modified: n(v, "lastModified").unwrap_or(0.0) as i64,
        creation_date: n(v, "creationDate").unwrap_or(0.0) as i64,
        uploader: s(v, "uploader"),
        creator: s(v, "creator"),
        version: s(v, "version"),
        rating: n(v, "rating").unwrap_or(0.0) as f32,
        filesize: n(v, "filesize").unwrap_or(0.0) as u64,
        modes,
    })
}

/// Entries matching every whitespace-separated word of `query` (case-insensitive)
/// in manufacturer, fixture or mode names, best first: exact fixture-name
/// matches, then by manufacturer/fixture, newest revision first within a fixture.
pub fn search<'a>(list: &'a [ShareEntry], query: &str) -> Vec<&'a ShareEntry> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let q = query.trim().to_lowercase();
    let mut hits: Vec<&ShareEntry> = list
        .iter()
        .filter(|e| {
            let hay = format!(
                "{} {} {}",
                e.manufacturer,
                e.fixture,
                e.modes.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join(" ")
            )
            .to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .collect();
    hits.sort_by(|a, b| {
        let exact = |e: &ShareEntry| {
            let f = e.fixture.to_lowercase();
            let mf = format!("{} {}", e.manufacturer, e.fixture).to_lowercase();
            !(f == q || mf == q)
        };
        exact(a)
            .cmp(&exact(b))
            .then_with(|| a.manufacturer.to_lowercase().cmp(&b.manufacturer.to_lowercase()))
            .then_with(|| a.fixture.to_lowercase().cmp(&b.fixture.to_lowercase()))
            .then_with(|| b.last_modified.cmp(&a.last_modified))
    });
    hits
}

/// The newest revision per fixture type (uuid, or manufacturer+fixture without one).
pub fn latest_revisions(list: &[ShareEntry]) -> Vec<&ShareEntry> {
    let mut best: std::collections::BTreeMap<String, &ShareEntry> = Default::default();
    for e in list {
        let key = if e.uuid.is_empty() {
            format!("{}\u{0}{}", e.manufacturer.to_lowercase(), e.fixture.to_lowercase())
        } else {
            e.uuid.to_lowercase()
        };
        best.entry(key)
            .and_modify(|cur| {
                if e.last_modified > cur.last_modified {
                    *cur = e;
                }
            })
            .or_insert(e);
    }
    best.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"{"result":true,"timestamp":1,"list":[
      {"rid":101,"fixture":"Robin Spikie","manufacturer":"Robe Lighting","revision":"v1","uuid":"A","lastModified":10,"modes":[{"name":"Mode 1","dmxfootprint":27}]},
      {"rid":"102","fixture":"Robin Spikie","manufacturer":"Robe Lighting","revision":"v2","uuid":"A","lastModified":"20","rating":"4.5","modes":[]},
      {"rid":200,"fixture":"Source Four","manufacturer":"ETC","revision":"","uuid":"B","lastModified":5},
      {"fixture":"no rid"}
    ]}"#;

    #[test]
    fn lenient_list() {
        let l = parse_list(LIST).unwrap();
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].modes[0].dmx_footprint, 27);
        assert_eq!(l[1].rid, 102);
        assert_eq!(l[1].rating, 4.5);
        assert!(parse_list(r#"{"result":false,"error":"no session"}"#).is_err());
    }

    #[test]
    fn search_and_latest() {
        let l = parse_list(LIST).unwrap();
        let s = search(&l, "robe spikie");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].rid, 102, "newest first");
        assert_eq!(search(&l, "source four")[0].rid, 200);
        assert!(search(&l, "martin").is_empty());
        let latest = latest_revisions(&l);
        assert_eq!(latest.len(), 2);
        assert!(latest.iter().any(|e| e.rid == 102));
    }

    #[test]
    fn cached_list_without_network() {
        let dir = std::env::temp_dir().join(format!("sc-gdtf-share-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("list.json"), LIST).unwrap();
        let mut c = ShareClient::with_base_url(&dir, "http://127.0.0.1:9");
        assert_eq!(c.list(Duration::from_secs(3600)).unwrap().len(), 3);
        // Stale cache is still used when the fetch fails (not logged in here).
        assert_eq!(c.list(Duration::ZERO).unwrap().len(), 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
