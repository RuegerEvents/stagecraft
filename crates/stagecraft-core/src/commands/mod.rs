//! All commands. Each submodule registers its own in `register`.
//!
//! Argument structs are the public API for AIs: their doc comments end up in the JSON
//! schema, so say units and defaults there.

mod annotate;
pub(crate) mod create;
mod doc;
pub(crate) mod fixture;
mod history;
mod layer;
mod mvr;
mod object;
mod rigging;
mod seating;
mod selection;
pub(crate) mod sheet;
mod sketch;
mod stage;
mod tag;

use serde_json::{Value, json};

use crate::command::{CmdError, Registry};
use crate::geom;
use crate::model::{Document, Id, Object};

pub(crate) fn register(r: &mut Registry) {
    doc::register(r);
    history::register(r);
    layer::register(r);
    tag::register(r);
    object::register(r);
    create::register(r);
    fixture::register(r);
    stage::register(r);
    rigging::register(r);
    seating::register(r);
    selection::register(r);
    sheet::register(r);
    mvr::register(r);
    sketch::register(r);
    annotate::register(r);
    r.add::<crate::engine::BatchArgs>(
        "batch",
        crate::command::Kind::Meta,
        "Run several edits as one undo step; all or nothing",
        |_, _| unreachable!("the engine runs batch itself"),
    );
}

/// Find a layer by id or exact name.
pub(crate) fn resolve_layer(doc: &Document, r: &str) -> Result<Id, CmdError> {
    if let Ok(id) = r.parse::<Id>()
        && doc.layers.contains_key(&id)
    {
        return Ok(id);
    }
    doc.layers.values().find(|l| l.name == r).map(|l| l.id).ok_or_else(|| CmdError::not_found("layer", r))
}

/// The layer given, or the first layer.
pub(crate) fn layer_or_default(doc: &Document, r: Option<&str>) -> Result<Id, CmdError> {
    match r {
        Some(r) => resolve_layer(doc, r),
        None => doc.layers_sorted().first().map(|l| l.id).ok_or_else(|| CmdError::invalid("the document has no layer")),
    }
}

/// Find a tag by id or exact name.
pub(crate) fn resolve_tag(doc: &Document, r: &str) -> Option<Id> {
    if let Ok(id) = r.parse::<Id>()
        && doc.tags.contains_key(&id)
    {
        return Some(id);
    }
    doc.tags.values().find(|t| t.name == r).map(|t| t.id)
}

/// The `id` field of a create command's result.
pub(crate) fn id_of(v: &Value) -> Id {
    serde_json::from_value(v["id"].clone()).expect("create commands return an id")
}

pub(crate) fn object(doc: &Document, id: Id) -> Result<&Object, CmdError> {
    doc.objects.get(&id).ok_or_else(|| CmdError::not_found("object", id))
}

pub(crate) fn ensure_exists(doc: &Document, ids: &[Id]) -> Result<(), CmdError> {
    for &id in ids {
        object(doc, id)?;
    }
    Ok(())
}

/// Drop ids whose ancestor is also in the list; they move with that ancestor.
pub(crate) fn top_level(doc: &Document, ids: &[Id]) -> Vec<Id> {
    let mut out: Vec<Id> = Vec::new();
    for &id in ids {
        if out.contains(&id) {
            continue;
        }
        let covered = ids.iter().any(|&other| other != id && doc.is_ancestor(other, id));
        if !covered {
            out.push(id);
        }
    }
    out
}

/// The short form used in lists.
pub(crate) fn summary(doc: &Document, o: &Object) -> Value {
    let world = geom::world_placement(doc, o.id).unwrap_or_default();
    json!({
        "id": o.id,
        "name": o.name,
        "type": o.kind.type_name(),
        "layer": doc.layers.get(&o.layer).map(|l| l.name.clone()),
        "parent": o.parent,
        "tags": tag_names(doc, &o.tags),
        "world": world,
    })
}

/// Everything about one object.
pub(crate) fn detail(doc: &Document, o: &Object) -> Value {
    let mut v = summary(doc, o);
    let m = v.as_object_mut().expect("summary is an object");
    m.insert("placement".into(), json!(o.placement));
    m.insert("kind".into(), json!(o.kind));
    m.insert("children".into(), json!(doc.children(o.id)));
    if !o.style.is_default() {
        m.insert("style".into(), json!(o.style));
    }
    if let Some(link) = &o.sketch {
        m.insert("sketch".into(), json!(link));
    }
    let l = crate::load::load_of(doc, o.id);
    m.insert("load".into(), json!({ "own_kg": l.own_kg, "attached_kg": l.attached_kg, "unknown_weights": l.unknown }));
    v
}

pub(crate) fn tag_names(doc: &Document, ids: &[Id]) -> Vec<String> {
    ids.iter().filter_map(|t| doc.tags.get(t).map(|t| t.name.clone())).collect()
}
