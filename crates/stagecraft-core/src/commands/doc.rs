use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::file;
use crate::model::Document;

pub(super) fn register(r: &mut Registry) {
    r.add("doc.info", Kind::Query, "File, dirty flag, counts, meta", info);
    r.add("doc.new", Kind::Meta, "Start an empty document (discards unsaved changes)", new);
    r.add("doc.open", Kind::Meta, "Open a .stagecraft file", open);
    r.add("doc.save", Kind::Meta, "Save to the current file or to `path`", save);
    r.add(
        "doc.set_meta",
        Kind::Edit,
        "Set project, venue, drawn-by, revision, company, date, logo (title block fields)",
        set_meta,
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
    let m = &mut ctx.doc_mut().meta;
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
    Ok(json!({ "project": m.project, "venue": m.venue, "drawn_by": m.drawn_by, "revision": m.revision,
               "company": m.company, "date": m.date, "logo": m.logo.is_some() }))
}
