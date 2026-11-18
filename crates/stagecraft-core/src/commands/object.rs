use std::collections::HashMap;

use glam::{DAffine3, DQuat, DVec3};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::{detail, ensure_exists, layer_or_default, object, resolve_layer, resolve_tag, summary, top_level};
use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::geom::{local_for_world, world};
use crate::model::{Document, Id, P3};

pub(super) fn register(r: &mut Registry) {
    r.add("object.list", Kind::Query, "List objects, filtered by layer, tag, type or parent", list);
    r.add("object.get", Kind::Query, "Everything about one object, with world placement and children", get);
    r.add("object.delete", Kind::Edit, "Delete objects; children are detached or deleted", delete);
    r.add("object.rename", Kind::Edit, "Rename an object", rename);
    r.add("object.move", Kind::Edit, "Move objects by a world-space offset; attached objects come along", move_by);
    r.add("object.rotate", Kind::Edit, "Rotate objects about a vertical axis through a pivot", rotate);
    r.add(
        "object.set_placement",
        Kind::Edit,
        "Set an object's placement relative to its parent (or layer)",
        set_placement,
    );
    r.add("object.set_layer", Kind::Edit, "Move objects to another layer", set_layer);
    r.add("object.attach", Kind::Edit, "Attach objects to a parent (fixture to truss, truss to batten …)", attach);
    r.add("object.detach", Kind::Edit, "Detach objects from their parent", detach);
    r.add("object.duplicate", Kind::Edit, "Copy objects with everything attached to them", duplicate);
}

#[derive(Deserialize, JsonSchema)]
struct ListArgs {
    /// Layer id or name.
    layer: Option<String>,
    /// Tag id or name.
    tag: Option<String>,
    /// Object type: floor, wall, truss, fixture, proscenium, curtain, wing_set, masking,
    /// batten, platform, truss_node, truss_run, support, hoist, ceiling.
    #[serde(rename = "type")]
    kind: Option<String>,
    /// Only direct children of this object.
    parent: Option<Id>,
    /// Only objects without a parent.
    #[serde(default)]
    roots_only: bool,
}

fn list(ctx: &mut Ctx, a: ListArgs) -> CmdResult {
    let doc = ctx.doc();
    let layer = a.layer.as_deref().map(|l| resolve_layer(doc, l)).transpose()?;
    let tag = match &a.tag {
        Some(t) => Some(resolve_tag(doc, t).ok_or_else(|| CmdError::not_found("tag", t))?),
        None => None,
    };
    let v: Vec<_> = doc
        .objects
        .values()
        .filter(|o| layer.is_none_or(|l| o.layer == l))
        .filter(|o| tag.is_none_or(|t| o.tags.contains(&t)))
        .filter(|o| a.kind.as_deref().is_none_or(|k| o.kind.type_name() == k))
        .filter(|o| a.parent.is_none_or(|p| o.parent == Some(p)))
        .filter(|o| !a.roots_only || o.parent.is_none())
        .map(|o| summary(doc, o))
        .collect();
    Ok(json!(v))
}

#[derive(Deserialize, JsonSchema)]
struct IdArg {
    id: Id,
}

fn get(ctx: &mut Ctx, a: IdArg) -> CmdResult {
    let doc = ctx.doc();
    Ok(detail(doc, object(doc, a.id)?))
}

#[derive(Deserialize, JsonSchema, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ChildPolicy {
    /// Children stay where they are in the world and lose their parent.
    #[default]
    Detach,
    /// Children are deleted too.
    Delete,
}

#[derive(Deserialize, JsonSchema)]
struct DeleteArgs {
    ids: Vec<Id>,
    /// What happens to objects attached to a deleted one. Default detach.
    #[serde(default)]
    children: ChildPolicy,
}

fn delete(ctx: &mut Ctx, a: DeleteArgs) -> CmdResult {
    ensure_exists(ctx.doc(), &a.ids)?;
    let doc = ctx.doc();
    let mut doomed: Vec<Id> = Vec::new();
    for &id in &a.ids {
        if a.children == ChildPolicy::Delete {
            doomed.extend(doc.subtree(id));
        } else {
            doomed.push(id);
        }
    }
    doomed.sort();
    doomed.dedup();
    // Surviving direct children of doomed objects, with their world transform.
    let orphans: Vec<(Id, DAffine3)> = doc
        .objects
        .values()
        .filter(|o| o.parent.is_some_and(|p| doomed.binary_search(&p).is_ok()))
        .filter(|o| doomed.binary_search(&o.id).is_err())
        .map(|o| (o.id, world(doc, o.id).expect("exists")))
        .collect();
    let doc = ctx.doc_mut();
    for (id, w) in &orphans {
        let layer = doc.objects[id].layer;
        let local = local_for_world(doc, w, None, layer);
        let o = doc.objects.get_mut(id).expect("exists");
        o.parent = None;
        o.placement = local;
    }
    for id in &doomed {
        doc.objects.remove(id);
    }
    let dims: Vec<Id> =
        doc.dimensions.values().filter(|d| doomed.iter().any(|id| d.uses_object(*id))).map(|d| d.id).collect();
    for d in dims {
        doc.dimensions.remove(&d);
    }
    Ok(json!({ "deleted": doomed, "detached": orphans.iter().map(|o| o.0).collect::<Vec<_>>() }))
}

