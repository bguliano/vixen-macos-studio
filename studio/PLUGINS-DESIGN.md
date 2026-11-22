# Vixen Studio plugin system: design

A Studio-only feature (vanilla never gets it), built in stages P1–P6. This file is the canonical design: each stage
keeps it current, including the status below.

## Status

| Stage | State |
|---|---|
| P1 Host core | Shipped: `crates/studio-plugins`, schemas in `studio/plugins/schema/`, FFI in `crates/vixen-ffi/src/studio_plugins.rs`. |
| P2 Controllers | Shipped: `crates/studio-plugins/src/controllers.rs` (+ `schema.rs`), the engine hook `crates/vixen-output/src/modules/studio_external.rs`. `display.snapshot` moved to P2b. |
| P3 MCP | Shipped: `crates/studio-plugins/src/{mcp,tools,harness,scaffold,install}.rs`, templates in `studio/plugins/templates/`, wired in `crates/studio-mcp`. |
| P4 UI | Not started. |
| P5 SDKs, examples, CLI | Not started. |
| P6 Docs | Not started. |

What P1 settled, beyond the design below:

- **Layout.** `Plugins/registry.json`, installed plugins in `Plugins/<id>/<version>/`, each plugin's `pluginDataDir` in
  `Plugins/data/<id>/`, logs in `Plugins/logs/<id>.log` (rotated at 1 MiB, one `.log.1` kept; the last 2,000 entries
  stay in memory). All 0700/0600. A damaged `registry.json` is moved aside and an empty one started.
- **Nothing runs uninvited.** A plugin folder found on disk without a registry entry is listed as `disabled`. Opening the
  host starts nothing; `start_enabled` (the app, once wired in P4) or an explicit enable/link/reload does.
- **Clean environment.** A plugin gets only `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin`),
  `HOME`, `USER`, `LOGNAME`, `TMPDIR`, `LANG`, `PYTHONUNBUFFERED=1`, `VIXEN_PLUGIN_ID` and `VIXEN_PLUGIN_DATA_DIR`, so
  nothing of Studio's environment leaks into it. Its working directory is its folder. `entry.command` containing `/`
  is absolute or resolved inside the plugin folder (never outside it); a bare name is looked up on that `PATH`.
- **Process group.** Each plugin runs in its own process group, and stopping it kills the group, so `npm start` → `node`
  never leaves an orphan. Dropping the host (or `shutdown_all`) stops every plugin; plugins must also exit on stdin EOF.
- **Protocol strictness.** Anything on stdout that isn't JSON-RPC 2.0, or a line over 1 MiB, stops the plugin with an
  error naming what it printed, then the normal restart backoff applies. The write queue to a plugin is bounded (1,024
  messages); a plugin that stops reading loses messages and requests time out, the host never blocks.
- **Version negotiation** happens twice: the manifest's `apiVersion` range must contain the host's API (1.0.0) or the
  plugin is `incompatible` and never launched; and `initialize`'s answer must name the same major version, or the
  plugin is stopped as `incompatible` without restarts.
- **Storage needs `"files": ["plugin-data"]`.** `storage.get/set` without it answer `-32003` (permission denied).
- **Manifest additions:** `permissions.serial` (boolean) for serial-port output plugins, and
  `contributes.controllers[].frameEncodings` (`u8`, `json`, `u16`; default `["u8"]`). Unknown fields are refused, so a
  typo is an error rather than a silently ignored setting.
- **Restart policy:** backoff 1 s, 5 s, 30 s; after three restarts within 60 s the plugin is `crashed` until reloaded.
  A failed launch (command missing, exits before answering, `initialize` timeout) counts as a crash.
- **Dev hot reload** polls the linked folder (file count, size and newest change; skipping hidden, `node_modules`,
  `target`, `build`, `dist`, `__pycache__`) and restarts once a change has held for two polls (about 1.5 s).
- **FFI** (free functions, JSON strings like the future MCP tools): `studio_plugins_list`, `studio_plugin_info`,
  `studio_plugin_logs`, `studio_plugin_set_enabled`, `studio_plugin_reload`, `studio_plugin_link_dev`,
  `studio_plugin_unlink_dev`, `studio_plugins_start_enabled`, `studio_plugins_set_muted`, `studio_plugins_required`
  (StudioPlugins.json against what is installed) and `studio_plugins_shutdown`. Not yet called by the Swift app (P4).
