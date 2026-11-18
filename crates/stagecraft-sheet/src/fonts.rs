//! The embedded sheet fonts (Arimo, a Helvetica/Arial-metric sans under the OFL, in
//! `assets/fonts/`) and text metrics from them.
//!
//! Text sizes in this crate are cap heights in mm, as in plan text. Superscript digits
//! (`6.15⁵`) are drawn as smaller raised normal digits; [`runs`] says where.

use std::sync::OnceLock;

use stagecraft_core::units::{SUP_RAISE, SUP_SCALE, superscript_runs};

pub static REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Arimo-Regular.ttf");
pub static BOLD: &[u8] = include_bytes!("../../../assets/fonts/Arimo-Bold.ttf");

/// Descender below the baseline / em size (for boxes around text).
pub const DESCENT_RATIO: f64 = 0.21;

struct Metrics {
    /// Advance per char code below 0x2500, in em; `NaN` where the font has no glyph.
    table: Vec<f32>,
    fallback: f32,
    /// Cap height / em.
    cap: f64,
    /// Ascender / em (hhea).
    ascent: f64,
}

fn load(data: &'static [u8]) -> Metrics {
    let face = ttf_parser::Face::parse(data, 0).expect("embedded font parses");
    let upem = face.units_per_em() as f32;
    let adv = |c: char| face.glyph_index(c).and_then(|g| face.glyph_hor_advance(g)).map(|a| a as f32 / upem);
    let table: Vec<f32> = (0u32..0x2500).map(|c| char::from_u32(c).and_then(adv).unwrap_or(f32::NAN)).collect();
    let fallback = adv('n').unwrap_or(0.55);
    let cap = face.capital_height().map_or(0.716, |c| c as f64 / upem as f64);
    let ascent = face.ascender() as f64 / upem as f64;
    Metrics { table, fallback, cap, ascent }
}

fn metrics(bold: bool) -> &'static Metrics {
    static R: OnceLock<Metrics> = OnceLock::new();
    static B: OnceLock<Metrics> = OnceLock::new();
    if bold { B.get_or_init(|| load(BOLD)) } else { R.get_or_init(|| load(REGULAR)) }
}

/// Cap height / em size of the regular face.
pub fn cap_ratio() -> f64 {
    metrics(false).cap
}

/// Ascender / em size (where a line's top sits above its baseline in text layout).
pub fn ascent_ratio() -> f64 {
    metrics(false).ascent
}

/// Em size for a cap height.
pub fn em(cap: f64) -> f64 {
    cap / cap_ratio()
}

fn plain_width(text: &str, cap: f64, bold: bool) -> f64 {
    let a = metrics(bold);
    let ems: f32 =
        text.chars().map(|c| a.table.get(c as usize).copied().filter(|v| !v.is_nan()).unwrap_or(a.fallback)).sum();
    ems as f64 * em(cap)
}

/// One run of a line of text as drawn: offset of its baseline start from the line's
/// (along the baseline, and raised), its text and cap height.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub dx: f64,
    pub dy: f64,
    pub text: String,
    pub size: f64,
}

/// A line split into normal and superscript runs.
pub fn runs(text: &str, cap: f64, bold: bool) -> Vec<Run> {
    let mut x = 0.0;
    superscript_runs(text)
        .into_iter()
        .map(|(t, sup)| {
            let size = if sup { cap * SUP_SCALE } else { cap };
            let r = Run { dx: x, dy: if sup { cap * SUP_RAISE } else { 0.0 }, size, text: t };
            x += plain_width(&r.text, size, bold) + if sup { cap * 0.05 } else { 0.0 };
            r
        })
        .collect()
}

/// Width of a single line of text in mm, for a cap height in mm.
pub fn text_width(text: &str, cap: f64, bold: bool) -> f64 {
    if !stagecraft_core::units::has_superscript(text) {
        return plain_width(text, cap, bold);
    }
    let r = runs(text, cap, bold);
    r.last().map_or(0.0, |l| l.dx + plain_width(&l.text, l.size, bold))
}

/// Text broken into lines no wider than `width` mm (at spaces; `\n` always breaks).
/// A word longer than the line stays whole.
pub fn wrap(text: &str, cap: f64, bold: bool, width: f64) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        // Spaces inside a line are kept as typed (`a  ·  b`).
        let mut line = String::new();
        let mut started = false;
        for word in para.split(' ') {
            let candidate = if started { format!("{line} {word}") } else { word.to_owned() };
            if started && !word.is_empty() && text_width(&candidate, cap, bold) > width && !line.trim().is_empty() {
                out.push(line.trim_end().to_owned());
                line = word.to_owned();
            } else {
                line = candidate;
            }
            started = true;
        }
        out.push(line.trim().to_owned());
    }
    out
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

    #[test]
    fn superscript_is_smaller_and_raised() {
        let r = runs("6,15⁵", 2.0, false);
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].text, "5");
        assert!(r[1].size < 2.0 && r[1].dy > 0.5 && r[1].dx > 0.0);
        assert!(text_width("6,15⁵", 2.0, false) < text_width("6,155", 2.0, false));
    }

    #[test]
    fn wraps_at_spaces() {
        let l = wrap("The Greatest Showman Musical 2025", 2.3, true, 40.0);
        assert!(l.len() >= 2, "{l:?}");
        assert!(l.iter().all(|x| text_width(x, 2.3, true) <= 40.0));
        assert_eq!(wrap("a\nb", 2.0, false, 100.0), vec!["a", "b"]);
    }
}
