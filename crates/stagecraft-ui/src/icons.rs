//! Toolbar icons: Lucide (ISC, `assets/icons/lucide/`) for the generic tools, StageCraft's
//! own stage icons in the same 24 × 24 grid, 2 px round stroke (`assets/icons/stage/`).
//!
//! The SVGs are prepared once (stroke in white, so the texture tints to any colour), rasterised with resvg at the size and pixel density they are
//! drawn at, and kept as textures in the egui context.

use std::sync::OnceLock;

use egui::{Color32, ColorImage, Rect, TextureHandle, TextureOptions, Ui};

use crate::state::Tool;

macro_rules! icons {
    ($($name:literal => $path:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!("../assets/icons/", $path)) as &[u8])),*]
    };
}

static ICONS: &[(&str, &[u8])] = icons![
    "select" => "lucide/mouse-pointer-2.svg",
    "move" => "lucide/move.svg",
    "rotate" => "lucide/rotate-cw.svg",
    "calibrate" => "lucide/ruler.svg",
    "search" => "lucide/search.svg",
    "undo" => "lucide/undo-2.svg",
    "redo" => "lucide/redo-2.svg",
    "fit" => "lucide/scan.svg",
    "floor" => "stage/floor.svg",
    "wall" => "stage/wall.svg",
    "dimension" => "stage/dimension.svg",
    "truss" => "stage/truss.svg",
    "run" => "stage/run.svg",
    "fixture" => "stage/fixture.svg",
    "batten" => "stage/batten.svg",
    "curtain" => "stage/curtain.svg",
    "portal" => "stage/portal.svg",
    "wings" => "stage/wings.svg",
    "platform" => "stage/platform.svg",
    "seats" => "stage/seats.svg",
    "support" => "stage/support.svg",
    "hoist" => "stage/hoist.svg",
    "ceiling" => "stage/ceiling.svg",
    "c-horizontal" => "stage/c-horizontal.svg",
    "c-vertical" => "stage/c-vertical.svg",
    "c-parallel" => "stage/c-parallel.svg",
    "c-perpendicular" => "stage/c-perpendicular.svg",
    "c-equal" => "stage/c-equal.svg",
    "c-symmetric" => "stage/c-symmetric.svg",
    "lock" => "lucide/lock.svg",
    "file-plus" => "lucide/file-plus.svg",
    "folder-open" => "lucide/folder-open.svg",
    "save" => "lucide/save.svg",
    "file-input" => "lucide/file-input.svg",
    "printer" => "lucide/printer.svg",
    "file-output" => "lucide/file-output.svg",
    "graduation-cap" => "lucide/graduation-cap.svg",
    "ellipsis" => "lucide/ellipsis.svg",
    "sheet" => "lucide/file-text.svg",
    "plus" => "lucide/plus.svg",
    "sun-moon" => "lucide/sun-moon.svg",
    "spline" => "lucide/spline.svg",
    "waypoints" => "lucide/waypoints.svg",
    "magnet" => "lucide/magnet.svg",
    "grid" => "lucide/grid-3x3.svg",
    "layers" => "lucide/layers.svg",
    "tags" => "lucide/tags.svg",
    "library" => "lucide/library.svg",
    "cloud-download" => "lucide/cloud-download.svg",
    "sliders" => "lucide/sliders-horizontal.svg",
    "table" => "lucide/table.svg",
    "menu" => "lucide/menu.svg",
    "chevron-down" => "lucide/chevron-down.svg",
    "chevron-right" => "lucide/chevron-right.svg",
    "box" => "lucide/box.svg",
    "eye" => "lucide/eye.svg",
    "eye-off" => "lucide/eye-off.svg",
    "file" => "lucide/file.svg",
    "pencil" => "lucide/pencil.svg",
    "pen-tool" => "lucide/pen-tool.svg",
    "image" => "lucide/image.svg",
    "circle-help" => "lucide/circle-help.svg",
    "terminal" => "lucide/terminal.svg",
    "trash-2" => "lucide/trash-2.svg",
    "copy" => "lucide/copy.svg",
    "link" => "lucide/link.svg",
    "unlink" => "lucide/unlink.svg",
    "text-cursor-input" => "lucide/text-cursor-input.svg",
    "crosshair" => "lucide/crosshair.svg",
    "braces" => "lucide/braces.svg",
    "info" => "lucide/info.svg",
    "list" => "lucide/list.svg",
    "history" => "lucide/history.svg",
    "square-dashed-mouse-pointer" => "lucide/square-dashed-mouse-pointer.svg",
    "x" => "lucide/x.svg",
    "square" => "lucide/square.svg",
    "columns-2" => "lucide/columns-2.svg",
    "zoom-in" => "lucide/zoom-in.svg",
    "hand" => "lucide/hand.svg",
    "camera" => "lucide/camera.svg",
    "scissors" => "lucide/scissors.svg",
    "sun" => "lucide/sun.svg",
    "moon" => "lucide/moon.svg",
    "app-window" => "lucide/app-window.svg",
    "scan-search" => "lucide/scan-search.svg",
    "refresh-cw" => "lucide/refresh-cw.svg",
    "map-pin" => "lucide/map-pin.svg",
    "combine" => "lucide/combine.svg",
    "weight" => "lucide/weight.svg",
    "hash" => "lucide/hash.svg",
    "wand-sparkles" => "lucide/wand-sparkles.svg",
    "triangle-alert" => "lucide/triangle-alert.svg",
    "tag" => "lucide/tag.svg",
    "frame" => "lucide/frame.svg",
    "layout-template" => "lucide/layout-template.svg",
    "sticky-note" => "lucide/sticky-note.svg",
    "settings-2" => "lucide/settings-2.svg",
    "clock" => "lucide/clock.svg",
];

