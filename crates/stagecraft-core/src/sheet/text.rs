//! The sheet-text catalog: every word printed on a sheet by StageCraft itself (title
//! block labels, legend, default view names, fixture list headers), per
//! [`Language`]. Title block templates refer to entries as `{t:key}`.

use crate::sheet::{Side, ViewKind};
use crate::units::Language;

/// `(key, English, German)`.
const CATALOG: &[(&str, &str, &str)] = &[
    // Title blocks.
    ("project", "Project", "Projekt"),
    ("production", "Production", "Produktion"),
    ("venue", "Venue", "Spielstätte"),
    ("drawing", "Drawing", "Zeichnung"),
    ("sheet", "Sheet", "Blatt"),
    ("drawn_by", "Drawn by", "Gezeichnet"),
    ("number", "No.", "Nr."),
    ("company", "Company", "Firma"),
    ("scale", "Scale", "Maßstab"),
    ("date", "Date", "Datum"),
    ("revision", "Rev.", "Rev."),
    ("date_rev", "Date / Rev", "Datum / Rev"),
    // Legend and fixture list.
    ("legend", "Legend", "Legende"),
    ("fixtures", "fixtures", "Geräte"),
    ("fixture", "fixture", "Gerät"),
    ("seats", "seats", "Sitzplätze"),
    ("status_draft", "DRAFT", "ENTWURF"),
    ("status_preliminary", "PRELIMINARY", "VORABZUG"),
    ("seat", "seat", "Sitzplatz"),
    ("iron", "Safety curtain", "Feuerschutzvorhang"),
    ("iron_keep_clear", "Safety curtain — keep clear", "Feuerschutzvorhang — freihalten"),
    ("generic_fixture", "Generic fixture", "Scheinwerfer (allgemein)"),
    ("channels", "ch", "Kan."),
    ("col_fid", "FID", "FID"),
    ("col_name", "Name", "Name"),
    ("col_type", "Type", "Typ"),
    ("col_mode", "Mode", "Modus"),
    ("col_channels", "Ch", "Kan."),
    ("col_address", "Address", "Adresse"),
    ("col_gel", "Gel", "Farbe"),
    ("col_circuit", "Circuit", "Stromkreis"),
    ("col_note", "Note", "Notiz"),
    ("notes", "Notes", "Anmerkungen"),
    // Views.
    ("view_plan", "Plan", "Grundriss"),
    ("view_front", "Front elevation", "Ansicht vorne"),
    ("view_back", "Back elevation", "Ansicht hinten"),
    ("view_left", "Left elevation", "Ansicht links"),
    ("view_right", "Right elevation", "Ansicht rechts"),
    ("view_section", "Section", "Schnitt"),
    ("view_plan_3d", "Plan from 3D", "Grundriss aus 3D"),
    ("view_rendered", "3D view", "3D-Ansicht"),
    ("view_fixture_list", "Fixture list", "Geräteliste"),
    ("rendered_placeholder", "3D view — rendered in the app", "3D-Ansicht — wird in der App gerendert"),
];

/// The text for `key`, or the key itself when the catalog has none.
pub fn tr(lang: Language, key: &str) -> &str {
    match CATALOG.iter().find(|e| e.0 == key) {
        Some((_, en, de)) => match lang {
            Language::En => en,
            Language::De => de,
        },
        None => key,
    }
}

/// Catalog key of a view kind's default name.
pub fn view_key(v: &ViewKind) -> &'static str {
    match v {
        ViewKind::Plan => "view_plan",
        ViewKind::Elevation { side } => match side {
            Side::Front => "view_front",
            Side::Back => "view_back",
            Side::Left => "view_left",
            Side::Right => "view_right",
        },
        ViewKind::Section { .. } => "view_section",
        ViewKind::PlanFrom3d { .. } => "view_plan_3d",
        ViewKind::Rendered { .. } => "view_rendered",
        ViewKind::FixtureList => "view_fixture_list",
    }
}

/// A view's default name in a language.
pub fn view_name(lang: Language, v: &ViewKind) -> String {
    tr(lang, view_key(v)).to_owned()
}

/// Every key, for tests.
pub fn keys() -> impl Iterator<Item = &'static str> {
    CATALOG.iter().map(|e| e.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_has_both_languages_and_unique_keys() {
        let mut seen = std::collections::HashSet::new();
        for (k, en, de) in CATALOG {
            assert!(seen.insert(*k), "duplicate {k}");
            assert!(!en.is_empty() && !de.is_empty(), "{k}");
        }
        assert_eq!(tr(Language::De, "production"), "Produktion");
        assert_eq!(tr(Language::En, "nope"), "nope");
    }
}

/// A date as sheets print it: ISO `YYYY-MM-DD` becomes `DD.MM.YYYY` in German and stays
/// ISO in English; anything else is printed as written.
pub fn date(lang: Language, s: &str) -> String {
    let t = s.trim();
    let p: Vec<&str> = t.split('-').collect();
    let iso = p.len() == 3
        && p[0].len() == 4
        && p[1].len() == 2
        && p[2].len() == 2
        && p.iter().all(|x| x.chars().all(|c| c.is_ascii_digit()));
    match (iso, lang) {
        (true, Language::De) => format!("{}.{}.{}", p[2], p[1], p[0]),
        _ => t.to_owned(),
    }
}

/// The status stamp's text: `draft`/`preliminary` translated, other text upper-cased.
pub fn status(lang: Language, s: &str) -> String {
    match s.trim().to_lowercase().as_str() {
        "" => String::new(),
        "draft" | "entwurf" => tr(lang, "status_draft").to_owned(),
        "preliminary" | "vorabzug" => tr(lang, "status_preliminary").to_owned(),
        other => other.to_uppercase(),
    }
}

#[cfg(test)]
mod date_tests {
    use super::*;
    #[test]
    fn dates_and_status_follow_the_language() {
        assert_eq!(date(Language::De, "2026-11-18"), "18.11.2026");
        assert_eq!(date(Language::En, "2026-11-18"), "2026-11-18");
        assert_eq!(date(Language::De, "Mai 2026"), "Mai 2026");
        assert_eq!(status(Language::De, "draft"), "ENTWURF");
        assert_eq!(status(Language::En, "draft"), "DRAFT");
        assert_eq!(status(Language::En, "for approval"), "FOR APPROVAL");
    }
}
