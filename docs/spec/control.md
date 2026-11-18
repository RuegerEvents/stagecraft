# Control channel and MCP

Everything the app can do is a command in `stagecraft-core`'s registry. There are three
ways in, all with the same four methods.

| Way | Who uses it |
|---|---|
| In-process `stagecraft_control::handle(&mut engine, method, params)` | the GUI, tests |
| TCP, JSON-RPC 2.0, one JSON object per line, 127.0.0.1 only | scripts, other apps, MCP |
| MCP on stdio (`stagecraft mcp`) | Claude and other AI clients |

## Methods

| Method | Params | Result |
|---|---|---|
| `ping` | – | `{app, version, path, dirty}` |
| `commands` | `{prefix?}` | `[{name, kind, summary}]`, sorted |
| `describe` | `{name}` | `{name, kind, summary, args}`; `args` is a JSON Schema |
| `execute` | `{command, args?}` | the command's JSON result |

`kind` is `query` (read only), `edit` (one undo step, rolled back on error) or `meta`
(files, undo/redo, selection, batch).

Errors use JSON-RPC codes; a failed command is `-32000` with `data.code` set to the
command's code (`invalid_args`, `not_found`, `invalid`, `io`, `unknown_command`).
A wrong token is `-32001`.

## GUI mode

`stagecraft` without a subcommand opens the GUI, which runs the same server with
`mode: "gui"`. There, `commands` and `describe` also list the GUI's own `ui.*`
commands (`kind: "ui"`), and `execute` runs them: `ui.state`, `ui.set_tool`,
`ui.set_view` (`plan`, `3d`, `split`, `sheets`), `ui.sheet` (Sheets mode: `sheet`,
`viewport`, `editor`, `fit`), `ui.set_label_template` (labels in the plan canvas), `ui.zoom`, `ui.pan`, `ui.zoom_to_fit`,
`ui.camera` (3D: `view` top/front/back/side/left/iso, `eye`, `target`,
`projection`, `fov`, `orbit`, `zoom`, `fit`), `ui.section` (3D: `axis` + `offset`,
`origin` + `normal`, `box {min,max}`, `off`, `hatch`), `ui.set_theme`,
`ui.toggle_panel`, `ui.set_tag_visible`, `ui.set_active_layer`, `ui.set_snap`,
`ui.set_tool_options`, `ui.palette`, `ui.screenshot {path}`. They change only what
the window shows, not the document, and are not undoable. `ui.state` reports the 3D
camera (`camera3d`) and section too. `ui.screenshot` answers once the next frame is
captured, with `{path, width, height}` in physical pixels; it includes the 3D view.
A window that draws no frames (screen locked, minimised) leaves it waiting.

## TCP framing

```json
{"jsonrpc":"2.0","id":1,"method":"execute","token":"…","params":{"command":"object.list","args":{"type":"fixture"}}}
```

Each request carries the instance's `token` at top level (compared in constant time). The
answer is one line. Lines are read bounded (512 MiB, for `data_base64` imports): a longer one
gets `-32600` and the connection closes.

## Discovery

A running instance writes `instances/<pid>.json` into the app data directory
(`~/Library/Application Support/StageCraft/` on macOS, or `$STAGECRAFT_HOME`):

```json
{ "pid": 123, "addr": "127.0.0.1:61094", "token": "…", "mode": "headless", "started": 1795000000 }
```

The file is 0600 and removed on clean shutdown. Readers ping each entry and delete
the files of instances that do not answer. `STAGECRAFT_TOKEN` fixes the token, e.g. for
scripts.

## MCP

`stagecraft mcp` connects to the newest running instance. If none runs, or with
`--standalone` / `--file`, it runs its own engine. Tools:

- `stagecraft_commands {prefix?}`
- `stagecraft_describe {name}`
- `stagecraft_execute {command, args?}`

Use the `batch` command for several edits as one undo step.

The protocol (lifecycle, framing, argument checks against the tool schemas) is
`craft-mcp-server`'s. `stagecraft_execute` of `ui.screenshot` (GUI) answers with an MCP
image (PNG, up to 16 MB) followed by the JSON result as text.

## CLI

```
stagecraft serve [FILE] [--port N]      headless instance
stagecraft instances                    running instances
stagecraft commands [PREFIX]
stagecraft describe NAME
stagecraft exec CMD ['{json}'] [--file F]   against a file (saved if changed) or the newest instance
```
