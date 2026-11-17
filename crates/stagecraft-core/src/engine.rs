//! The engine owns one document, its history and the selection.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::command::{CmdError, CmdResult, Kind, registry};
use crate::model::{Document, Id};

/// How many undo steps are kept.
pub const UNDO_LIMIT: usize = 500;

#[derive(Clone, Debug)]
pub(crate) struct Version {
    pub doc: Document,
    pub label: String,
}

/// A successful change, in order. Enough to replay a session onto the same start.
#[derive(Clone, Debug, Serialize)]
pub struct JournalEntry {
    pub seq: u64,
    pub command: String,
    pub args: Value,
}

pub struct State {
    pub doc: Document,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    /// Selected object ids. Not part of the document, not undoable.
    pub selection: Vec<Id>,
    pub(crate) undo: Vec<Version>,
    pub(crate) redo: Vec<Version>,
    journal: Vec<JournalEntry>,
    seq: u64,
}

impl State {
    fn new(doc: Document) -> Self {
        Self {
            doc,
            path: None,
            dirty: false,
            selection: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            journal: Vec::new(),
            seq: 0,
        }
    }

    pub(crate) fn push_undo(&mut self, before: Document, label: String) {
        self.undo.push(Version { doc: before, label });
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    /// Replace the whole document (open, new). Clears history.
    pub(crate) fn reset(&mut self, doc: Document, path: Option<PathBuf>) {
        self.doc = doc;
        self.path = path;
        self.dirty = false;
        self.selection.clear();
        self.undo.clear();
        self.redo.clear();
    }

    /// Drop selected ids that no longer exist.
    pub(crate) fn prune_selection(&mut self) {
        let doc = &self.doc;
        self.selection.retain(|id| doc.objects.contains_key(id));
    }

    pub fn journal_since(&self, seq: u64) -> &[JournalEntry] {
        let start = self.journal.partition_point(|e| e.seq <= seq);
        &self.journal[start..]
    }

    fn record(&mut self, command: &str, args: &Value) {
        self.seq += 1;
        self.journal.push(JournalEntry { seq: self.seq, command: command.to_owned(), args: args.clone() });
    }
}

/// What a command function gets.
pub struct Ctx<'a> {
    pub(crate) state: &'a mut State,
    changed: bool,
}

impl<'a> Ctx<'a> {
    fn new(state: &'a mut State) -> Self {
        Self { state, changed: false }
    }
    pub fn doc(&self) -> &Document {
        &self.state.doc
    }
    /// Mutable access. Marks the command as having changed the document.
    pub fn doc_mut(&mut self) -> &mut Document {
        self.changed = true;
        &mut self.state.doc
    }
    pub fn selection(&self) -> &[Id] {
        &self.state.selection
    }
    pub(crate) fn mark_changed(&mut self) {
        self.changed = true;
    }
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct BatchArgs {
    /// Commands to run in order. Edits and queries only. If one fails, none of them
    /// stay applied.
    pub commands: Vec<BatchStep>,
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct BatchStep {
    pub command: String,
    #[serde(default)]
    pub args: Value,
}

pub struct Engine {
    state: State,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Self::with_document(Document::new())
    }

    pub fn with_document(doc: Document) -> Self {
        Self { state: State::new(doc) }
    }

    pub fn open(path: impl Into<PathBuf>) -> Result<Self, CmdError> {
        let path = path.into();
        let doc = crate::file::load(&path)?;
        let mut e = Self::with_document(doc);
        e.state.path = Some(path);
        Ok(e)
    }

    pub fn doc(&self) -> &Document {
        &self.state.doc
    }
    pub fn path(&self) -> Option<&PathBuf> {
        self.state.path.as_ref()
    }
    pub fn dirty(&self) -> bool {
        self.state.dirty
    }
    pub fn selection(&self) -> &[Id] {
        &self.state.selection
    }
    pub fn journal(&self) -> &[JournalEntry] {
        self.state.journal_since(0)
    }
    pub fn can_undo(&self) -> bool {
        !self.state.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.state.redo.is_empty()
    }

    /// Run a command by name. `args` may be `null` for commands without arguments.
    pub fn execute(&mut self, name: &str, args: Value) -> CmdResult {
        if name == "batch" {
            return self.batch(args);
        }
        let entry = registry().get(name).ok_or_else(|| CmdError::unknown_command(name))?;
        match entry.kind {
            Kind::Query => entry.run(&mut Ctx::new(&mut self.state), args),
            Kind::Meta => {
                let mut ctx = Ctx::new(&mut self.state);
                let out = entry.run(&mut ctx, args.clone());
                if out.is_ok() && ctx.changed {
                    self.state.record(name, &args);
                }
                out
            }
            Kind::Edit => {
                let before = self.state.doc.clone();
                let mut ctx = Ctx::new(&mut self.state);
                let out = entry.run(&mut ctx, args.clone());
                let changed = ctx.changed;
                match out {
                    Err(e) => {
                        self.state.doc = before;
                        Err(e)
                    }
                    Ok(v) => {
                        if changed {
                            self.state.push_undo(before, name.to_owned());
                            self.state.prune_selection();
                            self.state.record(name, &args);
                        }
                        Ok(v)
                    }
                }
            }
        }
    }

    fn batch(&mut self, args: Value) -> CmdResult {
        let raw = args.clone();
        let b: BatchArgs = serde_json::from_value(args).map_err(|e| CmdError::invalid_args(format!("batch: {e}")))?;
        let before = self.state.doc.clone();
        let mut changed = false;
        let mut results = Vec::with_capacity(b.commands.len());
        for (i, step) in b.commands.into_iter().enumerate() {
            let fail = |e: CmdError| CmdError::new(e.code, format!("step {i} ({}): {}", step.command, e.message));
            let entry = match registry().get(&step.command) {
                Some(e) if e.kind != Kind::Meta => e,
                Some(_) => {
                    self.state.doc = before;
                    return Err(fail(CmdError::invalid("meta commands cannot run inside a batch")));
                }
                None => {
                    self.state.doc = before;
                    return Err(fail(CmdError::unknown_command(&step.command)));
                }
            };
            let mut ctx = Ctx::new(&mut self.state);
            match entry.run(&mut ctx, step.args) {
                Ok(v) => {
                    changed |= ctx.changed;
                    results.push(v);
                }
                Err(e) => {
                    self.state.doc = before;
                    return Err(fail(e));
                }
            }
        }
        if changed {
            self.state.push_undo(before, "batch".into());
            self.state.prune_selection();
            self.state.record("batch", &raw);
        }
        Ok(json!({ "results": results }))
    }
}
