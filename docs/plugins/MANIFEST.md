# The manifest: `vixen-plugin.json`

Every plugin folder has a `vixen-plugin.json` at its root. It says who the plugin is, how Studio starts it, what it
may do and what it adds. The JSON Schema is
[`studio/plugins/schema/manifest.schema.json`](../../studio/plugins/schema/manifest.schema.json) (MCP resource
`plugins://schema/manifest`); Studio's validator applies the same rules, and a test checks this page against both.

**Unknown fields are refused**, so a typo is an error rather than a setting silently ignored. `plugin validate` and
`plugin_validate` report every problem with its path (`contributes.controllers[0].typeId: must be a UUID`).

## A complete example

```json vixen-plugin.json
{
  "id": "io.github.someone.lamp-output",
  "name": "Lamp Output",
  "version": "1.2.0",
  "apiVersion": "^1.0",
  "description": "Drive LAN lamps as a Vixen controller.",
  "author": "Someone",
  "license": "MIT",
  "homepage": "https://github.com/someone/vixen-lamp-output",
  "entry": { "command": "python3", "args": ["main.py"] },
  "permissions": { "network": ["udp:lan"], "files": ["plugin-data"] },
  "contributes": {
    "controllers": [{
      "typeId": "6f1c2a8e-41d7-4b2e-9a35-8d2f1b6a4e90",
      "name": "Lamp (LAN)",
      "configSchema": {
        "type": "object",
        "x-order": ["host", "port"],
        "properties": {
          "host": { "type": "string", "format": "ipv4", "title": "Address", "default": "10.0.0.2" },
          "port": { "type": "integer", "title": "Port", "minimum": 1, "maximum": 65535, "default": 4048 }
        },
        "required": ["host"],
        "additionalProperties": false
      },
      "defaultOutputs": 3,
      "maxOutputs": 300,
      "dataPolicy": "highestWins8Bit",
      "frameEncodings": ["u8"]
    }],
    "tools": [{
      "name": "discover",
      "description": "Find lamps on the local network.",
      "inputSchema": { "type": "object", "properties": { "timeoutMs": { "type": "integer", "minimum": 100, "maximum": 5000 } } },
      "readOnly": true
    }],
    "commands": [{ "id": "discover", "title": "Discover Lamps…", "tool": "discover" }],
    "events": ["liveOutput.changed"]
  }
}
```

## Top level

