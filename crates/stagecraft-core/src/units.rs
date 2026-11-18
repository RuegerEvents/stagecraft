//! How lengths read: the project unit (what the GUI shows and takes, [`Units`]), and on
//! drawings the sheet language, the dimension text format and the decimal separator, all
//! per document (`Meta`), and the text they give.
//!
//! The document and every command argument stay in millimetres; the project unit is only
//! how people see and type lengths.
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
    /// The project unit and its precision (default for new documents): mm `6155`, cm
    /// `615.5`, m `6.155`, ft-in `20' 2 5/16"`.
    #[default]
    Project,
    /// Metres with centimetres, odd millimetres raised: `6.15⁵`, `4.00`.
    MSupMm,
    /// Metres with unit: `6.155 m`, `4.00 m`.
    M,
    /// Millimetres: `6155`.
    Mm,
    /// Feet and inches to 1/16": `20' 2 5/16"`.
    FtIn,
    /// Decimal feet, two decimals: `20.19'`.
    Ft,
}

impl DimFormat {
    pub const ALL: [DimFormat; 6] =
        [DimFormat::Project, DimFormat::MSupMm, DimFormat::M, DimFormat::Mm, DimFormat::FtIn, DimFormat::Ft];

    pub fn code(self) -> &'static str {
        match self {
            DimFormat::Project => "project",
            DimFormat::MSupMm => "m_sup_mm",
            DimFormat::M => "m",
            DimFormat::Mm => "mm",
            DimFormat::FtIn => "ft_in",
            DimFormat::Ft => "ft",
        }
    }

    /// What documents written before format 5 without the field meant: metres with
    /// raised millimetres.
    pub fn legacy() -> Self {
        DimFormat::MSupMm
    }
}

/// The unit lengths are shown and typed in across the GUI.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    #[default]
    Mm,
    Cm,
    M,
    /// Feet and inches (`15' 6"`).
    FtIn,
}

impl LengthUnit {
    pub const ALL: [LengthUnit; 4] = [LengthUnit::Mm, LengthUnit::Cm, LengthUnit::M, LengthUnit::FtIn];

    pub fn code(self) -> &'static str {
        match self {
            LengthUnit::Mm => "mm",
            LengthUnit::Cm => "cm",
            LengthUnit::M => "m",
            LengthUnit::FtIn => "ft_in",
        }
    }

    /// The short label next to fields: `mm`, `cm`, `m`, `ft-in`.
    pub fn label(self) -> &'static str {
        match self {
            LengthUnit::FtIn => "ft-in",
            u => u.code(),
        }
    }

    /// Decimals for mm/cm/m; for ft-in the finest inch fraction's denominator.
    pub fn default_precision(self) -> u8 {
        match self {
            LengthUnit::Mm => 0,
            LengthUnit::Cm => 1,
            LengthUnit::M => 3,
            LengthUnit::FtIn => 16,
        }
    }

    /// The precisions the unit takes.
    pub fn precisions(self) -> &'static [u8] {
        match self {
            LengthUnit::Mm => &[0, 1, 2],
            LengthUnit::Cm => &[0, 1, 2, 3],
            LengthUnit::M => &[1, 2, 3, 4],
            LengthUnit::FtIn => &[1, 2, 4, 8, 16, 32, 64],
        }
    }

    /// mm per unit (inches for ft-in: what a bare number means there).
    pub fn mm(self) -> f64 {
        match self {
            LengthUnit::Mm => 1.0,
            LengthUnit::Cm => 10.0,
            LengthUnit::M => 1000.0,
            LengthUnit::FtIn => INCH,
        }
    }
}

/// The project unit as stored in the document (`meta.units`).
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash, JsonSchema)]
pub struct UnitSettings {
    /// Unit of lengths in the GUI. Default mm.
    #[serde(default)]
    pub length: LengthUnit,
    /// Decimals (mm, cm, m) or the inch fraction's denominator (ft-in); `None` = the
    /// unit's default (mm 0, cm 1, m 3, ft-in 16).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<u8>,
}

