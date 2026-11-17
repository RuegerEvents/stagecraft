mod common;

use common::{assert_near, save, segments};
use stagecraft_hlr::primitives::{bar, box_truss, cuboid, cylinder, hollow_box, prism};
use stagecraft_hlr::{
    DVec2, DVec3, Line3, LineClass, Material, Mesh, ObjectId, Options, Scene, SectionPlane, View, draw,
};

const ID: ObjectId = ObjectId(1);
const MAT: Material = Material(0);

fn scene(meshes: Vec<Mesh>) -> Scene {
    Scene { meshes, lines: Vec::new() }
}

fn unit_cube() -> Mesh {
    cuboid(ID, MAT, DVec3::ZERO, DVec3::splat(1000.0))
}

#[test]
fn cube_iso_has_nine_visible_and_three_hidden_edges() {
    let d = draw(&scene(vec![unit_cube()]), &View::iso(), &Options::default());
    save("cube_iso", &d);
    let edge = 1000.0 * (2.0f64 / 3.0).sqrt();
    assert_eq!(segments(&d, LineClass::Visible).len(), 9);
    assert_eq!(segments(&d, LineClass::Hidden).len(), 3);
    assert_near(d.length(LineClass::Visible), 9.0 * edge, 1e-6);
    // The last few µm at the hidden corner sit within the depth tolerance.
    assert_near(d.length(LineClass::Hidden), 3.0 * edge, 0.01);
    for (a, b) in segments(&d, LineClass::Visible) {
        assert_near(a.distance(b), edge, 1e-6);
    }
}

#[test]
fn cube_result_does_not_depend_on_sharing_or_winding() {
    let reference = draw(&scene(vec![unit_cube()]), &View::iso(), &Options::default());
    for mesh in [unit_cube().unshared(), unit_cube().flipped(), unit_cube().flipped().unshared()] {
        let d = draw(&scene(vec![mesh]), &View::iso(), &Options::default());
        assert_near(d.length(LineClass::Visible), reference.length(LineClass::Visible), 1e-6);
        assert_near(d.length(LineClass::Hidden), reference.length(LineClass::Hidden), 1e-6);
    }
}

#[test]
fn cube_without_hidden_lines() {
    let opts = Options { hidden_lines: false, ..Options::default() };
    let d = draw(&scene(vec![unit_cube()]), &View::iso(), &opts);
    assert_eq!(segments(&d, LineClass::Visible).len(), 9);
    assert_eq!(d.lines(LineClass::Hidden).count(), 0);
}

#[test]
fn cube_front_view_is_a_square_without_back_edges() {
    let d = draw(&scene(vec![unit_cube()]), &View::front(), &Options::default());
    save("cube_front", &d);
    assert_near(d.length(LineClass::Visible), 4000.0, 1e-6);
    // The back edges lie exactly under the front outline and are dropped.
    assert_eq!(d.lines(LineClass::Hidden).count(), 0);
    let (lo, hi) = d.bounds().unwrap();
    assert!(lo.abs_diff_eq(DVec2::ZERO, 1e-9) && hi.abs_diff_eq(DVec2::splat(1000.0), 1e-9));
}

#[test]
fn overlapping_boxes_hide_the_rear_one_partly() {
    let front = cuboid(ObjectId(1), MAT, DVec3::ZERO, DVec3::splat(1000.0));
    let rear = cuboid(ObjectId(2), MAT, DVec3::splat(500.0), DVec3::splat(1500.0));
    let d = draw(&scene(vec![front, rear]), &View::front(), &Options::default());
    save("boxes_front", &d);
    let by = |id: u64, class: LineClass| -> f64 {
        d.lines(class)
            .filter(|l| l.object.0 == id)
            .map(|l| {
                let mut s: f64 = l.points.windows(2).map(|w| w[0].distance(w[1])).sum();
                if l.closed {
                    s += l.points.last().unwrap().distance(l.points[0]);
                }
                s
            })
            .sum()
    };
    assert_near(by(1, LineClass::Visible), 4000.0, 1e-6);
    assert_near(by(1, LineClass::Hidden), 0.0, 1e-6);
    // Rear: top and right edge fully, bottom and left from 1000 to 1500.
    assert_near(by(2, LineClass::Visible), 3000.0, 1e-6);
    // Inside the front box's square: 500 along the bottom and 500 up the left.
    assert_near(by(2, LineClass::Hidden), 1000.0, 1e-6);
    // The hidden pieces sit at x = 500 and z = 500, between 500 and 1000.
    for (a, b) in segments(&d, LineClass::Hidden) {
        for p in [a, b] {
            assert!((p.x - 500.0).abs() < 1e-6 || (p.y - 500.0).abs() < 1e-6, "{p}");
            assert!(p.x <= 1000.0 + 1e-6 && p.y <= 1000.0 + 1e-6, "{p}");
        }
    }

    let iso = draw(
        &scene(vec![
            cuboid(ObjectId(1), MAT, DVec3::ZERO, DVec3::splat(1000.0)),
            cuboid(ObjectId(2), MAT, DVec3::splat(500.0), DVec3::splat(1500.0)),
        ]),
        &View::iso(),
        &Options::default(),
    );
    save("boxes_iso", &iso);
}

