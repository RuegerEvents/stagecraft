use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::command::{CmdError, CmdResult, Kind, Registry};
use crate::engine::{Ctx, Version};

pub(super) fn register(r: &mut Registry) {
    r.add("history.undo", Kind::Meta, "Undo the last edit", undo);
    r.add("history.redo", Kind::Meta, "Redo the last undone edit", redo);
    r.add("history.list", Kind::Query, "Labels of the undo and redo stacks", list);
    r.add("journal.list", Kind::Query, "Successful changes in order, for replay", journal);
}

#[derive(Deserialize, JsonSchema)]
struct StepArgs {
    /// How many steps. Default 1.
    #[serde(default = "one")]
    steps: usize,
}

fn one() -> usize {
    1
}

fn undo(ctx: &mut Ctx, a: StepArgs) -> CmdResult {
    let mut done = Vec::new();
    for _ in 0..a.steps {
        let Some(v) = ctx.state.undo.pop() else { break };
        let current = std::mem::replace(&mut ctx.state.doc, v.doc);
        ctx.state.redo.push(Version { doc: current, label: v.label.clone() });
        done.push(v.label);
    }
    if done.is_empty() {
        return Err(CmdError::invalid("nothing to undo"));
    }
    ctx.state.dirty = true;
    ctx.state.prune_selection();
    ctx.mark_changed();
    Ok(json!({ "undone": done }))
}

fn redo(ctx: &mut Ctx, a: StepArgs) -> CmdResult {
    let mut done = Vec::new();
    for _ in 0..a.steps {
        let Some(v) = ctx.state.redo.pop() else { break };
        let current = std::mem::replace(&mut ctx.state.doc, v.doc);
        ctx.state.undo.push(Version { doc: current, label: v.label.clone() });
        done.push(v.label);
    }
    if done.is_empty() {
        return Err(CmdError::invalid("nothing to redo"));
    }
    ctx.state.dirty = true;
    ctx.state.prune_selection();
    ctx.mark_changed();
    Ok(json!({ "redone": done }))
}

#[derive(Deserialize, JsonSchema)]
struct NoArgs {}

fn list(ctx: &mut Ctx, _: NoArgs) -> CmdResult {
    let labels = |v: &[Version]| v.iter().rev().map(|v| v.label.clone()).collect::<Vec<_>>();
    Ok(json!({
        "undo": labels(&ctx.state.undo),
        "redo": labels(&ctx.state.redo),
    }))
}

#[derive(Deserialize, JsonSchema)]
struct JournalArgs {
    /// Only entries with a sequence number greater than this.
    #[serde(default)]
    since: u64,
}

fn journal(ctx: &mut Ctx, a: JournalArgs) -> CmdResult {
    Ok(json!(ctx.state.journal_since(a.since)))
}
