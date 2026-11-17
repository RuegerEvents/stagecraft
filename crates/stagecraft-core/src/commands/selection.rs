//! Selection lives in the engine, not the document, so an AI can point at things in
//! the running app.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use super::ensure_exists;
use crate::command::{CmdResult, Kind, Registry};
use crate::engine::Ctx;
use crate::model::Id;

pub(super) fn register(r: &mut Registry) {
    r.add("selection.get", Kind::Query, "Selected object ids", get);
    r.add("selection.set", Kind::Meta, "Replace the selection", set);
    r.add("selection.add", Kind::Meta, "Add objects to the selection", add);
    r.add("selection.clear", Kind::Meta, "Select nothing", clear);
}

#[derive(Deserialize, JsonSchema)]
struct NoArgs {}

#[derive(Deserialize, JsonSchema)]
struct IdsArgs {
    ids: Vec<Id>,
}

fn get(ctx: &mut Ctx, _: NoArgs) -> CmdResult {
    Ok(json!(ctx.selection()))
}

fn set(ctx: &mut Ctx, a: IdsArgs) -> CmdResult {
    ensure_exists(ctx.doc(), &a.ids)?;
    let mut ids = a.ids;
    ids.dedup();
    ctx.state.selection = ids;
    Ok(json!(ctx.selection()))
}

fn add(ctx: &mut Ctx, a: IdsArgs) -> CmdResult {
    ensure_exists(ctx.doc(), &a.ids)?;
    for id in a.ids {
        if !ctx.state.selection.contains(&id) {
            ctx.state.selection.push(id);
        }
    }
    Ok(json!(ctx.selection()))
}

fn clear(ctx: &mut Ctx, _: NoArgs) -> CmdResult {
    ctx.state.selection.clear();
    Ok(json!([]))
}