const INCH: f64 = 25.4;
const FOOT: f64 = 304.8;

/// The project unit resolved for display and input: unit, precision and the document's
/// decimal separator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Units {
    pub unit: LengthUnit,
    /// Decimals, or the inch fraction's denominator for ft-in.
    pub precision: u8,
    pub separator: char,
}

impl Default for Units {
    fn default() -> Self {
        Units::new(LengthUnit::Mm)
    }
}

impl Units {
    /// The unit with its default precision and a point.
    pub fn new(unit: LengthUnit) -> Self {
        Units { unit, precision: unit.default_precision(), separator: '.' }
    }

    /// The document's project unit.
    pub fn of(meta: &Meta) -> Self {
        let unit = meta.units.length;
        let precision = meta.units.precision.filter(|p| unit.precisions().contains(p));
        Units {
            unit,
            precision: precision.unwrap_or(unit.default_precision()),
            separator: meta.decimal_separator.unwrap_or(meta.sheet_language.decimal_separator()),
        }
    }

    /// The label next to fields and in titles: `mm`, `cm`, `m`, `ft-in`.
    pub fn label(&self) -> &'static str {
        self.unit.label()
    }

    /// A length as a field shows it: in the unit, no trailing zeros, no unit name
    /// (ft-in carries its marks: `15' 6"`).
    pub fn field(&self, mm: f64) -> String {
        match self.unit {
            LengthUnit::FtIn => feet_inches(mm, self.precision),
            u => trim_zeros(&decimal(mm / u.mm(), self.precision, self.separator), self.separator),
        }
    }

    /// A length with all its decimals (status bar, live values), no unit name.
    pub fn fixed(&self, mm: f64) -> String {
        match self.unit {
            LengthUnit::FtIn => feet_inches(mm, self.precision),
            u => decimal(mm / u.mm(), self.precision, self.separator),
        }
    }

    /// [`Units::fixed`] with the unit: `6.155 m`, `6155 mm`, `20' 2"`.
    pub fn with_unit(&self, mm: f64) -> String {
        match self.unit {
            LengthUnit::FtIn => self.fixed(mm),
            u => format!("{} {}", self.fixed(mm), u.label()),
        }
    }

    /// [`Units::field`] with the unit name.
    pub fn field_with_unit(&self, mm: f64) -> String {
        match self.unit {
            LengthUnit::FtIn => self.field(mm),
            u => format!("{} {}", self.field(mm), u.label()),
        }
    }

    /// A typed length in mm. A bare number is in the project unit (inches for ft-in);
    /// an explicit unit wins: `1500mm`, `150 cm`, `2,5m`, `15'`, `15' 6"`, `15'-6 1/2"`,
    /// `6"`, `10ft`, `6in`. Decimal point or comma. `None` for anything else.
    pub fn parse(&self, text: &str) -> Option<f64> {
        let s = text.trim().replace(',', ".").replace('−', "-").to_lowercase();
        if s.is_empty() {
            return None;
        }
        if let Some(v) = parse_imperial(&s) {
            return Some(v);
        }
        let (num, factor) = if let Some(n) = s.strip_suffix("mm") {
            (n, 1.0)
        } else if let Some(n) = s.strip_suffix("cm") {
            (n, 10.0)
        } else if let Some(n) = s.strip_suffix('m') {
            (n, 1000.0)
        } else {
            (s.as_str(), self.unit.mm())
        };
        let v = number(num.trim())?;
        v.is_finite().then_some(v * factor)
    }
}

/// `x` with `decimals` decimals and the separator.
fn decimal(x: f64, decimals: u8, sep: char) -> String {
    let d = usize::from(decimals);
    let mut s = format!("{x:.d$}");
    if s.starts_with('-') && s[1..].chars().all(|c| c == '0' || c == '.') {
        s.remove(0);
    }
    if sep == '.' { s } else { s.replace('.', &sep.to_string()) }
}

