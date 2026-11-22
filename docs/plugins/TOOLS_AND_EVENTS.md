# Agent tools, menu commands and events

Besides controller types, a plugin can give AI agents new tools, put commands in Studio's Plugins menu, and hear about
what happens in Studio.

## Agent tools

Declare each tool in `contributes.tools` ([MANIFEST.md](MANIFEST.md#contributestools)) and answer `tool.call`
([PROTOCOL.md](PROTOCOL.md#toolcall)). Studio's MCP server lists them to every connected agent next to its own tools.

**Names.** A tool appears as `plugin__<short-id>__<name>`. The short id is the last part of your plugin id with anything
but `a-z0-9` turned into `_` (`io.github.someone.govee-output` → `govee_output`, so its `discover` tool is
`plugin__govee_output__discover`). When two listed plugins share a last part, both use their whole id that way. The
name is cut to fit MCP's 64 characters.

**When they're listed.** For every enabled plugin with a valid manifest that hasn't crashed, whether it's running or
not. Calling a tool of a stopped plugin starts it (waiting for `initialize`); the call then has 30 s.

**Annotations.** `"readOnly": true` marks the tool read-only, so an agent's client may run it without asking. Every
other tool is marked destructive: be honest, since that is the user's protection.

**Answers.** `{"content": [{"type": "text", "text": "…"}], "isError": false}`, MCP's own shape. Set `isError: true`
when the call failed in a way the agent should read and act on. The SDKs make this easy:

| You return (Python / TypeScript) | The agent gets |
|---|---|
| a string | that text |
| `{"content": […]}` | your content as it is |
| anything else | its JSON as text |
| raise `ToolError("why")` (Rust: `Err("why")`) | `isError: true` with that text |

**Changing the list at runtime.** Send `tools.changed {tools}` (SDK: `plugin.tools_changed(tools)`) to replace the
manifest's list until the plugin restarts, for example after a discovery finds devices worth a tool each; send it
without `tools` to go back to the manifest's. Studio's MCP server advertises `tools.listChanged` and sends
`notifications/tools/list_changed` before its next answer whenever the plugin tools differ from what the client last
listed (a plugin started, stopped, was enabled or disabled, or sent `tools.changed`). Clients that ignore the
notification see the change next time they list tools.

**Good tools** do one thing, describe their arguments with `inputSchema`, answer in a sentence or a small JSON object,
and never act on real lights or the network beyond what their description says.

## Menu commands

`contributes.commands` adds items to **Plugins ▸ *your plugin's name***, above Manage Plugins…. Each runs one of your
tools with no arguments (starting the plugin if needed) and shows the answer's text. An item is enabled while its tool
can be called. The ⌘K palette finds them too, and `command_palette` reports each with its `agentTool`, so an agent can
run the same thing.

```json commands
{ "contributes": {
    "tools": [{ "name": "discover", "description": "Find lamps on the local network.", "readOnly": true }],
    "commands": [{ "id": "discover", "title": "Discover Lamps…", "tool": "discover" }] } }
```

## Events

List the events you want in `contributes.events`; Studio sends only those, as `event {name, data}` notifications.

| Event | Data | Sent |
|---|---|---|
| `liveOutput.changed` | `{"muted": true\|false}` | Whenever Live Output turns on or off, or Studio is muted. The SDKs also update `plugin.muted`. |
| `show.started` | | Not yet: accepted in manifests for later versions. |
| `show.stopped` | | Not yet. |
| `sequence.playing` | | Not yet. |
| `sequence.stopped` | | Not yet. |

Event data contains names and ids only: never audio paths or show contents.

`initialize` also tells you `muted` at start, so a plugin knows from the first message whether real output may flow.
