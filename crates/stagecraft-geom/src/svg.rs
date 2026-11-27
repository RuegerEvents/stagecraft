//! A small SVG reader for plan symbols: shapes become flattened polygons and
//! polylines in SVG user units (y down). Supports `path` (all commands, arcs and
//! curves flattened), `rect` (with `rx`), `circle`, `ellipse`, `line`, `polyline`,
//! `polygon`, nested `g` and `transform` (matrix, translate, scale, rotate, skewX/Y).
//! Text, gradients, `use` and CSS beyond `style="fill:…;stroke:…"` are ignored.

use stagecraft_core::model::P2;

/// One shape after flattening.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub points: Vec<P2>,
    pub closed: bool,
    /// Has a fill other than `none`.
    pub filled: bool,
    /// Drawn with a stroke. Without a `stroke` attribute an unfilled shape is
    /// stroked (else it would be invisible); a filled one gets an outline too, as
    /// plan symbols are line drawings.
    pub stroked: bool,
    /// `class` attribute (line class names `thin`, `medium`, `thick` …).
    pub class: Option<String>,
}

/// The parsed symbol.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Svg {
    pub shapes: Vec<Shape>,
    /// `viewBox` (min x, min y, width, height), if given.
    pub view_box: Option<[f64; 4]>,
}

impl Svg {
    /// Bounds of all shape points: (min, max).
    pub fn bounds(&self) -> Option<(P2, P2)> {
        let mut it = self.shapes.iter().flat_map(|s| s.points.iter());
        let f = *it.next()?;
        Some(it.fold((f, f), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])])))
    }
}

type Affine = [f64; 6]; // a b c d e f: x' = a x + c y + e, y' = b x + d y + f

const IDENTITY: Affine = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn mul(m: &Affine, n: &Affine) -> Affine {
    [
        m[0] * n[0] + m[2] * n[1],
        m[1] * n[0] + m[3] * n[1],
        m[0] * n[2] + m[2] * n[3],
        m[1] * n[2] + m[3] * n[3],
        m[0] * n[4] + m[2] * n[5] + m[4],
        m[1] * n[4] + m[3] * n[5] + m[5],
    ]
}

fn apply(m: &Affine, p: P2) -> P2 {
    [m[0] * p[0] + m[2] * p[1] + m[4], m[1] * p[0] + m[3] * p[1] + m[5]]
}

/// Parse SVG text. Broken XML gives an empty symbol.
pub fn parse(text: &str) -> Svg {
    let Ok(doc) = roxmltree::Document::parse(text) else { return Svg::default() };
    let root = doc.root_element();
    let view_box = root.attribute("viewBox").and_then(|v| {
        let n = numbers(v);
        (n.len() == 4 && n[2] > 0.0 && n[3] > 0.0).then(|| [n[0], n[1], n[2], n[3]])
    });
    let mut out = Svg { shapes: Vec::new(), view_box };
    walk(root, &IDENTITY, &Style::default(), &mut out.shapes);
    out
}

#[derive(Clone, Default)]
struct Style {
    fill: Option<bool>,
    stroke: Option<bool>,
    class: Option<String>,
}

fn paint(v: &str) -> bool {
    let v = v.trim();
    !(v.is_empty() || v == "none" || v == "transparent")
}

fn style_of(n: roxmltree::Node, parent: &Style) -> Style {
    let mut s = parent.clone();
    if let Some(f) = n.attribute("fill") {
        s.fill = Some(paint(f));
    }
    if let Some(f) = n.attribute("stroke") {
        s.stroke = Some(paint(f));
    }
    if let Some(st) = n.attribute("style") {
        for decl in st.split(';') {
            if let Some((k, v)) = decl.split_once(':') {
                match k.trim() {
                    "fill" => s.fill = Some(paint(v)),
                    "stroke" => s.stroke = Some(paint(v)),
                    _ => {}
                }
            }
        }
    }
    if let Some(c) = n.attribute("class") {
        s.class = Some(c.trim().to_owned());
    }
    s
}