fn l_profile() -> Vec<[f64; 2]> {
    vec![[0.0, 0.0], [1000.0, 0.0], [1000.0, 300.0], [300.0, 300.0], [300.0, 1000.0], [0.0, 1000.0]]
}

/// An L extruded along y from 0 to 500; the profile lies in x/z.
fn l_shape() -> Mesh {
    // prism() puts the profile in its own (u, v) frame; build the L directly instead.
    let mut m = Mesh::new(ID, MAT);
    let p = l_profile();
    let n = p.len() as u32;
    for y in [0.0, 500.0] {
        for q in &p {
            m.positions.push(DVec3::new(q[0], y, q[1]));
        }
    }
    for i in 0..n {
        let j = (i + 1) % n;
        m.triangles.push([i, j, n + j]);
        m.triangles.push([i, n + j, n + i]);
    }
    // Caps as rectangles: [0,1,2,3'] lower leg and [0,3,4,5] upper leg.
    for cap in [[0, 1, 2, 3], [0, 3, 4, 5]] {
        for base in [0, n] {
            m.triangles.push([base + cap[0], base + cap[1], base + cap[2]]);
            m.triangles.push([base + cap[0], base + cap[2], base + cap[3]]);
        }
    }
    m
}

#[test]
fn l_shape_front_side_and_iso() {
    let d = draw(&scene(vec![l_shape()]), &View::front(), &Options::default());
    save("l_front", &d);
    // The outline only: no diagonals from the cap triangulation, no line between legs.
    assert_near(d.length(LineClass::Visible), 4000.0, 1e-6);
    assert_eq!(segments(&d, LineClass::Visible).len(), 6);
    assert_eq!(d.lines(LineClass::Hidden).count(), 0);

    let d = draw(&scene(vec![l_shape()]), &View::side_right(), &Options::default());
    save("l_side", &d);
    // Rectangle 500 × 1000 plus the step edge at z = 300.
    assert_near(d.length(LineClass::Visible), 3500.0, 1e-6);
    assert_eq!(d.lines(LineClass::Hidden).count(), 0);

    let d = draw(&scene(vec![l_shape()]), &View::iso(), &Options::default());
    save("l_iso", &d);
    // 18 edges; the concave edge and the edges behind are partly or fully hidden.
    assert!(d.length(LineClass::Hidden) > 0.0);
}

#[test]
fn cylinder_shows_silhouettes_not_facets() {
    let cyl = cylinder(ID, MAT, DVec3::ZERO, DVec3::new(0.0, 0.0, 600.0), 200.0, 32);
    let d = draw(&scene(vec![cyl.clone()]), &View::front(), &Options::default());
    save("cylinder_front", &d);
    let vertical: Vec<_> =
        segments(&d, LineClass::Visible).into_iter().filter(|(a, b)| (a.x - b.x).abs() < 1e-6).collect();
    assert_eq!(vertical.len(), 2, "two silhouettes: {vertical:?}");
    for (a, b) in &vertical {
        assert_near(a.distance(*b), 600.0, 1e-6);
        assert!((a.x.abs() - 200.0).abs() < 2.0, "silhouette near the radius: {a}");
    }

    let d = draw(&scene(vec![cyl.clone()]), &View::iso(), &Options::default());
    save("cylinder_iso", &d);
    let long: Vec<_> = segments(&d, LineClass::Visible).into_iter().filter(|(a, b)| a.distance(*b) > 100.0).collect();
    assert_eq!(long.len(), 2, "only the two silhouettes are long");

    // A crease threshold below the facet angle (11.25°) shows the facets.
    let opts = Options { crease_angle_deg: 5.0, ..Options::default() };
    let d = draw(&scene(vec![cyl]), &View::front(), &opts);
    save("cylinder_front_facets", &d);
    let vertical = segments(&d, LineClass::Visible).into_iter().filter(|(a, b)| (a.x - b.x).abs() < 1e-6).count();
    assert!(vertical > 10, "facets visible: {vertical}");
}

