//! Every GDTF in the private test data must parse without panic, resolve every mode
//! and give a non-empty mesh. Sources (git-ignored, skipped when missing):
//!
//! - `.gdtf` entries inside the MVR files under `testdata/private/` (repo root),
//! - `.gdtf` files under `testdata/private/gdtf/` (GDTF Share downloads).
//!
//! `cargo test --test real_files -- --nocapture` prints a summary table.

use std::collections::BTreeSet;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use stagecraft_gdtf::mesh::Lod;
use stagecraft_gdtf::models::MeshSource;
use stagecraft_gdtf::{Gdtf, silhouette};

fn private_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/private")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        let hidden = p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'));
        if hidden {
            continue; // e.g. the GDTF Share cache, which holds the same files by rid
        }
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// (label, bytes) of every distinct GDTF found.
fn collect() -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    walk(&private_dir(), &mut files);
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for f in files {
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let Ok(bytes) = std::fs::read(&f) else { continue };
        match ext.as_str() {
            "gdtf" => {
                let label = f.file_name().unwrap().to_string_lossy().into_owned();
                if seen.insert((label.clone(), bytes.len())) {
                    out.push((label, bytes));
                }
            }
            "mvr" => {
                let Ok(mut z) = zip::ZipArchive::new(Cursor::new(bytes)) else { continue };
                for i in 0..z.len() {
                    let Ok(mut e) = z.by_index(i) else { continue };
                    let name = String::from_utf8_lossy(e.name_raw()).into_owned();
                    if !name.to_lowercase().ends_with(".gdtf") {
                        continue;
                    }
                    let mut data = Vec::new();
                    if e.read_to_end(&mut data).is_err() {
                        continue;
                    }
                    let label = name.rsplit('/').next().unwrap_or(&name).to_string();
                    if seen.insert((label.clone(), data.len())) {
                        out.push((label, data));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn short(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { s.chars().take(n - 1).collect::<String>() + "…" }
}

#[test]
fn every_private_gdtf_parses() {
    let all = collect();
    if all.is_empty() {
        eprintln!("no private GDTF test data; skipped");
        return;
    }
    let mut failures = Vec::new();
    println!(
        "\n{:<44} {:>5} {:>6} {:>5} {:>7} {:>5} {:>4} {:>5} {:>4} {:>16}  footprints (break 1, first modes)",
        "file", "modes", "kg", "W", "tris", "parts", "axes", "beams", "warn", "size mm"
    );
    for (label, bytes) in &all {
        let g = match Gdtf::from_bytes(bytes) {
            Ok(g) => g,
            Err(e) => {
                println!("{:<44} FAILED: {e}", short(label, 44));
                failures.push(format!("{label}: {e}"));
                continue;
            }
        };
        let modes = g.resolve_modes();
        assert_eq!(modes.len(), g.fixture.modes.len());
        let mesh = g.mesh(None, Lod::Default);
        let flat = mesh.flatten();
        if flat.is_empty() {
            failures.push(format!("{label}: empty mesh"));
        }
        let sil = silhouette::silhouette(&mesh);
        if sil.hull.len() < 3 {
            failures.push(format!("{label}: no silhouette"));
        }
        // The mesh of every mode root must build too (Astera files have one per mode).
        let roots: BTreeSet<&str> = g.fixture.modes.iter().map(|m| m.geometry.as_str()).collect();
        for m in g.fixture.modes.iter().filter(|m| roots.contains(m.geometry.as_str())).take(8) {
            if g.mesh(Some(&m.name), Lod::Default).flatten().is_empty() {
                failures.push(format!("{label}: empty mesh for mode {}", m.name));
            }
        }
        let fps: Vec<String> = modes.iter().take(6).map(|m| m.footprint_break1().to_string()).collect();
        let multi = modes.iter().any(|m| m.footprint.len() > 1);
        let warn = g.warnings.len() + mesh.warnings.len() + modes.iter().map(|m| m.warnings.len()).sum::<usize>();
        let files = mesh.sources.values().filter(|s| matches!(s, MeshSource::File(_))).count();
        println!(
            "{:<44} {:>5} {:>6} {:>5} {:>7} {:>5} {:>4} {:>5} {:>4} {:>16}  {}{}{}  [{} file / {} prim models]",
            short(label, 44),
            g.fixture.modes.len(),
            g.fixture.physical.weight_kg.map(|w| format!("{w:.1}")).unwrap_or("-".into()),
            g.fixture.power_w().map(|w| format!("{w:.0}")).unwrap_or("-".into()),
            flat.triangle_count(),
            mesh.parts.len(),
            mesh.axes().len(),
            mesh.beams().len(),
            warn,
            flat.bounds()
                .map(|(lo, hi)| {
                    let d = hi - lo;
                    format!("{:.0}×{:.0}×{:.0}", d.x, d.y, d.z)
                })
                .unwrap_or_default(),
            fps.join(","),
            if modes.len() > 6 { ",…" } else { "" },
            if multi { " (multi-break)" } else { "" },
            files,
            mesh.sources.len() - files,
        );
        if std::env::var_os("GDTF_WARNINGS").is_some() {
            for w in g.warnings.iter().chain(&mesh.warnings).chain(modes.iter().flat_map(|m| &m.warnings)).take(12) {
                println!("    {w}");
            }
        }
    }
    println!("{} GDTF files, {} failures", all.len(), failures.len());
    assert!(failures.is_empty(), "{failures:#?}");
}
