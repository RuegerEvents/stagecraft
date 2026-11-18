//! Underlays for StageCraft: turn an imported drawing into something a plan can show.
//!
//! - PNG and JPEG are shown as they are; only their pixel size is read.
//! - SVG is rendered with resvg (embedded Roboto stands in for every font).
//! - PDF pages are rendered with hayro (pure Rust).
//! - DXF is read with the `dxf` crate into plain [`Linework`]: polylines (arcs, circles,
//!   ellipses and bulges flattened), texts, block inserts expanded.
//!
//! Everything is in **source units with y up**: pixels for rasters (origin at the lower
//! left corner of the image), points for PDF pages, SVG user units, DXF drawing units.
//! [`Converted::unit_mm`] says how long one source unit is on paper (or in the model,
//! for DXF), so the caller can pick a first scale before calibrating.
//!
//! No StageCraft dependencies; builds for `wasm32-unknown-unknown`.

mod dxf_lines;

use serde::{Deserialize, Serialize};

pub use dxf_lines::read_dxf;

/// What kind of file an underlay comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// PNG or JPEG.
    Image,
    Pdf,
    Svg,
    Dxf,
}

impl Format {
    /// The format for a file extension (case-insensitive).
    pub fn from_ext(ext: &str) -> Option<Self> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "png" | "jpg" | "jpeg" => Format::Image,
            "pdf" => Format::Pdf,
            "svg" => Format::Svg,
            "dxf" => Format::Dxf,
            _ => return None,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a readable image: {0}")]
    Image(String),
    #[error("not a readable SVG: {0}")]
    Svg(String),
    #[error("not a readable PDF: {0}")]
    Pdf(String),
    #[error("the PDF has {pages} page(s); page {page} does not exist")]
    NoPage { page: u32, pages: u32 },
    #[error("not a readable DXF: {0}")]
    Dxf(String),
    #[error("the drawing is empty")]
    Empty,
}

/// A rendered raster: PNG bytes and the pixel size.
#[derive(Clone, Debug)]
pub struct Raster {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// One polyline of a DXF, in drawing units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Path {
    pub points: Vec<[f64; 2]>,
    #[serde(default)]
    pub closed: bool,
}

/// One text of a DXF, in drawing units. `pos` is the insertion point (left baseline),
/// `height` the cap height, `rotation` degrees counter-clockwise.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Text {
    pub pos: [f64; 2],
    pub text: String,
    pub height: f64,
    #[serde(default)]
    pub rotation: f64,
}

/// Vector content of a DXF, flattened. Stored as a JSON asset next to the DXF.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Linework {
    pub paths: Vec<Path>,
    #[serde(default)]
    pub texts: Vec<Text>,
}

impl Linework {
    /// `[min, max]` over every point and text position; `None` if empty.
    pub fn extent(&self) -> Option<[[f64; 2]; 2]> {
        let mut b = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
        let mut add = |p: [f64; 2]| {
            b[0] = [b[0][0].min(p[0]), b[0][1].min(p[1])];
            b[1] = [b[1][0].max(p[0]), b[1][1].max(p[1])];
        };
        self.paths.iter().flat_map(|p| p.points.iter()).for_each(|p| add(*p));
        self.texts.iter().for_each(|t| add(t.pos));
        (b[0][0] <= b[1][0]).then_some(b)
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("linework serialises")
    }

