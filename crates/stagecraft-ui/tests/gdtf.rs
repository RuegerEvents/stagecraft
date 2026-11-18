//! GDTF in the GUI: the library panel against a mocked GDTF Share (success,
//! failure, a request that never answers), the real Share cache when present, and
//! table edits of type, mode and addresses.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use egui::{Event, Key, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};
use stagecraft_core::{Engine, Id, ObjectKind};
use stagecraft_gdtf::share::{ShareEntry, ShareMode};
use stagecraft_ui::Studio;
use stagecraft_ui::library::ShareBackend;

fn harness(engine: Engine) -> Harness<'static, Studio> {
    let mut h = Harness::builder()
        // egui 0.37 styles a widget from its previous-pass state, so a widget that appears
        // asks for one more frame; closing a modal can take six.
        .with_max_steps(8)
        .with_size(vec2(1400.0, 900.0))
        .build_ui_state(|ui, s: &mut Studio| s.show(ui), Studio::new(engine));
    h.run_steps(4);
    h
}

fn run(h: &mut Harness<'static, Studio>, name: &str, args: Value) -> Value {
    let out = h.state_mut().run(name, args).unwrap_or_else(|e| panic!("{name}: {e}"));
    h.run_steps(4);
    out
}

fn id(v: &Value) -> Id {
    v["id"].as_str().unwrap().parse().unwrap()
}

fn testdata() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

fn entry(rid: u64, fixture: &str) -> ShareEntry {
    ShareEntry {
        rid,
        manufacturer: "StageCraft Demo".into(),
        fixture: fixture.into(),
        revision: "r1".into(),
        modes: vec![ShareMode { name: "Standard".into(), dmx_footprint: 16 }],
        ..Default::default()
    }
}

/// Serves the demo GDTFs as "downloads".
struct Mock;

impl ShareBackend for Mock {
    fn list(&mut self, _: bool) -> Result<Vec<ShareEntry>, String> {
        Ok(vec![entry(11, "Spot 300"), entry(12, "Profile 750"), entry(13, "Broken")])
    }
    fn download(&mut self, e: &ShareEntry) -> Result<PathBuf, String> {
        if e.fixture == "Broken" {
            return Err("GDTF Share is not reachable (offline)".into());
        }
        Ok(testdata().join(format!("gdtf/StageCraft Demo@{}.gdtf", e.fixture)))
    }
}

/// Never answers until told to (a firewall holding the connection).
struct Blocked(Receiver<()>);

impl ShareBackend for Blocked {
    fn list(&mut self, _: bool) -> Result<Vec<ShareEntry>, String> {
        let _ = self.0.recv();
        Err("GDTF Share is not reachable: timed out".into())
    }
    fn download(&mut self, _: &ShareEntry) -> Result<PathBuf, String> {
        Err("unused".into())
    }
}

/// Run frames until the library has no jobs (or fail after 5 s).
fn settle(h: &mut Harness<'static, Studio>) {
    let t = Instant::now();
    while h.state().ui.library.busy() {
        assert!(t.elapsed() < Duration::from_secs(5), "GDTF Share job did not finish");
        std::thread::sleep(Duration::from_millis(5));
        h.run_steps(4);
    }
    h.run_steps(4);
}

#[test]
fn share_search_download_and_import() {
    let mut h = harness(Engine::new());
    h.state_mut().ui.library.set_backend(Box::new(Mock));
    // The Share tab, then typing into the search field and pressing the button.
    h.get_by_label("GDTF Share").click();
    h.run();
    h.get_by_label("Search GDTF Share").click();
    h.run_steps(4);
    h.get_by_label("Search GDTF Share").type_text("spot");
    h.run_steps(4);
    h.get_by_label("Search GDTF Share now").click();
    h.run_steps(4);
    settle(&mut h);
    assert_eq!(h.state().ui.library.hits(10).len(), 1);
    h.get_by_label("Add 11").click();
    h.run_steps(4);
    settle(&mut h);
    let doc = h.state().engine.doc();
    assert!(
        doc.fixture_types.contains_key("StageCraft Demo@Spot 300"),
        "{:?}",
        doc.fixture_types.keys().collect::<Vec<_>>()
    );
    assert_eq!(doc.assets.len(), 1);
    // The new type is the fixture tool's type now; the import is one undo step.
    assert_eq!(h.state().ui.options.fixture_type, "StageCraft Demo@Spot 300");
    assert!(h.state().engine.can_undo());

    // A failed download shows a message and changes nothing.
    let r = run(&mut h, "ui.share_search", json!({ "query": "broken" }));
    assert_eq!(r["hits"][0]["rid"], 13);
    run(&mut h, "ui.share_add", json!({ "rid": 13 }));
    settle(&mut h);
    let (msg, err) = h.state().ui.library.message.clone().unwrap();
    assert!(err && msg.contains("not reachable"), "{msg}");
    assert_eq!(h.state().engine.doc().fixture_types.len(), 1);
}

#[test]
fn a_blocked_network_never_stalls_the_ui() {
    let mut h = harness(Engine::new());
    let (tx, rx) = channel();
    h.state_mut().ui.library.set_backend(Box::new(Blocked(rx)));
    run(&mut h, "ui.share_search", json!({ "query": "robe" }));
    // Frames keep coming while the request hangs, and the spinner says so.
    let t = Instant::now();
    for _ in 0..20 {
        h.run_steps(4);
    }
    assert!(t.elapsed() < Duration::from_secs(5));
    assert!(h.state().ui.library.busy());
    assert!(h.query_by_label_contains("Loading the GDTF Share list").is_some());
    // Commands still work meanwhile.
    run(&mut h, "fixture.create", json!({}));
    tx.send(()).unwrap();
    settle(&mut h);
    let (msg, err) = h.state().ui.library.message.clone().unwrap();
    assert!(err && msg.contains("timed out"), "{msg}");
}