/// The SVG of `name` with `currentColor` in white; `select` for an unknown name.
fn svg(name: &str) -> &'static [u8] {
    static SET: OnceLock<std::collections::HashMap<&'static str, Vec<u8>>> = OnceLock::new();
    let set = SET.get_or_init(|| {
        ICONS
            .iter()
            .map(|(n, bytes)| (*n, String::from_utf8_lossy(bytes).replace("currentColor", "#ffffff").into_bytes()))
            .collect()
    });
    set.get(name).or_else(|| set.get("select")).map_or(&[], Vec::as_slice)
}

/// Every icon name, for tests.
pub fn names() -> impl Iterator<Item = &'static str> {
    ICONS.iter().map(|(n, _)| *n)
}

pub fn tool_icon(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "select",
        Tool::Move => "move",
        Tool::Rotate => "rotate",
        Tool::Floor => "floor",
        Tool::Wall => "wall",
        Tool::Dimension => "dimension",
        Tool::Calibrate => "calibrate",
        Tool::Truss => "truss",
        Tool::Run => "run",
        Tool::Fixture => "fixture",
        Tool::Batten => "batten",
        Tool::Curtain => "curtain",
        Tool::Proscenium => "portal",
        Tool::Wings => "wings",
        Tool::Platform => "platform",
        Tool::Support => "support",
        Tool::Hoist => "hoist",
        Tool::Ceiling => "ceiling",
    }
}

/// Lucide's stroke in grid units (the 24 × 24 viewBox).
const GRID_STROKE: f32 = 2.0;

/// The stroke an icon gets at `px` device pixels: Lucide's 2 units scaled, rounded to whole
/// pixels (at least one), so every line covers whole pixels instead of smearing over two.
pub fn stroke_px(px: u32) -> f32 {
    (GRID_STROKE * px as f32 / 24.0).round().max(1.0)
}