fn trim_zeros(s: &str, sep: char) -> String {
    if !s.contains(sep) {
        return s.to_owned();
    }
    s.trim_end_matches('0').trim_end_matches(sep).to_owned()
}

/// Feet and inches to `1/den`: `15' 6"`, `20' 2 5/16"`, `6"`, `-1' 0"`.
fn feet_inches(mm: f64, den: u8) -> String {
    let den = u64::from(den.max(1));
    let n = (mm.abs() / INCH * den as f64).round() as u64;
    let neg = mm < 0.0 && n > 0;
    let per_foot = 12 * den;
    let (feet, rest) = (n / per_foot, n % per_foot);
    let (inches, mut num) = (rest / den, rest % den);
    let mut d = den;
    while num > 0 && num % 2 == 0 && d % 2 == 0 {
        num /= 2;
        d /= 2;
    }
    let inch = if num == 0 {
        format!("{inches}\"")
    } else if inches == 0 && feet == 0 {
        format!("{num}/{d}\"")
    } else {
        format!("{inches} {num}/{d}\"")
    };
    let s = if feet == 0 { inch } else { format!("{feet}' {inch}") };
    if neg { format!("-{s}") } else { s }
}

/// A number, or a fraction `1/2`, or a whole and a fraction `6 1/2`.
fn number(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (neg, body) = match s.strip_prefix('-') {
        Some(r) => (true, r.trim_start()),
        None => (false, s.strip_prefix('+').unwrap_or(s).trim_start()),
    };
    let frac = |t: &str| -> Option<f64> {
        let (a, b) = t.split_once('/')?;
        let (a, b): (f64, f64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
        (b != 0.0).then(|| a / b)
    };
    let v = if body.contains('/') {
        match body.split_once(' ') {
            Some((whole, f)) => whole.trim().parse::<f64>().ok()? + frac(f)?,
            None => frac(body)?,
        }
    } else {
        if body.starts_with(['-', '+']) {
            return None;
        }
        body.parse::<f64>().ok()?
    };
    v.is_finite().then_some(if neg { -v } else { v })
}

/// Feet and inches: `15'`, `15' 6"`, `15'6`, `15'-6 1/2"`, `6"`, `1/2"`, `15ft`, `6in`,
/// `15 ft 6 in`. Lower case, point decimals.
fn parse_imperial(s: &str) -> Option<f64> {
    let s = s.replace("feet", "'").replace("foot", "'").replace("ft", "'");
    let s = s.replace("inches", "\"").replace("inch", "\"").replace("in", "\"").replace("''", "\"");
    if !s.contains(['\'', '"']) {
        return None;
    }
    let (neg, s) = match s.trim().strip_prefix('-') {
        Some(r) => (true, r.trim_start().to_owned()),
        None => (false, s.trim().to_owned()),
    };
    let (feet, rest) = match s.split_once('\'') {
        Some((f, r)) => (number(f)?, r),
        None => (0.0, s.as_str()),
    };
    if feet < 0.0 {
        return None;
    }
    let rest = rest.trim().trim_start_matches('-').trim();
    let rest = rest.strip_suffix('"').unwrap_or(rest).trim();
    if rest.contains(['\'', '"']) {
        return None;
    }
    let inches = if rest.is_empty() { 0.0 } else { number(rest)? };
    if inches < 0.0 {
        return None;
    }
    let v = feet * FOOT + inches * INCH;
    v.is_finite().then_some(if neg { -v } else { v })
}

/// A dimension format with its decimal separator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LengthFormat {
    pub format: DimFormat,
    pub separator: char,
    /// The project unit [`DimFormat::Project`] follows.
    pub units: Units,
}

