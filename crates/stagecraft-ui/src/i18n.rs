//! UI text. Every visible string goes through [`t`] with a key from the catalog in
//! `assets/i18n/en.json` (English, embedded). Missing keys show the key itself.
//!
//! Placeholders are `{name}`; fill them with [`tf`].

use std::collections::HashMap;
use std::sync::OnceLock;

fn catalog() -> &'static HashMap<String, String> {
    static C: OnceLock<HashMap<String, String>> = OnceLock::new();
    C.get_or_init(|| serde_json::from_str(include_str!("../assets/i18n/en.json")).expect("en.json is valid"))
}

/// The text for `key`.
pub fn t(key: &str) -> &'static str {
    catalog().get(key).map_or_else(|| leak(key), String::as_str)
}

/// The text for `key` with `{name}` placeholders replaced.
pub fn tf(key: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut s = t(key).to_owned();
    for (k, v) in args {
        s = s.replace(&format!("{{{k}}}"), &v.to_string());
    }
    s
}

/// Unknown keys are returned as-is; this keeps `t`'s lifetime simple for them.
fn leak(key: &str) -> &'static str {
    static MISSING: OnceLock<std::sync::Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let mut m = MISSING.get_or_init(Default::default).lock().expect("not poisoned");
    m.entry(key.to_owned()).or_insert_with(|| Box::leak(key.to_owned().into_boxed_str()))
}

pub fn has(key: &str) -> bool {
    catalog().contains_key(key)
}

/// Search aliases (`assets/i18n/aliases.json`): an English term of the titles and the
/// words people also type for it, German and Vectorworks names included ("Zug",
/// "Lattenzug", "pipe" for batten; "Klasse" for tag). All lower case.
pub fn aliases() -> &'static [(String, Vec<String>)] {
    static A: OnceLock<Vec<(String, Vec<String>)>> = OnceLock::new();
    A.get_or_init(|| {
        let m: std::collections::BTreeMap<String, Vec<String>> =
            serde_json::from_str(include_str!("../assets/i18n/aliases.json")).expect("aliases.json is valid");
        m.into_iter().map(|(k, v)| (k.to_lowercase(), v.into_iter().map(|a| a.to_lowercase()).collect())).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `t("…")` / `tf("…"` literal in the sources has an entry.
    #[test]
    fn every_key_used_is_in_the_catalog() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut missing = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(dir)];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                if p.ends_with("i18n.rs") {
                    continue;
                }
                let src = std::fs::read_to_string(&p).unwrap();
                for pat in ["t(\"", "tf(\""] {
                    for (i, _) in src.match_indices(pat) {
                        let before = src[..i].chars().last();
                        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                            continue;
                        }
                        let rest = &src[i + pat.len()..];
                        let key = &rest[..rest.find('"').unwrap()];
                        if !has(key) && !key.is_empty() {
                            missing.push(format!("{}: {key}", p.display()));
                        }
                    }
                }
            }
        }
        assert!(missing.is_empty(), "missing keys:\n{}", missing.join("\n"));
    }

    #[test]
    fn placeholders() {
        assert_eq!(tf("status.created", &[("what", &"Wall")]), "Created Wall");
        assert_eq!(t("no.such.key"), "no.such.key");
    }
}