#[test]
fn plan_view_matches_world_xy() {
    let objects = vec![
        cuboid(ObjectId(1), MAT, DVec3::new(-2000.0, 0.0, 0.0), DVec3::new(2000.0, 3000.0, 600.0)),
        cylinder(ObjectId(2), MAT, DVec3::new(500.0, 1000.0, 600.0), DVec3::new(500.0, 1000.0, 2000.0), 300.0, 48),
    ];
    let d = draw(&scene(objects.clone()), &View::plan(), &Options::default());
    save("plan", &d);
    let (lo, hi) = d.bounds().unwrap();
    assert!(lo.abs_diff_eq(DVec2::new(-2000.0, 0.0), 1e-6), "{lo}");
    assert!(hi.abs_diff_eq(DVec2::new(2000.0, 3000.0), 1e-6), "{hi}");
    let circle = d.lines(LineClass::Visible).find(|l| l.object.0 == 2).unwrap();
    assert!(circle.closed);
    for p in &circle.points {
        assert_near(p.distance(DVec2::new(500.0, 1000.0)), 300.0, 1e-6);
    }

    // Plan cut at 1 m: the column becomes a cut region, the platform stays a view.
    let d = draw(&scene(objects), &View::plan_cut(1000.0), &Options::default());
    save("plan_cut", &d);
    assert_eq!(d.regions.len(), 1);
    let disc = 48.0 / 2.0 * 300.0f64.powi(2) * (std::f64::consts::TAU / 48.0).sin();
    assert_near(d.regions[0].area(), disc, 1e-3);
    assert_eq!(d.regions[0].object, ObjectId(2));
    assert!(d.length(LineClass::Cut) > 1800.0);
}

#[test]
fn section_through_hollow_box_has_a_hole() {
    let hb = hollow_box(ID, Material(3), DVec3::ZERO, DVec3::splat(1000.0), 100.0);
    // Inside the cavity behind the plane: seen through the hole.
    let inside = cuboid(ObjectId(2), MAT, DVec3::new(400.0, 600.0, 400.0), DVec3::new(600.0, 800.0, 600.0));
    // Inside the cavity in front of the plane: cut away.
    let gone = cuboid(ObjectId(3), MAT, DVec3::new(200.0, 150.0, 200.0), DVec3::new(300.0, 250.0, 300.0));
    // Behind the far wall: hidden.
    let behind = cuboid(ObjectId(4), MAT, DVec3::new(400.0, 1200.0, 400.0), DVec3::new(600.0, 1400.0, 600.0));
    let sc = scene(vec![hb, inside, gone, behind]);
    let d = draw(&sc, &View::section_front(500.0), &Options::default());
    save("hollow_section_front", &d);

    assert_eq!(d.regions.len(), 1);
    let r = &d.regions[0];
    assert_eq!(r.holes.len(), 1);
    assert_eq!(r.material, Material(3));
    assert_near(r.area(), 1000.0 * 1000.0 - 800.0 * 800.0, 1e-6);
    assert_near(d.length(LineClass::Cut), 4000.0 + 3200.0, 1e-6);
    let vis = |id: u64| d.lines(LineClass::Visible).filter(|l| l.object.0 == id).count();
    assert!(vis(2) > 0, "the box in the cavity shows through the hole");
    assert_eq!(vis(3), 0, "the box in front of the plane is gone");
    assert_eq!(d.lines(LineClass::Hidden).filter(|l| l.object.0 == 3).count(), 0);
    assert_eq!(vis(4), 0, "the box behind the far wall is hidden");

    // An oblique view into the cut shows the cavity walls.
    let view = View::new(DVec3::new(-0.35, 1.0, -0.45), DVec3::Z)
        .with_section(SectionPlane::new(DVec3::new(0.0, 500.0, 0.0), -DVec3::Y));
    let d = draw(&sc, &view, &Options::default());
    save("hollow_section_oblique", &d);
    assert_eq!(d.regions.len(), 1);
    assert_eq!(d.regions[0].holes.len(), 1);
    assert!(d.lines(LineClass::Visible).any(|l| l.object.0 == 1), "cavity walls visible");
}

