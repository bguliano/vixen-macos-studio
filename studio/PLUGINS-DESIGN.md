# Vixen Studio plugin system: design

A Studio-only feature (vanilla never gets it), built in stages P1–P6. This file is the canonical design: each stage
keeps it current, including the status below.

## Status

| Stage | State |
|---|---|
| P1 Host core | Shipped: `crates/studio-plugins`, schemas in `studio/plugins/schema/`, FFI in `crates/vixen-ffi/src/studio_plugins.rs`. |
| P2 Controllers | Shipped: `crates/studio-plugins/src/controllers.rs` (+ `schema.rs`), the engine hook `crates/vixen-output/src/modules/studio_external.rs`. `display.snapshot` moved to P2b. |
| P2b Display snapshot | Shipped (B40): `crates/vixen-session/src/studio_plugin_display.rs` builds the view, `studio-plugins` answers it through `PluginHost::set_display`, wired in `crates/vixen-ffi/src/studio_plugins.rs`. |
| P3 MCP | Shipped: `crates/studio-plugins/src/{mcp,tools,harness,scaffold,install}.rs`, templates in `studio/plugins/templates/`, wired in `crates/studio-mcp`. |
| P4 UI | Shipped: Settings ▸ Plugins (`app/Sources/VixenAdmin/StudioPluginsSettings.swift`), the missing-plugin banner, the Plugins menu (`app/Sources/VixenKit/StudioPlugins.swift`), the plugin controller inspector and Add Controller's Plugins heading (`app/Sources/VixenSetup/Window/StudioPluginControllerInspector.swift`, `StudioAddControllerMenu.swift`); the form, permissions and commands are worked out in `crates/studio-plugins/src/ui.rs`. Harness: `scripts/check_studio_plugins_ui.py`. |
| P5 SDKs, examples, CLI | Shipped: SDKs in `studio/plugins/sdk/{python,typescript}/` and `crates/studio-plugin-sdk` (package `vixen-plugin-sdk`), templates in `studio/plugins/templates/{python,typescript,rust}/`, examples in `studio/plugins/examples/`, the CLI in `crates/studio-plugins/src/{cli,pack}.rs` (`vixen-studio-mcp plugin …`). |
| P6 Docs | Shipped: developer docs in `docs/plugins/` (served as `plugins://api` and `plugins://docs/<page>`), the drift tests `crates/studio-plugins/tests/docs.rs` and `crates/studio-mcp/tests/plugins.rs` (MCP-TOOLS.md, tool names), the end-to-end agent check `crates/studio-mcp/tests/plugin_agent.rs`. |

What P1 settled, beyond the design below:

- **Layout.** `Plugins/registry.json`, installed plugins in `Plugins/<id>/<version>/`, each plugin's `pluginDataDir` in
  `Plugins/data/<id>/`, logs in `Plugins/logs/<id>.log` (rotated at 1 MiB, one `.log.1` kept; the last 2,000 entries
  stay in memory). All 0700/0600. A damaged `registry.json` is moved aside and an empty one started.
- **One owner per folder.** The app and a headless `vixen-studio-mcp` (another profile) can open the same folder. The
  first host to take `Plugins/.lock` (`flock`, holding its pid) owns it: only it writes `registry.json` and starts
  plugins. Any other host lists the plugins read-only (`readOnly`, `ownerPid` in `plugin_list`), re-reads the
  registry on refresh, refuses start/enable/install/link/reload with the owner's pid, and takes over within a second
  of the owner quitting, starting the enabled plugins if it had been asked to. Plugins never inherit the lock.
  Settings ▸ Plugins in a read-only process says the plugins run in another process (with its pid), shows enabled
  plugins as Elsewhere and disables every change until it takes over (backlog B45).
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
- **Deferred:** the `sandbox-exec` profile derived from permissions (settled in backlog B39, below).

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
- **P2b:** `display.snapshot`, shipped as below.

What P2b settled:

