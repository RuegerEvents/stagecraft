//! `cargo test --release --test bench -- --ignored --nocapture` (add `--features parallel`).

mod common;

use stagecraft_hlr::primitives::{box_truss, cuboid};
use stagecraft_hlr::{DVec3, LineClass, Material, ObjectId, Options, Scene, View, draw};

/// About 300k triangles: 64 runs of 12 m box truss (16-sided chords, 8-sided
/// diagonals) on 8 lines and 8 heights, plus a stage deck.
fn rig() -> Scene {
    let mut meshes = Vec::new();
    let mut id = 0;
    for j in 0..8 {
        for k in 0..8 {
            id += 1;
            meshes.push(box_truss(
                ObjectId(id),
                Material(0),
                DVec3::new(-6000.0, j as f64 * 1200.0, 3000.0 + k as f64 * 900.0),
                12000.0,
                290.0,
                25.0,
                16,
            ));
        }
    }
    meshes.push(cuboid(
        ObjectId(0),
        Material(1),
        DVec3::new(-8000.0, -1000.0, -1000.0),
        DVec3::new(8000.0, 10000.0, 0.0),
    ));
    Scene { meshes, lines: Vec::new() }
}

#[test]
#[ignore]
fn bench_300k_triangles() {
    let sc = rig();
    let tris: usize = sc.meshes.iter().map(|m| m.triangles.len()).sum();
    eprintln!("scene: {tris} triangles, parallel = {}", cfg!(feature = "parallel"));
    for hidden in [true, false] {
        let opts = Options { hidden_lines: hidden, ..Options::default() };
        for (name, view) in [
            ("front", View::front()),
            ("iso", View::iso()),
            ("plan", View::plan()),
            ("section", View::section_front(4000.0)),
        ] {
            let t = std::time::Instant::now();
            let d = draw(&sc, &view, &opts);
            let segs: usize = d.polylines.iter().map(|p| p.points.len()).sum();
            eprintln!(
                "{name:8} hidden={hidden:5}: {:>8.0} ms, {} polylines, {segs} points, visible {:.0} m",
                t.elapsed().as_secs_f64() * 1000.0,
                d.polylines.len(),
                d.length(LineClass::Visible) / 1000.0
            );
            if hidden && name == "iso" {
                common::save("bench_iso", &d);
            }
        }
    }
}