#[derive(Deserialize, JsonSchema)]
struct RenameArgs {
    id: Id,
    name: String,
}

fn rename(ctx: &mut Ctx, a: RenameArgs) -> CmdResult {
    object(ctx.doc(), a.id)?;
    ctx.doc_mut().objects.get_mut(&a.id).expect("checked").name = a.name;
    Ok(json!({}))
}

/// Applies `f` to the world transform of each top-level object and writes back local
/// placements. Descendants follow because their placements are relative.
///
/// Walls and floors that follow the sketch move by dragging their sketch points instead,
/// so constraints hold (a fixed corner stays put).
fn transform_world(doc: &mut Document, ids: &[Id], f: impl Fn(DAffine3) -> DAffine3) -> Result<(), CmdError> {
    let (linked, free): (Vec<Id>, Vec<Id>) =
        top_level(doc, ids).into_iter().partition(|id| doc.objects[id].sketch.is_some());
    if !linked.is_empty() {
        crate::sketch::transform_linked(doc, &linked, |p| {
            let q = f(DAffine3::from_translation(DVec3::new(p[0], p[1], 0.0))).translation;
            [q.x, q.y]
        })?;
    }
    for id in free {
        let w = world(doc, id).expect("exists");
        let (parent, layer) = {
            let o = &doc.objects[&id];
            (o.parent, o.layer)
        };
        let local = local_for_world(doc, &f(w), parent, layer);
        doc.objects.get_mut(&id).expect("exists").placement = local;
    }
    Ok(())
}

#[derive(Deserialize, JsonSchema)]
struct MoveArgs {
    ids: Vec<Id>,
    /// World-space offset in mm.
    delta: P3,
}

fn move_by(ctx: &mut Ctx, a: MoveArgs) -> CmdResult {
    ensure_exists(ctx.doc(), &a.ids)?;
    let t = DAffine3::from_translation(DVec3::from_array(a.delta));
    transform_world(ctx.doc_mut(), &a.ids, |w| t * w)?;
    Ok(json!({}))
}

#[derive(Deserialize, JsonSchema)]
struct RotateArgs {
    ids: Vec<Id>,
    /// Degrees, counter-clockwise seen from above.
    angle: f64,
    /// World point the vertical axis goes through, mm. Default: the centre of the
    /// objects' positions.
    pivot: Option<P3>,
}

fn rotate(ctx: &mut Ctx, a: RotateArgs) -> CmdResult {
    ensure_exists(ctx.doc(), &a.ids)?;
    let doc = ctx.doc();
    let pivot = match a.pivot {
        Some(p) => DVec3::from_array(p),
        None => {
            let tops = top_level(doc, &a.ids);
            let sum: DVec3 = tops.iter().map(|&id| world(doc, id).expect("exists").translation).sum();
            sum / tops.len().max(1) as f64
        }
    };
    let r = DAffine3::from_translation(pivot)
        * DAffine3::from_quat(DQuat::from_rotation_z(a.angle.to_radians()))
        * DAffine3::from_translation(-pivot);
    transform_world(ctx.doc_mut(), &a.ids, |w| r * w)?;
    Ok(json!({ "pivot": pivot.to_array() }))
}

#[derive(Deserialize, JsonSchema)]
struct SetPlacementArgs {
    id: Id,
    /// mm, relative to the parent (or the layer for roots).
    position: Option<P3>,
    /// Degrees about x, y, z, applied as Rz · Ry · Rx.
    rotation: Option<P3>,
}

fn set_placement(ctx: &mut Ctx, a: SetPlacementArgs) -> CmdResult {
    object(ctx.doc(), a.id)?;
    let o = ctx.doc_mut().objects.get_mut(&a.id).expect("checked");
    if let Some(p) = a.position {
        o.placement.position = p;
    }
    if let Some(r) = a.rotation {
        o.placement.rotation = r;
    }
    // Sketch-backed walls and floors keep only their height; x, y and rotation come
    // from the sketch.
    crate::sketch::sync_objects(ctx.doc_mut(), &[a.id]);
    Ok(json!(ctx.doc().objects[&a.id].placement))
}

#[derive(Deserialize, JsonSchema)]
struct SetLayerArgs {
    ids: Vec<Id>,
    /// Layer id or name.
    layer: String,
    /// Keep world position (default true). Otherwise roots jump by the elevation
    /// difference.
    #[serde(default = "yes")]
    keep_world: bool,
}

