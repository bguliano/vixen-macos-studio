# Vixen Studio plugins

Plugins add capability to Vixen Studio without changing Studio itself: most importantly **new controller types**
(drive a Wi-Fi lamp, a cloud-controlled light, an MQTT bridge or a custom board from a Vixen show), and also **agent
tools**, **Plugins-menu commands** and **event hooks**. Plugins are a Studio feature; the vanilla macOS port and
Windows Vixen don't load them, though shows using plugin controllers still open there.

A plugin is a folder with a manifest (`vixen-plugin.json`) and a program in any language. Studio runs the program and
talks to it over stdin/stdout with JSON-RPC 2.0, one message per line.

## Pages

| Page | Read it when |
|---|---|
| [GETTING_STARTED.md](GETTING_STARTED.md) | You're new: build a working output plugin in Python in 15 minutes. |
| [CONTROLLERS.md](CONTROLLERS.md) | You're writing an output plugin: outputs, frames, encodings, settings forms, slow devices, colour order. |
| [TOOLS_AND_EVENTS.md](TOOLS_AND_EVENTS.md) | You're adding agent tools, menu commands or event handlers. |
| [MANIFEST.md](MANIFEST.md) | You need every manifest field and its rules. |
| [PROTOCOL.md](PROTOCOL.md) | You want every message in detail, or are writing a plugin without an SDK. |
| [PERMISSIONS.md](PERMISSIONS.md) | You want to know what a plugin can do and what Studio enforces. |
| [TESTING.md](TESTING.md) | Validating, running and debugging a plugin. |
| [PACKAGING.md](PACKAGING.md) | Packing, versioning, signing and sharing a plugin. |
| [TROUBLESHOOTING.md](TROUBLESHOOTING.md) | Something doesn't work. |
| [AGENTS.md](AGENTS.md) | An AI agent is building the plugin through Studio's MCP server. |

## How it fits together

```
 Vixen Studio                                       stdin: requests,       your plugin (any language)
                                                    frames
 show engine ── plugin controller ─┐                ──────────────►    SDK file ──► your code ──► device
  (20–40 fps)  (latest frame wins) │                                         (protocol)   (Controller,   (UDP, HTTP,
 MCP server ── plugin__x__tool ────┼── plugin host   ◄──────────────                  tools)        serial…)
 Plugins menu, Settings ▸ Plugins ─┘                stdout: answers,
                                                    status, logs
                                                    stderr: the plugin's log
```

- **One process per plugin**, serving every controller of its types. Studio starts it, restarts it after a crash
  (1 s, 5 s, 30 s; it gives up after three in a minute) and stops it when Studio quits.
- **The engine never waits for a plugin.** Each controller keeps only the newest frame for its plugin; a slow plugin
  skips frames, and a crashed one leaves the show running.
- **Settings are a JSON Schema** in the manifest. Studio turns it into the controller inspector's form, checks every
  edit against it, and stores the settings in the show.
- **Shows remember what they need.** A show using a plugin's controller lists the plugin in `StudioPlugins.json`;
  opened without it, the controller and patches stay and Studio says which plugin to install.

## SDKs

Each SDK is one file you copy next to your main file (`plugin new` and `plugin_scaffold` do it for you). It handles
the protocol, frame decoding, logging, settings checks and errors, so your code is only your device's logic.

| Language | File | Needs |
|---|---|---|
| Python | [`vixen_plugin.py`](../../studio/plugins/sdk/python/vixen_plugin.py) | Python 3.9+, standard library only. |
| TypeScript / JavaScript | [`vixen-plugin.mjs`](../../studio/plugins/sdk/typescript/vixen-plugin.mjs) + [`vixen-plugin.d.ts`](../../studio/plugins/sdk/typescript/vixen-plugin.d.ts) | Node 20+, no dependencies, no build step. |
| Rust | [`crates/studio-plugin-sdk`](../../crates/studio-plugin-sdk) (package `vixen-plugin-sdk`), or its `src/lib.rs` copied in as `src/vixen_plugin_sdk.rs` | `serde_json` only. |

Any other language works too: implement [PROTOCOL.md](PROTOCOL.md) directly.

## Examples

In [`studio/plugins/examples/`](../../studio/plugins/examples), each passing the conformance harness with live frames in
Studio's tests:

- **`console-output`** (Python): the tutorial plugin. Prints the values it gets; settings with an extra check, status,
  a tool with a menu command, an event, storage.
- **`udp-pixel-output`** (TypeScript): raw RGB per frame in one UDP datagram, colour order, a frame-rate cap, stats,
  and a `discover` tool.
- **`rust-http-output`** (Rust): JSON POSTs to a URL from a worker thread, rate-limited and only on change: the shape a
  cloud API device needs.

## The command line

The helper inside the app (`Vixen Studio.app/Contents/Helpers/vixen-studio-mcp`) has plugin commands that need no show
open:

```sh
vixen-studio-mcp plugin new --id io.github.you.lamp --name "Lamp" --language python   # a working plugin from a template
vixen-studio-mcp plugin validate ./lamp [--live]                                      # the conformance harness
vixen-studio-mcp plugin run ./lamp --frames chase                                     # one live controller, with a readout
vixen-studio-mcp plugin pack ./lamp                                                   # lamp's .vixenplugin + .sha256
```

## Security in one paragraph

A plugin runs as you, like any app you install. Studio shows what a plugin says it does before you install it, gives it
a clean environment and its own data folder, starts it under a macOS sandbox profile made from its permissions (files,
network ports and serial it didn't declare are refused), enforces the permissions wherever Studio hands something out
(storage, events, show data), and keeps real lights behind Live Output and your permission for agents. The sandbox
keeps honest plugins honest; it isn't a security boundary.
Install plugins from people you trust. Details: [PERMISSIONS.md](PERMISSIONS.md).

## Versions

The plugin API is **1.0.0**. Minor versions only add; anything breaking would be 2.0 with a migration guide. Declare
`"apiVersion": "^1.0"` in your manifest. Your plugin's own `version` follows semantic versioning, and shows record the
`^major.minor` they were set up with ([PACKAGING.md](PACKAGING.md#versions)).

The design behind all this, stage by stage, is in [`studio/PLUGINS-DESIGN.md`](../../studio/PLUGINS-DESIGN.md).