- **Deferred:** the `sandbox-exec` profile derived from permissions (a later stage).

What P2 settled:

- **Engine hook.** vixen-output has one marked `ControllerConfig::External` variant and a process-wide provider
  (`studio_external::register`). Without a provider (vanilla, Windows-format tests) nothing changes: plugin data is an
  unknown module type, left alone. The app registers the shared host's provider before a profile loads
  (`VixenCore::start`), so plugin controllers compile into the output graph like built-in ones.
- **Module data** (`ModuleStore.xml`, under the plugin's `typeId`, `dataModelType`
  `VixenStudio.Plugins.PluginControllerData, StudioPlugins`): `<PluginControllerData>` with `ModuleInstanceId`,
  `ModuleTypeId`, `PluginId`, `PluginName`, `PluginVersion` and `Config` (the configuration as JSON text, escaped
  rather than CDATA, which reads the same). The element is kept whole, so a save without the plugin, or by an older
  or newer Studio, writes it back unchanged (tested through Studio with and without a plugin host). Windows Vixen
  can't resolve the data type, so it skips the entry while loading and leaves it out of its own next save; the
  controller itself survives there with its outputs.
- **Requirements.** Each system save rewrites `StudioPlugins.json` from the plugin controllers in the show, needing
  `^major.minor` of the version each was set up with (`^0.minor.patch` for 0.x).
- **Missing, disabled, too old or crashed plugin:** the controller, its outputs and patches stay; health is `error`
  with `Needs plugin Lamp Output (com.example.lamp) ^1.2 — not installed` (or `— version 1.1.0 is installed`, `— it
  is disabled`); nothing is sent. Installing or enabling the plugin brings it back without an edit.
