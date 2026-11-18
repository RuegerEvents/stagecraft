//! Zooming shared by the plan canvas, the sheets and the 3D view: the zoom factor an
//! input asks for, whether a view is mid-zoom, and text that stays cheap while it is
//! (glyphs from a few sizes, scaled; never rasterised huge; skipped off screen).

use std::sync::Arc;

use egui::{Color32, Event, FontId, Galley, MouseWheelUnit, Painter, Pos2, Rect};

use crate::theme;

/// Zoom per mouse wheel notch (one line of scrolling): about what CAD programs use.
pub const WHEEL_ZOOM: f64 = 1.25;
/// Zoom per point of ⌘- or Ctrl-scrolling on a trackpad: e-fold per 100 points.
pub const SCROLL_ZOOM: f64 = 0.01;
/// Trackpad pinch: the platform's magnification raised to this power, so a pinch
/// covers a plan's range of scales in a gesture or two.
pub const PINCH_GAIN: f64 = 1.5;
/// The most one frame zooms by (fast free-spinning wheels).
const MAX_FRAME_ZOOM: f64 = 4.0;

/// The zoom factor this frame's input asks for: wheel notches, ⌘/Ctrl-scroll on a
/// trackpad and pinch, proportional to their deltas and applied as they arrive (no
/// smoothing, which lags behind the fingers). Plain trackpad scrolling is not zoom.
pub fn zoom_factor(events: &[Event]) -> f64 {
    let mut f = 1.0f64;
    for e in events {
        match e {
            Event::MouseWheel { unit, delta, modifiers, .. } => {
                let dy = delta.y as f64;
                match unit {
                    MouseWheelUnit::Line => f *= WHEEL_ZOOM.powf(dy),
                    MouseWheelUnit::Page => f *= WHEEL_ZOOM.powf(dy * 3.0),
                    MouseWheelUnit::Point if modifiers.command || modifiers.ctrl => f *= (dy * SCROLL_ZOOM).exp(),
                    MouseWheelUnit::Point => {}
                }
            }
            Event::Zoom(z) if *z > 0.0 => f *= (*z as f64).powf(PINCH_GAIN),
            _ => {}
        }
    }
    f.clamp(1.0 / MAX_FRAME_ZOOM, MAX_FRAME_ZOOM)
}

/// How long after the last zoom step text is laid out at its own size again.
const ZOOM_SETTLE: f64 = 0.25;

fn zoom_key() -> egui::Id {
    egui::Id::unique("view_zoomed_at")
}

/// Remember when a view (plan or sheet) last zoomed, and ask for the frame after it
/// settles (text laid out at its exact size again). Call once a frame from the view's
/// navigation, with whether its scale changed.
pub fn note_zoom(ctx: &egui::Context, zoomed: bool) {
    let now = ctx.input(|i| i.time);
    if zoomed {
        ctx.data_mut(|d| d.insert_temp(zoom_key(), now));
    }
    if let Some(t) = ctx.data(|d| d.get_temp::<f64>(zoom_key())) {
        let left = t + ZOOM_SETTLE - now;
        if left > 0.0 {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(left));
        }
    }
}

/// Whether a view is being zoomed (a zoom step in the last [`ZOOM_SETTLE`] seconds).
pub fn zooming(ctx: &egui::Context) -> bool {
    let now = ctx.input(|i| i.time);
    ctx.data(|d| d.get_temp::<f64>(zoom_key())).is_some_and(|t| now - t < ZOOM_SETTLE)
}

/// The text size glyphs are rasterised at while zooming: the next half octave at or
/// above `px`, so a zoom gesture meets only a few sizes and finds them in egui's glyph
/// atlas.
pub fn ladder_size(px: f32) -> f32 {
    2f32.powf((px.max(0.5).log2() * 2.0 - 1e-4).ceil() / 2.0).min(MAX_TEXT_PX)
}

