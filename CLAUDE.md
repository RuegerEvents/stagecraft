# StageCraft

Stage and lighting planning in Rust, in the spirit of Vectorworks Spotlight with better
UX. Commits are GPG-signed.

## Where behaviour is written down

- `docs/spec/overview.md`: what the product does, settled with the user. Do not change
  scope there without asking.
- `docs/spec/decisions.md`: choices made where the spec was silent. Add a row when you
  make one.
- `docs/spec/control.md`: control channel, discovery, MCP, CLI.
- `docs/roadmap.md`: the blocks and their status.

## Rules of the code

- `stagecraft-core` holds the document and every command. No UI, no sockets, no clock.
  It must keep building for `wasm32-unknown-unknown`
  (`cargo check -p stagecraft-core --target wasm32-unknown-unknown`).
- **Every action is a command** registered in `crates/stagecraft-core/src/commands/`.
  The GUI never mutates the document directly. A new feature without a command is not
  done.
- Argument structs are the API for AIs: their doc comments become the JSON schema. State
  units (mm, degrees) and defaults there.
- Edits change the document through `ctx.doc_mut()`; that is what marks an undo step.
- Placements are relative to the parent, or to the layer for roots. Moves and rotations
  from the user are world-space and go through `transform_world` in `object.rs`.
  Attachment behaviour has tests in `crates/stagecraft-core/tests/engine.rs`; extend
  them when you touch it.
- Units mm and degrees, axes as in MVR: x right from the audience, y upstage, z up.

## Test data

`testdata/private/` holds real MVR files from client projects and is git-ignored.
`vectorworks/` are Vectorworks 31 exports; `other/` comes from other tools (one from
the-pult). Never commit them or paste their contents anywhere outside this machine.

Perastage (`/Applications/Perastage.app`) opens MVR files and serves as an independent
viewer: `open -a Perastage file.mvr`.

## Trying it

```
cargo test
cargo run -p stagecraft-app -- commands
cargo run -p stagecraft-app -- serve /tmp/plan.stagecraft
cargo run -p stagecraft-app -- exec object.list
```