/// The real client on the GDTF Share cache the crate's live test left (offline,
/// no login needed for cached data). Skips without it.
#[test]
fn share_cache_offline() {
    let cache = testdata().join("private/gdtf/.share-cache");
    if !cache.join("list.json").exists() {
        eprintln!("no GDTF Share cache; skipped");
        return;
    }
    let mut h = harness(Engine::new());
    h.state_mut().ui.library.set_backend(Box::new(stagecraft_ui::library::LiveShare::new(cache.clone())));
    run(&mut h, "ui.share_search", json!({ "query": "robe megapointe" }));
    settle(&mut h);
    let r = run(&mut h, "ui.share_search", json!({ "query": "robe megapointe" }));
    assert!(!r["hits"].as_array().unwrap().is_empty(), "{r}");
    // Import a revision that is in the cache's files.
    let cached: Vec<u64> = std::fs::read_dir(cache.join("files"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|f| f.path().file_stem()?.to_str()?.parse().ok())
        .collect();
    let Some(rid) = cached.first() else { return };
    run(&mut h, "ui.share_add", json!({ "rid": rid }));
    settle(&mut h);
    let (msg, err) = h.state().ui.library.message.clone().unwrap();
    assert!(!err, "{msg}");
    assert_eq!(h.state().engine.doc().fixture_types.len(), 1);
}

fn demo_types(h: &mut Harness<'static, Studio>) {
    for n in ["Profile 750", "Spot 300", "Pixel Bar 8"] {
        run(h, "gdtf.import", json!({ "path": testdata().join(format!("gdtf/StageCraft Demo@{n}.gdtf")) }));
    }
}

fn kind(h: &Harness<'static, Studio>, id: Id) -> ObjectKind {
    h.state().engine.doc().objects[&id].kind.clone()
}

#[test]
fn table_edits_type_mode_and_addresses() {
    let mut h = harness(Engine::new());
    demo_types(&mut h);
    let ids: Vec<Id> = (0..3)
        .map(|i| {
            id(&run(
                &mut h,
                "fixture.create",
                json!({ "gdtf": "StageCraft Demo@Spot 300", "fid": i + 1, "position": [i * 1000, 0, 0] }),
            ))
        })
        .collect();
    // Mode for all three from the choice list.
    h.get_by_label("Mode 1").click();
    h.run_steps(4);
    h.get_by_label("Mode 3").click_modifiers(egui::Modifiers::SHIFT);
    h.run_steps(4);
    h.key_press(Key::Enter);
    h.run_steps(4);
    h.get_by_label("Choose Basic").click();
    h.run_steps(4);
    for id in &ids {
        let ObjectKind::Fixture { mode, .. } = kind(&h, *id) else { panic!() };
        assert_eq!(mode.as_deref(), Some("Basic"));
    }
    // One undo step for the range.
    run(&mut h, "history.undo", json!(null));
    let ObjectKind::Fixture { mode, .. } = kind(&h, ids[2]) else { panic!() };
    assert_eq!(mode.as_deref(), Some("Standard"));
    run(&mut h, "history.redo", json!(null));

    // Fill series on addresses follows footprints (Basic = 6 channels).
    h.get_by_label("Address 1").click();
    h.run_steps(4);
    h.event(Event::Text("1.101".into()));
    h.run_steps(4);
    h.key_press(Key::Enter);
    h.run_steps(4);
    h.get_by_label("Address 1").click();
    h.run_steps(4);
    h.get_by_label("Address 3").click_modifiers(egui::Modifiers::SHIFT);
    h.run_steps(4);
    h.get_by_label("Fill series").click();
    h.run_steps(4);
    let addr: Vec<Option<u32>> = ids
        .iter()
        .map(|id| match kind(&h, *id) {
            ObjectKind::Fixture { address, .. } => address,
            _ => None,
        })
        .collect();
    assert_eq!(addr, [Some(101), Some(107), Some(113)]);

    // Type by a part of its name; the pixel bar's second break by "a / b".
    h.get_by_label("Fixture type 2").click();
    h.run_steps(4);
    h.event(Event::Text("pixel".into()));
    h.run_steps(4);
    h.key_press(Key::Enter);
    h.run_steps(4);
    run(&mut h, "fixture.update", json!({ "id": ids[1], "mode": "Split" }));
    h.get_by_label("Address 2").click();
    h.run_steps(4);
    h.event(Event::Text("2.001 / 3.001".into()));
    h.run_steps(4);
    h.key_press(Key::Enter);
    h.run_steps(4);
    let ObjectKind::Fixture { gdtf, address, extra_addresses, .. } = kind(&h, ids[1]) else { panic!() };
    assert_eq!(gdtf.as_deref(), Some("StageCraft Demo@Pixel Bar 8"));
    assert_eq!((address, extra_addresses), (Some(513), vec![1025]));

    // A clash shows in the Patch column.
    run(&mut h, "fixture.update", json!({ "id": ids[2], "address": "1.103" }));
    let text = stagecraft_ui::table::cell_text(
        h.state().engine.doc(),
        &h.state().engine.doc().objects[&ids[2]],
        stagecraft_ui::table::Col::Conflict,
    );
    assert_eq!(text, "overlaps 1");
}
