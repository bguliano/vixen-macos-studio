# Troubleshooting

Start with the plugin's state in Settings ▸ Plugins (agents: `plugin_list`), then its log (Log…, `plugin_logs`) and
`plugin_info` for the last error and each controller instance. `plugin validate` reproduces most problems outside
Studio.

## The plugin won't start

| State or message | Cause and fix |
|---|---|
| `disabled` | Found on disk without a registry entry, or switched off. Turn it on in Settings ▸ Plugins, or link/install it. Nothing found on disk runs uninvited. |
| `invalid` / manifest errors | `vixen-plugin.json` breaks a rule; each error names its path. Unknown fields are refused: check spelling. See [MANIFEST.md](MANIFEST.md). |
| `incompatible` | `apiVersion` doesn't contain 1.1.0, or `initialize` answered another major version. Use `^1.0` and answer `"1.1.0"` (any 1.x works). |
| "plugins are run by another Vixen Studio process" | The app and a `vixen-studio-mcp` share one Plugins folder and only the first to open it runs plugins; the other is read-only (`readOnly` in `plugin_list`). Make the change in that process, or quit it: the other takes over within a second. |
| `crashed` | Three restarts within 60 s. Read the log for why, fix it, then Reload (agents: `plugin_reload`), which forgets earlier crashes. |
| "command not found" / "exits before answering" | `entry.command` isn't on the plugin's `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin`; your shell's `PATH` doesn't apply), a path with `/` points outside the folder, or a Rust plugin wasn't built (`./build.sh`). Use an absolute path for interpreters elsewhere (`/Users/me/.pyenv/shims/python3`). |
| "initialize timed out" | The program never answered: it is waiting for something, or printing its answer without a newline, or buffering stdout. Flush after each line (the SDKs do; Python plugins get `PYTHONUNBUFFERED=1`). |

## The plugin keeps being stopped

- **"wrote something other than JSON-RPC to stdout"**: a `print`, `console.log` or library banner went to stdout. Send
  everything that isn't a protocol message to stderr (`print(..., file=sys.stderr)`, `console.error`, `eprintln!`).
- **A line over 1 MiB**: split large tool answers, and use the `u8` encoding rather than `json` for big controllers.
- **It exits when Studio closes stdin**: that is right after `shutdown`, but if it happens at other times, something in
  the plugin closed or read stdin by mistake.

## A controller doesn't light

1. **Is it started?** The Controllers section and `controller_overview` show its run state and health. Frames only flow
   with Live Output on; Studio is muted when run headless, with `VIXEN_MUTE`, or under UI tests.
2. **Health says "Needs plugin …"**: the plugin is missing, disabled, too old, or crashed; the message says which.
   Install or enable it; the controller comes back without an edit.
3. **Is it patched?** Patch an element to the controller, or use the Test submenu (`output_test_pattern`) on the
   controller itself, which doesn't need patches.
4. **Does the plugin get frames?** `plugin_info` shows each instance's `framesSent`, `framesDropped` and
   `lastFrameAgeMs`. Frames sent but nothing at the device: the problem is between your plugin and the device (address,
   port, colour order). Log what you send for a moment.
5. **Lots of `framesDropped`**: your `frame` handler is slow. Hand device I/O to a worker and keep only the newest
   values ([CONTROLLERS.md](CONTROLLERS.md#talking-to-slow-devices)).

## Settings are refused

- **Under a field in the inspector**: Studio's schema check, or your plugin's `-32004` answer. Read the message; change
  the setting or the schema.
- **Every edit refused**: your `configure` raises for settings the schema allows. Run `plugin validate`: its
  `controller.configure` check uses the schema's defaults.
- **The harness warns about "configure (invalid)"**: your plugin accepted settings that break its own schema. With an
  SDK this can't happen; without one, check the schema in `controller.create`/`configure`.

## Agents can't see or call the plugin's tools

- Tools are listed for enabled plugins with a valid manifest that haven't crashed. Check `plugin_list`.
- The client may have cached the list: Studio sends `notifications/tools/list_changed`, but some clients only re-list
  when restarted or asked.
- A call has 30 s, counting the plugin's start. Long jobs should answer quickly with "started" and report later.

## Agents get "Allow agents to …"

- **"Allow agents to manage plugins"**: installing, linking, enabling and reloading need that switch in Settings ▸
  Plugins (off each launch) and `confirm: true`.
- **"Allow agents to control live output"**: starting plugin controllers, test patterns and `plugin_validate`'s live part
  need that switch in Settings ▸ Agents (off each launch), Live Output on and `confirm: true`.

## Development folder doesn't reload

The watcher ignores hidden files, `node_modules`, `target`, `build`, `dist` and `__pycache__`, and waits for a change to
hold for two polls (about 1.5 s). A Rust plugin reloads when `build.sh` copies the program into `bin/`, not when
`cargo build` writes into `target/`. A plugin installed from a package doesn't reload: link the folder instead (unlink
it before installing the same id from a package).
