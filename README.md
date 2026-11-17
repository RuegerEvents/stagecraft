# StageCraft

Stage and lighting planning: rooms, stage, curtains, truss, GDTF fixtures, patch, plans
as PDF, MVR exchange. Every function is a command, so other tools and AIs can drive it.

| Crate | What |
|---|---|
| `stagecraft-core` | document model, command registry, undo/redo, `.stagecraft` files |
| `stagecraft-control` | JSON-RPC over TCP, instance discovery, MCP server |
| `stagecraft-app` | the `stagecraft` binary (headless server, MCP, CLI; GUI to come) |

See `docs/spec/overview.md` for the product and `docs/roadmap.md` for the state.

## MCP

```json
{ "mcpServers": { "stagecraft": { "command": "/path/to/stagecraft", "args": ["mcp"] } } }
```