- **Provider hook.** `studio-plugins` can't see `AppCore`, so the app installs a `DisplaySource` with
  `PluginHost::set_display` on every `studio_plugins_sync` (the show window's poll). It holds the core weakly: a closed
  show answers `-32603` "no show is open", as does a host with no source (the harness, the CLI, tests). Running
  plugins use whatever source is current at each call.
- **Permission first.** Without `"display": "read"` the host answers `-32003` and never calls the source. Params are
  checked before the show is read (`-32602` for anything but non-negative `elementOffset`/`patchOffset`).
- **Off the reader thread.** Reading the show takes the app's locks (the system, playback's running controllers), and
  the plugin's stdout reader must keep reading answers meanwhile (a controller starting while the plugin asks would
  otherwise wait out its timeout), so each snapshot is built on its own thread. One at a time per plugin: a second
  request while one is being built gets `-32603`.
- **What it shows.** A fresh `SetupSession::open` each call, so the saved and applied display, never a half-made edit.
  `{revision, roots, controllers, elements, patches}`: elements `{id, name, children}` (tree node ids, the ids MCP and
  `patch_ranges` use), controllers in pane order `{id, name, typeId, typeName, outputs, running, instanceId?}` (the
  plugin's own controllers carry the `instanceId` of its `controller.*` messages), patches as runs of consecutive
  outputs one light feeds on one controller `{elementId, controllerId, firstOutput, lastOutput, from, to}` (one-based;
  `from`/`to` universe:channel on E1.31, as `patch_ranges` writes them). Channel-level detail (which colour on which
  output) is left out: a plugin that needs it can follow the light's run order.
- **Pages.** A 12,000-pixel show is well over the 1 MiB line limit, so answers are pages under 768 KiB
  (`studio_plugin_display::PAGE_BYTES`): elements, then patches, whole items, at least one per page. `more`,
  `elementOffset`, `patchOffset`, `elementCount`, `patchCount`; `revision` lets a plugin restart when the show changed
  between pages. The SDKs' `display_snapshot()` / `displaySnapshot()` gather every page (and retry up to five times
  on a revision change). `initialize` advertises `capabilities.display: true`.
- **Tests.** `studio_plugin_display` unit tests (content, paging, a 12,000-light show under the limit) and the host
  fixture test `display_snapshot_needs_the_display_permission_and_reads_the_open_show` (`--display=<params>`).

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
  `Plugins/dev/`, never over anything. P5 replaced these templates with SDK-based ones and added Rust.
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
- **Resources and prompt.** `plugins://api` served this file until P6 (now `docs/plugins/PROTOCOL.md`);
  `plugins://examples/<language>` serves the P5 examples.

What P4 settled:

- **Settings ▸ Plugins** lists every installed and linked plugin (name, version, Development, state, an On switch) and
  the selected one's description, controller types, agent tools and permissions in words; Reload, Show in Finder,
  Log… (a live log sheet, filtered by level) and Uninstall… (Unlink… for a dev folder) confirm destructive steps with
  verbs. Install from File… (a folder or `.vixenplugin`, also by dropping one on the list) first reads the package
  without installing it (`inspect_package`: stage, validate, delete) and shows an install sheet with what it adds and
  "This plugin can: …" from `ui::permission_sentences`; only Install copies it. The "Allow agents to manage plugins"
  switch lives here. Plugins ▸ Manage Plugins… and the banner open Settings on this tab (`studio.settingsTab`).
- **The app drives the host.** The show window calls `VixenCore::studio_plugins_sync` when the profile is up and every
  3 s: it gives the host the profile, sets `muted` from Live Output (and `VIXEN_MUTE`/`VIXEN_UI_TEST`), starts the
  enabled plugins once per launch, and returns the show's requirements. Quitting calls `studio_plugins_shutdown`.
  `VIXEN_STUDIO_PLUGINS` (absolute) replaces the plugin folder, so UI tests never touch the user's plugins.
- **Missing-plugin banner** above the show window's sections while any requirement isn't satisfied, naming each with
  the host's reason; MCP `plugin_required` returns the same list. A save rewrites `StudioPlugins.json` from the show's
  controllers, so the banner follows the show as it is now.
