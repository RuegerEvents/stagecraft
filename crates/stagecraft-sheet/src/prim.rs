//! What a composed sheet is made of: vector primitives in paper mm (origin lower left,
//! y up), with pens in mm. PDF, SVG and the GUI draw the same list.

use std::sync::Arc;

use serde::Serialize;
use stagecraft_core::Id;

pub type P2 = [f64; 2];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0, 0, 0);
    pub const WHITE: Rgb = Rgb(255, 255, 255);
}

/// A stroke: width in mm, optional dash pattern in mm (on, off, on, off …).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Pen {
    pub width: f64,
    pub dash: Option<Vec<f64>>,
    pub color: Rgb,
}

impl Pen {
    pub fn solid(width: f64) -> Self {
        Pen { width, dash: None, color: Rgb::BLACK }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize)]
pub enum HAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize)]
pub enum VAlign {
    /// `pos` is on the baseline.
    #[default]
    Baseline,
    /// `pos` is half the cap height above the baseline.
    Middle,
}

/// Pixels or an encoded file for an image.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum ImageData {
    /// PNG or JPEG file bytes.
    #[serde(skip)]
    Encoded(Arc<Vec<u8>>),
    /// Straight RGBA, row by row from the top.
    #[serde(skip)]
    Rgba { width: u32, height: u32, pixels: Arc<Vec<u8>> },
}

impl ImageData {
    /// Pixel size, if known.
    pub fn size(&self) -> Option<(u32, u32)> {
        match self {
            ImageData::Rgba { width, height, .. } => Some((*width, *height)),
            ImageData::Encoded(b) => encoded_size(b),
        }
    }

    pub fn is_jpeg(&self) -> bool {
        matches!(self, ImageData::Encoded(b) if b.starts_with(&[0xFF, 0xD8]))
    }
}

/// Width and height from a PNG header or a JPEG start-of-frame.
pub fn encoded_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.starts_with(b"\x89PNG") && b.len() >= 24 {
        let w = u32::from_be_bytes(b[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(b[20..24].try_into().ok()?);
        return Some((w, h));
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                return None;
            }
            let marker = b[i + 1];
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
                let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
                let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
                return Some((w, h));
            }
            i += 2 + len;
        }
    }
    None
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "prim", rename_all = "snake_case")]
pub enum Prim {
    /// Rings filled together (even-odd, so inner rings are holes), each stroked.
    Path {
        rings: Vec<Vec<P2>>,
        closed: bool,
        pen: Option<Pen>,
        fill: Option<Rgb>,
    },
    Circle {
        center: P2,
        radius: f64,
        pen: Option<Pen>,
        fill: Option<Rgb>,
    },
    /// `size` is the cap height, mm; `angle` degrees counter-clockwise about `pos`.
    Text {
        pos: P2,
        text: String,
        size: f64,
        angle: f64,
        h: HAlign,
        v: VAlign,
        bold: bool,
        color: Rgb,
    },
    Image {
        min: P2,
        max: P2,
        data: ImageData,
    },
    /// Children clipped to a rectangle.
    Clip {
        min: P2,
        max: P2,
        prims: Vec<Prim>,
    },
}

impl Prim {
    pub fn line(a: P2, b: P2, pen: Pen) -> Prim {
        Prim::Path { rings: vec![vec![a, b]], closed: false, pen: Some(pen), fill: None }
    }

    pub fn polyline(points: Vec<P2>, closed: bool, pen: Pen) -> Prim {
        Prim::Path { rings: vec![points], closed, pen: Some(pen), fill: None }
    }

    pub fn rect(min: P2, max: P2, pen: Option<Pen>, fill: Option<Rgb>) -> Prim {
        let r = vec![min, [max[0], min[1]], max, [min[0], max[1]]];
        Prim::Path { rings: vec![r], closed: true, pen, fill }
    }

    pub fn text(pos: P2, text: impl Into<String>, size: f64) -> Prim {
        Prim::Text {
            pos,
            text: text.into(),
            size,
            angle: 0.0,
            h: HAlign::Left,
            v: VAlign::Baseline,
            bold: false,
            color: Rgb::BLACK,
        }
    }
}

/// A clickable region the GUI needs: where a viewport or a label ended up.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ViewportHit {
    pub id: Id,
    pub min: P2,
    pub max: P2,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LabelHit {
    pub viewport: Id,
    pub fixture: Id,
    pub min: P2,
    pub max: P2,
}

/// One composed sheet.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Page {
    /// Paper size, mm.
    pub size: P2,
    pub prims: Vec<Prim>,
    pub viewports: Vec<ViewportHit>,
    pub labels: Vec<LabelHit>,
}

/// Hatch lines inside rings (even-odd), at `angle` degrees, `spacing` mm apart.
pub fn hatch(rings: &[Vec<P2>], angle: f64, spacing: f64) -> Vec<[P2; 2]> {
    let (s, c) = angle.to_radians().sin_cos();
    // Rotate into a frame where the hatch lines are horizontal.
    let to = |p: P2| [p[0] * c + p[1] * s, -p[0] * s + p[1] * c];
    let back = |q: P2| [q[0] * c - q[1] * s, q[0] * s + q[1] * c];
    let rot: Vec<Vec<P2>> = rings.iter().map(|r| r.iter().map(|p| to(*p)).collect()).collect();
    let (mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in rot.iter().flatten() {
        y0 = y0.min(p[1]);
        y1 = y1.max(p[1]);
    }
    if y1 <= y0 || y1.is_nan() || spacing <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut y = (y0 / spacing).floor() * spacing + spacing * 0.5;
    let mut guard = 0;
    while y < y1 && guard < 20_000 {
        guard += 1;
        let mut xs: Vec<f64> = Vec::new();
        for r in &rot {
            let n = r.len();
            for i in 0..n {
                let (a, b) = (r[i], r[(i + 1) % n]);
                if (a[1] <= y) != (b[1] <= y) {
                    xs.push(a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        for pair in xs.chunks_exact(2) {
            if pair[1] - pair[0] > 1e-6 {
                out.push([back([pair[0], y]), back([pair[1], y])]);
            }
        }
        y += spacing;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hatch_fills_a_square_and_skips_its_hole() {
        let outer = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let lines = hatch(std::slice::from_ref(&outer), 0.0, 1.0);
        assert_eq!(lines.len(), 10);
        assert!(lines.iter().all(|l| (l[1][0] - l[0][0] - 10.0).abs() < 1e-9));
        let hole = vec![[3.0, 3.0], [3.0, 7.0], [7.0, 7.0], [7.0, 3.0]];
        let lines = hatch(&[outer, hole], 0.0, 1.0);
        // Rows through the hole are split in two.
        assert_eq!(lines.len(), 14);
        let diag = hatch(&[vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]], 45.0, 1.0);
        assert!(diag.len() > 10);
    }

    #[test]
    fn png_size_from_header() {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend_from_slice(&40u32.to_be_bytes());
        b.extend_from_slice(&20u32.to_be_bytes());
        assert_eq!(encoded_size(&b), Some((40, 20)));
    }
}