#[test]
fn centre_lines_and_explicit_lines() {
    let mut sc = scene(vec![unit_cube()]);
    sc.lines.push(Line3 {
        object: ObjectId(9),
        points: vec![DVec3::new(500.0, -500.0, 500.0), DVec3::new(500.0, 1500.0, 500.0)],
        class: LineClass::Centre,
        occluded: false,
    });
    sc.lines.push(Line3 {
        object: ObjectId(10),
        points: vec![DVec3::new(-500.0, 500.0, 500.0), DVec3::new(1500.0, 500.0, 500.0)],
        class: LineClass::Centre,
        occluded: true,
    });
    let d = draw(&sc, &View::plan(), &Options::default());
    save("centre_lines", &d);
    let len = |id: u64| {
        d.lines(LineClass::Centre)
            .filter(|l| l.object.0 == id)
            .map(|l| l.points.windows(2).map(|w| w[0].distance(w[1])).sum::<f64>())
            .sum::<f64>()
    };
    assert_near(len(9), 2000.0, 1e-6);
    // Through the middle of the cube: only the parts outside it remain.
    assert_near(len(10), 1000.0, 1e-6);
}

#[test]
fn bars_meeting_at_a_node_are_clean() {
    // Two crossing bars: no hidden pieces leaking through either.
    let a = bar(ObjectId(1), MAT, DVec3::new(-1000.0, 0.0, 0.0), DVec3::new(1000.0, 0.0, 0.0), 100.0);
    let b = bar(ObjectId(2), MAT, DVec3::new(0.0, 0.0, -1000.0), DVec3::new(0.0, 0.0, 1000.0), 60.0);
    let d = draw(&scene(vec![a, b]), &View::front(), &Options::default());
    save("bars_cross", &d);
    // The vertical bar's outline is interrupted where it passes through the horizontal
    // one (it is inside it, not in front).
    for (p, q) in segments(&d, LineClass::Visible).into_iter().filter(|(p, q)| (p.x - q.x).abs() < 1e-6) {
        let (lo, hi) = (p.y.min(q.y), p.y.max(q.y));
        assert!(hi <= -50.0 + 1e-6 || lo >= 50.0 - 1e-6 || (p.x.abs() - 1000.0).abs() < 1e-6, "{p} {q}");
    }
}

#[test]
fn prism_with_concave_profile() {
    let m = prism(ID, MAT, DVec3::ZERO, DVec3::new(0.0, 500.0, 0.0), &l_profile());
    let d = draw(&scene(vec![m]), &View::front(), &Options::default());
    assert_eq!(segments(&d, LineClass::Visible).len(), 6);
}

#[test]
fn truss_lattice_runs_quickly() {
    let mut meshes = Vec::new();
    for i in 0..6 {
        meshes.push(box_truss(
            ObjectId(i),
            MAT,
            DVec3::new(-6000.0, i as f64 * 1500.0, 6000.0),
            12000.0,
            290.0,
            25.0,
            16,
        ));
    }
    let tris: usize = meshes.iter().map(|m| m.triangles.len()).sum();
    let sc = scene(meshes);
    for (name, view) in [("truss_front", View::front()), ("truss_iso", View::iso()), ("truss_plan", View::plan())] {
        let t = std::time::Instant::now();
        let d = draw(&sc, &view, &Options::default());
        eprintln!("{name}: {tris} triangles in {:?}, {} polylines", t.elapsed(), d.polylines.len());
        save(name, &d);
        assert!(d.length(LineClass::Visible) > 0.0);
    }
}

#[test]
fn truss_detail_views() {
    let t = box_truss(ID, MAT, DVec3::ZERO, 1200.0, 290.0, 25.0, 16);
    for (name, view) in [
        ("truss_detail_iso", View::iso()),
        ("truss_detail_front", View::front()),
        ("truss_detail_side", View::side_right()),
    ] {
        let d = draw(&scene(vec![t.clone()]), &view, &Options::default());
        save(name, &d);
        eprintln!(
            "{name}: {} polylines, {} visible segments",
            d.polylines.len(),
            segments(&d, LineClass::Visible).len()
        );
    }
}

