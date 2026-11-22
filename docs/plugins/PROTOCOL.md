# Plugin protocol reference (API 1.0)

Every message between Vixen Studio and a plugin, with its parameters, result and errors. The machine-readable
version is [`studio/plugins/schema/protocol.schema.json`](../../studio/plugins/schema/protocol.schema.json) (MCP
resource `plugins://schema/protocol`); a test (`crates/studio-plugins/tests/docs.rs`) checks this page against it, so
the tables below list exactly the fields the schema has.

You only need this page if you write a plugin without one of the SDKs, or want to know what an SDK does for you. The
SDKs ([Python, TypeScript, Rust](README.md#sdks)) handle all of it.

## Framing

- **JSON-RPC 2.0** over the plugin's stdin (Studio → plugin) and stdout (plugin → Studio).
- **One message per line**, UTF-8, ending in `\n`. At most **1048576 bytes** (1 MiB) per line.
- **stdout is only for protocol messages.** Anything else there (a stray `print`, a banner) stops the plugin with an
  error naming what it printed, then the normal restart backoff applies. Write logs to **stderr**: Studio keeps each
  line in the plugin's log at level `info`.
- Requests carry an `id` (a number or a string) and get exactly one answer; notifications have no `id` and get none.
- Both sides may send requests at any time after `initialize` has been answered.
- The host API version is **1.0.0**. Your manifest's `apiVersion` range must contain it, and `initialize`'s answer must
  name the same major version.

```json example request
{"jsonrpc": "2.0", "id": 1, "method": "controller.start", "params": {"instanceId": "5d1c0f8e-2b4a-4c61-9e0f-7a3b2c1d0e9f"}}
```

```json example answer
{"jsonrpc": "2.0", "id": 1, "result": {}}
```

```json example notification
{"jsonrpc": "2.0", "method": "log", "params": {"level": "warn", "message": "device 2 did not answer"}}
```

## Timeouts

Studio waits **5000 ms** for an answer to its requests, except `controller.start` (15000 ms), `shutdown` (3000 ms) and
`tool.call` (30000 ms). A request that runs out answers the caller with `-32001`. A plugin that doesn't answer
`initialize` in time counts as a crash.

## Errors

Error answers are `{"jsonrpc": "2.0", "id": …, "error": {"code": …, "message": …, "data"?: …}}`.

| Code | Meaning |
|---|---|
| `-32700` | Parse error |
| `-32600` | Invalid request |
| `-32601` | Method not found |
| `-32602` | Invalid params |
| `-32603` | Internal error |
| `-32001` | Request timed out |
| `-32002` | Plugin exited |
| `-32003` | Permission denied |
| `-32004` | Invalid configuration (data.errors lists {field, message}) |

A plugin answers `-32004` to `controller.create` or `controller.configure` when it refuses settings. `data.errors` lists
each problem with the [JSON Pointer](https://www.rfc-editor.org/rfc/rfc6901) of the field (`""` for the whole
configuration) and a sentence the inspector shows under that field:

```json example error
{"jsonrpc": "2.0", "id": 7, "error": {"code": -32004, "message": "Invalid configuration", "data": {"errors": [{"field": "/host", "message": "must be an IPv4 address"}]}}}
```

Studio answers `-32003` to `storage.get`/`storage.set` from a plugin without `"files": ["plugin-data"]` and to
`display.snapshot` from one without `"display": "read"`, and `-32601` to a method it doesn't know.

## Lifecycle

1. Studio starts the plugin's `entry.command` in its folder with a clean environment (see
   [PERMISSIONS.md](PERMISSIONS.md#what-the-process-gets)) and sends `initialize`.
2. Controller instances: when a controller of the plugin's type starts (Live Output on and not muted), Studio sends
   `controller.create`, then `controller.start`, then `controller.frame` notifications. Edits while it runs arrive as
   `controller.configure` and `controller.setOutputCount`. When it stops: `controller.stop`, then `controller.destroy`.
   The conformance harness also creates, configures and destroys instances without starting them, so accept that.
3. One process serves every instance of every type the plugin declares, told apart by `instanceId`.
4. `shutdown`: answer `{}` once you have cleaned up. Studio then closes stdin and kills the process group after 3 s.
   Exit on stdin end-of-file as well: that is how Studio's own exit reaches you.
5. If the plugin exits, crashes, or breaks the protocol, its instances report `Error("plugin exited: …")`, the show
   keeps running, and Studio restarts it after 1 s, 5 s, then 30 s. After three restarts within 60 s it stays
   `crashed` until reloaded. After a restart Studio creates, configures and starts the instances that were running.

## Host → plugin

### `initialize`

Host → plugin · request · timeout 5000 ms

The first message. Answer with the API version you speak; Studio stops a plugin whose major version differs, as
`incompatible`, without restarting it. `capabilities` tells you what this Studio offers (today `log`, `storage`,
`events: ["liveOutput.changed"]`, `controllers`, `frameEncodings`, `tools`, `display`). `profile` is `null` when no show is open
(the harness, the CLI).

| Param | Type | Required | Meaning |
|---|---|---|---|
| `apiVersion` | string | yes | The host's plugin API version, e.g. 1.0.0. |
| `hostVersion` | string | yes | Studio's version, ending in `-studio`. |
| `pluginDataDir` | string | yes | A folder only this plugin uses (state.json lives here). |
| `profile` | object \| null | no | The open show: `{name, id}`. |
| `muted` | boolean | yes | True when no real output may flow (headless, VIXEN_MUTE, Live Output off). |
| `capabilities` | object | yes | What the host supports. |

| Result | Type | Required | Meaning |
|---|---|---|---|
| `apiVersion` | string | yes | The API version the plugin speaks; its major must equal the host's. |
| `capabilities` | object | no | Anything you want Studio to know; shown by `plugin_info`. |

```json initialize params
{"apiVersion": "1.0.0", "hostVersion": "3.13.4-studio", "pluginDataDir": "/Users/me/Library/Application Support/Vixen Studio/Plugins/data/com.example.lamp", "profile": {"name": "Vixen 3", "id": "0c6f3f0e-7f83-4f39-a2a5-0d2f1b6a4e90"}, "muted": true, "capabilities": {"log": true, "storage": true, "events": ["liveOutput.changed"], "controllers": true, "frameEncodings": ["u8", "json", "u16"], "tools": true, "display": true}}
```

```json initialize result
{"apiVersion": "1.0.0", "capabilities": {}}
```

### `shutdown`

Host → plugin · request · timeout 3000 ms

Stop and destroy your instances, save what you keep, then answer. Studio closes stdin once it has the answer and
kills the process group 3 s later.

Param: `{}`.

Result: `{}`.

### `event`

Host → plugin · notification

Only for events your manifest lists in `contributes.events`. This version of Studio sends `liveOutput.changed` with
`{"muted": true|false}` whenever Live Output turns on or off (or Studio is muted); the other names are accepted in
manifests but not sent yet. Event data never carries audio paths or show contents beyond names and ids.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `name` | string | yes | The event's name. |
| `data` | any | no | The event's details. |

```json event params
{"name": "liveOutput.changed", "data": {"muted": false}}
```

### `controller.create`

Host → plugin · request · timeout 5000 ms

A controller of one of your types is starting. `config` has already been checked against the type's `configSchema` and
has its defaults filled in. Answer `{}`, or `{config}` to normalise the settings (Studio keeps your version), or
`-32004` to refuse them. `outputNames` is accepted but not applied to output names yet.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `instanceId` | string | yes | Stable for the controller (its module instance id). |
| `typeId` | string | yes | One of your `contributes.controllers[].typeId`. |
| `config` | object | yes | The controller's settings. |
| `outputCount` | integer | yes | How many outputs (channels) the controller has. |

| Result | Type | Required | Meaning |
|---|---|---|---|
| `config` | object | no | The settings as you normalised them. |
| `outputNames` | array of string | no | Names for the outputs (not applied yet). |

```json controller.create params
{"instanceId": "5d1c0f8e-2b4a-4c61-9e0f-7a3b2c1d0e9f", "typeId": "6f1c2a8e-0000-4000-8000-00000000c0de", "config": {"host": "10.0.0.7", "port": 4048}, "outputCount": 9}
```

```json controller.create result
{"config": {"host": "10.0.0.7", "port": 4048}}
```

### `controller.configure`

Host → plugin · request · timeout 5000 ms

The user or an agent changed the settings. Apply them and answer `{config}` (the settings you now use), or `-32004`
with `data.errors`: the controller keeps its previous settings in the plugin and the inspector shows your errors under
the fields.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `instanceId` | string | yes |  |
| `config` | object | yes | The new settings, checked against `configSchema`. |

| Result | Type | Required | Meaning |
|---|---|---|---|
| `config` | object | yes | The settings in use. |

```json controller.configure params
{"instanceId": "5d1c0f8e-2b4a-4c61-9e0f-7a3b2c1d0e9f", "config": {"host": "10.0.0.8", "port": 4048}}
```

```json controller.configure result
{"config": {"host": "10.0.0.8", "port": 4048}}
```

### `controller.setOutputCount`

Host → plugin · request · timeout 5000 ms

The controller's output count changed. Later frames have the new length.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `instanceId` | string | yes |  |
| `outputCount` | integer | yes |  |

| Result | Type | Required | Meaning |
|---|---|---|---|
| `outputNames` | array of string | no | Names for the outputs (not applied yet). |

```json controller.setOutputCount params
{"instanceId": "5d1c0f8e-2b4a-4c61-9e0f-7a3b2c1d0e9f", "outputCount": 12}
```

### `controller.start`

Host → plugin · request · timeout 15000 ms

Live output begins: connect to your device. Frames follow. Report `controller.status` `unavailable` or `error` if
the device can't be reached rather than failing the request, so the controller stays started and recovers.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `instanceId` | string | yes |  |

Result: `{}`.

### `controller.stop`

Host → plugin · request · timeout 5000 ms

Live output ended: stop sending and disconnect. A real device should be left dark or as its own default, not frozen
on the last frame, unless that is the device's documented behaviour.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `instanceId` | string | yes |  |

Result: `{}`.

### `controller.pause`

Host → plugin · request · timeout 5000 ms

The controller was paused: frames stop until `controller.resume`. Keep the connection.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `instanceId` | string | yes |  |

Result: `{}`.

### `controller.resume`

Host → plugin · request · timeout 5000 ms

| Param | Type | Required | Meaning |
|---|---|---|---|
| `instanceId` | string | yes |  |

Result: `{}`.

### `controller.destroy`

Host → plugin · request · timeout 5000 ms

The instance is gone (stopped, removed from the show, or the plugin is shutting down). Free what it held.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `instanceId` | string | yes |  |

Result: `{}`.

### `controller.frame`

Host → plugin · notification

New channel values for a started instance, at the show's frame rate (Vixen runs 20–40 fps). Never answered. See
[CONTROLLERS.md](CONTROLLERS.md#frames) for the encodings in detail.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `instanceId` | string | yes |  |
| `seq` | integer | yes | Counts up per instance; a gap means frames were skipped. |
| `timeMs` | integer | yes | Milliseconds since the instance started. |
| `encoding` | "u8" \| "json" \| "u16" | yes | The first encoding the type lists in `frameEncodings`. |
| `values` | any | yes | u8: base64, one byte per output (an output without an 8-bit value sends 0, as byte-oriented controllers do); u16: base64 little-endian, 8-bit values scaled to the full range (x257); json: array of null \| integer \| {r,g,b} \| string. Frames are latest-wins per instance: a plugin that reads slowly receives fewer frames, never stale ones. |

```json controller.frame params
{"instanceId": "5d1c0f8e-2b4a-4c61-9e0f-7a3b2c1d0e9f", "seq": 418, "timeMs": 10450, "encoding": "u8", "values": "/wAAAP8AAAD/"}
```

```json controller.frame params
{"instanceId": "5d1c0f8e-2b4a-4c61-9e0f-7a3b2c1d0e9f", "seq": 419, "timeMs": 10475, "encoding": "json", "values": [255, null, {"r": 255, "g": 128, "b": 0}]}
```

### `tool.call`

Host → plugin · request · timeout 30000 ms

An MCP client called one of your tools (listed as `plugin__<short-id>__<name>`, see
[TOOLS_AND_EVENTS.md](TOOLS_AND_EVENTS.md)). Answer MCP-style content. `isError: true` reports a failure the agent
should read; a JSON-RPC error answer reaches the agent as an error too.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `name` | string | yes | The tool's name as your manifest declares it (without the prefix). |
| `arguments` | object | no | The agent's arguments, matching the tool's `inputSchema`. |

| Result | Type | Required | Meaning |
|---|---|---|---|
| `content` | array | yes | MCP content items, usually `{"type": "text", "text": …}`. |
| `isError` | boolean | no | True when the call failed. |

```json tool.call params
{"name": "discover", "arguments": {"timeoutMs": 1000}}
```

```json tool.call result
{"content": [{"type": "text", "text": "Found 2 receivers: 10.0.0.40, 10.0.0.41"}], "isError": false}
```

## Plugin → host

### `log`

Plugin → host · notification or request

Adds an entry to the plugin's log (Settings ▸ Plugins ▸ Log…, `plugin_logs`, and `Plugins/logs/<id>.log`). Sent as a
request it is answered `{}`.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `level` | "debug" \| "info" \| "warn" \| "error" | no | Default `info`. |
| `message` | string | yes |  |

Result: `{}`.

```json log params
{"level": "info", "message": "connected to 10.0.0.7"}
```

### `storage.get`

Plugin → host · request

Reads one key from the plugin's small JSON store (`pluginDataDir/state.json`). Needs `"files": ["plugin-data"]` in the
manifest's permissions, otherwise `-32003`.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `key` | string | yes | 1 to 128 characters. |

| Result | Type | Required | Meaning |
|---|---|---|---|
| `value` | any | no | null when the key is unset. |

```json storage.get params
{"key": "devices"}
```

```json storage.get result
{"value": [{"name": "Porch", "address": "10.0.0.40"}]}
```

### `storage.set`

Plugin → host · request

Writes one key, atomically. The whole store is limited to 1 MiB (`-32602` beyond that). Same permission as
`storage.get`.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `key` | string | yes | 1 to 128 characters. |
| `value` | any | no | null removes the key. |

Result: `{}`.

```json storage.set params
{"key": "devices", "value": [{"name": "Porch", "address": "10.0.0.40"}]}
```

### `controller.status`

Plugin → host · notification

The instance's state, shown as the controller's health in the Controllers section, `controller_overview` and
`plugin_info`. A status you report while starting stands (Studio only sets `running` itself if you said nothing).

| Param | Type | Required | Meaning |
|---|---|---|---|
| `instanceId` | string | yes | One of your own instances; others are ignored. |
| `state` | "running" \| "unavailable" \| "error" | yes |  |
| `message` | string | no | Shown with the state. |

```json controller.status params
{"instanceId": "5d1c0f8e-2b4a-4c61-9e0f-7a3b2c1d0e9f", "state": "unavailable", "message": "10.0.0.7 is not answering"}
```

### `controller.stats`

Plugin → host · notification

The instance's counters, which `controller_overview`, `controller_monitor` and `plugin_info` report. Each value is a
**running total** since the instance was created: Studio raises its counter to it and never lowers it. Send them now
and then (once a second is plenty), not per frame. The SDKs' `stats()` does the adding: pass it what you sent since
the last call and it sends the totals.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `instanceId` | string | yes |  |
| `packetsSent` | integer | no | Packets sent so far. |
| `bytesSent` | integer | no | Bytes sent so far. |
| `errors` | integer | no | Send errors so far. |

```json controller.stats params
{"instanceId": "5d1c0f8e-2b4a-4c61-9e0f-7a3b2c1d0e9f", "packetsSent": 4120, "bytesSent": 370800, "errors": 2}
```

### `controller.message`

Plugin → host · notification

A message for the user about one controller. Studio collects it with the other controller messages; it never blocks
the engine.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `instanceId` | string | yes |  |
| `title` | string | yes |  |
| `heading` | string | no |  |
| `text` | string | yes |  |

```json controller.message params
{"instanceId": "5d1c0f8e-2b4a-4c61-9e0f-7a3b2c1d0e9f", "title": "Lamp Output", "heading": "Firmware too old", "text": "Update the lamp's firmware to 2.1 or later."}
```

### `display.snapshot`

Plugin → host · request

A read-only view of the open show: its elements (the tree), controllers and patches, as saved and applied (never a
half-made edit in a dialog). Needs `"display": "read"` in the manifest's permissions, otherwise `-32003`; `-32603`
when no show is open. Nothing you do with it changes the show. Away from Studio, `plugin validate` and `plugin run`
answer with a sample show, or the one in a `--profile` folder ([TESTING.md](TESTING.md)).

A big show doesn't fit one 1 MiB line, so answers come in pages: elements first, then patches, each item whole. While
`more` is true, ask again with the `elementOffset` and `patchOffset` the page gave you. If `revision` changes between
pages the show was edited meanwhile: start over from `{}`. The SDKs' `display_snapshot()` / `displaySnapshot()` do
this and return every page gathered into one. Studio answers one `display.snapshot` per plugin at a time; a second
asked before the first is answered gets `-32603`.

| Param | Type | Required | Meaning |
|---|---|---|---|
| `elementOffset` | integer | no | Where this page's elements start (the previous page's `elementOffset`); 0 when left out. |
| `patchOffset` | integer | no | Where this page's patches start (the previous page's `patchOffset`); 0 when left out. |

```json display.snapshot params
{"elementOffset": 2, "patchOffset": 0}
```

| Result | Type | Required | Meaning |
|---|---|---|---|
| `revision` | integer | yes | Changes whenever the display (elements, controllers, patching) does. |
| `roots` | array of string | yes | The ids of the top-level elements, in order. |
| `controllers` | array of object | yes | Every controller in the Controllers pane's order: `{id, name, typeId, typeName, outputs, running, instanceId?}`. `running` means set to send while Live Output is on. Your own plugin's controllers carry the `instanceId` your `controller.*` messages use. |
| `elements` | array of object | yes | This page's elements, each `{id, name, children}` with the children's ids in order; a light has no children. An element in several groups is listed once. |
| `patches` | array of object | yes | This page's patched runs, in controller order and then output order: `{elementId, controllerId, firstOutput, lastOutput, from, to}`, one run per light per controller (an RGB pixel on three outputs is one run). Outputs are one-based; `from`/`to` are `universe:channel` on E1.31 controllers and output numbers on others. |
| `elementCount` | integer | yes | Elements in the whole show. |
| `patchCount` | integer | yes | Patched runs in the whole show. |
| `more` | boolean | yes | Another page follows. |
| `elementOffset` | integer | yes | The `elementOffset` to ask for the next page. |
| `patchOffset` | integer | yes | The `patchOffset` to ask for the next page. |

```json display.snapshot result
{"revision": 12, "roots": ["2f0c9a52-1b7e-4a39-9d55-0c6e4b8f1a21"], "controllers": [{"id": "9b5e2d4c-3f1a-4e8b-a6c7-5d2e1f0a9b38", "name": "Porch E1.31", "typeId": "b0b3c3b4-8f3a-4c4e-9a4e-9a3f1c6a2b10", "typeName": "E1.31", "outputs": 510, "running": true}], "elements": [{"id": "2f0c9a52-1b7e-4a39-9d55-0c6e4b8f1a21", "name": "Porch", "children": ["7a1d3e6f-9c2b-4d8a-b5e4-1f0c2a3b4d5e"]}, {"id": "7a1d3e6f-9c2b-4d8a-b5e4-1f0c2a3b4d5e", "name": "Porch Pixel 1", "children": []}], "patches": [{"elementId": "7a1d3e6f-9c2b-4d8a-b5e4-1f0c2a3b4d5e", "controllerId": "9b5e2d4c-3f1a-4e8b-a6c7-5d2e1f0a9b38", "firstOutput": 1, "lastOutput": 3, "from": "1:1", "to": "1:3"}], "elementCount": 2, "patchCount": 1, "more": false, "elementOffset": 2, "patchOffset": 1}
```

### `tools.changed`

Plugin → host · notification

Your agent tools changed (after a discovery, say). `tools` replaces the manifest's `contributes.tools` until the plugin
restarts, with the same rules for each tool; leaving it out goes back to the manifest's list. Studio then tells MCP
clients the tool list changed. Also accepted as a request (answered `{}`).

| Param | Type | Required | Meaning |
|---|---|---|---|
| `tools` | array of tool | no | Each item as in the manifest's `contributes.tools`; at most 64. |

```json tools.changed params
{"tools": [{"name": "discover", "description": "Find lamps on the local network.", "readOnly": true}, {"name": "identify_porch", "description": "Blink the porch lamp three times."}]}
```