fn yes() -> bool {
    true
}

fn set_layer(ctx: &mut Ctx, a: SetLayerArgs) -> CmdResult {
    ensure_exists(ctx.doc(), &a.ids)?;
    let layer = resolve_layer(ctx.doc(), &a.layer)?;
    let doc = ctx.doc_mut();
    for id in &a.ids {
        let w = world(doc, *id).expect("exists");
        let parent = doc.objects[id].parent;
        let o = doc.objects.get_mut(id).expect("exists");
        if o.layer != layer {
            // Its edges stay in the old layer's sketch.
            o.sketch = None;
        }
        o.layer = layer;
        if a.keep_world && parent.is_none() {
            let local = local_for_world(doc, &w, None, layer);
            doc.objects.get_mut(id).expect("exists").placement = local;
        }
    }
    Ok(json!({}))
}

#[derive(Deserialize, JsonSchema)]
struct AttachArgs {
    ids: Vec<Id>,
    parent: Id,
    /// Keep the objects where they are in the world (default true). If false, their
    /// current placement is reused as the placement relative to the parent.
    #[serde(default = "yes")]
    keep_world: bool,
}

fn attach(ctx: &mut Ctx, a: AttachArgs) -> CmdResult {
    let doc = ctx.doc();
    ensure_exists(doc, &a.ids)?;
    object(doc, a.parent)?;
    for &id in &a.ids {
        if doc.objects[&id].sketch.is_some() {
            return Err(CmdError::invalid(format!(
                "{} follows the layer sketch and cannot hang on another object; sketch.unlink it first",
                doc.objects[&id].name
            )));
        }
        if doc.is_ancestor(id, a.parent) {
            return Err(CmdError::invalid(format!("attaching {id} to {} would make a loop", a.parent)));
        }
    }
    let doc = ctx.doc_mut();
    for id in &a.ids {
        let w = world(doc, *id).expect("exists");
        let layer = doc.objects[id].layer;
        let local =
            if a.keep_world { local_for_world(doc, &w, Some(a.parent), layer) } else { doc.objects[id].placement };
        let o = doc.objects.get_mut(id).expect("exists");
        o.parent = Some(a.parent);
        o.placement = local;
    }
    Ok(json!({}))
}

#[derive(Deserialize, JsonSchema)]
struct DetachArgs {
    ids: Vec<Id>,
}

fn detach(ctx: &mut Ctx, a: DetachArgs) -> CmdResult {
    ensure_exists(ctx.doc(), &a.ids)?;
    let doc = ctx.doc_mut();
    for id in &a.ids {
        if doc.objects[id].parent.is_none() {
            continue;
        }
        let w = world(doc, *id).expect("exists");
        let layer = doc.objects[id].layer;
        let local = local_for_world(doc, &w, None, layer);
        let o = doc.objects.get_mut(id).expect("exists");
        o.parent = None;
        o.placement = local;
    }
    Ok(json!({}))
}

#[derive(Deserialize, JsonSchema)]
struct DuplicateArgs {
    ids: Vec<Id>,
    /// World-space offset for the copies, mm. Default none.
    #[serde(default)]
    offset: P3,
    /// Layer for the copies. Default: same as the originals.
    layer: Option<String>,
}

fn duplicate(ctx: &mut Ctx, a: DuplicateArgs) -> CmdResult {
    ensure_exists(ctx.doc(), &a.ids)?;
    let doc = ctx.doc();
    let layer = a.layer.as_deref().map(|l| layer_or_default(doc, Some(l))).transpose()?;
    let tops = top_level(doc, &a.ids);
    let mut map: HashMap<Id, Id> = HashMap::new();
    let mut copies = Vec::new();
    for &top in &tops {
        for id in doc.subtree(top) {
            let new_id = Uuid::new_v4();
            map.insert(id, new_id);
            let mut o = doc.objects[&id].clone();
            o.id = new_id;
            // A copy of a sketch-backed wall or floor is free geometry.
            o.sketch = None;
            if let Some(l) = layer {
                o.layer = l;
            }
            copies.push(o);
        }
    }
    for o in &mut copies {
        // Inside a copied subtree, point at the copied parent; tops keep the original's.
        if let Some(p) = o.parent
            && let Some(&np) = map.get(&p)
        {
            o.parent = Some(np);
        }
    }
    let new_tops: Vec<Id> = tops.iter().map(|t| map[t]).collect();
    let doc = ctx.doc_mut();
    for o in copies {
        doc.objects.insert(o.id, o);
    }
    let t = DAffine3::from_translation(DVec3::from_array(a.offset));
    transform_world(doc, &new_tops, |w| t * w)?;
    let pairs: HashMap<String, Id> = map.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    Ok(json!({ "ids": new_tops, "map": pairs }))
}
