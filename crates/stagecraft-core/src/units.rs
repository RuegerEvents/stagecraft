//! How lengths read on drawings: the sheet language, the dimension text format and the
//! decimal separator, all per document (`Meta`), and the text they give.
//!
//! The `m_sup_mm` format writes the millimetre digit as a Unicode superscript digit
//! (`6.15⁵`), so the value stays a plain string everywhere; whatever draws it turns
//! superscript runs into smaller raised glyphs ([`superscript_runs`]).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::Meta;

/// Language of everything printed on sheets (title block labels, legend, view names).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    En,
    De,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::En, Language::De];

    pub fn code(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::De => "de",
        }
    }

    /// The language's own name.
    pub fn name(self) -> &'static str {
        match self {
            Language::En => "English",
            Language::De => "Deutsch",
        }
    }

    pub fn decimal_separator(self) -> char {
        match self {
            Language::En => '.',
            Language::De => ',',
        }
    }
}

/// How dimension values are written.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DimFormat {
    /// Metres with centimetres, odd millimetres raised: `6.15⁵`, `4.00`.
    #[default]
    MSupMm,
    /// Metres with unit: `6.155 m`, `4.00 m`.
    M,
    /// Millimetres: `6155`.
    Mm,
}

impl DimFormat {
    pub const ALL: [DimFormat; 3] = [DimFormat::MSupMm, DimFormat::M, DimFormat::Mm];

    pub fn code(self) -> &'static str {
        match self {
            DimFormat::MSupMm => "m_sup_mm",
            DimFormat::M => "m",
            DimFormat::Mm => "mm",
        }
    }
}

/// A dimension format with its decimal separator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LengthFormat {
    pub format: DimFormat,
    pub separator: char,
}

impl Default for LengthFormat {
    fn default() -> Self {
        LengthFormat { format: DimFormat::MSupMm, separator: '.' }
    }
}

const SUP: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

impl LengthFormat {
    /// The document's format: its own separator, else the sheet language's.
    pub fn of(meta: &Meta) -> Self {
        LengthFormat {
            format: meta.dimension_format,
            separator: meta.decimal_separator.unwrap_or(meta.sheet_language.decimal_separator()),
        }
    }

    /// A length in mm as the format writes it.
    pub fn length(&self, mm: f64) -> String {
        let mm = mm.round();
        let neg = mm < 0.0;
        let a = mm.abs() as u64;
        let (m, rest) = (a / 1000, a % 1000);
        let sep = self.separator;
        let s = match self.format {
            DimFormat::Mm => a.to_string(),
            DimFormat::MSupMm => {
                let base = format!("{m}{sep}{:02}", rest / 10);
                if rest % 10 == 0 { base } else { format!("{base}{}", SUP[(rest % 10) as usize]) }
            }
            DimFormat::M => {
                if rest % 10 == 0 {
                    format!("{m}{sep}{:02} m", rest / 10)
                } else {
                    format!("{m}{sep}{rest:03} m")
                }
            }
        };
        if neg && a > 0 { format!("-{s}") } else { s }
    }

    /// An angle in degrees, one decimal.
    pub fn angle(&self, deg: f64) -> String {
        format!("{deg:.1}°").replace('.', &self.separator.to_string())
    }

    /// A plain decimal number (scale bars, levels) with the separator.
    pub fn number(&self, text: &str) -> String {
        if self.separator == '.' { text.to_owned() } else { text.replace('.', &self.separator.to_string()) }
    }
}

/// Splits text into runs of normal and superscript characters; superscript digits come
/// back as plain digits with `true`.
pub fn superscript_runs(text: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    for c in text.chars() {
        let (c, sup) = match SUP.iter().position(|s| *s == c) {
            Some(d) => (char::from(b'0' + d as u8), true),
            None => (c, false),
        };
        match out.last_mut() {
            Some((s, k)) if *k == sup => s.push(c),
            _ => out.push((c.to_string(), sup)),
        }
    }
    out
}

/// Whether the text has superscript digits.
pub fn has_superscript(text: &str) -> bool {
    text.chars().any(|c| SUP.contains(&c))
}

/// Cap height of superscript glyphs relative to the text's.
pub const SUP_SCALE: f64 = 0.62;
/// Baseline raise of superscript glyphs, in cap heights of the text.
pub const SUP_RAISE: f64 = 0.48;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let f = |format, separator| LengthFormat { format, separator };
        let sup = f(DimFormat::MSupMm, ',');
        assert_eq!(sup.length(6155.0), "6,15⁵");
        assert_eq!(sup.length(4000.0), "4,00");
        assert_eq!(sup.length(4064.4), "4,06⁴");
        assert_eq!(sup.length(-750.0), "-0,75");
        assert_eq!(sup.length(0.2), "0,00");
        assert_eq!(f(DimFormat::MSupMm, '.').length(15050.0), "15.05");
        assert_eq!(f(DimFormat::M, '.').length(6155.0), "6.155 m");
        assert_eq!(f(DimFormat::M, '.').length(2250.0), "2.25 m");
        assert_eq!(f(DimFormat::Mm, ',').length(6155.4), "6155");
        assert_eq!(sup.angle(90.0), "90,0°");
    }

    #[test]
    fn superscript_splits() {
        assert_eq!(superscript_runs("6,15⁵"), vec![("6,15".into(), false), ("5".into(), true)]);
        assert_eq!(superscript_runs("(1.2⁰)"), vec![("(1.2".into(), false), ("0".into(), true), (")".into(), false)]);
        assert!(!has_superscript("6.155 m"));
    }
}
