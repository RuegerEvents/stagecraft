use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::file;
use crate::model::Document;
use crate::sheet::text;
use crate::units::{DimFormat, Language, LengthUnit};

pub(super) fn register(r: &mut Registry) {
    r.add("doc.info", Kind::Query, "File, dirty flag, counts, meta", info);
    r.add("doc.new", Kind::Meta, "Start an empty document (discards unsaved changes)", new);
    r.add("doc.open", Kind::Meta, "Open a .stagecraft file", open);
    r.add("doc.save", Kind::Meta, "Save to the current file or to `path`", save);
    r.add(
        "doc.set_meta",
        Kind::Edit,
        "Set title block fields (project, venue, address, dates, company, drawn-by, website, logo …), sheet language and dimension format",
        set_meta,
    );
    r.add(
        "doc.set_units",
        Kind::Edit,
        "Set the project unit the GUI shows and takes lengths in (mm, cm, m, ft-in) and its precision. Display and typing only: the document and every command argument stay in mm",
        set_units,
    );
}

#[derive(Deserialize, JsonSchema)]
struct NoArgs {}

fn info(ctx: &mut Ctx, _: NoArgs) -> CmdResult {
    let s = &ctx.state;
    Ok(json!({
        "path": s.path,
        "dirty": s.dirty,
        "meta": s.doc.meta,
        "layers": s.doc.layers.len(),
        "tags": s.doc.tags.len(),
        "objects": s.doc.objects.len(),
        "can_undo": !s.undo.is_empty(),
        "can_redo": !s.redo.is_empty(),
    }))
}

#[derive(Deserialize, JsonSchema)]
struct NewArgs {
    /// Must be true when the current document has unsaved changes.
    #[serde(default)]
    discard: bool,
}

fn new(ctx: &mut Ctx, a: NewArgs) -> CmdResult {
    if ctx.state.dirty && !a.discard {
        return Err(CmdError::invalid("the document has unsaved changes; save it or pass discard: true"));
    }
    ctx.state.reset(Document::new(), None);
    ctx.mark_changed();
    Ok(json!({}))
}

#[derive(Deserialize, JsonSchema)]
struct OpenArgs {
    /// Path to a .stagecraft file.
    path: PathBuf,
    /// Must be true when the current document has unsaved changes.
    #[serde(default)]
    discard: bool,
}

fn open(ctx: &mut Ctx, a: OpenArgs) -> CmdResult {
    if ctx.state.dirty && !a.discard {
        return Err(CmdError::invalid("the document has unsaved changes; save it or pass discard: true"));
    }
    let doc = file::load(&a.path)?;
    let n = doc.objects.len();
    ctx.state.reset(doc, Some(a.path));
    ctx.mark_changed();
    Ok(json!({ "objects": n }))
}

#[derive(Deserialize, JsonSchema)]
struct SaveArgs {
    /// Where to save. Defaults to the file the document came from. Becomes the
    /// document's file.
    path: Option<PathBuf>,
}

fn save(ctx: &mut Ctx, a: SaveArgs) -> CmdResult {
    let path = a
        .path
        .or_else(|| ctx.state.path.clone())
        .ok_or_else(|| CmdError::invalid_args("the document has no file yet; pass `path`"))?;
    file::save(&ctx.state.doc, &path)?;
    ctx.state.path = Some(path.clone());
    ctx.state.dirty = false;
    Ok(json!({ "path": path }))
}

#[derive(Deserialize, JsonSchema)]
struct MetaArgs {
    project: Option<String>,
    venue: Option<String>,
    drawn_by: Option<String>,
    revision: Option<String>,
    /// Company shown in title blocks.
    company: Option<String>,
    /// Date printed in title blocks, free text.
    date: Option<String>,
    /// PNG or JPEG file for the title block logo; empty string removes the logo.
    logo_file: Option<PathBuf>,
    /// Venue address, lines separated by `\n`.
    venue_address: Option<String>,
    /// Production dates, free text, e.g. `02.05.2025 – 05.05.2025`. Compact title
    /// blocks show the date instead while this is empty.
    production_dates: Option<String>,
    website: Option<String>,
    /// Language of sheet text: `en` (default) or `de`. Viewports still named by
    /// default ("Plan", "Section" …) are renamed into the new language.
    sheet_language: Option<Language>,
    /// Dimension text: `project` (follow the project unit, default of new documents),
    /// `m_sup_mm` (6.15⁵), `m` (6.155 m), `mm` (6155), `ft_in` (20' 2 5/16") or `ft`
    /// (20.19').
    dimension_format: Option<DimFormat>,
    /// Decimal separator of lengths: `.` or `,`; `auto` (or empty) follows the sheet
    /// language (en `.`, de `,`).
    decimal_separator: Option<String>,
}

