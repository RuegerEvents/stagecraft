//! Toolbar icons: Lucide (ISC, `assets/icons/lucide/`) for the generic tools, StageCraft's
//! own stage icons in the same 24 × 24 grid, 2 px round stroke (`assets/icons/stage/`).
//!
//! The SVGs are prepared once in a `craft_ui::icons::SvgIconSet` (stroke in white, so the
//! texture tints to any colour), rasterised with resvg at the size and pixel density they are
//! drawn at, and kept as textures in the egui context.

use std::sync::OnceLock;

use craft_ui::icons::{SvgIconSet, SvgStyle};
use egui::{Color32, ColorImage, Rect, TextureHandle, TextureOptions, Ui, load::Bytes};

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
];

fn set() -> &'static SvgIconSet {
    static SET: OnceLock<SvgIconSet> = OnceLock::new();
    SET.get_or_init(|| {
        SvgIconSet::new(
            ICONS,
            SvgStyle { current_color: "#ffffff", stroke_width: None },
            "bytes://stagecraft/icons/",
            "select",
        )
    })
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

/// The icon as white-on-transparent pixels, `px` wide and high.
pub fn rasterise(name: &str, px: u32) -> Option<ColorImage> {
    let egui::ImageSource::Bytes { bytes, .. } = set().source(name) else { return None };
    let data: &[u8] = match &bytes {
        Bytes::Static(b) => b,
        Bytes::Shared(b) => b,
    };
    let tree = resvg::usvg::Tree::from_data(data, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(px, px)?;
    let s = px as f32 / tree.size().width().max(tree.size().height());
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(s, s), &mut pixmap.as_mut());
    Some(ColorImage::from_rgba_premultiplied([px as usize, px as usize], pixmap.data()))
}

/// Paint icon `name` centred in `rect` at `size` points, tinted `colour`.
pub fn paint(ui: &Ui, rect: Rect, name: &'static str, size: f32, colour: Color32) {
    paint_with(ui.painter(), ui.ctx(), rect, name, size, colour);
}

/// [`paint`] with any painter (another layer, a clip rect).
pub fn paint_with(
    painter: &egui::Painter,
    ctx: &egui::Context,
    rect: Rect,
    name: &'static str,
    size: f32,
    colour: Color32,
) {
    let px = (size * ctx.pixels_per_point()).round().max(1.0) as u32;
    let id = egui::Id::unique(("stagecraft_icon", name, px));
    let tex = ctx.data(|d| d.get_temp::<TextureHandle>(id)).or_else(|| {
        let img = rasterise(name, px)?;
        let tex = ctx.load_texture(format!("icon-{name}-{px}"), img, TextureOptions::LINEAR);
        ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
        Some(tex)
    });
    if let Some(tex) = tex {
        let r = Rect::from_center_size(rect.center(), egui::vec2(size, size));
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