/// Text is never rasterised bigger than this (points); larger text is the glyphs
/// of this size scaled up. A note zoomed to half a metre on screen would otherwise put
/// glyphs of a thousand pixels into egui's atlas, which fills and is rebuilt.
pub const MAX_TEXT_PX: f32 = 96.0;

/// Text laid out at `px` points in `font(size)`. Rasterising glyphs at a new size is
/// what made zooming slow (every frame of a zoom met new sizes for every note, label and
/// sheet text): while the view zooms, the text is laid out at [`ladder_size`] and its
/// glyph quads are scaled down to `px`; once the zoom settles it is laid out at its own
/// size, crisp.
pub fn galley(
    p: &Painter,
    text: String,
    px: f32,
    colour: Color32,
    zooming: bool,
    font: impl Fn(f32) -> FontId,
) -> Arc<Galley> {
    if !zooming && px <= MAX_TEXT_PX {
        return p.layout_no_wrap(text, font(px), colour);
    }
    let q = if zooming { ladder_size(px) } else { MAX_TEXT_PX };
    let g = p.layout_no_wrap(text, font(q), colour);
    scale_galley(&g, px / q)
}

/// Plan text (Inter) laid out at `px` points; see [`galley`].
pub fn plan_galley(p: &Painter, text: String, px: f32, colour: Color32, zooming: bool) -> Arc<Galley> {
    galley(p, text, px, colour, zooming, theme::proportional)
}

/// A galley with its glyph quads (and rows, bounds) scaled by `k` about its origin. The
/// texture coordinates stay, so the glyphs come from the atlas at the size laid out.
pub fn scale_galley(g: &Galley, k: f32) -> Arc<Galley> {
    let rect = |r: Rect| Rect::from_min_max((r.min.to_vec2() * k).to_pos2(), (r.max.to_vec2() * k).to_pos2());
    let mut g = g.clone();
    for placed in &mut g.rows {
        placed.pos = (placed.pos.to_vec2() * k).to_pos2();
        let row = Arc::make_mut(&mut placed.row);
        for v in &mut row.visuals.mesh.vertices {
            v.pos = (v.pos.to_vec2() * k).to_pos2();
        }
        row.visuals.mesh_bounds = rect(row.visuals.mesh_bounds);
        row.size *= k;
        for glyph in &mut row.glyphs {
            glyph.pos = (glyph.pos.to_vec2() * k).to_pos2();
            glyph.advance_width *= k;
            glyph.line_height *= k;
        }
    }
    g.rect = rect(g.rect);
    g.mesh_bounds = rect(g.mesh_bounds);
    Arc::new(g)
}