fn set_meta(ctx: &mut Ctx, a: MetaArgs) -> CmdResult {
    let logo = match &a.logo_file {
        None => None,
        Some(p) if p.as_os_str().is_empty() => Some(None),
        Some(p) => {
            let data = std::fs::read(p).map_err(|e| CmdError::io(format!("{}: {e}", p.display())))?;
            let png = data.starts_with(b"\x89PNG");
            let jpeg = data.starts_with(&[0xFF, 0xD8]);
            if !(png || jpeg) {
                return Err(CmdError::invalid_args("the logo must be a PNG or JPEG file"));
            }
            Some(Some(crate::sheet::base64_encode(&data)))
        }
    };
    let separator = match a.decimal_separator.as_deref() {
        None => None,
        Some("" | "auto") => Some(None),
        Some(".") => Some(Some('.')),
        Some(",") => Some(Some(',')),
        Some(other) => {
            return Err(CmdError::invalid_args(format!("decimal separator `{other}`: use `.`, `,` or `auto`")));
        }
    };
    if let Some(lang) = a.sheet_language {
        let old = ctx.doc().meta.sheet_language;
        if old != lang {
            rename_default_views(ctx.doc_mut(), old, lang);
        }
    }
    let m = &mut ctx.doc_mut().meta;
    if let Some(v) = a.sheet_language {
        m.sheet_language = v;
    }
    if let Some(v) = a.dimension_format {
        m.dimension_format = v;
    }
    if let Some(v) = separator {
        m.decimal_separator = v;
    }
    if let Some(v) = a.venue_address {
        m.venue_address = v;
    }
    if let Some(v) = a.production_dates {
        m.production_dates = v;
    }
    if let Some(v) = a.website {
        m.website = v;
    }
    if let Some(v) = a.company {
        m.company = v;
    }
    if let Some(v) = a.date {
        m.date = v;
    }
    if let Some(v) = logo {
        m.logo = v;
    }
    if let Some(v) = a.project {
        m.project = v;
    }
    if let Some(v) = a.venue {
        m.venue = v;
    }
    if let Some(v) = a.drawn_by {
        m.drawn_by = v;
    }
    if let Some(v) = a.revision {
        m.revision = v;
    }
    let mut out = serde_json::to_value(&*m).unwrap_or_default();
    out["logo"] = json!(m.logo.is_some());
    Ok(out)
}

#[derive(Deserialize, JsonSchema)]
struct UnitsArgs {
    /// Unit of lengths in the GUI (Inspector, tool options, heads-up boxes, table,
    /// status bar): `mm` (default), `cm`, `m` or `ft_in`. Only display and input:
    /// command arguments stay mm whatever is set here.
    length_unit: Option<LengthUnit>,
    /// Decimals for mm (0–2, default 0), cm (0–3, default 1) and m (1–4, default 3);
    /// for `ft_in` the finest inch fraction's denominator: 1, 2, 4, 8, 16 (default),
    /// 32 or 64. Omitted with a new `length_unit`: that unit's default; `-1` resets.
    precision: Option<i32>,
}

fn set_units(ctx: &mut Ctx, a: UnitsArgs) -> CmdResult {
    let cur = ctx.doc().meta.units;
    let unit = a.length_unit.unwrap_or(cur.length);
    let precision = match a.precision {
        Some(p) if p < 0 => None,
        Some(p) => {
            let p = u8::try_from(p).ok().filter(|p| unit.precisions().contains(p)).ok_or_else(|| {
                CmdError::invalid_args(format!(
                    "precision {p} does not fit {}: use one of {:?}",
                    unit.code(),
                    unit.precisions()
                ))
            })?;
            Some(p)
        }
        None if unit != cur.length => None,
        None => cur.precision,
    };
    let m = &mut ctx.doc_mut().meta;
    m.units.length = unit;
    m.units.precision = precision;
    let u = crate::units::Units::of(m);
    Ok(json!({ "length_unit": unit, "precision": u.precision, "label": u.label(), "example": u.with_unit(6155.0) }))
}

/// Viewports whose name is the default for their view in `from` get the one in `to`.
fn rename_default_views(doc: &mut Document, from: Language, to: Language) {
    let ids: Vec<_> = doc.sheets.keys().copied().collect();
    for id in ids {
        let s = doc.sheets.get_mut(&id).expect("listed");
        for v in &mut s.viewports {
            if v.name == text::view_name(from, &v.view) {
                v.name = text::view_name(to, &v.view);
            }
        }
    }
}
