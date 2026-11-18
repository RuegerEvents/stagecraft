//! Frame times on the Hall A document (`testdata/private/hall_a/
//! hall_a.stagecraft`, private; the tests skip without it): zooming and panning
//! the plan, and clicking and dragging a sketch point, must stay interactive.
//!
//! The budgets hold for release builds (`cargo test --release -p stagecraft-ui --test
//! perf -- --nocapture` prints the numbers); debug builds only report them.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Event, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect, vec2};
use stagecraft_core::Engine;
use stagecraft_core::sketch::{PointId, SolveOptions};
use stagecraft_ui::Studio;
use stagecraft_ui::sketch_ui::SketchItem;

fn document() -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/private/hall_a/hall_a.stagecraft");
    if p.exists() {
        Some(p)
    } else {
        eprintln!("skipped: {} is missing", p.display());
        None
    }
}

/// A window with the studio in it, driven frame by frame like eframe does: input,
/// `run_ui`, then tessellation (part of every real frame's CPU time).
struct Window {
    ctx: egui::Context,
    studio: Studio,
    time: f64,
    pointer: Pos2,
}

#[derive(Clone, Copy, Debug, Default)]
struct Frame {
    run: Duration,
    tessellate: Duration,
}

impl Frame {
    fn total(&self) -> Duration {
        self.run + self.tessellate
    }
}

impl Window {
    fn open(path: &PathBuf) -> Self {
        let engine = Engine::open(path).expect("opens");
        // `STAGECRAFT_PERF_PREFS=<dir>`: a settings folder (a copy of a user's), for their layout.
        let studio = match std::env::var("STAGECRAFT_PERF_PREFS") {
            Ok(dir) => Studio::with_prefs(
                engine,
                craft_settings::SettingsStore::open(craft_settings::Location::Dir(dir.into())).0,
            ),
            Err(_) => Studio::new(engine),
        };
        let mut w = Self { ctx: egui::Context::default(), studio, time: 0.0, pointer: Pos2::ZERO };
        for _ in 0..6 {
            w.frame(Vec::new());
        }
        w.pointer = w.studio.ui.camera.rect.center();
        eprintln!("{} objects, canvas {:?}", w.studio.engine.doc().objects.len(), w.studio.ui.camera.rect);
        w.frame(vec![Event::PointerMoved(w.pointer)]);
        w
    }

    fn frame(&mut self, events: Vec<Event>) -> Frame {
        self.time += 1.0 / 60.0;
        let mut input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0))),
            time: Some(self.time),
            // What wgpu offers on a Mac (egui's default of 2048 fills with big text).
            max_texture_side: Some(8192),
            events,
            ..Default::default()
        };
        // A Retina screen: glyphs are rasterised at twice the size.
        input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(2.0);
        let studio = &mut self.studio;
        let t0 = Instant::now();
        let mut out = self.ctx.run_ui(input, |ui| studio.show(ui));
        // No GPU here: the font atlas uploads go nowhere.
        out.textures_delta.clear();
        let run = t0.elapsed();
        let t1 = Instant::now();
        let _ = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        Frame { run, tessellate: t1.elapsed() }
    }

    fn wheel(&mut self, unit: MouseWheelUnit, delta: egui::Vec2, modifiers: Modifiers) -> Frame {
        let source = if unit == MouseWheelUnit::Line { MouseWheelSource::Wheel } else { MouseWheelSource::Trackpad };
        let e = Event::MouseWheel { unit, delta, modifiers, phase: egui::TouchPhase::Move, source };
        self.frame(vec![Event::PointerMoved(self.pointer), e])
    }

    fn press(&mut self, pos: Pos2, pressed: bool) -> Frame {
        self.pointer = pos;
        let e = Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.frame(vec![Event::PointerMoved(pos), e])
    }

    fn move_to(&mut self, pos: Pos2) -> Frame {
        self.pointer = pos;
        self.frame(vec![Event::PointerMoved(pos)])
    }
}