#[test]
fn empty_scene_gives_empty_drawing() {
    let d = draw(&Scene::default(), &View::iso(), &Options::default());
    assert!(d.polylines.is_empty() && d.regions.is_empty());
    assert!(d.bounds().is_none());
}

#[test]
fn open_sheet_is_cut_as_a_line_without_fill() {
    // A curtain: a vertical sheet in x/z at y = 1000, two triangles, open.
    let curtain = Mesh {
        object: ObjectId(5),
        material: MAT,
        positions: vec![
            DVec3::new(-2000.0, 1000.0, 0.0),
            DVec3::new(2000.0, 1000.0, 0.0),
            DVec3::new(2000.0, 1000.0, 4000.0),
            DVec3::new(-2000.0, 1000.0, 4000.0),
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
    };
    let d = draw(&scene(vec![curtain.clone()]), &View::plan_cut(2000.0), &Options::default());
    save("curtain_plan_cut", &d);
    assert!(d.regions.is_empty());
    assert_near(d.length(LineClass::Cut), 4000.0, 1e-6);

    // Seen from the front it is a rectangle (boundary edges), no diagonal.
    let d = draw(&scene(vec![curtain]), &View::front(), &Options::default());
    assert_eq!(segments(&d, LineClass::Visible).len(), 4);
}

#[test]
fn plane_on_a_face_keeps_the_face() {
    // Plan cut exactly at the top of a 600 mm platform with a column on it.
    let objects = vec![
        cuboid(ObjectId(1), MAT, DVec3::new(0.0, 0.0, 0.0), DVec3::new(2000.0, 1000.0, 600.0)),
        cuboid(ObjectId(2), MAT, DVec3::new(900.0, 400.0, 600.0), DVec3::new(1100.0, 600.0, 2000.0)),
    ];
    let d = draw(&scene(objects), &View::plan_cut(600.0), &Options::default());
    save("plane_on_face", &d);
    // The column's footprint is a cut region; the platform top stays a plain view.
    assert_eq!(d.regions.len(), 1);
    assert_eq!(d.regions[0].object, ObjectId(2));
    assert_near(d.regions[0].area(), 200.0 * 200.0, 1e-6);
    let platform: f64 = d
        .lines(LineClass::Visible)
        .filter(|l| l.object.0 == 1)
        .map(|l| {
            let mut s: f64 = l.points.windows(2).map(|w| w[0].distance(w[1])).sum();
            if l.closed {
                s += l.points.last().unwrap().distance(l.points[0]);
            }
            s
        })
        .sum();
    assert_near(platform, 6000.0, 1e-6);
}

#[test]
fn penetrating_parts_draw_their_intersection() {
    let block = cuboid(ObjectId(1), MAT, DVec3::ZERO, DVec3::splat(1000.0));
    let post = cuboid(ObjectId(2), MAT, DVec3::new(450.0, 450.0, 500.0), DVec3::new(550.0, 550.0, 1500.0));
    let sc = scene(vec![block, post]);
    let with = draw(&sc, &View::iso(), &Options::default());
    save("intersection_iso", &with);
    let without = draw(&sc, &View::iso(), &Options { intersections: false, ..Options::default() });
    // The two front sides of the post's footprint on the top face.
    let edge = 100.0 * (2.0f64 / 3.0).sqrt();
    assert_near(with.length(LineClass::Visible) - without.length(LineClass::Visible), 2.0 * edge, 1e-6);

    // In plan the footprint lies under the post's outline and adds nothing.
    let with = draw(&sc, &View::plan(), &Options::default());
    let without = draw(&sc, &View::plan(), &Options { intersections: false, ..Options::default() });
    assert_near(with.length(LineClass::Visible), without.length(LineClass::Visible), 1e-6);

    // Separate components of one mesh intersect too.
    let mut one = cuboid(ID, MAT, DVec3::ZERO, DVec3::splat(1000.0));
    one.append(&cuboid(ID, MAT, DVec3::new(450.0, 450.0, 500.0), DVec3::new(550.0, 550.0, 1500.0)));
    let d = draw(&scene(vec![one]), &View::iso(), &Options::default());
    assert_near(
        d.length(LineClass::Visible),
        draw(&sc, &View::iso(), &Options::default()).length(LineClass::Visible),
        1e-6,
    );
}