- **Lifecycle.** `OutputModule::start` only happens when Live Output is on and not muted (the runner's rule), so
  muted/headless never sends `controller.start` or frames. A starting controller starts its enabled plugin if it isn't
  running. Each module has a worker thread: `controller.create` (the plugin may normalise `config`) →
  `controller.start` (15 s) → frames; `set_config` → `controller.configure` (a `-32004` refusal keeps
  `data.errors` for the inspector and sets the status); output count → `controller.setOutputCount`; pause/resume;
  stop → `controller.stop` + `controller.destroy`. When the plugin restarts, the worker creates, configures and
  starts the instance again. `outputNames` answers are not applied yet (P4, with the inspector).
- **Frames** use the first encoding the type lists. Each instance has one frame slot; a newer frame replaces one not
  yet written (`framesDropped`), and frames are only written once the plugin has read all but 1 KiB of its stdin
  pipe, so a slow plugin is never a pipe's worth of stale frames behind and requests never queue behind frames. The
  engine's `update` never blocks.
- **Plugin → host:** `controller.status` sets the module status (running / unavailable / error),
  `controller.stats` raises the packet, byte and error counters `controller_overview` reads, `controller.message`
  queues a `ControllerMessage`. A plugin can only address its own instances.
- **Session:** plugin types join `controller_types` and `controller_create`; `controller_configure` takes `config`
  (validated against `configSchema`: the subset in `schema.rs`) and `outputs`; `controller_network`/`controller_info`
  show `plugin` (ids, versions, config, schema, problem); `controller_overview` health reports the plugin's problem or
  its error status; `output_start` accepts plugin controllers under the same live-output permission as E1.31/DDP, and
  commissioning tests reach them as frames (tested end to end with the fixture plugin).
- **`plugin_info`** lists each started instance: status, live, framesSent/framesDropped, lastFrameAgeMs, counters
  and configErrors.
- **P2b:** `display.snapshot` (needs a read-only display view from the open profile).

What P3 settled:

- **Contributed tools.** `plugin__<short-id>__<tool>`: the short id is the id's last part with anything but `a-z0-9`
  as `_` (`io.github.someone.govee-output` → `govee_output`), the whole id the same way when two listed plugins share
  a last part, cut so the name fits 64 characters. Listed for every enabled, valid plugin that hasn't crashed, whether
  running or not; a call starts it (waiting for `initialize`) and allows 30 s for `tool.call`. A `readOnly` tool is
  annotated read-only, every other one destructive. The answer must be `{content: [{type, …}], isError?}`.
- **`tools.changed {tools?}`.** `tools` (same rules as `contributes.tools`) replaces the manifest's list until the
  plugin restarts; no `tools` goes back to it. Accepted as a notification or a request.
- **`list_changed`** without a second channel: the server remembers the plugin tools it last listed and, when they
  differ, writes `notifications/tools/list_changed` before its next answer. `initialize` advertises
  `tools.listChanged`. Plugins learn `capabilities.tools: true`.
- **Manage switch.** `studio_plugins::agents_may_manage` / `set_agents_may_manage` (FFI
  `studio_plugins_agents_may_manage`, `studio_plugins_set_agents_may_manage`), an in-memory flag, so it is off at
  every launch; no MCP setter. Install, uninstall, enable, disable, link, unlink and reload need it and
  `confirm:true`; FFI `studio_plugin_install`/`studio_plugin_uninstall` serve Settings ▸ Plugins (P4).
- **Install.** A folder or `.vixenplugin` zip (manifest at the root) is copied or unpacked into a temporary
  `Plugins/.install-*` folder (at most 20,000 files and 512 MiB; symlinks, absolute paths and `..` refused; only the
  executable bit of a file's mode kept, as 0700/0600), its manifest validated there, then renamed to
  `Plugins/<id>/<version>/`; other versions are removed, the quarantine flag cleared, the registry records the
  package's SHA-256, and the plugin is enabled and started (a failed start leaves it installed, reported as
  `startError`). A plugin linked as a dev folder must be unlinked first. Uninstall keeps `data/<id>` and the log.
- **Scaffold.** Python (python3, standard library) and TypeScript (a plain Node 20 ES module, `main.mjs`, no build
  step) templates, each serving both kinds: `controller` (a type with a `prefix` setting that logs the values it gets,
  refusing unknown settings with `-32004`) and `tools` (`hello`). Written only into a new folder under
  `Plugins/dev/`, never over anything. Rust and Swift wait for the SDKs (P5), which will replace these templates.
- **Harness** (`harness::validate`, also usable from tests and the P5 CLI): manifest and API range, a valid
  configuration made from `configSchema` (defaults, else the smallest value each keyword allows) and an invalid one
  (an undeclared field when `additionalProperties` is false, else a typed field given the wrong type), `entry.command`
  resolution, launch in a throwaway data folder, `initialize` (muted unless live), per controller type create (its
  normalised config must still satisfy the schema), configure, configure invalid (should be `-32004` with
  `data.errors`; anything else warns, since Studio checks the schema first), setOutputCount up and back, start, one
  burst per accepted encoding (a chase at 40 fps, warning when over 10 % are dropped, failing when the plugin then
  stops answering), stop and destroy, then protocol health (only JSON-RPC on stdout, still running), `shutdown` and
  exit within the grace period. Tools are listed, not called (a tool can act on the world). `plugin_validate` runs only
  folders under `Plugins/dev` or installed/linked plugins, and start + frames only with `liveOutput:true`, `confirm`,
  Live Output on and the live-output permission.
- **`plugin_send_test_frame {controllerId, pattern?, …, confirm}`** is `output_test_pattern` on one plugin controller
  (movingPixel by default), after checking the controller is a plugin's. Explicit `values` were left out: the
  commissioning patterns cover testing.
- **Resources and prompt.** `plugins://api` serves this file until P6 writes `docs/plugins/PROTOCOL.md`;
  `plugins://examples/<language>` the scaffold's controller plugin as Markdown until P5's examples exist.

## Why

Braden wants to drive Govee lights from a Vixen show **without** Govee support being built into Studio. Studio needs a
general, documented way to add capability from outside: first and most importantly new **controller types** (an output
adapter like Govee LAN, Hue, Nanoleaf, MQTT, a custom serial board), then agent tools, commands and event hooks. It
must be equally pleasant for a human developer (docs, examples, SDKs, a test harness) and for an AI agent (MCP tools to
scaffold, validate, run, inspect and debug a plugin, plus machine-readable schemas and docs as MCP resources).

Non-goal: a Govee plugin in this repo. The examples must be generic (console, UDP pixel, HTTP). A Govee plugin will be
its own repo built *against* this system.

## Core decision: out-of-process plugins over JSON-RPC

A plugin is a folder with a manifest and an executable (any language). Studio launches it as a child process and talks
**JSON-RPC 2.0 over stdio, newline-delimited** (the same framing the MCP bridge uses; logs go to stderr, never stdout).

Chosen over the alternatives because:
- **Native dylibs**: unsafe (a plugin crash kills the show), no stable Rust ABI, and a notarized hardened-runtime app with
  library validation can't load unsigned third-party dylibs. Rejected.
- **WASM (wasmtime)**: sandboxed, but output plugins need UDP/HTTP/serial and arbitrary libraries (Govee LAN is UDP
  multicast); every capability would need a host shim, and it locks developers into WASM toolchains. Possible later as a
  second runtime behind the same protocol; not v1.
- **Out-of-process JSON-RPC** (LSP / MCP / VS Code extension-host model): any language (Python, Node, Swift, Rust, Go),
  crash isolation, trivially testable from a terminal, and the protocol an AI agent already understands. Frame rates are
  low (Vixen runs 20–40 fps; a Govee plugin needs a handful of channels), so pipe overhead is irrelevant; large pixel
  plugins use the compact binary frame encoding below.

Security honesty (write this in the docs): the process runs as the same user, like the MCP bridge (same-user permissions
are the trust boundary, not a sandbox). Declared permissions are shown at install and enforced wherever Studio mediates
(host API calls, which events/data a plugin receives, live-output gating). Optional hardening, P1 if cheap otherwise
backlog: launch under a generated `sandbox-exec` profile derived from the manifest's permissions.

## Where plugins live

- Installed: `~/Library/Application Support/Vixen Studio/Plugins/<plugin-id>/<version>/` (dir 0700). One active version
  per id; `Plugins/registry.json` records installed versions, enabled flag, install source, sha256 of the package.
- Dev-linked: a registry entry pointing at an absolute folder (symlink-free check on the folder itself), marked `dev`.
  Dev plugins hot-reload: a file change in the folder restarts the process (debounced) and re-runs config.
- Per profile: `<profile>/SystemData/StudioPlugins.json` lists the plugins the show's controllers need
  (`{id, versionRange}`), so a show moved to another Mac can say "needs Govee Output ≥1.2" instead of silently dropping
  lights. Studio writes it when a plugin controller is created/removed.
- Package format: `.vixenplugin` = zip of the plugin folder (manifest at root). Install = verify, unpack to a temp dir,
  validate, show permissions, move into place atomically. Strip `com.apple.quarantine` only after the user confirms.

## Manifest: `vixen-plugin.json`

JSON Schema lives at `studio/plugins/schema/manifest.schema.json` (single source of truth for the host validator, the
SDKs, the docs and the MCP resource).

```json
{
  "id": "io.github.someone.govee-output",
  "name": "Govee Output",
  "version": "1.2.0",
  "apiVersion": "^1.0",
  "description": "Drive Govee lights over the LAN API as a Vixen controller.",
  "author": "Someone", "license": "MIT", "homepage": "https://github.com/someone/vixen-govee",
  "entry": { "command": "python3", "args": ["main.py"] },
  "permissions": {
    "network": ["udp:lan", "http:openapi.api.govee.com"],
    "files": ["plugin-data"],
    "display": "read"
  },
  "contributes": {
    "controllers": [{
      "typeId": "6f1c2a8e-…",
      "name": "Govee (LAN)",
      "configSchema": { "type": "object", "properties": { "devices": { "type": "array", "items": { "…": "…" } } } },
      "defaultOutputs": 3,
      "maxOutputs": 3000,
      "dataPolicy": "highestWins8Bit",
      "outputNaming": "Device {n} {channel}"
    }],
    "tools": [{ "name": "discover", "description": "Find Govee devices on the LAN.", "inputSchema": {}, "readOnly": true }],
    "commands": [{ "id": "discover", "title": "Discover Govee Devices…", "tool": "discover" }],
    "events": ["show.started", "show.stopped", "sequence.playing", "sequence.stopped", "liveOutput.changed"]
  }
}
```

Rules: `id` reverse-DNS, lowercase; `typeId` a UUID the plugin owns forever (it's what `ModuleStore.xml`/`SystemConfig`
store); `apiVersion` a semver range checked against the host's API version (start at 1.0.0; additive = minor, breaking
= major with a documented migration).

