//! Lengths as people see and type them: the document's project unit
//! (`stagecraft_core::units::Units`, set with `doc.set_units`). Every field, label and
//! readout that shows a length goes through here; commands still get mm.

use stagecraft_core::units::Units;

use crate::app::Studio;
use crate::i18n::tf;

/// The open document's project unit.
pub fn of(s: &Studio) -> Units {
    Units::of(&s.engine.doc().meta)
}

/// A field label with the unit: "Width (m)".
pub fn label(u: &Units, text: &str) -> String {
    format!("{text} ({})", u.label())
}

/// A typed length in mm, or a status bar error naming the text.
pub fn parse_or_error(s: &mut Studio, text: &str) -> Option<f64> {
    let r = of(s).parse(text);
    if r.is_none() {
        s.ui.error(tf("status.bad_number", &[("text", &text)]));
    }
    r
}

/// A plain number (kg, counts, factors): point or comma, no units.
pub fn parse_number(text: &str) -> Option<f64> {
    text.trim().replace(',', ".").parse().ok().filter(|v: &f64| v.is_finite())
}

/// A plain number without trailing zeros, one decimal at most (kg, degrees).
pub fn fmt_number(v: f64) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r.fract() == 0.0 { format!("{r:.0}") } else { format!("{r:.1}") }
}