fn report(name: &str, frames: &[Frame]) -> Duration {
    let n = frames.len().max(1) as u32;
    let avg = frames.iter().map(Frame::total).sum::<Duration>() / n;
    let max = frames.iter().map(Frame::total).max().unwrap_or_default();
    let run = frames.iter().map(|f| f.run).sum::<Duration>() / n;
    let tess = frames.iter().map(|f| f.tessellate).sum::<Duration>() / n;
    eprintln!("{name:<28} avg {avg:>10.2?} (ui {run:>9.2?}, tessellate {tess:>9.2?})  max {max:>10.2?}");
    avg
}

fn release() -> bool {
    !cfg!(debug_assertions)
}

/// Pinch frames: `n` in by `f` a frame, then `n` out.
fn pinch(w: &mut Window, n: usize, f: f32) -> Vec<Frame> {
    (0..2 * n)
        .map(|i| {
            let z = if i < n { f } else { 1.0 / f };
            let p = w.pointer;
            w.frame(vec![Event::PointerMoved(p), Event::Zoom(z)])
        })
        .collect()
}

fn pan(w: &mut Window) -> Vec<Frame> {
    (0..60)
        .map(|i| w.wheel(MouseWheelUnit::Point, vec2(if i % 20 < 10 { 8.0 } else { -8.0 }, 3.0), Modifiers::NONE))
        .collect()
}

#[test]
fn zooming_the_hall_a_plan_is_as_cheap_as_panning() {
    let Some(path) = document() else { return };
    let mut w = Window::open(&path);
    let still: Vec<Frame> = (0..20).map(|_| w.frame(Vec::new())).collect();
    let pan_fit = pan(&mut w);
    // `STAGECRAFT_PERF_REPEAT=n` repeats the pinch n times, for a profiler.
    let repeat: usize = std::env::var("STAGECRAFT_PERF_REPEAT").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    // Trackpad pinch in small steps: ×5.9 in and out again.
    let zoom = pinch(&mut w, 60, 1.03);
    // Mouse wheel notches.
    let wheel: Vec<Frame> = (0..24)
        .map(|i| w.wheel(MouseWheelUnit::Line, vec2(0.0, if i < 12 { 1.0 } else { -1.0 }), Modifiers::NONE))
        .collect();
    // Close up on the stage front, where the notes, seats and dimensions are big.
    let (_, _, at) = front_point(&w);
    look_at(&mut w, at, 0.25);
    let pan_close: Vec<Frame> = (0..repeat).flat_map(|_| pan(&mut w)).collect();
    let zoom_close = pinch(&mut w, 40, 1.04);
    // The zoom settles: one frame lays the text out at its own size again.
    let settle: Vec<Frame> = (0..30).map(|_| w.frame(Vec::new())).collect();
    report("still", &still);
    let p = report("pan", &pan_fit);
    let z = report("pinch zoom", &zoom);
    let m = report("wheel zoom", &wheel);
    let pc = report("pan, close up", &pan_close);
    let zc = report("pinch zoom, close up", &zoom_close);
    report("settle", &settle);
    if release() {
        for (name, t) in [("zoom", z), ("wheel zoom", m), ("close-up zoom", zc)] {
            assert!(t < Duration::from_millis(16), "{name} frames take {t:?}");
        }
        assert!(z.as_secs_f64() < p.as_secs_f64() * 2.0 + 0.002, "zoom {z:?} costs much more than pan {p:?}");
        assert!(zc.as_secs_f64() < pc.as_secs_f64() * 2.0 + 0.002, "zoom {zc:?} costs much more than pan {pc:?}");
    }
}