## Protocol (API v1)

All messages JSON-RPC 2.0, one per line, UTF-8. Machine-readable schemas for every method in
`studio/plugins/schema/protocol.schema.json`. Host→plugin requests time out (5 s default; `start` 15 s); a plugin may
send host requests any time after `initialize`.

Host → plugin
- `initialize {apiVersion, hostVersion, pluginDataDir, profile: {name, id}, muted, capabilities}` → `{apiVersion,
  capabilities}`. Version negotiation happens here; mismatch → plugin disabled with a clear status.
- `shutdown` → `{}` then the host closes stdin; kill after 3 s.
- Controllers: `controller.create {instanceId, typeId, config, outputCount}` → `{config?, outputNames?}` (plugin may
  normalise config), `controller.configure {instanceId, config}` → `{config}` or a JSON-RPC error with field-level
  `data.errors` (shown inline in the inspector), `controller.setOutputCount`, `controller.start`, `controller.stop`,
  `controller.pause`, `controller.resume`, `controller.destroy`.
- `controller.frame {instanceId, seq, timeMs, encoding, values}`: a **notification** (no reply). `encoding` is `"u8"`
  (base64 of one byte per output, the common case), `"json"` (array of `null|int|{"r","g","b"}|string`, the full
  `OutputCommand` range) or `"u16"` (base64 little-endian). The plugin declares which it accepts.
