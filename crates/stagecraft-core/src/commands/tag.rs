use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::{ensure_exists, resolve_tag};
use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::model::{Document, Id, Tag};

pub(super) fn register(r: &mut Registry) {
    r.add("tag.list", Kind::Query, "Tags with object counts", list);
    r.add("tag.create", Kind::Edit, "Create a tag", create);
    r.add("tag.update", Kind::Edit, "Rename or recolour a tag", update);
    r.add("tag.delete", Kind::Edit, "Delete a tag and remove it from all objects", delete);
    r.add("object.tag", Kind::Edit, "Add tags to objects; unknown tag names are created", add);
    r.add("object.untag", Kind::Edit, "Remove tags from objects", remove);
}

#[derive(Deserialize, JsonSchema)]
struct NoArgs {}

fn list(ctx: &mut Ctx, _: NoArgs) -> CmdResult {
    let doc = ctx.doc();
    let mut v: Vec<_> = doc.tags.values().collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    let v: Vec<_> = v
        .into_iter()
        .map(|t| {
            let n = doc.objects.values().filter(|o| o.tags.contains(&t.id)).count();
            json!({ "id": t.id, "name": t.name, "color": t.color, "objects": n })
        })
        .collect();
    Ok(json!(v))
}

fn new_tag(doc: &mut Document, name: &str, color: Option<String>) -> Result<Id, CmdError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CmdError::invalid_args("tag name is empty"));
    }
    if doc.tags.values().any(|t| t.name == name) {
        return Err(CmdError::invalid(format!("a tag named `{name}` exists")));
    }
    let t = Tag { id: Uuid::new_v4(), name: name.to_owned(), color };
    let id = t.id;
    doc.tags.insert(id, t);
    Ok(id)
}

#[derive(Deserialize, JsonSchema)]
struct CreateArgs {
    name: String,
    /// `#RRGGBB`.
    color: Option<String>,
}

fn create(ctx: &mut Ctx, a: CreateArgs) -> CmdResult {
    let id = new_tag(ctx.doc_mut(), &a.name, a.color)?;
    Ok(json!({ "id": id }))
}

#[derive(Deserialize, JsonSchema)]
struct UpdateArgs {
    /// Tag id or name.
    tag: String,
    name: Option<String>,
    /// `#RRGGBB`; empty string clears it.
    color: Option<String>,
}

fn update(ctx: &mut Ctx, a: UpdateArgs) -> CmdResult {
    let id = resolve_tag(ctx.doc(), &a.tag).ok_or_else(|| CmdError::not_found("tag", &a.tag))?;
    if let Some(n) = &a.name
        && ctx.doc().tags.values().any(|t| t.id != id && &t.name == n)
    {
        return Err(CmdError::invalid(format!("a tag named `{n}` exists")));
    }
    let t = ctx.doc_mut().tags.get_mut(&id).expect("resolved");
    if let Some(n) = a.name {
        t.name = n;
    }
    if let Some(c) = a.color {
        t.color = (!c.is_empty()).then_some(c);
    }
    Ok(json!(t))
}

#[derive(Deserialize, JsonSchema)]
struct DeleteArgs {
    /// Tag id or name.
    tag: String,
}

fn delete(ctx: &mut Ctx, a: DeleteArgs) -> CmdResult {
    let id = resolve_tag(ctx.doc(), &a.tag).ok_or_else(|| CmdError::not_found("tag", &a.tag))?;
    let doc = ctx.doc_mut();
    doc.tags.remove(&id);
    let mut n = 0;
    let tagged: Vec<Id> = doc.objects.values().filter(|o| o.tags.contains(&id)).map(|o| o.id).collect();
    for oid in &tagged {
        doc.objects.get_mut(oid).expect("listed").tags.retain(|t| *t != id);
        n += 1;
    }
    doc.edit_viewports(|v| {
        v.hidden_tags.retain(|t| *t != id);
        v.tag_lines.remove(&id);
    });
    Ok(json!({ "untagged": n }))
}

#[derive(Deserialize, JsonSchema)]
struct TagArgs {
    ids: Vec<Id>,
    /// Tag ids or names.
    tags: Vec<String>,
}

/// Tag ids for ids or names; unknown names become new tags.
pub(super) fn ensure_tags(doc: &mut Document, refs: &[String]) -> Result<Vec<Id>, CmdError> {
    let mut out = Vec::new();
    for t in refs {
        let id = match resolve_tag(doc, t) {
            Some(id) => id,
            None => new_tag(doc, t, None)?,
        };
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out.sort();
    Ok(out)
}

fn add(ctx: &mut Ctx, a: TagArgs) -> CmdResult {
    ensure_exists(ctx.doc(), &a.ids)?;
    let tag_ids = ensure_tags(ctx.doc_mut(), &a.tags)?;
    let doc = ctx.doc_mut();
    for oid in &a.ids {
        let o = doc.objects.get_mut(oid).expect("checked");
        for t in &tag_ids {
            if let Err(i) = o.tags.binary_search(t) {
                o.tags.insert(i, *t);
            }
        }
    }
    Ok(json!({ "tags": tag_ids }))
}

fn remove(ctx: &mut Ctx, a: TagArgs) -> CmdResult {
    ensure_exists(ctx.doc(), &a.ids)?;
    let tag_ids: Vec<Id> = a.tags.iter().filter_map(|t| resolve_tag(ctx.doc(), t)).collect();
    let doc = ctx.doc_mut();
    for oid in &a.ids {
        let o = doc.objects.get_mut(oid).expect("checked");
        o.tags.retain(|t| !tag_ids.contains(t));
    }
    Ok(json!({}))
}