/// The downstage-most sketch point (the middle of the stage front, which juts into the
/// hall): layer, point and world position.
fn front_point(w: &Window) -> (stagecraft_core::Id, PointId, [f64; 2]) {
    let doc = w.studio.engine.doc();
    let mut best: Option<(stagecraft_core::Id, PointId, [f64; 2])> = None;
    for (l, s) in &doc.sketches {
        for p in &s.points {
            if best.is_none_or(|b| p.pos.y < b.2[1]) {
                best = Some((*l, p.id, [p.pos.x, p.pos.y]));
            }
        }
    }
    best.expect("a sketch point")
}

/// The document's sketch points with their world positions: all in release builds, a
/// few in debug builds (unoptimised frames are slow, and there the test only reports).
fn sketch_points(w: &Window) -> Vec<(PointId, [f64; 2])> {
    let doc = w.studio.engine.doc();
    let all = doc.sketches.values().flat_map(|s| s.points.iter().map(|p| (p.id, [p.pos.x, p.pos.y])));
    all.take(if release() { usize::MAX } else { 4 }).collect()
}

/// Centre the plan on `at` at `scale` and put the pointer there (twice, so egui
/// knows it hovers).
fn look_at(w: &mut Window, at: [f64; 2], scale: f64) -> Pos2 {
    w.studio.ui.camera.scale = scale;
    w.studio.ui.camera.center = at;
    w.frame(Vec::new());
    let pos = w.studio.ui.camera.to_screen(at);
    w.move_to(pos);
    w.move_to(pos);
    pos
}

/// Frames until the next click is not a double click (half a second).
fn pause(w: &mut Window) {
    for _ in 0..30 {
        w.frame(Vec::new());
    }
}

#[test]
fn clicking_hall_a_sketch_points_is_instant() {
    let Some(path) = document() else { return };
    let mut w = Window::open(&path);
    // The point the user clicked first: the stage front's middle, close up (at the
    // overview a dimension's value sits on it, and a click picks the dimension).
    let (_, front, at) = front_point(&w);
    let pos = look_at(&mut w, at, 0.05);
    let t = Instant::now();
    w.press(pos, true);
    w.press(pos, false);
    let click = t.elapsed();
    let after: Vec<Frame> = (0..5).map(|_| w.frame(Vec::new())).collect();
    eprintln!("click on the stage front point {click:.2?}");
    report("frames after the click", &after);
    assert_eq!(w.studio.ui.sketch.picked, vec![SketchItem::Point(front)], "the click picks the point");
    // Then every other point, at two zooms.
    let mut worst = (Duration::ZERO, None);
    for scale in [0.05, 0.2] {
        for (id, at) in sketch_points(&w) {
            pause(&mut w);
            let pos = look_at(&mut w, at, scale);
            let t = Instant::now();
            w.press(pos, true);
            w.press(pos, false);
            w.frame(Vec::new());
            let d = t.elapsed();
            if d > worst.0 {
                worst = (d, Some(id));
            }
        }
    }
    eprintln!("worst click with its next frame {:.2?} at {:?}", worst.0, worst.1);
    if release() {
        assert!(click < Duration::from_millis(16), "the click took {click:?}");
        assert!(worst.0 < Duration::from_millis(50), "a click took {:?}", worst.0);
    }
}

#[test]
fn dragging_hall_a_sketch_points_stays_responsive() {
    let Some(path) = document() else { return };
    let mut w = Window::open(&path);
    let mut frames = Vec::new();
    let mut drops = Vec::new();
    let mut points = 0;
    for (_, at) in sketch_points(&w) {
        pause(&mut w);
        let pos = look_at(&mut w, at, 0.05);
        w.press(pos, true);
        // A click with a little hand jitter becomes a drag; then a real one.
        for i in 1..=12 {
            frames.push(w.move_to(pos + vec2(i as f32 * 1.5, 0.0)));
        }
        if matches!(w.studio.ui.gesture.drag, Some(stagecraft_ui::state::Drag::SketchPoint { .. })) {
            points += 1;
        }
        let t = Instant::now();
        w.press(w.pointer, false);
        w.frame(Vec::new());
        drops.push(t.elapsed());
        let _ = w.studio.run("undo", serde_json::Value::Null);
    }
    let avg = report("drag frames", &frames);
    let max = frames.iter().map(Frame::total).max().unwrap_or_default();
    let drop = drops.iter().max().copied().unwrap_or_default();
    eprintln!("{points} sketch points dragged, worst drop (sketch.drag + frame) {drop:.2?}");
    assert!(points > 0, "the drags grab sketch points");
    if release() {
        assert!(avg < Duration::from_millis(16), "drag frames take {avg:?}");
        assert!(max < Duration::from_millis(33), "a drag frame took {max:?}");
        assert!(drop < Duration::from_millis(50), "a drop took {drop:?}");
    }
}