/// Whether text at `at` (its anchor, screen) of `px` points can reach `clip`, judged
/// before laying it out: a circle around the anchor as wide as the longest line could
/// be (any rotation). Zoomed in, most text is off screen; laying it out is what costs.
pub fn text_may_show(clip: Rect, at: Pos2, text: &str, px: f32) -> bool {
    let longest = text.lines().map(|l| l.chars().count()).max().unwrap_or(0) as f32;
    let lines = text.lines().count().max(1) as f32;
    let r = longest * px * 0.7 + lines * px * 1.4 + 4.0;
    clip.expand(r).contains(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wheel(unit: MouseWheelUnit, dy: f32, modifiers: egui::Modifiers) -> Event {
        Event::MouseWheel {
            unit,
            delta: egui::vec2(0.0, dy),
            modifiers,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
        }
    }

    #[test]
    fn zoom_follows_the_deltas() {
        let none = egui::Modifiers::NONE;
        let f = zoom_factor(&[wheel(MouseWheelUnit::Line, 1.0, none)]);
        assert!((f - WHEEL_ZOOM).abs() < 1e-9);
        // Two notches in one frame zoom twice as far (in log), half a notch half.
        let two = zoom_factor(&[wheel(MouseWheelUnit::Line, 1.0, none), wheel(MouseWheelUnit::Line, 1.0, none)]);
        assert!((two - WHEEL_ZOOM * WHEEL_ZOOM).abs() < 1e-9);
        assert!((zoom_factor(&[wheel(MouseWheelUnit::Line, 0.5, none)]) - WHEEL_ZOOM.sqrt()).abs() < 1e-9);
        // Notches out undo notches in.
        let back = zoom_factor(&[wheel(MouseWheelUnit::Line, 1.0, none), wheel(MouseWheelUnit::Line, -1.0, none)]);
        assert!((back - 1.0).abs() < 1e-9);
        // Trackpad scrolling pans; with ⌘ it zooms by its points.
        assert_eq!(zoom_factor(&[wheel(MouseWheelUnit::Point, 40.0, none)]), 1.0);
        let cmd = zoom_factor(&[wheel(MouseWheelUnit::Point, 40.0, egui::Modifiers::COMMAND)]);
        assert!((cmd - (40.0 * SCROLL_ZOOM).exp()).abs() < 1e-9);
        // Pinch is amplified, and a free-spinning wheel is capped.
        let pinch = zoom_factor(&[Event::Zoom(1.1)]);
        assert!(pinch > 1.1 && (pinch - 1.1f64.powf(PINCH_GAIN)).abs() < 1e-6);
        assert_eq!(zoom_factor(&[wheel(MouseWheelUnit::Line, 40.0, none)]), MAX_FRAME_ZOOM);
    }

    #[test]
    fn zooming_text_meets_few_sizes() {
        // Every size maps to a half octave at or above it, so a zoom over 10..40 points
        // rasterises five sizes, not one per frame.
        let mut sizes: Vec<u32> = (0..300).map(|i| ladder_size(10.0 * 1.0047f32.powi(i)).to_bits()).collect();
        sizes.dedup();
        assert!(sizes.len() <= 5, "{} sizes", sizes.len());
        for px in [4.0f32, 7.3, 11.5, 16.0, 31.9, 60.0] {
            let q = ladder_size(px);
            assert!(q >= px - 1e-3 && q < px * 1.42, "{px} -> {q}");
        }
        assert_eq!(ladder_size(500.0), MAX_TEXT_PX);
    }

    #[test]
    fn a_scaled_galley_keeps_its_glyphs_and_shrinks() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let p = ui.painter();
            let g = p.layout_no_wrap("Note A1".into(), theme::proportional(32.0), Color32::WHITE);
            let s = scale_galley(&g, 0.5);
            assert!((s.size().x - g.size().x * 0.5).abs() < 0.01 && (s.size().y - g.size().y * 0.5).abs() < 0.01);
            let (a, b) = (&g.rows[0].visuals.mesh, &s.rows[0].visuals.mesh);
            assert_eq!(a.vertices.len(), b.vertices.len());
            for (va, vb) in a.vertices.iter().zip(&b.vertices) {
                assert_eq!(va.uv, vb.uv, "same atlas texels");
                assert!((vb.pos.x - va.pos.x * 0.5).abs() < 1e-4);
            }
            // While zooming the plan lays out at a ladder size and scales: same size.
            let z = plan_galley(p, "Note A1".into(), 27.0, Color32::WHITE, true);
            let exact = plan_galley(p, "Note A1".into(), 27.0, Color32::WHITE, false);
            assert!((z.size().x - exact.size().x).abs() < exact.size().x * 0.05);
        });
        out.textures_delta.clear();
    }

    #[test]
    fn off_screen_text_is_skipped_before_layout() {
        let clip = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        assert!(text_may_show(clip, Pos2::new(400.0, 300.0), "A1", 12.0));
        // Starts left of the screen but is long enough to reach into it.
        assert!(text_may_show(clip, Pos2::new(-200.0, 300.0), "a long note that reaches in", 20.0));
        assert!(!text_may_show(clip, Pos2::new(-2000.0, 300.0), "A1", 12.0));
        assert!(!text_may_show(clip, Pos2::new(400.0, 5000.0), "line\nline", 30.0));
    }
}
