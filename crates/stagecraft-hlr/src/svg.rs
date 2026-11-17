//! A small SVG writer for debugging and snapshot tests.

use std::fmt::Write;

use crate::{Drawing, LineClass};

/// How [`to_svg`] draws. Widths and dashes are in paper mm.
#[derive(Clone, Debug)]
pub struct SvgStyle {
    /// Drawing mm per paper mm (50 = 1:50). Ignored when `fit_width` is set.
    pub scale: f64,
    /// Scale the drawing to this paper width (mm) instead.
    pub fit_width: Option<f64>,
    pub margin: f64,
    pub cut_width: f64,
    pub visible_width: f64,
    pub hidden_width: f64,
    pub centre_width: f64,
    pub hidden_dash: String,
    pub centre_dash: String,
    /// Draw hatching in cut regions.
    pub hatch: bool,
}

impl Default for SvgStyle {
    fn default() -> Self {
        Self {
            scale: 20.0,
            fit_width: Some(200.0),
            margin: 5.0,
            cut_width: 0.5,
            visible_width: 0.25,
            hidden_width: 0.13,
            centre_width: 0.13,
            hidden_dash: "1.2 0.8".into(),
            centre_dash: "4 0.8 0.8 0.8".into(),
            hatch: true,
        }
    }
}

/// The drawing as an SVG document; 1 user unit = 1 paper mm, y flipped so view +y is up.
pub fn to_svg(d: &Drawing, style: &SvgStyle) -> String {
    let (lo, hi) = d.bounds().unwrap_or_default();
    let size = (hi - lo).max(glam::DVec2::splat(1e-9));
    let scale = match style.fit_width {
        Some(w) => size.x.max(size.y) / w,
        None => style.scale,
    };
    let m = style.margin;
    let (w, h) = (size.x / scale + 2.0 * m, size.y / scale + 2.0 * m);
    let px = |x: f64| (x - lo.x) / scale + m;
    let py = |y: f64| (hi.y - y) / scale + m;
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.2}mm" height="{h:.2}mm" viewBox="0 0 {w:.3} {h:.3}">"#
    );
    let _ = writeln!(s, r#"<rect width="100%" height="100%" fill="white"/>"#);
    let mut materials: Vec<u32> = d.regions.iter().map(|r| r.material.0).collect();
    materials.sort_unstable();
    materials.dedup();
    let _ = writeln!(s, "<defs>");
    for mat in &materials {
        let angle = 45.0 + (*mat as f64 * 60.0) % 180.0;
        let _ = writeln!(
            s,
            r#"<pattern id="h{mat}" width="1.5" height="1.5" patternUnits="userSpaceOnUse" patternTransform="rotate({angle})"><line x1="0" y1="0" x2="0" y2="1.5" stroke="black" stroke-width="0.1"/></pattern>"#
        );
    }
    let _ = writeln!(s, "</defs>");
    let path = |pts: &[glam::DVec2], closed: bool| {
        let mut p = String::new();
        for (i, q) in pts.iter().enumerate() {
            let _ = write!(p, "{}{:.3} {:.3} ", if i == 0 { "M" } else { "L" }, px(q.x), py(q.y));
        }
        if closed {
            p.push('Z');
        }
        p
    };
    for r in &d.regions {
        let mut p = path(&r.outer, true);
        for hole in &r.holes {
            p.push(' ');
            p.push_str(&path(hole, true));
        }
        let _ = writeln!(s, r##"<path d="{p}" fill="#ddd" fill-rule="evenodd" stroke="none"/>"##);
        if style.hatch {
            let _ = writeln!(s, r#"<path d="{p}" fill="url(#h{})" fill-rule="evenodd" stroke="none"/>"#, r.material.0);
        }
    }
    for class in [LineClass::Hidden, LineClass::Centre, LineClass::Visible, LineClass::Cut] {
        let (width, dash, colour) = match class {
            LineClass::Cut => (style.cut_width, "", "black"),
            LineClass::Visible => (style.visible_width, "", "black"),
            LineClass::Hidden => (style.hidden_width, style.hidden_dash.as_str(), "#555"),
            LineClass::Centre => (style.centre_width, style.centre_dash.as_str(), "#a33"),
        };
        let dash = if dash.is_empty() { String::new() } else { format!(r#" stroke-dasharray="{dash}""#) };
        let _ = writeln!(
            s,
            r#"<g fill="none" stroke="{colour}" stroke-width="{width}"{dash} stroke-linecap="round" stroke-linejoin="round">"#
        );
        for l in d.lines(class) {
            let _ = writeln!(s, r#"<path d="{}"/>"#, path(&l.points, l.closed));
        }
        let _ = writeln!(s, "</g>");
    }
    s.push_str("</svg>\n");
    s
}
