//! GDTF Share credentials from the macOS keychain (feature `keychain`).
//!
//! A generic password item: service [`super::KEYCHAIN_SERVICE`], account = GDTF
//! Share user name, secret = password. Create it with
//! `security add-generic-password -s "StageCraft GDTF Share" -a <user> -w`.
//!
//! The account comes from an attribute search (no prompt). The secret is read
//! with `/usr/bin/security`, which is on the access list of items it created, so
//! no dialog appears; reading the secret through the Security framework instead
//! asks the user to allow every newly built binary, which blocks unattended runs.
//! The framework read stays as a fallback for items `security` may not read.

use security_framework::item::{ItemClass, ItemSearchOptions};
use security_framework::passwords::get_generic_password;

use super::ShareError;

/// (user, password) of the first item for `service`.
pub fn credentials(service: &str) -> Result<(String, String), ShareError> {
    let found = ItemSearchOptions::new()
        .class(ItemClass::generic_password())
        .service(service)
        .load_attributes(true)
        .limit(1)
        .search()
        .map_err(|e| ShareError::NoCredentials(format!("keychain search for \"{service}\": {e}")))?;
    let user = found
        .first()
        .and_then(|r| r.simplify_dict())
        .and_then(|d| d.get("acct").cloned())
        .filter(|a| !a.is_empty())
        .ok_or_else(|| ShareError::NoCredentials(format!("no account on keychain item \"{service}\"")))?;
    let secret = match security_cli(service, &user) {
        Some(s) => s,
        None => get_generic_password(service, &user)
            .map_err(|e| ShareError::NoCredentials(format!("keychain password for \"{service}\": {e}")))?,
    };
    let password =
        String::from_utf8(secret).map_err(|_| ShareError::NoCredentials("keychain password is not UTF-8".into()))?;
    Ok((user, password))
}

/// The secret via `/usr/bin/security find-generic-password -w` (stdout captured,
/// never echoed). `None` when the tool fails.
fn security_cli(service: &str, account: &str) -> Option<Vec<u8>> {
    let out = std::process::Command::new("/usr/bin/security")
        .args(["find-generic-password", "-s", service, "-a", account, "-w"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let mut s = out.stdout;
    while s.last() == Some(&b'\n') {
        s.pop();
    }
    (!s.is_empty()).then_some(s)
}
