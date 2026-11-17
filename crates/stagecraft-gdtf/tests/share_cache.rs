//! The GDTF Share client against the cache the live test (or an earlier session)
//! left in `testdata/private/gdtf/.share-cache`: real list format, search, and
//! downloads served from disk without a login. Skips when the cache is missing.

#![cfg(feature = "share")]

use std::path::Path;
use std::time::Duration;

use stagecraft_gdtf::Gdtf;
use stagecraft_gdtf::share::{self, ShareClient};

#[test]
fn real_list_and_cached_downloads() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/private/gdtf/.share-cache");
    if !dir.join("list.json").exists() {
        eprintln!("no GDTF Share cache; skipped");
        return;
    }
    // Not logged in: list() must serve the cache even when it is old.
    let mut c = ShareClient::new(&dir);
    let list = c.list(Duration::from_secs(365 * 24 * 3600)).unwrap();
    assert!(list.len() > 1000, "{}", list.len());
    assert!(list.iter().all(|e| e.rid > 0));
    assert!(list.iter().filter(|e| !e.modes.is_empty()).count() > list.len() / 2);
    let latest = share::latest_revisions(&list);
    println!("{} revisions, {} fixture types", list.len(), latest.len());
    let hits = share::search(&list, "robe megapointe");
    assert!(!hits.is_empty());
    println!("'robe megapointe': {} hits, first rid {} {}", hits.len(), hits[0].rid, hits[0].fixture);

    let mut n = 0;
    for f in std::fs::read_dir(dir.join("files")).into_iter().flatten().flatten() {
        let Some(rid) = f.path().file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse::<u64>().ok()) else {
            continue;
        };
        let bytes = c.download(rid).expect("served from cache");
        let g = Gdtf::from_bytes(&bytes).unwrap();
        let entry = list.iter().find(|e| e.rid == rid).expect("rid in list");
        // The footprints GDTF Share reports match ours.
        for m in &entry.modes {
            if let Some(r) = g.resolve_mode(&m.name) {
                assert_eq!(r.footprint_break1(), m.dmx_footprint, "{} / {}", entry.fixture, m.name);
            }
        }
        println!("rid {rid}: {} ({} modes, footprints agree)", g.fixture.name, entry.modes.len());
        n += 1;
    }
    assert!(n > 0);
}