| Field | Type | Required | Rules |
|---|---|---|---|
| `id` | string | yes | Lowercase reverse-DNS with at least one dot (`io.github.someone.lamp-output`), 3–128 characters. Never change it: installs, logs, data and shows key on it. |
| `name` | string | yes | Shown in Settings ▸ Plugins, the Plugins menu and health messages. 1–80 characters. |
| `version` | string | yes | Semantic version `MAJOR.MINOR.PATCH`, optionally `-pre` and `+build`. See [PACKAGING.md](PACKAGING.md#versions). |
| `apiVersion` | string | yes | The range of plugin API versions you support, checked against Studio's (1.1.0). Use `^1.0`, or `^1.1` when you need 1.1's fields (`outputCount`, `actions`) as manifest fields. |
| `description` | string | no | One or two sentences, up to 500 characters; shown in Settings ▸ Plugins and the install sheet. |
| `author` | string | no | Up to 120 characters. |
| `license` | string | no | An SPDX identifier such as `MIT`, up to 64 characters. |
| `homepage` | string | no | An `http://` or `https://` URL. |
| `entry` | object | yes | How Studio starts the plugin (below). |
| `permissions` | object | no | What the plugin may do (below). |
| `contributes` | object | no | What it adds to Studio (below). |

`apiVersion` ranges: `^1.0` (any 1.x from 1.0), `>=1.0 <2`, `1.0.x`. A range that doesn't contain 1.1.0 makes the
plugin `incompatible`: it is listed but never launched.

## `entry`

| Field | Type | Required | Rules |
|---|---|---|---|
| `command` | string | yes | A bare name (`python3`, `node`) is looked up on the plugin's `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin`). A command containing `/` is an absolute path or a path inside the plugin folder (`bin/lamp`); it may not point outside the folder. |
| `args` | array of string | no | Up to 64 arguments. |

The working directory is the plugin folder, so `["main.py"]` finds the file next to the manifest.

## `permissions`

What the install sheet tells the user and what Studio enforces where it mediates. Read
[PERMISSIONS.md](PERMISSIONS.md) for exactly what is and isn't enforced.

| Field | Type | Required | Rules |
|---|---|---|---|
| `network` | array of string | no | Up to 32 rules `<protocol>:<target>`. Protocols: `udp`, `tcp`, `http`, `https`, `ws`, `wss`. Target: `lan` (the local network) or a host name with an optional port (`openapi.api.example.com`, `hub.local:8080`). |
| `files` | array of string | no | `plugin-data`: the plugin keeps state in its data folder and may use `storage.get`/`storage.set`. |
| `display` | string | no | `none` (default) or `read`: may read the show's elements, controllers and patches (`display.snapshot`). |
| `serial` | boolean | no | Uses serial ports (USB adapters, boards). Default false. |

## `contributes`

| Field | Type | Required | Rules |
|---|---|---|---|
| `controllers` | array of object | no | Up to 32 controller (output) types. |
| `tools` | array of object | no | Up to 64 agent tools. |
| `commands` | array of object | no | Up to 64 Plugins-menu commands, each running one of the tools. |
| `events` | array of string | no | Events to receive: `show.started`, `show.stopped`, `sequence.playing`, `sequence.stopped`, `liveOutput.changed`. Only `liveOutput.changed` is sent today. |

## `contributes.controllers[]`

A controller type: what appears under "Plugins" in Add Controller. [CONTROLLERS.md](CONTROLLERS.md) explains each field
in depth.

| Field | Type | Required | Rules |
|---|---|---|---|
| `typeId` | string | yes | A lowercase UUID you generate once (`uuidgen \| tr A-Z a-z`) and keep forever. Shows store it in `ModuleStore.xml`; changing it orphans every controller made with the old one. |
| `name` | string | yes | The type's name in Add Controller and the controller list. 1–80 characters. |
| `configSchema` | object | no | JSON Schema of the controller's settings. Must describe an object. It becomes the inspector's form; see [CONTROLLERS.md](CONTROLLERS.md#settings-configschema). |
| `defaultOutputs` | integer | no | Outputs a new controller gets. 1–100000, default 1. |
| `maxOutputs` | integer | no | The most outputs it may have. 1–100000, default 100000. |
| `dataPolicy` | string | no | How Vixen combines values when several effects drive one output: `highestWins8Bit` (default), `discardExcess8Bit`, `average8Bit`, `oneChannelColor`. |
| `outputNaming` | string | no | A pattern for output names, `{n}` the output number (`Pixel {n}`). Recorded, not applied yet. |
| `frameEncodings` | array of string | no | The frame encodings it accepts, preferred first: `u8`, `json`, `u16`. Default `["u8"]`. Studio uses the first. |
| `outputCount` | string | no | Who sets the output count: `user` (default; the inspector's Outputs…) or `fromConfig` (API 1.1: your plugin, from the settings, through `controller.outputsFor`; the count is read-only in the inspector). See [CONTROLLERS.md](CONTROLLERS.md#output-counts-from-the-settings). |
| `actions` | array of object | no | Settings actions, up to 8 (API 1.1): buttons in the controller inspector, also in its ⋯ menu and `controller_action` for agents. See below. |

A plugin that must also load in Studio before plugin API 1.1, which refuses unknown fields here, puts `outputCount` and
`actions` at the root of its `configSchema` as `x-outputCount` and `x-actions` instead: every Studio's schema checker
ignores `x-` keywords, and 1.1 reads them from there when the fields themselves are absent.

## `contributes.controllers[].actions[]`

A settings action, like Detect Lights. Pressing it calls the named tool with `{instanceId, typeId, config}` (the current
settings); the tool answers new settings (`config` or `mergePatch`, see [PROTOCOL.md](PROTOCOL.md#toolcall)), which
Studio applies like an edit: checked against `configSchema`, resized for `fromConfig` types, one step on Undo.

| Field | Type | Required | Rules |
|---|---|---|---|
| `id` | string | yes | `a-z`, then `a-z0-9_`, up to 48 characters; unique in the type. Agents pass it to `controller_action`. |
| `title` | string | yes | The button's title, 1–40 characters (`Detect Lights`). |
| `tool` | string | yes | The name of one of this plugin's tools. |
| `description` | string | no | The button's help text, up to 300 characters. |

## `contributes.tools[]`

An agent tool, listed to MCP clients as `plugin__<short-id>__<name>`. See [TOOLS_AND_EVENTS.md](TOOLS_AND_EVENTS.md).

| Field | Type | Required | Rules |
|---|---|---|---|
| `name` | string | yes | `a-z`, then `a-z0-9_`, up to 48 characters. |
| `description` | string | yes | What it does, for the agent. 1–1000 characters. |
| `inputSchema` | object | no | JSON Schema of its arguments. Default: no arguments. |
| `readOnly` | boolean | no | True when calling it changes nothing in the world (the agent's client may then run it without asking). Default false: annotated destructive. |

## `contributes.commands[]`

A Plugins-menu item (Plugins ▸ *your plugin* ▸ *title*) and ⌘K palette entry. It calls its tool with no arguments and
shows the answer's text.

| Field | Type | Required | Rules |
|---|---|---|---|
| `id` | string | yes | Same rules as a tool name. |
| `title` | string | yes | The menu item, up to 80 characters. End it with `…` if the tool asks something or takes a while. |
| `tool` | string | yes | The name of one of this plugin's tools. |
