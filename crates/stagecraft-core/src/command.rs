//! The command registry: name, kind, JSON schema of the arguments, and the function.

use std::collections::HashMap;
use std::sync::OnceLock;

use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::engine::Ctx;

pub type CmdResult = Result<Value, CmdError>;

/// How the engine treats a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Reads only. Never touches history.
    Query,
    /// Changes the document. One undo step; rolled back on error.
    Edit,
    /// Works on the engine itself: undo, files, selection, batch.
    Meta,
}

#[derive(Clone, Debug, Serialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct CmdError {
    pub code: &'static str,
    pub message: String,
}

impl CmdError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
    pub fn unknown_command(name: &str) -> Self {
        Self::new("unknown_command", format!("no command named `{name}`"))
    }
    pub fn invalid_args(message: impl Into<String>) -> Self {
        Self::new("invalid_args", message)
    }
    pub fn not_found(what: &str, id: impl std::fmt::Display) -> Self {
        Self::new("not_found", format!("{what} {id} does not exist"))
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid", message)
    }
    pub fn io(message: impl Into<String>) -> Self {
        Self::new("io", message)
    }
}

type RunFn = Box<dyn Fn(&mut Ctx, Value) -> CmdResult + Send + Sync>;

pub struct Entry {
    pub name: &'static str,
    pub kind: Kind,
    pub summary: &'static str,
    schema: fn() -> Value,
    run: RunFn,
}

impl Entry {
    pub fn schema(&self) -> Value {
        (self.schema)()
    }
    pub(crate) fn run(&self, ctx: &mut Ctx, args: Value) -> CmdResult {
        (self.run)(ctx, args)
    }
    pub fn info(&self) -> CommandInfo {
        CommandInfo { name: self.name, kind: self.kind, summary: self.summary }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct CommandInfo {
    pub name: &'static str,
    pub kind: Kind,
    pub summary: &'static str,
}

#[derive(Default)]
pub struct Registry {
    entries: Vec<Entry>,
    by_name: HashMap<&'static str, usize>,
}

fn schema_of<A: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(A)).expect("schema serialises")
}

impl Registry {
    /// Register a command. `A`'s doc comments become the schema descriptions, so write
    /// them for an AI reading the schema.
    pub fn add<A>(&mut self, name: &'static str, kind: Kind, summary: &'static str, f: fn(&mut Ctx, A) -> CmdResult)
    where
        A: DeserializeOwned + JsonSchema + 'static,
    {
        assert!(!self.by_name.contains_key(name), "command `{name}` registered twice");
        let run: RunFn = Box::new(move |ctx, args| {
            let args = if args.is_null() { Value::Object(Default::default()) } else { args };
            let a: A = serde_json::from_value(args).map_err(|e| CmdError::invalid_args(format!("{name}: {e}")))?;
            f(ctx, a)
        });
        self.by_name.insert(name, self.entries.len());
        self.entries.push(Entry { name, kind, summary, schema: schema_of::<A>, run });
    }

    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.by_name.get(name).map(|&i| &self.entries[i])
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn list(&self) -> Vec<CommandInfo> {
        let mut v: Vec<_> = self.entries.iter().map(Entry::info).collect();
        v.sort_by_key(|c| c.name);
        v
    }
}

/// The one registry with every command.
pub fn registry() -> &'static Registry {
    static REG: OnceLock<Registry> = OnceLock::new();
    REG.get_or_init(|| {
        let mut r = Registry::default();
        crate::commands::register(&mut r);
        r
    })
}