impl Default for LengthFormat {
    fn default() -> Self {
        LengthFormat { format: DimFormat::MSupMm, separator: '.', units: Units::default() }
    }
}

const SUP: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

impl LengthFormat {
    /// The document's format: its own separator, else the sheet language's.
    pub fn of(meta: &Meta) -> Self {
        LengthFormat {
            format: meta.dimension_format,
            separator: meta.decimal_separator.unwrap_or(meta.sheet_language.decimal_separator()),
            units: Units::of(meta),
        }
    }

    /// A length in mm as the format writes it.
    pub fn length(&self, mm: f64) -> String {
        match self.format {
            DimFormat::Project => {
                let u = Units { separator: self.separator, ..self.units };
                return u.fixed(mm);
            }
            DimFormat::FtIn => return feet_inches(mm, 16),
            DimFormat::Ft => return format!("{}'", decimal(mm / FOOT, 2, self.separator)),
            _ => {}
        }
        let mm = mm.round();
        let neg = mm < 0.0;
        let a = mm.abs() as u64;
        let (m, rest) = (a / 1000, a % 1000);
        let sep = self.separator;
        let s = match self.format {
            DimFormat::Mm | DimFormat::Project | DimFormat::FtIn | DimFormat::Ft => a.to_string(),
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
        let f = |format, separator| LengthFormat { format, separator, units: Units::default() };
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

    fn units(unit: LengthUnit, sep: char) -> Units {
        Units { separator: sep, ..Units::new(unit) }
    }

    #[test]
    fn fields_in_every_unit() {
        use LengthUnit::*;
        let rows: &[(LengthUnit, char, f64, &str, &str, &str)] = &[
            // unit, separator, mm, field, fixed, with unit
            (Mm, '.', 6155.0, "6155", "6155", "6155 mm"),
            (Mm, '.', 12.34, "12", "12", "12 mm"),
            (Mm, '.', -0.2, "0", "0", "0 mm"),
            (Cm, '.', 6155.0, "615.5", "615.5", "615.5 cm"),
            (Cm, ',', 6150.0, "615", "615,0", "615,0 cm"),
            (M, '.', 6155.0, "6.155", "6.155", "6.155 m"),
            (M, ',', 4000.0, "4", "4,000", "4,000 m"),
            (M, '.', -750.0, "-0.75", "-0.750", "-0.750 m"),
            (FtIn, '.', 15.0 * FOOT + 6.0 * INCH, "15' 6\"", "15' 6\"", "15' 6\""),
            (FtIn, '.', 6155.0, "20' 2 5/16\"", "20' 2 5/16\"", "20' 2 5/16\""),
            (FtIn, '.', 6.0 * INCH, "6\"", "6\"", "6\""),
            (FtIn, '.', 12.7, "1/2\"", "1/2\"", "1/2\""),
            (FtIn, '.', FOOT, "1' 0\"", "1' 0\"", "1' 0\""),
            (FtIn, '.', -FOOT, "-1' 0\"", "-1' 0\"", "-1' 0\""),
            (FtIn, '.', 0.0, "0\"", "0\"", "0\""),
        ];
        for &(unit, sep, mm, field, fixed, with) in rows {
            let u = units(unit, sep);
            assert_eq!(u.field(mm), field, "{unit:?} field {mm}");
            assert_eq!(u.fixed(mm), fixed, "{unit:?} fixed {mm}");
            assert_eq!(u.with_unit(mm), with, "{unit:?} with unit {mm}");
        }
        let m2 = Units { precision: 2, ..units(M, '.') };
        assert_eq!(m2.fixed(6155.0), "6.16");
        let quarter = Units { precision: 4, ..units(FtIn, '.') };
        assert_eq!(quarter.field(6155.0), "20' 2 1/4\"");
    }

    #[test]
    fn typing_in_every_unit() {
        use LengthUnit::*;
        let close = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-6);
        let rows: &[(LengthUnit, &str, f64)] = &[
            // Bare numbers are the project unit.
            (Mm, "1500", 1500.0),
            (Cm, "150", 1500.0),
            (M, "1.5", 1500.0),
            (M, "1,5", 1500.0),
            (FtIn, "6", 6.0 * INCH),
            (FtIn, "6 1/2", 6.5 * INCH),
            (Mm, "-1000", -1000.0),
            (Mm, "−1000", -1000.0),
            (Mm, "+20", 20.0),
            // Explicit units win, whatever the project unit.
            (Mm, "15m", 15000.0),
            (Mm, "2,5m", 2500.0),
            (Mm, "2.5 m", 2500.0),
            (M, "1500mm", 1500.0),
            (M, "45 cm", 450.0),
            (M, "15'", 15.0 * FOOT),
            (M, "15' 6\"", 15.0 * FOOT + 6.0 * INCH),
            (Mm, "15'6", 15.0 * FOOT + 6.0 * INCH),
            (Mm, "15'-6 1/2\"", 15.0 * FOOT + 6.5 * INCH),
            (Cm, "6\"", 6.0 * INCH),
            (Cm, "1/2\"", 0.5 * INCH),
            (Mm, "10ft", 10.0 * FOOT),
            (Mm, "15 ft 6 in", 15.0 * FOOT + 6.0 * INCH),
            (Mm, "6in", 6.0 * INCH),
            (Mm, "-2'", -2.0 * FOOT),
            (FtIn, "1500mm", 1500.0),
            (FtIn, "20' 2 5/16\"", 20.0 * FOOT + (2.0 + 5.0 / 16.0) * INCH),
            (Mm, "4,5 M", 4500.0),
        ];
        for &(unit, text, mm) in rows {
            let got = units(unit, '.').parse(text);
            assert!(close(got, mm), "{unit:?} {text:?}: {got:?}, want {mm}");
        }
        for bad in ["", "abc", "1/0", "15' 6' ", "--4", "m", "1.2.3"] {
            assert_eq!(units(Mm, '.').parse(bad), None, "{bad:?}");
        }
        // What a field shows parses back to the same length.
        for unit in LengthUnit::ALL {
            for sep in ['.', ','] {
                let u = units(unit, sep);
                for mm in [0.0, 6155.0, -750.0, 304.8, 12.7, 15000.0] {
                    let back = u.parse(&u.field(mm)).unwrap();
                    assert!(
                        (back - mm).abs() <= 0.5 * unit.mm() / 10f64.powi(i32::from(u.precision)) + 1.6,
                        "{unit:?} {mm} → {back}"
                    );
                }
            }
        }
    }

    #[test]
    fn project_dimension_format_follows_the_unit() {
        let f = |unit, separator| LengthFormat { format: DimFormat::Project, separator, units: Units::new(unit) };
        assert_eq!(f(LengthUnit::Mm, '.').length(6155.0), "6155");
        assert_eq!(f(LengthUnit::Cm, '.').length(6155.0), "615.5");
        assert_eq!(f(LengthUnit::M, ',').length(6155.0), "6,155");
        assert_eq!(f(LengthUnit::FtIn, '.').length(6155.0), "20' 2 5/16\"");
        let g = |format| LengthFormat { format, separator: '.', units: Units::default() };
        assert_eq!(g(DimFormat::FtIn).length(6155.0), "20' 2 5/16\"");
        assert_eq!(g(DimFormat::Ft).length(6155.0), "20.19'");
        assert_eq!(g(DimFormat::MSupMm).length(6155.0), "6.15⁵");
    }

    #[test]
    fn superscript_splits() {
        assert_eq!(superscript_runs("6,15⁵"), vec![("6,15".into(), false), ("5".into(), true)]);
        assert_eq!(superscript_runs("(1.2⁰)"), vec![("(1.2".into(), false), ("0".into(), true), (")".into(), false)]);
        assert!(!has_superscript("6.155 m"));
    }
}