/// The icon as white-on-transparent pixels, `px` wide and high, grid-fitted: every point of a
/// stroked path is moved to the nearest pixel centre (odd stroke widths) or pixel corner (even
/// ones), Bézier control points move with their end points, and the stroke is [`stroke_px`].
/// So a 1 px line at 1× lands on one pixel column, not half on two. Fills are drawn as they
/// are.
pub fn rasterise(name: &str, px: u32) -> Option<ColorImage> {
    use resvg::tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};
    use resvg::usvg::{Group, Node};

    let data = svg(name);
    let tree = resvg::usvg::Tree::from_data(data, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = Pixmap::new(px, px)?;
    let scale = px as f32 / tree.size().width().max(tree.size().height());
    let width = stroke_px(px);
    let off = if width as u32 % 2 == 1 { 0.5 } else { 0.0 };
    let snap = |v: f32| (v - off).round() + off;
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;

    fn walk(g: &Group, out: &mut Vec<resvg::usvg::Path>) {
        for n in g.children() {
            match n {
                Node::Group(g) => walk(g, out),
                Node::Path(p) if p.is_visible() => out.push((**p).clone()),
                _ => {}
            }
        }
    }
    let mut paths = Vec::new();
    walk(tree.root(), &mut paths);
    for p in &paths {
        let ts = p.abs_transform().post_scale(scale, scale);
        let Some(path) = p.data().clone().transform(ts) else { continue };
        if p.fill().is_some() {
            pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }
        let Some(stroke) = p.stroke() else { continue };
        let mut pb = PathBuilder::new();
        let mut delta = (0.0f32, 0.0f32);
        let fit = |x: f32, y: f32| {
            let (sx, sy) = (snap(x), snap(y));
            ((sx, sy), (sx - x, sy - y))
        };
        for seg in path.segments() {
            use resvg::tiny_skia::PathSegment as S;
            match seg {
                S::MoveTo(q) => {
                    let (a, d) = fit(q.x, q.y);
                    pb.move_to(a.0, a.1);
                    delta = d;
                }
                S::LineTo(q) => {
                    let (a, d) = fit(q.x, q.y);
                    pb.line_to(a.0, a.1);
                    delta = d;
                }
                S::QuadTo(c, q) => {
                    let (a, d) = fit(q.x, q.y);
                    let k = ((delta.0 + d.0) / 2.0, (delta.1 + d.1) / 2.0);
                    pb.quad_to(c.x + k.0, c.y + k.1, a.0, a.1);
                    delta = d;
                }
                S::CubicTo(c1, c2, q) => {
                    let (a, d) = fit(q.x, q.y);
                    pb.cubic_to(c1.x + delta.0, c1.y + delta.1, c2.x + d.0, c2.y + d.1, a.0, a.1);
                    delta = d;
                }
                S::Close => pb.close(),
            }
        }
        let Some(fitted) = pb.finish() else { continue };
        let mut st = stroke.to_tiny_skia();
        st.width = width;
        pixmap.stroke_path(&fitted, &paint, &st, Transform::identity(), None);
    }
    Some(ColorImage::from_rgba_premultiplied([px as usize, px as usize], pixmap.data()))
}

/// The device-pixel square an icon of `size` points centred in `rect` is drawn in: `px`
/// texels wide, its corner on the pixel grid, so the texture maps 1:1 onto the screen.
pub fn pixel_rect(rect: Rect, size: f32, ppp: f32) -> (Rect, u32) {
    let px = (size * ppp).round().max(1.0);
    let min = (rect.center() * ppp - egui::vec2(px, px) / 2.0).round();
    (Rect::from_min_size(min / ppp, egui::vec2(px, px) / ppp), px as u32)
}

/// Paint icon `name` centred in `rect` at `size` points, tinted `colour`.
pub fn paint(ui: &Ui, rect: Rect, name: &'static str, size: f32, colour: Color32) {
    paint_with(ui.painter(), ui.ctx(), rect, name, size, colour);
}

/// [`paint`] with any painter (another layer, a clip rect). The texture is rasterised for
/// the pixel density of the viewport being drawn and cached per icon and pixel size, so a
/// window moved to a screen with another density gets new textures.
pub fn paint_with(
    painter: &egui::Painter,
    ctx: &egui::Context,
    rect: Rect,
    name: &'static str,
    size: f32,
    colour: Color32,
) {
    let (r, px) = pixel_rect(rect, size, ctx.pixels_per_point());
    let id = egui::Id::unique(("stagecraft_icon", name, px));
    let tex = ctx.data(|d| d.get_temp::<TextureHandle>(id)).or_else(|| {
        let img = rasterise(name, px)?;
        // Drawn 1:1 on the pixel grid: nearest keeps every texel as it is.
        let tex = ctx.load_texture(format!("icon-{name}-{px}"), img, TextureOptions::NEAREST);
        ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
        Some(tex)
    });
    if let Some(tex) = tex {
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(tex.id(), r, uv, colour);
    }
}

