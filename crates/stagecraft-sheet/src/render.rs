//! Pages to files: vector PDF through krilla (Roboto embedded and subset), and SVG.
//! Both turn paper mm (y up) into their own y-down space.

use std::fmt::Write as _;

use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Size, Transform};
use krilla::image::Image;
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, LineCap, LineJoin, Stroke, StrokeDash};
use krilla::surface::Surface;
use krilla::text::{Font, TextDirection};

use crate::fonts::{self, em, text_width};
use crate::prim::{HAlign, ImageData, P2, Page, Pen, Prim, Rgb, VAlign};

/// Points per mm.
const PT: f64 = 72.0 / 25.4;

/// Where a text's baseline starts, paper mm, after alignment.
fn text_origin(pos: P2, text: &str, size: f64, angle: f64, h: HAlign, v: VAlign, bold: bool) -> P2 {
    let w = text_width(text, size, bold);
    let dx = match h {
        HAlign::Left => 0.0,
        HAlign::Center => -w / 2.0,
        HAlign::Right => -w,
    };
    let dy = match v {
        VAlign::Baseline => 0.0,
        VAlign::Middle => -size / 2.0,
    };
    let (s, c) = angle.to_radians().sin_cos();
    [pos[0] + dx * c - dy * s, pos[1] + dx * s + dy * c]
}

/// PNG bytes of straight RGBA pixels.
pub fn encode_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        if let Ok(mut w) = enc.write_header() {
            let _ = w.write_image_data(pixels);
        }
    }
    out
}

// ---- SVG ----

