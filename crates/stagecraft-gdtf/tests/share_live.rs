//! Live GDTF Share test: logs in with the keychain item "StageCraft GDTF Share",
//! fetches the list and downloads a few common fixtures into
//! `testdata/private/gdtf/` (git-ignored), where `real_files.rs` picks them up.
//!
//! `cargo test --features keychain --test share_live -- --ignored --nocapture`
//!
//! A per-app firewall (LuLu here) holds connections of every newly built test
//! binary until it is allowed; the test then times out after two minutes.

#![cfg(all(feature = "keychain", target_os = "macos"))]

use std::path::Path;
use std::time::Duration;

use stagecraft_gdtf::Gdtf;
use stagecraft_gdtf::share::{self, ShareClient};

/// (search words, must contain in fixture name) for the fixtures to fetch.
const WANTED: &[(&str, &str)] = &[
    ("Robe MegaPointe", "megapointe"),
    ("Martin MAC Aura XB", "aura xb"),
    ("ETC Source 4wrd Color", "source 4wrd"),
    ("StairVille LED PAR64 CX-3 RGBW", "par64"),
    ("Chauvet Rogue R2 Wash", "rogue r2 wash"),
];

#[test]
#[ignore = "needs network and the keychain item"]
fn login_list_download() {
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/private/gdtf");
    let mut client = ShareClient::new(out.join(".share-cache"));
    client.login_from_keychain().expect("login");
    let list = client.list(Duration::from_secs(24 * 3600)).expect("list");
    println!("GDTF Share list: {} revisions", list.len());
    assert!(list.len() > 1000, "list looks too short");
    let latest = share::latest_revisions(&list);
    let latest: Vec<_> = latest.into_iter().cloned().collect();
    let mut got = 0;
    for (query, must) in WANTED {
        let hits = share::search(&latest, query);
        let Some(e) = hits.iter().find(|e| e.fixture.to_lowercase().contains(must)) else {
            println!("  {query}: no match");
            continue;
        };
        let bytes = client.download(e.rid).expect("download");
        let g = Gdtf::from_bytes(&bytes).expect("downloaded GDTF parses");
        let name = format!("{}@{}.gdtf", e.manufacturer, e.fixture).replace(['/', '\\'], "_");
        std::fs::write(out.join(&name), &bytes).unwrap();
        println!(
            "  {query}: rid {} {} / {} rev {:?}, {} bytes, {} modes",
            e.rid,
            e.manufacturer,
            e.fixture,
            e.revision,
            bytes.len(),
            g.fixture.modes.len()
        );
        got += 1;
    }
    assert!(got >= 3, "only {got} fixtures downloaded");
}