fn walk(n: roxmltree::Node, m: &Affine, parent: &Style, out: &mut Vec<Shape>) {
    for c in n.children().filter(|c| c.is_element()) {
        let tag = c.tag_name().name();
        if matches!(tag, "defs" | "symbol" | "clipPath" | "mask" | "title" | "desc" | "metadata" | "text" | "style") {
            continue;
        }
        let m = match c.attribute("transform") {
            Some(t) => mul(m, &transform(t)),
            None => *m,
        };
        let style = style_of(c, parent);
        let f = |k: &str| c.attribute(k).and_then(|v| numbers(v).first().copied()).unwrap_or(0.0);
        // SVG default: fill black, no stroke.
        let filled = style.fill.unwrap_or(true);
        let rings: Vec<(Vec<P2>, bool)> = match tag {
            "g" | "svg" | "a" => {
                walk(c, &m, &style, out);
                continue;
            }
            "path" => path(c.attribute("d").unwrap_or_default()),
            "rect" => {
                let (x, y, w, h) = (f("x"), f("y"), f("width"), f("height"));
                let rx = c.attribute("rx").map(|_| f("rx")).or(c.attribute("ry").map(|_| f("ry"))).unwrap_or(0.0);
                let ry = c.attribute("ry").map(|_| f("ry")).unwrap_or(rx);
                if w <= 0.0 || h <= 0.0 {
                    continue;
                }
                vec![(rounded_rect(x, y, w, h, rx.min(w / 2.0), ry.min(h / 2.0)), true)]
            }
            "circle" => vec![(ellipse(f("cx"), f("cy"), f("r"), f("r")), true)],
            "ellipse" => vec![(ellipse(f("cx"), f("cy"), f("rx"), f("ry")), true)],
            "line" => vec![(vec![[f("x1"), f("y1")], [f("x2"), f("y2")]], false)],
            "polyline" | "polygon" => {
                let n = numbers(c.attribute("points").unwrap_or_default());
                vec![(n.as_chunks::<2>().0.iter().map(|p| [p[0], p[1]]).collect(), tag == "polygon")]
            }
            _ => continue,
        };
        for (pts, closed) in rings {
            if pts.len() < 2 {
                continue;
            }
            let filled = filled && closed && pts.len() >= 3;
            let stroked = style.stroke.unwrap_or(true) || !filled;
            out.push(Shape {
                points: pts.into_iter().map(|p| apply(&m, p)).collect(),
                closed,
                filled,
                stroked,
                class: style.class.clone(),
            });
        }
    }
}

/// All numbers in an attribute (commas, spaces, `1-2`, `.5.5`, exponents).
pub fn numbers(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut lex = Lexer { s: s.as_bytes(), i: 0 };
    while let Some(n) = lex.number() {
        out.push(n);
    }
    out
}

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
}

impl Lexer<'_> {
    fn skip(&mut self) {
        while self.i < self.s.len() && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',') {
            self.i += 1;
        }
    }

    fn peek_command(&mut self) -> Option<u8> {
        self.skip();
        let c = *self.s.get(self.i)?;
        c.is_ascii_alphabetic().then_some(c).filter(|c| *c != b'e' && *c != b'E')
    }

    fn number(&mut self) -> Option<f64> {
        self.skip();
        let start = self.i;
        let s = self.s;
        if self.i < s.len() && (s[self.i] == b'-' || s[self.i] == b'+') {
            self.i += 1;
        }
        let mut dot = false;
        let mut digits = false;
        while self.i < s.len() {
            match s[self.i] {
                b'0'..=b'9' => digits = true,
                b'.' if !dot => dot = true,
                _ => break,
            }
            self.i += 1;
        }
        if digits && self.i < s.len() && (s[self.i] == b'e' || s[self.i] == b'E') {
            let save = self.i;
            self.i += 1;
            if self.i < s.len() && (s[self.i] == b'-' || s[self.i] == b'+') {
                self.i += 1;
            }
            let exp_start = self.i;
            while self.i < s.len() && s[self.i].is_ascii_digit() {
                self.i += 1;
            }
            if self.i == exp_start {
                self.i = save;
            }
        }
        if !digits {
            self.i = start;
            return None;
        }
        std::str::from_utf8(&s[start..self.i]).ok()?.parse().ok()
    }

    /// An arc flag: a single 0 or 1, possibly without separator.
    fn flag(&mut self) -> Option<bool> {
        self.skip();
        let c = *self.s.get(self.i)?;
        self.i += 1;
        match c {
            b'0' => Some(false),
            b'1' => Some(true),
            _ => None,
        }
    }
}

