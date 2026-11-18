//! The embedded fonts (Roboto, shared with the GUI in `assets/fonts/`) and text
//! metrics from them.
//!
//! Text sizes in this crate are cap heights in mm, as in plan text. Roboto's cap height
//! is 1456 of 2048 units per em.

use std::sync::OnceLock;

pub static REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Roboto-Regular.ttf");
pub static BOLD: &[u8] = include_bytes!("../../../assets/fonts/Roboto-Bold.ttf");

/// Cap height / em size.
pub const CAP_RATIO: f64 = 1456.0 / 2048.0;
/// Descender below the baseline / em size (for boxes around text).
pub const DESCENT_RATIO: f64 = 0.21;

/// Em size for a cap height.
pub fn em(cap: f64) -> f64 {
    cap / CAP_RATIO
}

struct Advances {
    /// Advance per char code below 0x2500, in em; `NaN` where the font has no glyph.
    table: Vec<f32>,
    fallback: f32,
}

fn load(data: &'static [u8]) -> Advances {
    let face = ttf_parser::Face::parse(data, 0).expect("embedded font parses");
    let upem = face.units_per_em() as f32;
    let adv = |c: char| face.glyph_index(c).and_then(|g| face.glyph_hor_advance(g)).map(|a| a as f32 / upem);
    let table: Vec<f32> = (0u32..0x2500).map(|c| char::from_u32(c).and_then(adv).unwrap_or(f32::NAN)).collect();
    let fallback = adv('n').unwrap_or(0.55);
    Advances { table, fallback }
}

fn advances(bold: bool) -> &'static Advances {
    static R: OnceLock<Advances> = OnceLock::new();
    static B: OnceLock<Advances> = OnceLock::new();
    if bold { B.get_or_init(|| load(BOLD)) } else { R.get_or_init(|| load(REGULAR)) }
}

/// Width of a single line of text in mm, for a cap height in mm.
pub fn text_width(text: &str, cap: f64, bold: bool) -> f64 {
    let a = advances(bold);
    let ems: f32 =
        text.chars().map(|c| a.table.get(c as usize).copied().filter(|v| !v.is_nan()).unwrap_or(a.fallback)).sum();
    ems as f64 * em(cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_grow_with_text_and_size() {
        let a = text_width("101", 2.0, false);
        assert!(a > 2.0 && a < 6.0, "{a}");
        assert!(text_width("1011", 2.0, false) > a);
        assert!((text_width("101", 4.0, false) - 2.0 * a).abs() < 1e-9);
        assert!(text_width("Lighting", 2.0, true) > text_width("Lighting", 2.0, false));
    }
}