- **Inspector form.** `ui::form(configSchema)` turns the schema subset into fields: string (`ipv4`/`hostname`
  placeholders), number, integer, boolean, choice (`enum`, labels from `x-enumNames`), object (a disclosure group),
  table (an array of flat objects: a row per item with Add Row and remove), list (an array of strings or numbers),
  anything else as JSON text. Properties come in the order of the object's `x-order` array (on a table, its `items`),
  then by a property's integer `x-order`, then by name; `title` and `description` label them. The inspector never grows
  past its column: a table too wide becomes one card per row, and in Display the inspector scrolls under the controller
  list. A new row's missing required values show once the row is edited or left. Copy Settings and Paste Settings move
  the configuration as JSON; a paste is one checked edit.
  Each finished edit (Return, leaving the field, a switch, a choice) is checked with `schema::validate`; problems show
  under their fields and the controller keeps its last good configuration, otherwise it is `controller_configure
  {id, config}` on the working copy, one step on the display's Undo (an edit that changes nothing makes none). The
  plugin's own refusals (`controller.configure` `-32004`) reach the form as `configErrors` in the controller's
  `plugin` description. A controller whose plugin is missing shows the reason and Manage Plugins….
- **Add Controller** lists built-in types, then plugin types under "Plugins" (Install Plugins… when there are none),
  read from Rust each time so a newly installed type appears without reopening the pane.
- **Plugins menu**: one submenu per plugin with its contributed commands (`commands()`: title, the MCP name of the tool
  it runs, enabled while that tool can be called), then Manage Plugins…. Running one calls its tool with no arguments
  (`run_command`, starting the plugin if needed) and shows the answer's text. The ⌘K palette finds them as menu items;
  the app hands Rust each one's tool, so palette entries and `command_palette` name it as `agentTool`.
- **Deferred:** applying `outputNames` answers to output names, and the controller context menu's Configure… for a
  plugin controller (the inspector does it).

What P5 settled:

- **One SDK file per language**, copied next to the plugin's main file so a plugin folder is self-contained and
  installs as it is: `vixen_plugin.py` (Python 3.9+, standard library), `vixen-plugin.mjs` + `vixen-plugin.d.ts`
  (Node 20+, no dependencies, types for TypeScript without a build step), and the Rust SDK's `src/lib.rs`, usable as a
  crate or copied in as `src/vixen_plugin_sdk.rs` (`mod vixen_plugin_sdk;`; `serde_json` only). The crate lives in
  `crates/studio-plugin-sdk` (Studio-only by name) but its package is named `vixen-plugin-sdk` (not published to crates.io); it is a workspace
  member so `cargo test` builds and checks the Rust example and template (its `[[example]]`s), and has its own version
  and edition so a copy builds anywhere. Tests keep every copy identical to its SDK (`tests/examples.rs`).
- **Same shape in every language:** a `Plugin` with controller types (a `Controller` subclass / trait object per
  instance: `configure`, `set_output_count`, `start`, `stop`, `frame`, `destroy`, `output_names`, plus `status`,
  `stats`, `message`), tools (a return string is text, `{content}` passes through, anything else is JSON text; a
  `ToolError`/`Err` answers `isError`), events (`liveOutput.changed` also updates `muted`), `on_initialize` /
  `on_shutdown`, `log` at four levels, `storage_get/set` and `tools_changed`. `controller(type)` takes the typeId or
  the type's name, or nothing when the manifest declares one type.
- **The SDKs check configurations for the plugin.** Each reads `vixen-plugin.json` from its working folder, fills in
  `configSchema` defaults, and refuses what breaks the schema with `-32004` and `data.errors` before `configure` sees
  it, using the same subset and the same messages as `schema.rs` (a test runs all four checkers on the same cases). So
  every template and example passes the harness's invalid-configuration check with no code of its own.
- **Threads and frames.** Each SDK reads stdin on its own thread (Node: the event loop) so requests to Studio work from
  any thread or handler, and keeps one frame slot per instance: a slow `frame` skips to the newest frame instead of
  queueing, keeping the host's latest-wins promise end to end. Requests are handled one at a time in arrival order
  (Node handlers may be async). Instances are stopped and destroyed, and `on_shutdown` runs, before `shutdown` is
  answered, since Studio closes stdin once it has the answer.
- **Templates** (`plugin_scaffold` and `plugin new` share them): a main file using the SDK (a `controller` type with a
  `prefix` setting that logs its values, and a `hello` tool), the SDK file(s), the manifest and a README. Rust adds
  `Cargo.toml` (its own `[workspace]`), `build.sh` and `.gitignore`; its `entry.command` is `bin/<crate>`, which
  `build.sh` fills (`target/` is ignored by hot reload, `bin/` is not, so each rebuild reloads a linked plugin).
  Swift was left out (optional in the plan).