    pub fn from_json(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// The result of reading an underlay file.
#[derive(Clone, Debug)]
pub struct Converted {
    /// What the plan shows for rasters (`None` for PNG/JPEG, which are shown as they
    /// are, and for DXF).
    pub raster: Option<Raster>,
    /// DXF content.
    pub linework: Option<Linework>,
    /// `[min, max]` of the content in source units, y up. Rasters span
    /// `[0, 0]`–`[width, height]` of their source units.
    pub extent: [[f64; 2]; 2],
    /// Length of one source unit in mm: 25.4/96 for image pixels and SVG units,
    /// 25.4/72 for PDF points, the drawing unit for DXF (`$INSUNITS`, mm if unset).
    pub unit_mm: f64,
    /// Pages in a PDF, else 1.
    pub pages: u32,
}

/// Longest side of a rendered SVG or PDF page, pixels.
pub const MAX_RASTER: u32 = 4096;

/// CSS pixel in mm.
pub const PX_MM: f64 = 25.4 / 96.0;
/// PDF point in mm.
pub const PT_MM: f64 = 25.4 / 72.0;

/// Read an underlay. `page` is 1-based and only matters for PDF.
pub fn convert(format: Format, bytes: &[u8], page: u32) -> Result<Converted, Error> {
    match format {
        Format::Image => {
            let (w, h) = image_size(bytes)?;
            Ok(Converted {
                raster: None,
                linework: None,
                extent: [[0.0, 0.0], [w as f64, h as f64]],
                unit_mm: PX_MM,
                pages: 1,
            })
        }
        Format::Svg => {
            let (raster, size) = render_svg(bytes)?;
            Ok(Converted { raster: Some(raster), linework: None, extent: [[0.0, 0.0], size], unit_mm: PX_MM, pages: 1 })
        }
        Format::Pdf => {
            let (raster, size, pages) = render_pdf(bytes, page)?;
            Ok(Converted { raster: Some(raster), linework: None, extent: [[0.0, 0.0], size], unit_mm: PT_MM, pages })
        }
        Format::Dxf => {
            let (lines, unit_mm) = read_dxf(bytes)?;
            let extent = lines.extent().ok_or(Error::Empty)?;
            Ok(Converted { raster: None, linework: Some(lines), extent, unit_mm, pages: 1 })
        }
    }
}

/// Pixel size of a PNG or JPEG.
pub fn image_size(bytes: &[u8]) -> Result<(u32, u32), Error> {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| Error::Image(e.to_string()))?
        .into_dimensions()
        .map_err(|e| Error::Image(e.to_string()))
}

fn pixel_scale(size: [f64; 2]) -> f64 {
    let longest = size[0].max(size[1]).max(1.0);
    // Enough for a sharp plan at 1:100 on A3, capped so a page stays a few MB.
    (MAX_RASTER as f64 / longest).min(8.0)
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    let enc = image::codecs::png::PngEncoder::new(&mut out);
    image::ImageEncoder::write_image(enc, rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| Error::Image(e.to_string()))?;
    Ok(out)
}

static ROBOTO: &[u8] = include_bytes!("../../../assets/fonts/Roboto-Regular.ttf");
static ROBOTO_BOLD: &[u8] = include_bytes!("../../../assets/fonts/Roboto-Bold.ttf");

/// Render an SVG on white. Returns the raster and the SVG's size in user units.
pub fn render_svg(bytes: &[u8]) -> Result<(Raster, [f64; 2]), Error> {
    use resvg::{tiny_skia, usvg};
    let mut opt = usvg::Options::default();
    let db = opt.fontdb_mut();
    db.load_font_data(ROBOTO.to_vec());
    db.load_font_data(ROBOTO_BOLD.to_vec());
    db.set_sans_serif_family("Roboto");
    db.set_serif_family("Roboto");
    db.set_monospace_family("Roboto");
    db.set_cursive_family("Roboto");
    db.set_fantasy_family("Roboto");
    // Every family the file names falls back to Roboto.
    opt.font_resolver.select_font = Box::new(|font, db| {
        let mut q = resvg::usvg::fontdb::Query {
            families: &[resvg::usvg::fontdb::Family::Name("Roboto")],
            weight: resvg::usvg::fontdb::Weight(font.weight()),
            ..Default::default()
        };
        if let Some(id) = db.query(&q) {
            return Some(id);
        }
        q.weight = resvg::usvg::fontdb::Weight::NORMAL;
        db.query(&q)
    });
    let text = String::from_utf8_lossy(bytes);
    let flat = resolve_css_functions(&strip_foreign_objects(&text));
    let tree = usvg::Tree::from_data(flat.as_bytes(), &opt).map_err(|e| Error::Svg(e.to_string()))?;
    let size = tree.size();
    let (w, h) = (size.width() as f64, size.height() as f64);
    let k = pixel_scale([w, h]);
    let (pw, ph) = ((w * k).round().max(1.0) as u32, (h * k).round().max(1.0) as u32);
    let mut pix = tiny_skia::Pixmap::new(pw, ph).ok_or(Error::Empty)?;
    pix.fill(tiny_skia::Color::WHITE);
    resvg::render(&tree, tiny_skia::Transform::from_scale(k as f32, k as f32), &mut pix.as_mut());
    // tiny-skia is premultiplied; on an opaque white page that equals straight alpha.
    let png = encode_png(pw, ph, pix.data())?;
    Ok((Raster { png, width: pw, height: ph }, [w, h]))
}

/// `<foreignObject>` elements removed. usvg cannot draw HTML and takes them as the
/// first choice of a `<switch>`, so the fallback (draw.io: a PNG of the label) never shows.
pub fn strip_foreign_objects(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(i) = rest.find("<foreignObject") {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        // a self-closing tag, or up to the closing tag (they do not nest)
        let open_end = tail.find('>').map(|j| j + 1).unwrap_or(tail.len());
        rest = if tail[..open_end].ends_with("/>") {
            &tail[open_end..]
        } else {
            match tail.find("</foreignObject>") {
                Some(j) => &tail[j + "</foreignObject>".len()..],
                None => "",
            }
        };
    }
    out.push_str(rest);
    out
}

/// CSS `var(--x, fallback)` becomes its fallback and `light-dark(a, b)` its light
/// value: usvg knows neither and would paint such fills black (draw.io writes both).
pub fn resolve_css_functions(svg: &str) -> String {
    let mut s = svg.to_owned();
    // Replacements only shrink the text before `from`, so scanning on from the start of
    // the last replacement finds nested calls too.
    let mut from = 0;
    while let Some((start, name)) =
        ["var(", "light-dark("].iter().filter_map(|n| s[from..].find(n).map(|i| (from + i, *n))).min_by_key(|x| x.0)
    {
        // the matching parenthesis and the top-level commas
        let open = start + name.len();
        let (mut depth, mut end, mut commas) = (1, None, Vec::new());
        for (i, c) in s[open..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(open + i);
                        break;
                    }
                }
                ',' if depth == 1 => commas.push(open + i),
                _ => {}
            }
        }
        let Some(end) = end else { break };
        let arg = |k: usize| -> &str {
            let a = if k == 0 { open } else { commas[k - 1] + 1 };
            let b = commas.get(k).copied().unwrap_or(end);
            s[a..b].trim()
        };
        let value = match (name, commas.len()) {
            ("var(", 0) => "currentColor".to_owned(),
            ("var(", _) => {
                let a = commas[0] + 1;
                s[a..end].trim().to_owned()
            }
            _ => arg(0).to_owned(),
        };
        s.replace_range(start..=end, &value);
        from = start;
    }
    s
}