fn svg_color(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn svg_pen(p: &Pen) -> String {
    let mut s = format!(" stroke=\"{}\" stroke-width=\"{:.3}\" stroke-linejoin=\"round\"", svg_color(p.color), p.width);
    if let Some(d) = &p.dash {
        let v: Vec<String> = d.iter().map(|x| format!("{x:.2}")).collect();
        let _ = write!(s, " stroke-dasharray=\"{}\"", v.join(" "));
    }
    s
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn b64(data: &[u8]) -> String {
    stagecraft_core::sheet::base64_encode(data)
}

fn svg_prims(prims: &[Prim], h: f64, clip: &mut usize, out: &mut String) {
    let y = |v: f64| h - v;
    for p in prims {
        match p {
            Prim::Path { rings, closed, pen, fill } => {
                let mut d = String::new();
                for r in rings {
                    for (i, q) in r.iter().enumerate() {
                        let _ = write!(d, "{}{:.3},{:.3}", if i == 0 { "M" } else { "L" }, q[0], y(q[1]));
                    }
                    if *closed {
                        d.push('Z');
                    }
                }
                let f = fill.map_or("none".into(), svg_color);
                let st = pen.as_ref().map_or(String::new(), svg_pen);
                let _ = writeln!(out, "<path d=\"{d}\" fill=\"{f}\" fill-rule=\"evenodd\"{st}/>");
            }
            Prim::Circle { center, radius, pen, fill } => {
                let f = fill.map_or("none".into(), svg_color);
                let st = pen.as_ref().map_or(String::new(), svg_pen);
                let _ = writeln!(
                    out,
                    "<circle cx=\"{:.3}\" cy=\"{:.3}\" r=\"{:.3}\" fill=\"{f}\"{st}/>",
                    center[0],
                    y(center[1]),
                    radius
                );
            }
            Prim::Text { pos, text, size, angle, h: ha, v, bold, color } => {
                let o = text_origin(*pos, text, *size, *angle, *ha, *v, *bold);
                let (ox, oy) = (o[0], y(o[1]));
                let rot = if angle.abs() > 1e-9 {
                    format!(" transform=\"rotate({:.3} {ox:.3} {oy:.3})\"", -angle)
                } else {
                    String::new()
                };
                let _ = writeln!(
                    out,
                    "<text x=\"{ox:.3}\" y=\"{oy:.3}\" font-family=\"Roboto, Helvetica, Arial, sans-serif\" font-size=\"{:.3}\" font-weight=\"{}\" fill=\"{}\"{rot}>{}</text>",
                    em(*size),
                    if *bold { 700 } else { 400 },
                    svg_color(*color),
                    esc(text)
                );
            }
            Prim::Image { min, max, data } => {
                let (mime, bytes) = match data {
                    ImageData::Encoded(b) if data.is_jpeg() => ("image/jpeg", b.as_ref().clone()),
                    ImageData::Encoded(b) => ("image/png", b.as_ref().clone()),
                    ImageData::Rgba { width, height, pixels } => ("image/png", encode_png(*width, *height, pixels)),
                };
                let _ = writeln!(
                    out,
                    "<image x=\"{:.3}\" y=\"{:.3}\" width=\"{:.3}\" height=\"{:.3}\" preserveAspectRatio=\"none\" href=\"data:{mime};base64,{}\"/>",
                    min[0],
                    y(max[1]),
                    max[0] - min[0],
                    max[1] - min[1],
                    b64(&bytes)
                );
            }
            Prim::Clip { min, max, prims } => {
                *clip += 1;
                let id = *clip;
                let _ = writeln!(
                    out,
                    "<clipPath id=\"c{id}\"><rect x=\"{:.3}\" y=\"{:.3}\" width=\"{:.3}\" height=\"{:.3}\"/></clipPath><g clip-path=\"url(#c{id})\">",
                    min[0],
                    y(max[1]),
                    max[0] - min[0],
                    max[1] - min[1]
                );
                svg_prims(prims, h, clip, out);
                out.push_str("</g>\n");
            }
        }
    }
}

/// One page as SVG, sized in mm.
pub fn svg(page: &Page) -> String {
    let [w, h] = page.size;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\" stroke-linecap=\"butt\">\n"
    );
    let mut clip = 0;
    svg_prims(&page.prims, h, &mut clip, &mut out);
    out.push_str("</svg>\n");
    out
}

// ---- PDF ----

struct Fonts {
    regular: Font,
    bold: Font,
}

fn paint(c: Rgb) -> krilla::paint::Paint {
    rgb::Color::new(c.0, c.1, c.2).into()
}

fn pdf_stroke(p: &Pen) -> Stroke {
    Stroke {
        paint: paint(p.color),
        width: (p.width * PT) as f32,
        miter_limit: 4.0,
        line_cap: LineCap::Butt,
        line_join: LineJoin::Round,
        opacity: NormalizedF32::ONE,
        dash: p.dash.as_ref().map(|d| StrokeDash { array: d.iter().map(|x| (x * PT) as f32).collect(), offset: 0.0 }),
    }
}

fn pdf_prims(s: &mut Surface, prims: &[Prim], h: f64, fonts: &Fonts) {
    let pt = |q: P2| ((q[0] * PT) as f32, ((h - q[1]) * PT) as f32);
    let draw = |s: &mut Surface, path: Option<krilla::geom::Path>, pen: &Option<Pen>, fill: &Option<Rgb>| {
        let Some(path) = path else { return };
        if let Some(f) = fill {
            s.set_stroke(None);
            s.set_fill(Some(Fill { paint: paint(*f), opacity: NormalizedF32::ONE, rule: FillRule::EvenOdd }));
            s.draw_path(&path);
        }
        if let Some(p) = pen {
            s.set_fill(None);
            s.set_stroke(Some(pdf_stroke(p)));
            s.draw_path(&path);
        }
    };
    for p in prims {
        match p {
            Prim::Path { rings, closed, pen, fill } => {
                let mut pb = PathBuilder::new();
                for r in rings.iter().filter(|r| r.len() >= 2) {
                    let (x, y) = pt(r[0]);
                    pb.move_to(x, y);
                    for q in &r[1..] {
                        let (x, y) = pt(*q);
                        pb.line_to(x, y);
                    }
                    if *closed {
                        pb.close();
                    }
                }
                draw(s, pb.finish(), pen, fill);
            }
            Prim::Circle { center, radius, pen, fill } => {
                let (cx, cy) = pt(*center);
                let r = (*radius * PT) as f32;
                let k = 0.552_284_8 * r;
                let mut pb = PathBuilder::new();
                pb.move_to(cx + r, cy);
                pb.cubic_to(cx + r, cy + k, cx + k, cy + r, cx, cy + r);
                pb.cubic_to(cx - k, cy + r, cx - r, cy + k, cx - r, cy);
                pb.cubic_to(cx - r, cy - k, cx - k, cy - r, cx, cy - r);
                pb.cubic_to(cx + k, cy - r, cx + r, cy - k, cx + r, cy);
                pb.close();
                draw(s, pb.finish(), pen, fill);
            }
            Prim::Text { pos, text, size, angle, h: ha, v, bold, color } => {
                let o = text_origin(*pos, text, *size, *angle, *ha, *v, *bold);
                let (x, y) = pt(o);
                // Counter-clockwise on paper is clockwise in the y-down page space.
                let (sn, cs) = (-angle).to_radians().sin_cos();
                let t = Transform::from_row(cs as f32, sn as f32, -sn as f32, cs as f32, x, y);
                s.push_transform(&t);
                s.set_stroke(None);
                s.set_fill(Some(Fill { paint: paint(*color), opacity: NormalizedF32::ONE, rule: FillRule::NonZero }));
                let font = if *bold { fonts.bold.clone() } else { fonts.regular.clone() };
                s.draw_text(Point::from_xy(0.0, 0.0), font, (em(*size) * PT) as f32, text, false, TextDirection::Auto);
                s.pop();
            }
            Prim::Image { min, max, data } => {
                let img = match data {
                    ImageData::Encoded(b) if data.is_jpeg() => Image::from_jpeg(b.as_ref().clone().into(), true),
                    ImageData::Encoded(b) => Image::from_png(b.as_ref().clone().into(), true),
                    ImageData::Rgba { width, height, pixels } => {
                        Image::from_png(encode_png(*width, *height, pixels).into(), true)
                    }
                };
                let Ok(img) = img else { continue };
                let (x, y) = pt([min[0], max[1]]);
                let size = Size::from_wh(((max[0] - min[0]) * PT) as f32, ((max[1] - min[1]) * PT) as f32);
                let Some(size) = size else { continue };
                s.push_transform(&Transform::from_translate(x, y));
                s.draw_image(img, size);
                s.pop();
            }
            Prim::Clip { min, max, prims } => {
                let (x0, y0) = pt([min[0], max[1]]);
                let (x1, y1) = pt([max[0], min[1]]);
                let mut pb = PathBuilder::new();
                if let Some(r) = krilla::geom::Rect::from_ltrb(x0, y0, x1, y1) {
                    pb.push_rect(r);
                }
                let Some(clip) = pb.finish() else { continue };
                s.push_clip_path(&clip, &FillRule::NonZero);
                pdf_prims(s, prims, h, fonts);
                s.pop();
            }
        }
    }
}

/// Pages as one PDF; `title` goes into the metadata.
pub fn pdf(pages: &[Page], title: &str) -> Result<Vec<u8>, String> {
    let fonts = Fonts {
        regular: Font::new(fonts::REGULAR.into(), 0).ok_or("embedded Roboto does not load")?,
        bold: Font::new(fonts::BOLD.into(), 0).ok_or("embedded Roboto Bold does not load")?,
    };
    let mut doc = krilla::Document::new();
    doc.set_metadata(krilla::metadata::Metadata::new().title(title.to_owned()).creator("StageCraft".into()));
    for page in pages {
        let [w, h] = page.size;
        let settings = PageSettings::from_wh((w * PT) as f32, (h * PT) as f32).ok_or("bad paper size")?;
        let mut p = doc.start_page_with(settings);
        let mut s = p.surface();
        pdf_prims(&mut s, &page.prims, h, &fonts);
        s.finish();
        p.finish();
    }
    doc.finish().map_err(|e| format!("PDF: {e:?}"))
}