- **Examples** (`studio/plugins/examples/`, served as `plugins://examples/<language>`, SDKs as `plugins://sdk/<language>`):
  `console-output` (Python, the tutorial: settings with an extra check, status, a tool with a menu command, an event,
  storage across restarts), `udp-pixel-output` (TypeScript: raw RGB per frame in one UDP datagram, colour order,
  frame-rate cap, aggregated stats, a `discover` tool broadcasting `VIXEN-DISCOVER`) and `rust-http-output` (Rust: JSON
  POSTs to a URL from a worker thread, at most one per `minIntervalMs` and only on change; plain `http://` with std).
  Each passes the harness with live frames in `cargo test`, aimed at a local UDP or HTTP receiver the test opens.
- **CLI** (`vixen-studio-mcp plugin …`, no profile or session needed): `new` (templates into `--dir`, never
  overwriting), `validate DIR [--live] [--json]` (the harness; exit 1 on a failed check), `pack DIR [--out] [--force]`
  (`<id>-<version>.vixenplugin`, deterministic: name order, fixed timestamps, executable bit only; leaves out `.git`,
  caches, `target/`, symlinks refused, a missing in-folder `entry.command` refused; writes `<package>.sha256`;
  `install::stage` accepts the result) and `run DIR [--frames chase|ramp|blink|rgb|off] [--seconds] [--fps] [--type]
  [--outputs] [--config]` (one live controller, printing the log, statuses, messages and stats as they arrive).

What P6 settled:

- **Pages.** `docs/plugins/`: README (overview, diagram, SDKs, examples, CLI, security, versions), GETTING_STARTED (the
  console-output tutorial from `plugin new`; walked through with the CLI), PROTOCOL, MANIFEST, CONTROLLERS,
  TOOLS_AND_EVENTS, PERMISSIONS, PACKAGING, TESTING, TROUBLESHOOTING and AGENTS. `docs/plugins/` is Studio-only (listed
  in STUDIO.md), so it needs no markers. Linked from README.md, STUDIO.md and studio/MCP.md.
- **The pages say what ships, not the plan:** only `liveOutput.changed` is sent; `display.snapshot` answered `-32601` until P2b;
  `outputNaming`/`outputNames` are recorded, not applied; network and serial permissions are declarations, not
  enforced (no sandbox yet; B39 added it). `controller.stats` values are running totals (the host raises its counters to them); the
  SDKs' docstrings and two examples treat them as increments, which is backlog B42.
- **Resources.** `plugins://api` is PROTOCOL.md; `plugins://docs/<page>` serves every page by name (`mcp::DOCS`,
  bundled with `include_str!`). The design file is no longer served. `build_plugin` points at `plugins://docs/AGENTS`
  and `plugins://docs/CONTROLLERS` first.