/// Render page `page` (1-based) of a PDF on white. Returns the raster, the page size in
/// points and the page count.
pub fn render_pdf(bytes: &[u8], page: u32) -> Result<(Raster, [f64; 2], u32), Error> {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{PixmapSettings, RenderCache, RenderSettings};
    let pdf = Pdf::new(bytes.to_vec()).map_err(|e| Error::Pdf(format!("{e:?}")))?;
    let pages = pdf.pages();
    let count = pages.len() as u32;
    let p = page.max(1);
    let Some(pg) = pages.get(p as usize - 1) else { return Err(Error::NoPage { page: p, pages: count }) };
    let (w, h) = pg.render_dimensions();
    let k = pixel_scale([w as f64, h as f64]) as f32;
    let cache = RenderCache::new();
    let pix = hayro::render(
        pg,
        &cache,
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &PixmapSettings { x_scale: k, y_scale: k, bg_color: WHITE },
    );
    let (pw, ph) = (pix.width() as u32, pix.height() as u32);
    let png = pix.into_png().map_err(|e| Error::Pdf(format!("{e:?}")))?;
    Ok((Raster { png, width: pw, height: ph }, [w as f64, h as f64], count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_by_extension() {
        assert_eq!(Format::from_ext("PNG"), Some(Format::Image));
        assert_eq!(Format::from_ext("jpeg"), Some(Format::Image));
        assert_eq!(Format::from_ext("pdf"), Some(Format::Pdf));
        assert_eq!(Format::from_ext("dwg"), None);
    }

    #[test]
    fn svg_renders_at_its_size() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
            <rect x="10" y="10" width="80" height="30" fill="#000"/>
            <text x="100" y="80" font-family="Helvetica" font-size="20">Hi</text></svg>"##;
        let c = convert(Format::Svg, svg, 1).unwrap();
        assert_eq!(c.extent, [[0.0, 0.0], [200.0, 100.0]]);
        let r = c.raster.unwrap();
        // 8 px per unit at most
        assert_eq!((r.width, r.height), (1600, 800));
        let img = image::load_from_memory(&r.png).unwrap().to_rgba8();
        // inside the rect: black; outside: white
        assert_eq!(img.get_pixel(50 * 8, 20 * 8).0[..3], [0, 0, 0]);
        assert_eq!(img.get_pixel(5 * 8, 90 * 8).0[..3], [255, 255, 255]);
        // the text drew something dark with the fallback font
        let dark = (100 * 8..140 * 8)
            .flat_map(|x| (60 * 8..80 * 8).map(move |y| (x, y)))
            .filter(|&(x, y)| img.get_pixel(x, y).0[0] < 100)
            .count();
        assert!(dark > 100, "{dark}");
    }

    #[test]
    fn css_functions_resolve_to_fallbacks() {
        let s = r#"style="fill: var(--ge-adaptive-bg, #ffffff); stroke: light-dark(#000, var(--x, #eee))""#;
        assert_eq!(resolve_css_functions(s), r#"style="fill: #ffffff; stroke: #000""#);
    }

    #[test]
    fn foreign_objects_are_removed() {
        let s = "<switch><foreignObject a=\"1\"><div>x</div></foreignObject><image/></switch><foreignObject/>";
        assert_eq!(strip_foreign_objects(s), "<switch><image/></switch>");
    }

    #[test]
    fn png_size_is_read() {
        let png = encode_png(3, 2, &[255; 24]).unwrap();
        let c = convert(Format::Image, &png, 1).unwrap();
        assert_eq!(c.extent[1], [3.0, 2.0]);
        assert!(c.raster.is_none());
    }
}
