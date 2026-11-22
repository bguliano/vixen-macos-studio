# Testing and debugging

Three tools check a plugin, from the quickest to the most real:

1. **`plugin validate`** (or the MCP tool `plugin_validate`): the conformance harness. Runs the plugin on its own,
   without Studio's engine or any show.
2. **`plugin run`**: one live controller of your type with a test pattern, printing everything the plugin reports.
3. **A development link in Studio**: the plugin in a real show, restarted each time you save a file.

`vixen-studio-mcp` is the helper inside the app: `/Applications/Vixen Studio.app/Contents/Helpers/vixen-studio-mcp`.
From a source checkout, `cargo run -p studio-mcp --bin vixen-studio-mcp -- plugin …` runs the same thing.

## The conformance harness

```sh
vixen-studio-mcp plugin validate path/to/my-plugin            # protocol, settings, lifecycle; no output
vixen-studio-mcp plugin validate path/to/my-plugin --live     # also start and 40 fps frame bursts
vixen-studio-mcp plugin validate path/to/my-plugin --json     # the report as JSON
```

It exits 1 when a check fails. The checks, in order:

| Check | Passes when |
|---|---|
| `manifest` | `vixen-plugin.json` is valid and its `apiVersion` range contains 1.0.0. |
| `configSchema <type>` | Each controller type's `configSchema` describes an object and its defaults satisfy it. |
| `entry.command` | The command resolves (in the folder, or on the plugin's `PATH`). |
| `launch` | The process starts in a throwaway data folder. |
| `initialize` | It answers in time with a matching major version (muted unless `--live`). |
| `tools` | Lists your tools. They are **not called**: a tool can act on the world. |
| `controller.create` | Per type: accepts a valid configuration made from the schema (defaults, else the smallest value each keyword allows); its normalised config still satisfies the schema. |
| `controller.configure` | Accepts the valid configuration again. |
| `controller.configure (invalid)` | Refuses an invalid one (an undeclared field when `additionalProperties` is false, else a typed field given the wrong type) with `-32004` and `data.errors`. Anything else is a warning, since Studio checks the schema first. |
| `controller.setOutputCount` | Accepts a larger count and the original back. |
| `controller.start`, `frames <encoding>`, `controller.stop` | Only with `--live`: starts, takes a one-second chase at 40 fps per accepted encoding (a warning above 10 % dropped; a failure if it stops answering), stops. |
| `controller.destroy` | Answers. |
| `protocol` | Only JSON-RPC on stdout, and still running. |
| `shutdown`, `exit` | Answers `shutdown` and exits within 3 s of stdin closing. |

**`--live` is real output.** A controller plugin sends what it gets to whatever its default settings point at. Point the
defaults at a test receiver (`127.0.0.1`, a spare lamp), never your show.

Through MCP, `plugin_validate {path}` runs folders under `Plugins/dev/` or installed and linked plugins (`{id}`), and
the live part only with `liveOutput: true`, `confirm: true`, Live Output on and the user's live-output permission.

## A live readout: `plugin run`

```sh
vixen-studio-mcp plugin run . --frames chase --seconds 5
vixen-studio-mcp plugin run . --frames rgb --outputs 30 --config '{"host":"127.0.0.1","port":7777}'
```

Starts one controller (`--type` picks a type by name or typeId; default the first) with your settings (default: the
schema's defaults), sends the pattern (`chase`, `ramp`, `blink`, `rgb`, `off`) at `--fps` for `--seconds`, and prints
the plugin's log, statuses, messages and stats as they arrive. This is live output too.

## `display.snapshot` without Studio

`plugin validate`, `plugin_validate` and `plugin run` have no open show, so a plugin with `"display": "read"` gets a
small sample one from `display.snapshot`: a Porch group of two RGB pixels and a single-channel Tree, patched to a
stopped E1.31 controller on universe 1 (revision 0, one page). To test against a real show, give `validate` or `run`
`--profile` with a Vixen profile folder (the one holding `SystemData/`):

```sh
vixen-studio-mcp plugin run . --seconds 5 --profile ~/Documents/Vixen\ 3
```

The show is read from the saved files once, at the start, without opening a session or writing anything, so Studio
can have the profile open. No controller is running in it.

A quick receiver for a UDP plugin, in another terminal:

```sh
python3 -c "import socket; s=socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.bind(('127.0.0.1', 7777)); [print(s.recv(65535)[:24].hex(' ')) for _ in range(20)]"
```

## In Studio

Link the folder in Settings ▸ Plugins ▸ Link Development Folder… (agents: `plugin_link_dev {path, confirm: true}`).
Studio starts it and restarts it about 1.5 s after a file in the folder changes and has stayed unchanged for two polls
(hidden files, `node_modules`, `target`, `build`, `dist` and `__pycache__` are ignored, so a Rust plugin reloads when
`build.sh` copies the new program into `bin/`). Then add a controller of your type in the Controllers section, patch
an element to it and use the Test submenu, or play a sequence with Live Output on.

## Logs

Everything the plugin writes to stderr (as `info`), every `log` message, and Studio's own notes about the plugin
(launches, crashes, refused messages) go to:

- Settings ▸ Plugins ▸ *the plugin* ▸ Log…, a live view filtered by level;
- `plugin_logs {id, limit, level}` for agents;
- `~/Library/Application Support/Vixen Studio/Plugins/logs/<id>.log`, rotated at 1 MiB with one `.log.1` kept. The
  last 2,000 entries also stay in memory.

`plugin_info {id}` adds the state, pid, uptime, restart count, last error, and per controller instance: status,
whether it's live, frames sent and dropped, the age of the last frame, counters and configuration errors.

## Testing your own code

The SDKs are single files with no dependencies, so a plugin's logic is easy to unit-test by calling your `Controller`
methods directly with a `Frame`, or by piping JSON-RPC lines into the program:

```sh
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"apiVersion":"1.0.0","hostVersion":"test","pluginDataDir":"/tmp/x","muted":true,"capabilities":{}}}' | python3 main.py
```

The examples' tests in Studio's repository (`crates/studio-plugins/tests/examples.rs`) run each example through the
harness with live frames aimed at a local receiver; copy that pattern for your CI.