/// The solver work behind clicks and drags, without the GUI: analysis and a drag solve
/// for every point.
#[test]
fn hall_a_sketch_solves_are_quick() {
    let Some(path) = document() else { return };
    let engine = Engine::open(&path).expect("opens");
    for (l, s) in &engine.doc().sketches {
        let t = Instant::now();
        let ok = s.analyse().is_ok();
        let analyse = t.elapsed();
        let mut worst = Duration::ZERO;
        for p in &s.points {
            let t = Instant::now();
            let _ = s.drag(p.id, stagecraft_core::sketch::v2([p.pos.x + 500.0, p.pos.y]), &SolveOptions::drag());
            worst = worst.max(t.elapsed());
        }
        eprintln!(
            "sketch on {l}: {} points, {} edges, {} constraints, {} dimensions; analyse {analyse:.2?} (ok {ok}), worst drag solve {worst:.2?}",
            s.points.len(),
            s.edges.len(),
            s.constraints.len(),
            s.dimensions.len()
        );
        if release() {
            assert!(analyse < Duration::from_millis(50) && worst < Duration::from_millis(16));
        }
    }
}

/// Average and worst frame.
fn stats(frames: &[Frame]) -> (Duration, Duration) {
    let n = frames.len().max(1) as u32;
    (frames.iter().map(Frame::total).sum::<Duration>() / n, frames.iter().map(Frame::total).max().unwrap_or_default())
}

/// The Sheets mode on sheet `number`, fitted, its page composed.
fn open_sheet(path: &PathBuf, number: &str) -> Window {
    let mut w = Window::open(path);
    w.studio.run("ui.set_view", serde_json::json!({ "view": "sheets" })).expect("sheets mode");
    w.studio.run("ui.sheet", serde_json::json!({ "sheet": number, "fit": true })).expect("the sheet");
    for _ in 0..6 {
        w.frame(Vec::new());
    }
    // Composing runs on a worker; wait until the page is there and current.
    let t = Instant::now();
    while stagecraft_ui::sheets::is_composing(&w.studio) && t.elapsed() < Duration::from_secs(120) {
        w.frame(Vec::new());
    }
    w.pointer = w.studio.ui.sheets.cam.rect.center();
    w.move_to(w.pointer);
    w.move_to(w.pointer);
    w
}

