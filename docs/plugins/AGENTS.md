# Building a plugin with an AI agent

Studio's MCP server gives an agent everything it needs to build, check and ship a plugin with nothing but the MCP
connection and its own file tools: documentation and schemas as resources, a step-by-step prompt, and tools to
scaffold, validate, link, inspect and test. Connect an agent as described in Settings ▸ Agents (the helper is
`Vixen Studio.app/Contents/Helpers/vixen-studio-mcp --profile <show folder>`).

Two switches stay with the user, both off each time Studio starts and with no MCP setter:

- **Settings ▸ Plugins ▸ Allow agents to manage plugins**: needed to link, install, enable, disable, reload or uninstall.
- **Settings ▸ Agents ▸ Allow agents to control live output**: needed for anything that reaches real lights.

## Resources

| URI | What it is |
|---|---|
| `plugins://api` | This documentation's protocol reference ([PROTOCOL.md](PROTOCOL.md)): every method with its fields and examples. |
| `plugins://docs/<page>` | Any page of this documentation by name: `README`, `GETTING_STARTED`, `PROTOCOL`, `MANIFEST`, `CONTROLLERS`, `TOOLS_AND_EVENTS`, `PERMISSIONS`, `PACKAGING`, `TESTING`, `TROUBLESHOOTING`, `AGENTS`. |
| `plugins://schema/manifest` | The manifest's JSON Schema. |
| `plugins://schema/protocol` | The protocol's JSON Schema. |
| `plugins://examples/<language>` | A complete example plugin's files: `python` (console-output), `typescript` (udp-pixel-output), `rust` (rust-http-output). |
| `plugins://sdk/<language>` | The SDK file a plugin copies next to its main file. |
| `plugins://<id>/manifest` | An installed or linked plugin's manifest. |

The prompt **`build_plugin`** (optional argument `goal`) is the whole workflow below in a form an agent can follow.

## Tools

| Tool | Needs | Does |
|---|---|---|
| `plugin_scaffold` | — | Writes a working plugin from the templates (`language`: python, typescript, rust; `kind`: controller or tools) into a new folder under `Plugins/dev/`, never anywhere else and never over anything. |
| `plugin_validate` | — (live part: live-output switch, Live Output on, `confirm`) | The conformance harness ([TESTING.md](TESTING.md#the-conformance-harness)) on a folder under `Plugins/dev/` or an installed/linked plugin. |
| `plugin_list`, `plugin_info`, `plugin_logs` | — | State, manifest, instances, frames, counters, errors and the log. |
| `plugin_link_dev`, `plugin_unlink_dev` | manage switch, `confirm` | Link a development folder (it starts, and restarts on every saved change) or remove the link. |
| `plugin_install`, `plugin_uninstall` | manage switch, `confirm` | From a folder or `.vixenplugin`. |
| `plugin_enable`, `plugin_disable`, `plugin_reload` | manage switch, `confirm` | |
| `plugin_send_test_frame` | live-output switch, Live Output on, `confirm` | A commissioning test pattern on one plugin controller (`movingPixel`, `colorBars`, `allOn`). |
| `plugin_required` | — | The plugins this show needs and whether each is satisfied. |
| `plugin__<short-id>__<tool>` | as the plugin declares | A plugin's own tools ([TOOLS_AND_EVENTS.md](TOOLS_AND_EVENTS.md)). |

Controllers made by plugins use Studio's usual tools: `controller_types`, `controller_create`, `controller_configure`
(with `config`), `controller_network`, `patch_elements`, `controller_overview`, `output_start`, `output_test_pattern`.

## The workflow

1. **Read** `plugins://api`, `plugins://docs/CONTROLLERS` (for an output plugin) and the example in the language you'll
   use (`plugins://examples/typescript`, …).
2. **Scaffold:** `plugin_scaffold {id: "io.github.someone.lamp-output", name: "Lamp Output", language: "typescript",
   kind: "controller"}`. It returns the folder and the files written. Keep the SDK file as it is.
3. **Implement** in that folder with your own file tools: the settings (`configSchema` in `vixen-plugin.json`, which
   becomes the inspector's form), then `configure`, `start`, `frame` and `stop` in the main file. Keep `typeId` once
   chosen: shows store it. Declare the permissions you use.
4. **Validate:** `plugin_validate {path}` until every check passes without warnings.
5. **Link:** ask the user to turn on Allow agents to manage plugins, then `plugin_link_dev {path, confirm: true}`.
   From now on Studio restarts the plugin whenever you save. Debug with `plugin_logs` and `plugin_info`.
6. **Try it in the show:** `controller_types` lists your type; `controller_create {name, typeId, outputs}`, then
   `controller_configure {id, config}` (field errors come back in words), then patch a test element to it with
   `patch_elements`. These are ordinary, undoable display edits.
7. **Test output:** with the user's permission for live output and Live Output on, `output_start {controllerId,
   confirm: true}` and `plugin_send_test_frame {controllerId, pattern: "allOn", durationMs: 3000, confirm: true}`.
   Point the controller at a test receiver (`127.0.0.1`) unless the user says real lights are fine. Without the
   permission, the call is refused and nothing is sent.
8. **Ship:** bump `version`, then `vixen-studio-mcp plugin pack <folder>` (or ask the user to) for a `.vixenplugin` and
   its SHA-256 ([PACKAGING.md](PACKAGING.md)).

Everything device-specific belongs in the plugin: don't ask for vendor support to be built into Studio.

## Proven end to end

`crates/studio-mcp/tests/plugin_agent.rs` runs this workflow through MCP alone against a live Studio core on a copy
of a real show: it scaffolds a TypeScript plugin, turns it into a UDP output, validates it, links it once the manage
switch is on, creates and configures a controller, patches a new element to it, is refused a test pattern without the
live-output permission (nothing reaches the receiver), and with it sees the test pattern's bytes arrive at a UDP
listener on `127.0.0.1`.