- **Drift tests.** `studio-plugins/tests/docs.rs`: PROTOCOL.md's methods per direction, each method's first line
  (direction, kind, timeout), its Param/Result tables (names, types rendered from the schema, required) and every JSON
  example (validated against its method's schema), the error table, framing limit and API version, against
  `protocol.schema.json`; MANIFEST.md's field tables (names, required, every enum value) against
  `manifest.schema.json`, and its complete example (plus CONTROLLERS.md's recipe schema) through `Manifest::parse`;
  across all pages, relative links and anchors, `plugins://` URIs (read through `read_resource`), CLI flags (against
  `cli::USAGE`), and `mcp::DOCS` equal to the files. `studio-mcp/tests/plugins.rs`: MCP-TOOLS.md equals the bridge's
  `tools/list` (regenerate with `STUDIO_REGENERATE_MCP_TOOLS=1 cargo test -p studio-mcp --test plugins mcp_tools`),
  and every tool name the pages put in backticks exists.
- **End-to-end agent check** (`studio-mcp/tests/plugin_agent.rs`): on a copy of target/show-copy's system data served
  by a live, unmuted `AppCore` (as the app serves it, with the shared host's provider registered and the host muted
  until Live Output), the agent uses only MCP plus file writes in its scaffolded folder: reads `plugins://docs/AGENTS`,
  `plugins://api` and the TypeScript example; scaffolds a TypeScript controller plugin; rewrites it into a UDP sender
  (host/port settings); validates it (all pass, no warnings); is refused `plugin_link_dev` until the manage switch,
  then links it; deletes the copy's own controllers (a Generic Serial board and a dummy) so nothing real can start;
  creates and configures a controller of the new type (a host name refused as `/host: must be an IPv4 address`);
  patches a new megatree to it; is refused `plugin_send_test_frame` and `output_start` without the live-output
  permission, and a 127.0.0.1 UDP listener receives nothing; then, with Live Output open, the host unmuted and the
  permission on, starts it (the only running controller) and sends All On at 40 %, and the listener receives the
  6-byte datagram of 102s; `plugin_info` shows the instance live with frames sent; revoking the permission stops it.
  The test fails when the plugin sends nothing.

What B39 settled (the sandbox):

- **Every launch** (host, `plugin validate`, `plugin run`) goes through `/usr/bin/sandbox-exec -p <profile>`, built by
  `studio_plugins::sandbox::profile` from the manifest. Off only when `sandbox-exec` is missing or Studio's own
  environment has `VIXEN_PLUGIN_SANDBOX=off` (the plugin's environment is cleared, so it can't set it).
- **Profile shape:** `(allow default)`, then refusals with narrow allows of the same operation (in SBPL an allow for
  `file-read*` loses to a `file-read-data` deny, so allows name the operation the deny does). Reading file contents in
  the home folder is refused except the plugin folder, data folder, temp folder and the program's install prefix
  (`…/bin/x` gives `…`); metadata stays readable so Python's `site` and Node's module walk behave. Writes go only to
  the temp folders, `/dev/null`-style devices and, with `files: ["plugin-data"]`, the data folder; the plugin folder is
  read-only. Network is refused without rules; each rule allows its protocol on a port (`lan` and portless `udp`/`tcp`
  any port, `http`/`ws` 80, `https`/`wss` 443, or the rule's port), UDP rules add bind/inbound, and any rule adds the
  `mDNSResponder` socket for lookups. Serial adds writes to `/dev/cu.*` and `/dev/tty.*`. SBPL can't filter by host
  name or subnet, so ports are the walls; the docs say so. Paths are canonicalized (`/tmp` is `/private/tmp`).
- **Visible:** `plugin_list`/`studio_plugins_list` show `sandboxed` for a running plugin, `plugin_info` adds
  `sandboxProfile`, and `vixen-studio-mcp plugin sandbox DIR` prints the profile. Settings ▸ Plugins shows a running
  plugin's Sandbox row (Sandboxed / Not sandboxed) with Copy Profile (backlog B43).
- **Tests:** unit tests on the generated text; `tests/sandbox.rs` runs a python3 probe with no permissions (own folder
  readable; a home-folder file, own-folder and data-folder writes and UDP refused) and with `udp:lan` + `plugin-data`
  (data write and UDP allowed, the packet arrives); it fails with the sandbox off. The three examples (python3, node
  UDP, Rust HTTP) pass the harness with live frames under it.

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
backlog: launch under a generated `sandbox-exec` profile derived from the manifest's permissions (done in B39).

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
- Output counts from the settings (API 1.1, backlog B61): a type with `outputCount: "fromConfig"` owns its count.
  `controller_configure` asks the plugin `controller.outputsFor {typeId, config}` (starting it if needed, no instance)
  and resizes the controller and names its outputs in the same edit, so one Undo step covers settings, outputs and
  patches; dropping patched outputs needs `removePatchedOutputs` (the inspector's Remove Outputs). The count shows
  read-only. Settings actions (`actions`: Detect Lights) run a plugin tool on the current settings; its `config` or
  `mergePatch` applies as an edit (`controller_action` for agents). A plugin that must also load on API 1.0 puts both
  at the configSchema root as `x-outputCount` / `x-actions` and checks `capabilities.controllerOutputs`.
- Lights from a plugin (backlog B62): the `controller.outputsFor` answer may list `lights` (`{name, outputCount}` in
  output order, 1/3/4 outputs each; ignored unless all valid and within the count). `controller_lights` reads them (or,
  without a list, groups outputs named `<light> R/G/B(/W)`), and `element_single_bulbs` `fromController` / the Single
  Bulbs sheet's Use the Controller's Lights make one Single Item per unpatched light, colour handling from its output
  count, each patched to its own outputs, one Undo step. SDK helpers: `lights_outputs`, `lightsOutputs`,
  `Outputs::for_lights`.

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
- `crates/studio-plugin-sdk`, package `vixen-plugin-sdk` (Rust, small, serde_json only; the app never links it).
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