/// Allocate room for icon `name` in the layout and paint it there.
pub fn paint_inline(ui: &mut Ui, name: &'static str, size: f32, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    paint(ui, rect, name, size, colour);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_rasterises_with_ink() {
        for name in names() {
            let img = rasterise(name, 48).unwrap_or_else(|| panic!("{name} does not load"));
            let ink = img.pixels.iter().filter(|p| p.a() > 128).count();
            assert!(ink > 60, "{name}: {ink} opaque pixels");
        }
        for tool in Tool::ALL {
            assert!(names().any(|n| n == tool_icon(tool)), "{tool:?}");
        }
    }

    /// resvg without grid fitting, to compare.
    fn rasterise_plain(name: &str, px: u32) -> ColorImage {
        let tree = resvg::usvg::Tree::from_data(svg(name), &resvg::usvg::Options::default()).unwrap();
        let mut pixmap = resvg::tiny_skia::Pixmap::new(px, px).unwrap();
        let s = px as f32 / 24.0;
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(s, s), &mut pixmap.as_mut());
        ColorImage::from_rgba_premultiplied([px as usize, px as usize], pixmap.data())
    }

    /// Share of inked pixels that are only partly covered (grey smear instead of a line).
    fn smear(img: &ColorImage) -> f32 {
        let inked = img.pixels.iter().filter(|p| p.a() > 0).count();
        let partial = img.pixels.iter().filter(|p| p.a() > 0 && p.a() < 250).count();
        partial as f32 / inked as f32
    }

    /// Grid fitting: at 1× a 16 px icon's strokes cover whole pixels. The wall
    /// (`M3 21V3h18v6H9v12z`) is straight lines; only its round corners stay grey.
    #[test]
    fn small_icons_land_on_whole_pixels() {
        assert_eq!(stroke_px(16), 1.0);
        assert_eq!(stroke_px(20), 2.0);
        assert_eq!(stroke_px(24), 2.0);
        assert_eq!(stroke_px(32), 3.0);
        for px in [16, 17, 20, 32] {
            let (fitted, plain) = (smear(&rasterise("wall", px).unwrap()), smear(&rasterise_plain("wall", px)));
            assert!(fitted < 0.3 && fitted < plain / 2.0, "{px} px: {fitted} fitted, {plain} plain");
        }
        // Over the whole set (diagonals and curves stay anti-aliased).
        let n = names().count() as f32;
        let fitted: f32 = names().map(|i| smear(&rasterise(i, 16).unwrap())).sum::<f32>() / n;
        let plain: f32 = names().map(|i| smear(&rasterise_plain(i, 16))).sum::<f32>() / n;
        assert!(fitted < 0.85 * plain, "{fitted} fitted, {plain} plain");
    }

    /// Drawn 1:1: the rect starts on the pixel grid and is as many texels wide as the texture.
    #[test]
    fn icons_are_placed_on_the_pixel_grid() {
        let rect = Rect::from_min_size(egui::pos2(10.3, 7.75), egui::vec2(30.0, 29.0));
        for ppp in [1.0, 1.25, 1.5, 2.0] {
            for size in [12.0, 16.0, 17.0] {
                let (r, px) = pixel_rect(rect, size, ppp);
                let min = r.min * ppp;
                assert!((min.x - min.x.round()).abs() < 1e-3 && (min.y - min.y.round()).abs() < 1e-3, "{ppp} {r:?}");
                assert!((r.width() * ppp - px as f32).abs() < 1e-3);
                assert!((r.center() - rect.center()).length() <= 0.71 / ppp);
            }
        }
    }

    /// `STAGECRAFT_ICONS=<dir>` writes every icon as a 96 px PNG, for a person to look at.
    #[test]
    fn icon_sheet() {
        let Ok(dir) = std::env::var("STAGECRAFT_ICONS") else { return };
        std::fs::create_dir_all(&dir).unwrap();
        for name in names() {
            let img = rasterise(name, 96).unwrap();
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| [255 - c.a(), 255 - c.a(), 255 - c.a(), 255]).collect();
            image::RgbaImage::from_raw(96, 96, bytes).unwrap().save(format!("{dir}/{name}.png")).unwrap();
        }
    }
}