- `tool.call {name, arguments}` → MCP-style `{content:[…], isError?}`.
- `event {name, data}` notification, only for events the manifest declares.

Plugin → host
- `log {level, message}` (also stderr lines, captured as `info`).
- `controller.status {instanceId, state: running|unavailable|error, message?}` → maps to `ModuleStatus`.
- `controller.stats {instanceId, packetsSent?, bytesSent?, errors?}` (feeds `controller_monitor`/`controller_overview`).
- `controller.message {instanceId, title, heading?, text}` → `ControllerMessage` (alerts collected by the UI, never
  blocking the engine).
- `display.snapshot` (only with `display: read`) → read-only elements/controllers/patches.
- `storage.get/set {key, value}` → small JSON KV in `pluginDataDir/state.json`.
- `tools.changed` notification (plugin-contributed tool list changed at runtime, e.g. after discovery).

Lifecycle guarantees (the host's job, all tested):
- One process per plugin, serving all of its controller instances.
- **The engine never waits on a plugin.** Each plugin controller keeps a latest-frame-wins slot: a slow plugin drops
  frames (counted as `framesDropped`), never stalls the controller thread or other controllers.
- Crash/exit/garbage on stdout → instances go `Error("plugin exited: …")`, the show keeps running, restart with backoff
  (1 s, 5 s, 30 s; give up after 3 in 60 s until the user or `plugin_reload` restarts it), then re-create/configure/start
  instances that were running.
- Muted (headless bridge, `VIXEN_MUTE`, Live Output off): `controller.start` is never sent and frames never flow; plugins
  are told `muted` in `initialize` and via `liveOutput.changed`.
- Live-output gating is identical to E1.31/DDP: agents need the existing local permission (`studioAllowAgentsLiveOutput`)
  to start a plugin controller or run a test pattern on it; revocation stops them.
- Messages are bounded (1 MiB per line like the MCP limits); the host never blocks on a plugin's stdout/stderr pipe.

## Controller extension point (the Govee use case)

- A `PluginOutputModule` implements `OutputModule` (crates/vixen-output) by forwarding to the plugin host. Prefer adding
  it in a Studio-only crate (`crates/studio-plugins`) behind a registration hook in vixen-output that is a small,
  marked (`STUDIO-BEGIN plugins`) change, so vanilla syncs stay clean. Plugin controller types join `controller_types`,
  `controller_create`, the Display pane's Add Controller menu, patching, test patterns, `controller_overview` health and
  `diagnostics_report`, exactly like built-in types.
- Config storage: the controller's module data in `ModuleStore.xml` holds the plugin id, version and the config as JSON
  in CDATA, under the plugin's `typeId`. Round-trip must be lossless in Studio with the plugin, Studio without it,
  vanilla and Windows Vixen (vanilla/Windows already keep unknown module types: `GraphWarning::UnknownControllerType`;
  prove the data survives a load/save there too).
- Plugin missing/disabled/incompatible: the controller stays, patches stay, health = `error` with text "Needs plugin
  Govee Output (io.github…) ≥1.2 — not installed", output silent. Installing the plugin brings it back without edits.
- Inspector: the controller inspector renders the plugin's `configSchema` as a native form (string, number, integer
  with min/max, boolean, enum, array of objects as an editable table, `format: ipv4`), validated live, with field
  errors from `controller.configure` shown inline. Edits are live with Undo (UI_GUIDE: no OK/Cancel windows), one undo
  step per edit, through the same Rust command layer the MCP uses (`controller_configure` gains a `config` object for
  plugin types).