/// Zooming and panning sheet `number`: at the fit, and close up on its first viewport.
fn sheet_navigation(number: &str) {
    let Some(path) = document() else { return };
    let mut w = open_sheet(&path, number);
    let page = stagecraft_ui::sheets::page(&mut w.studio).expect("a page");
    eprintln!("sheet {number}: {} primitives, {} viewports", count_prims(&page.prims), page.viewports.len());
    let still: Vec<Frame> = (0..20).map(|_| w.frame(Vec::new())).collect();
    let pan_fit = pan(&mut w);
    let zoom = pinch(&mut w, 60, 1.03);
    let wheel: Vec<Frame> = (0..24)
        .map(|i| w.wheel(MouseWheelUnit::Line, vec2(0.0, if i < 12 { 1.0 } else { -1.0 }), Modifiers::NONE))
        .collect();
    // Close up: the middle of the first viewport at 8 points per paper mm.
    let h = &page.viewports[0];
    w.studio.ui.sheets.cam.center = [(h.min[0] + h.max[0]) / 2.0, (h.min[1] + h.max[1]) / 2.0];
    w.studio.ui.sheets.cam.scale = 8.0;
    w.frame(Vec::new());
    w.move_to(w.studio.ui.sheets.cam.rect.center());
    let pan_close = pan(&mut w);
    let zoom_close = pinch(&mut w, 40, 1.04);
    let settle: Vec<Frame> = (0..30).map(|_| w.frame(Vec::new())).collect();
    report("still", &still);
    let p = report("pan", &pan_fit);
    let z = report("pinch zoom", &zoom);
    let m = report("wheel zoom", &wheel);
    let pc = report("pan, close up", &pan_close);
    let zc = report("pinch zoom, close up", &zoom_close);
    report("settle", &settle);
    if release() {
        for (name, f) in [
            ("pan", &pan_fit),
            ("zoom", &zoom),
            ("wheel zoom", &wheel),
            ("close-up pan", &pan_close),
            ("close-up zoom", &zoom_close),
        ] {
            let (avg, max) = stats(f);
            assert!(avg < Duration::from_millis(16), "{number}: {name} frames take {avg:?}");
            assert!(max < Duration::from_millis(50), "{number}: a {name} frame took {max:?}");
        }
        assert!(z.as_secs_f64() < p.as_secs_f64() * 2.0 + 0.002, "{number}: zoom {z:?} costs much more than pan {p:?}");
        assert!(
            m.as_secs_f64() < p.as_secs_f64() * 2.0 + 0.002,
            "{number}: wheel {m:?} costs much more than pan {p:?}"
        );
        assert!(
            zc.as_secs_f64() < pc.as_secs_f64() * 2.0 + 0.002,
            "{number}: zoom {zc:?} costs much more than pan {pc:?}"
        );
    }
}

fn count_prims(prims: &[stagecraft_sheet::Prim]) -> usize {
    prims
        .iter()
        .map(|p| if let stagecraft_sheet::Prim::Clip { prims, .. } = p { 1 + count_prims(prims) } else { 1 })
        .sum()
}

#[test]
fn zooming_the_hall_a_sheet_lb1_is_cheap() {
    sheet_navigation("LB-1");
}

#[test]
fn zooming_the_hall_a_sheet_lbr_is_cheap() {
    sheet_navigation("LB-R");
}

/// Dragging a viewport on LB-1: the live preview draws it where it goes, every frame.
#[test]
fn dragging_a_hall_a_viewport_stays_responsive() {
    let Some(path) = document() else { return };
    let mut w = open_sheet(&path, "LB-1");
    let page = stagecraft_ui::sheets::page(&mut w.studio).expect("a page");
    let h = page.viewports.iter().max_by(|a, b| {
        let area = |h: &stagecraft_sheet::prim::ViewportHit| (h.max[0] - h.min[0]) * (h.max[1] - h.min[1]);
        area(a).total_cmp(&area(b))
    });
    let h = h.expect("a viewport").clone();
    let pos = w.studio.ui.sheets.cam.to_screen([(h.min[0] + h.max[0]) / 2.0, (h.min[1] + h.max[1]) / 2.0]);
    w.move_to(pos);
    w.press(pos, true);
    let frames: Vec<Frame> = (1..=60).map(|i| w.move_to(pos + vec2(i as f32 * 2.0, i as f32))).collect();
    let live = w.studio.ui.sheets.live.clone().expect("a live preview while dragging");
    assert_eq!(live.viewport, h.id);
    assert!(live.moved && live.min[0] > h.min[0], "the preview follows the pointer");
    w.press(w.pointer, false);
    let (avg, max) = stats(&frames);
    report("viewport drag", &frames);
    if release() {
        assert!(avg < Duration::from_millis(16), "drag frames take {avg:?}");
        assert!(max < Duration::from_millis(50), "a drag frame took {max:?}");
    }
}
