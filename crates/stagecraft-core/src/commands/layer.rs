use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::resolve_layer;
use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::model::Layer;

pub(super) fn register(r: &mut Registry) {
    r.add("layer.list", Kind::Query, "Layers in order, with object counts", list);
    r.add("layer.create", Kind::Edit, "Create a spatial layer (storey/area) at an elevation", create);
    r.add("layer.update", Kind::Edit, "Rename a layer, change elevation, order or visibility", update);
    r.add("layer.delete", Kind::Edit, "Delete a layer; its objects move to another layer", delete);
}

#[derive(Deserialize, JsonSchema)]
struct NoArgs {}

fn list(ctx: &mut Ctx, _: NoArgs) -> CmdResult {
    let doc = ctx.doc();
    let v: Vec<_> = doc
        .layers_sorted()
        .into_iter()
        .map(|l| {
            let n = doc.objects.values().filter(|o| o.layer == l.id).count();
            json!({ "id": l.id, "name": l.name, "elevation": l.elevation,
                    "order": l.order, "visible": l.visible, "objects": n })
        })
        .collect();
    Ok(json!(v))
}

#[derive(Deserialize, JsonSchema)]
struct CreateArgs {
    name: String,
    /// mm above the document origin. Default 0.
    #[serde(default)]
    elevation: f64,
    /// Position in the layer list. Default: after the last layer.
    order: Option<i32>,
}

fn create(ctx: &mut Ctx, a: CreateArgs) -> CmdResult {
    let name = a.name.trim().to_owned();
    if name.is_empty() {
        return Err(CmdError::invalid_args("layer name is empty"));
    }
    if ctx.doc().layers.values().any(|l| l.name == name) {
        return Err(CmdError::invalid(format!("a layer named `{name}` exists")));
    }
    let order = a.order.unwrap_or_else(|| ctx.doc().layers.values().map(|l| l.order + 1).max().unwrap_or(0));
    let layer = Layer { id: Uuid::new_v4(), name, elevation: a.elevation, order, visible: true };
    let id = layer.id;
    ctx.doc_mut().layers.insert(id, layer);
    Ok(json!({ "id": id }))
}

#[derive(Deserialize, JsonSchema)]
struct UpdateArgs {
    /// Layer id or name.
    layer: String,
    name: Option<String>,
    /// mm.
    elevation: Option<f64>,
    order: Option<i32>,
    visible: Option<bool>,
}

fn update(ctx: &mut Ctx, a: UpdateArgs) -> CmdResult {
    let id = resolve_layer(ctx.doc(), &a.layer)?;
    if let Some(n) = &a.name
        && ctx.doc().layers.values().any(|l| l.id != id && &l.name == n)
    {
        return Err(CmdError::invalid(format!("a layer named `{n}` exists")));
    }
    let l = ctx.doc_mut().layers.get_mut(&id).expect("resolved");
    if let Some(v) = a.name {
        l.name = v;
    }
    if let Some(v) = a.elevation {
        l.elevation = v;
    }
    if let Some(v) = a.order {
        l.order = v;
    }
    if let Some(v) = a.visible {
        l.visible = v;
    }
    Ok(json!(l))
}

#[derive(Deserialize, JsonSchema)]
struct DeleteArgs {
    /// Layer id or name.
    layer: String,
    /// Layer that receives the objects. Required if the layer is not empty.
    move_objects_to: Option<String>,
}

fn delete(ctx: &mut Ctx, a: DeleteArgs) -> CmdResult {
    let doc = ctx.doc();
    let id = resolve_layer(doc, &a.layer)?;
    if doc.layers.len() == 1 {
        return Err(CmdError::invalid("a document keeps at least one layer"));
    }
    let on_it: Vec<_> = doc.objects.values().filter(|o| o.layer == id).map(|o| o.id).collect();
    let target = match (&a.move_objects_to, on_it.is_empty()) {
        (Some(t), _) => Some(resolve_layer(doc, t)?),
        (None, true) => None,
        (None, false) => {
            return Err(CmdError::invalid(format!("layer has {} objects; pass move_objects_to", on_it.len())));
        }
    };
    if target == Some(id) {
        return Err(CmdError::invalid_args("move_objects_to is the layer being deleted"));
    }
    let doc = ctx.doc_mut();
    crate::sketch::forget_layer(doc, id);
    if let Some(t) = target {
        for oid in &on_it {
            doc.objects.get_mut(oid).expect("listed").layer = t;
        }
    }
    doc.layers.remove(&id);
    doc.edit_viewports(|v| v.hidden_layers.retain(|l| *l != id));
    Ok(json!({ "moved": on_it.len() }))
}