## Agent tools, commands and events

- Plugin-contributed tools appear in Studio's MCP `tools/list` as `plugin__<short-id>__<name>` (sanitised), with the
  manifest's schema, proxied to `tool.call`. Advertise `tools.listChanged` and send `notifications/tools/list_changed`
  when plugins load/unload or send `tools.changed` (check the current adapter's protocol version supports it; otherwise
  document that clients re-list).
- Contributed commands appear in a **Plugins** menu (one submenu per plugin) and the ⌘K palette (`command_palette`
  reports them with their agentTool).
- Events: declared names only; payloads contain no audio paths or show data beyond names/ids (same rule as activity
  events).

## MCP developer tools (for AI agents), resources and prompt

Read-only unless noted. All follow existing conventions (confirm for destructive, bounded output, structured errors).
- `plugin_list`: installed + dev plugins, version, enabled, state (running/stopped/crashed/incompatible), pid, uptime,
  contributed types/tools, last error, restart count.
- `plugin_info {id}`: manifest, permissions, per-instance controller status and stats (frames received/dropped, last
  frame age, round-trip latency of the last request).
- `plugin_logs {id, limit, level?}`: bounded tail of the plugin's captured stderr + `log` messages (ring buffer, also
  written to `Plugins/logs/<id>.log`, rotated).
- `plugin_scaffold {id, name, language: python|typescript|rust|swift, kind: controller|tools, folder?}` (write): creates
  a working plugin from the SDK templates in a dev folder under `~/Library/Application Support/Vixen Studio/Plugins/dev/`
  (only there; no arbitrary paths), returns the files written and next steps.
- `plugin_validate {path|id}`: manifest + schema checks, then the **conformance harness**: spawn the plugin in isolation,
  run initialize, create/configure (with schema-generated valid and invalid configs), start, a burst of synthetic frames
  in each encoding it accepts at 40 fps, stop, destroy, shutdown; report pass/fail per check, timings, dropped frames,
  stderr. Never touches the live engine or real output unless `liveOutput:true` with the normal permission and confirm.
- `plugin_install {path}` / `plugin_uninstall {id}` / `plugin_enable` / `plugin_disable` / `plugin_link_dev {path}` /
  `plugin_reload {id}` (write): require `confirm:true` **and** a local-UI-only switch "Allow agents to manage plugins"
  (default off, resets each launch, like the live-output permission; no MCP setter). Read-only tools and scaffold/validate
  don't need it.
- `plugin_send_test_frame {instanceId, pattern|values, durationMs}`: drive one plugin controller with a test pattern via
  the existing commissioning path (`studio_commission.rs`), same gating.