const CURVE_STEPS: usize = 12;

/// A path's subpaths as (points, closed).
fn path(d: &str) -> Vec<(Vec<P2>, bool)> {
    let mut lex = Lexer { s: d.as_bytes(), i: 0 };
    let mut out: Vec<(Vec<P2>, bool)> = Vec::new();
    let mut cur: Vec<P2> = Vec::new();
    let mut pos = [0.0, 0.0];
    let mut start = [0.0, 0.0];
    let mut last_ctrl: Option<P2> = None;
    let mut cmd = b'M';
    let flush = |cur: &mut Vec<P2>, out: &mut Vec<(Vec<P2>, bool)>, closed: bool| {
        if cur.len() >= 2 {
            out.push((std::mem::take(cur), closed));
        } else {
            cur.clear();
        }
    };
    for _ in 0..1_000_000 {
        if let Some(c) = lex.peek_command() {
            cmd = c;
            lex.i += 1;
        } else if lex.i >= lex.s.len() {
            break;
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { pos } else { [0.0, 0.0] };
        let pt = |lex: &mut Lexer| -> Option<P2> {
            let x = lex.number()?;
            let y = lex.number()?;
            Some([base[0] + x, base[1] + y])
        };
        let before = lex.i;
        match cmd.to_ascii_uppercase() {
            b'M' => {
                let Some(p) = pt(&mut lex) else { break };
                flush(&mut cur, &mut out, false);
                cur.push(p);
                pos = p;
                start = p;
                // Further pairs are line-tos.
                cmd = if rel { b'l' } else { b'L' };
                last_ctrl = None;
            }
            b'L' => {
                let Some(p) = pt(&mut lex) else { break };
                cur.push(p);
                pos = p;
                last_ctrl = None;
            }
            b'H' => {
                let Some(x) = lex.number() else { break };
                pos = [if rel { pos[0] + x } else { x }, pos[1]];
                cur.push(pos);
                last_ctrl = None;
            }
            b'V' => {
                let Some(y) = lex.number() else { break };
                pos = [pos[0], if rel { pos[1] + y } else { y }];
                cur.push(pos);
                last_ctrl = None;
            }
            b'C' | b'S' => {
                let c1 = if cmd.eq_ignore_ascii_case(&b'C') {
                    let Some(c1) = pt(&mut lex) else { break };
                    c1
                } else {
                    last_ctrl.map_or(pos, |c| [2.0 * pos[0] - c[0], 2.0 * pos[1] - c[1]])
                };
                let (Some(c2), Some(p)) = (pt(&mut lex), pt(&mut lex)) else { break };
                if cur.is_empty() {
                    cur.push(pos);
                }
                for k in 1..=CURVE_STEPS {
                    let t = k as f64 / CURVE_STEPS as f64;
                    let u = 1.0 - t;
                    let b = |i: usize| {
                        u * u * u * pos[i] + 3.0 * u * u * t * c1[i] + 3.0 * u * t * t * c2[i] + t * t * t * p[i]
                    };
                    cur.push([b(0), b(1)]);
                }
                last_ctrl = Some(c2);
                pos = p;
            }
            b'Q' | b'T' => {
                let c = if cmd.eq_ignore_ascii_case(&b'Q') {
                    let Some(c) = pt(&mut lex) else { break };
                    c
                } else {
                    last_ctrl.map_or(pos, |c| [2.0 * pos[0] - c[0], 2.0 * pos[1] - c[1]])
                };
                let Some(p) = pt(&mut lex) else { break };
                if cur.is_empty() {
                    cur.push(pos);
                }
                for k in 1..=CURVE_STEPS {
                    let t = k as f64 / CURVE_STEPS as f64;
                    let u = 1.0 - t;
                    let b = |i: usize| u * u * pos[i] + 2.0 * u * t * c[i] + t * t * p[i];
                    cur.push([b(0), b(1)]);
                }
                last_ctrl = Some(c);
                pos = p;
            }
            b'A' => {
                let (Some(rx), Some(ry), Some(rot)) = (lex.number(), lex.number(), lex.number()) else { break };
                let (Some(large), Some(sweep)) = (lex.flag(), lex.flag()) else { break };
                let Some(p) = pt(&mut lex) else { break };
                if cur.is_empty() {
                    cur.push(pos);
                }
                cur.extend(arc(pos, p, rx.abs(), ry.abs(), rot, large, sweep));
                pos = p;
                last_ctrl = None;
            }
            b'Z' => {
                pos = start;
                flush(&mut cur, &mut out, true);
                last_ctrl = None;
                // A command letter must follow; a number after Z would loop.
                if lex.peek_command().is_none() && lex.number().is_some() {
                    break;
                }
                continue;
            }
            _ => break,
        }
        if lex.i == before {
            break;
        }
    }
    flush(&mut cur, &mut out, false);
    out
}

/// Endpoint-parameterised elliptical arc (SVG implementation notes F.6), flattened,
/// without the start point.
fn arc(p0: P2, p1: P2, mut rx: f64, mut ry: f64, rot_deg: f64, large: bool, sweep: bool) -> Vec<P2> {
    if rx < 1e-12 || ry < 1e-12 || (p0[0] - p1[0]).hypot(p0[1] - p1[1]) < 1e-12 {
        return vec![p1];
    }
    let (s, c) = rot_deg.to_radians().sin_cos();
    let dx = (p0[0] - p1[0]) / 2.0;
    let dy = (p0[1] - p1[1]) / 2.0;
    let x1 = c * dx + s * dy;
    let y1 = -s * dx + c * dy;
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut k = (num / den).max(0.0).sqrt();
    if large == sweep {
        k = -k;
    }
    let cx1 = k * rx * y1 / ry;
    let cy1 = -k * ry * x1 / rx;
    let cx = c * cx1 - s * cy1 + (p0[0] + p1[0]) / 2.0;
    let cy = s * cx1 + c * cy1 + (p0[1] + p1[1]) / 2.0;
    let ang = |ux: f64, uy: f64| uy.atan2(ux);
    let t1 = ang((x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut dt = ang((-x1 - cx1) / rx, (-y1 - cy1) / ry) - t1;
    let tau = std::f64::consts::TAU;
    if sweep && dt < 0.0 {
        dt += tau;
    } else if !sweep && dt > 0.0 {
        dt -= tau;
    }
    let n = ((dt.abs() / tau * 32.0).ceil() as usize).max(2);
    (1..=n)
        .map(|i| {
            let t = t1 + dt * i as f64 / n as f64;
            let (st, ct) = t.sin_cos();
            [cx + c * rx * ct - s * ry * st, cy + s * rx * ct + c * ry * st]
        })
        .collect()
}

fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<P2> {
    if rx <= 0.0 || ry <= 0.0 {
        return Vec::new();
    }
    (0..32)
        .map(|i| {
            let t = i as f64 / 32.0 * std::f64::consts::TAU;
            [cx + rx * t.cos(), cy + ry * t.sin()]
        })
        .collect()
}

fn rounded_rect(x: f64, y: f64, w: f64, h: f64, rx: f64, ry: f64) -> Vec<P2> {
    if rx <= 0.0 || ry <= 0.0 {
        return vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
    }
    let mut v = Vec::new();
    let corners = [
        (x + w - rx, y + ry, -90.0),
        (x + w - rx, y + h - ry, 0.0),
        (x + rx, y + h - ry, 90.0),
        (x + rx, y + ry, 180.0),
    ];
    for (cx, cy, a0) in corners {
        for k in 0..=4 {
            let t = (a0 + 90.0 * k as f64 / 4.0_f64).to_radians();
            v.push([cx + rx * t.cos(), cy + ry * t.sin()]);
        }
    }
    v
}

fn transform(t: &str) -> Affine {
    let mut m = IDENTITY;
    let mut rest = t;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let Some(close) = rest[open..].find(')') else { break };
        let n = numbers(&rest[open + 1..open + close]);
        let g = |i: usize, d: f64| n.get(i).copied().unwrap_or(d);
        let next: Affine = match name {
            "matrix" if n.len() == 6 => [n[0], n[1], n[2], n[3], n[4], n[5]],
            "translate" => [1.0, 0.0, 0.0, 1.0, g(0, 0.0), g(1, 0.0)],
            "scale" => [g(0, 1.0), 0.0, 0.0, g(1, g(0, 1.0)), 0.0, 0.0],
            "rotate" => {
                let (s, c) = g(0, 0.0).to_radians().sin_cos();
                let (cx, cy) = (g(1, 0.0), g(2, 0.0));
                let r = [c, s, -s, c, 0.0, 0.0];
                mul(&mul(&[1.0, 0.0, 0.0, 1.0, cx, cy], &r), &[1.0, 0.0, 0.0, 1.0, -cx, -cy])
            }
            "skewX" => [1.0, 0.0, g(0, 0.0).to_radians().tan(), 1.0, 0.0, 0.0],
            "skewY" => [1.0, g(0, 0.0).to_radians().tan(), 0.0, 1.0, 0.0, 0.0],
            _ => IDENTITY,
        };
        m = mul(&m, &next);
        rest = &rest[open + close + 1..];
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_in_compact_form() {
        assert_eq!(numbers("1,2 -3-4.5.5e1"), vec![1.0, 2.0, -3.0, -4.5, 5.0]);
    }

    #[test]
    fn shapes_and_transforms() {
        let s = parse(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50">
              <g transform="translate(10 5)">
                <rect x="0" y="0" width="20" height="10" fill="#000"/>
                <path d="M0 0 h10 v10 z m 20 0 l 5 5" fill="none" class="thick"/>
                <circle cx="50" cy="20" r="5" fill="none"/>
              </g>
              <text>ignored</text>
            </svg>"##,
        );
        assert_eq!(s.view_box, Some([0.0, 0.0, 100.0, 50.0]));
        assert_eq!(s.shapes.len(), 4);
        assert!(s.shapes[0].filled);
        assert_eq!(s.shapes[0].points[0], [10.0, 5.0]);
        assert_eq!(s.shapes[1].points, vec![[10.0, 5.0], [20.0, 5.0], [20.0, 15.0]]);
        assert!(s.shapes[1].closed && !s.shapes[1].filled);
        assert_eq!(s.shapes[1].class.as_deref(), Some("thick"));
        assert_eq!(s.shapes[2].points, vec![[30.0, 5.0], [35.0, 10.0]]);
        let (lo, hi) = s.bounds().unwrap();
        assert!((lo[0] - 10.0).abs() < 1e-9 && (hi[0] - 65.0).abs() < 1e-9);
    }

    #[test]
    fn arcs_reach_their_end() {
        let s = parse(r#"<svg><path d="M 0 0 A 10 10 0 0 1 20 0" fill="none"/></svg>"#);
        let p = &s.shapes[0].points;
        let last = p.last().unwrap();
        assert!((last[0] - 20.0).abs() < 1e-9 && last[1].abs() < 1e-9);
        // Sweep 1 with y down goes through y < 0 (up on screen) … through the top.
        assert!(p.iter().any(|q| q[1] < -9.0), "{p:?}");
    }

    #[test]
    fn broken_input_is_empty() {
        assert!(parse("<svg").shapes.is_empty());
        assert!(parse(r#"<svg><path d="M 1 L x"/></svg>"#).shapes.is_empty());
    }
}