- Resources: `plugins://api` (the protocol reference as Markdown), `plugins://schema/manifest`,
  `plugins://schema/protocol`, `plugins://examples/<language>` (the example plugin's files), `plugins://<id>/manifest`.
- Prompt: `build_plugin` — the step-by-step workflow (scaffold → implement → validate → link dev → test frames →
  package), so an agent with only the MCP connection can ship a plugin.

## SDKs, examples, harness

- `studio/plugins/sdk/python/vixen_plugin.py`: single file, stdlib only, decorators for controllers/tools/events,
  handles framing, base64 frames, logging, errors.
- `studio/plugins/sdk/typescript/` (Node ≥20, zero runtime deps, not published to npm; docs explain vendoring).
- `crates/vixen-plugin-sdk` (Rust, small, serde only; not a workspace member of the app build if that inflates builds —
  excluded from the app target).
- Swift: optional (template only if cheap).
- Examples under `studio/plugins/examples/`: `console-output` (Python: prints channel values; the tutorial plugin),
  `udp-pixel-output` (TypeScript: RGB over UDP to a configurable host:port, with a `discover`-style tool), `rust-http-output`
  (Rust: POSTs JSON colour state to a URL, rate-limited — the shape a cloud-API device needs). Each passes
  `plugin_validate` in CI-able tests.
- CLI for humans/CI: `vixen-studio-mcp plugin validate <dir>`, `plugin pack <dir>` (→ `.vixenplugin` with sha256),
  `plugin new …` (same templates as `plugin_scaffold`), `plugin run <dir> --frames pattern` (harness with a live readout).

## Human documentation (`docs/plugins/`)

- `README.md` overview: what plugins can do, architecture diagram, security model, version policy.
- `GETTING_STARTED.md`: 15-minute tutorial building `console-output` in Python from `plugin new` to seeing it in a show.
- `PROTOCOL.md`: full API reference, every method with params/results/errors and JSON examples (generated or checked
  against `protocol.schema.json` by a test so it can't drift).
- `MANIFEST.md`, `CONTROLLERS.md` (channels, data policies, output naming, frame encodings, timing, rate-limiting a slow
  device, colour order, recipes for "one RGB device per 3 outputs"), `TOOLS_AND_EVENTS.md`, `PERMISSIONS.md`,
  `PACKAGING.md` (packing, versioning, signing/notarising your own binaries, distribution), `TESTING.md` (harness,
  `plugin_validate`, debugging with logs), `TROUBLESHOOTING.md`, `AGENTS.md` (how an AI agent builds a plugin through MCP).
- Link from README.md, STUDIO.md and studio/MCP.md; regenerate studio/MCP-TOOLS.md.

## UI (follow docs/UI_GUIDE.md and studio/UI-ROADMAP.md conventions)

- Settings ▸ Plugins: list (icon/name/version/state/source), enable switch, Install from file… (`.vixenplugin` or folder,
  drag-and-drop), Link Development Folder…, Reload, Uninstall, Reveal in Finder, a permissions sheet at install
  ("This plugin can: send UDP on your local network…"), a log viewer, the "Allow agents to manage plugins" switch.
- Missing-plugin banner when a show needs plugins that aren't installed.
- Controller inspector auto-form (above); Add Controller lists plugin types under a "Plugins" heading.
- Plugins menu + palette entries for contributed commands. MCP parity (UI_GUIDE principle 6): every UI action has its
  tool and vice versa.

## Stages (in order; each must build, test and ship on its own)

- **P1 Host core**: `crates/studio-plugins` — manifest + JSON Schemas, discovery/registry, process supervisor, JSON-RPC
  framing, initialize/shutdown, logs ring buffer, crash/backoff, StudioPlugins.json, FFI surface for the UI. Tests with a
  fixture plugin (a tiny Python or Rust test binary). Commit this spec as `studio/PLUGINS-DESIGN.md`.
- **P2 Controllers**: `PluginOutputModule`, registration in controller types/create/configure, ModuleStore round-trip
  (incl. without the plugin and through vanilla's loader), missing-plugin health, latest-frame slot, frame encodings,
  status/stats/messages, live-output gating, commissioning tests on plugin controllers.
- **P3 MCP**: developer tools, contributed tools proxy + list_changed, resources, `build_plugin` prompt, agent-manage
  permission switch (FFI + Rust; UI wiring in P4), `plugin_validate` conformance harness.
- **P4 UI**: Settings ▸ Plugins, inspector auto-form, missing-plugin banner, Plugins menu/palette, Add Controller list.
  Studio Capture screenshots of each.
- **P5 SDKs + examples + CLI**: Python/TypeScript/Rust SDKs, templates shared by `plugin new` and `plugin_scaffold`, the
  three examples passing validation, `plugin pack/run`.
- **P6 Docs**: `docs/plugins/*` as above, PROTOCOL.md drift test, links, MCP-TOOLS regen. Final end-to-end check: from a
  clean dev folder, an agent using only MCP scaffolds a UDP output plugin, validates it, links it, patches a test element
  to it on a show copy and runs a muted→permitted test pattern (verified with a local UDP listener, never real lights).

If a stage turns out too big to finish well at once, ship the verified part and split the rest into a `P<n>b` stage.
