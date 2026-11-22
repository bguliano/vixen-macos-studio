# Permissions and security

## The honest version

A plugin is a program running as you, the same user as Studio, like the MCP helper or any app you install. Studio
starts it under a macOS sandbox profile made from its permissions (below), which keeps an honest plugin inside what it
declared, but **it is not a security boundary**: a plugin can still reach whatever your account can through system
services. Install plugins from people you trust, the way you would any other software.

What Studio does on top of that:

- **It tells you what a plugin says it does** before anything is installed: the install sheet reads the package
  without installing it and lists "This plugin can: …" from the manifest's `permissions`.
- **It enforces the permissions wherever it is the one handing something out**: the plugin's storage, the events it
  receives, the show data it can read, and when live output may flow.
- **It keeps the plugin away from Studio's own state**: a clean environment, its own process group, its own data folder.
- **It starts the plugin under a sandbox profile** derived from its permissions, so files, network and serial ports it
  didn't declare are refused by macOS (see [The sandbox](#the-sandbox)).

## Declaring permissions

In `vixen-plugin.json` ([MANIFEST.md](MANIFEST.md#permissions)):

| Permission | The install sheet says | Studio enforces |
|---|---|---|
| `"network": ["udp:lan"]` | Send and receive UDP on your local network | The sandbox allows UDP (any port). |
| `"network": ["tcp:hub.local:8080"]` | Open TCP connections to hub.local:8080 | The sandbox allows TCP connections to port 8080. |
| `"network": ["http:…"]` / `"https:…"` | Make HTTP requests / Make HTTPS requests to … | The sandbox allows TCP to port 80 / 443, or the rule's port. |
| `"network": ["ws:…"]` / `"wss:…"` | Open WebSocket connections to … | The sandbox allows TCP to port 80 / 443, or the rule's port. |
| `"serial": true` | Use serial ports (USB adapters and boards) | The sandbox lets it write `/dev/cu.*` and `/dev/tty.*`. |
| `"files": ["plugin-data"]` | Keep its own settings in its data folder | `storage.get` / `storage.set` answer `-32003` without it, and the sandbox lets it write the data folder. |
| `"display": "read"` | Read the show's elements, controllers and patches | `display.snapshot` (not available yet) will require it. |

Declare what you use, no more: users see the list, and an honest short list is what makes them trust a plugin. Name
specific hosts (`https:openapi.api.govee.com`) rather than claiming the whole network when you only talk to one API.

## What the process gets

- **Environment:** only `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin`), `HOME`, `USER`,
  `LOGNAME`, `TMPDIR`, `LANG`, `PYTHONUNBUFFERED=1`, `VIXEN_PLUGIN_ID` and `VIXEN_PLUGIN_DATA_DIR`. Nothing else of
  Studio's environment leaks in.
- **Working directory:** the plugin folder.
- **Data folder:** `~/Library/Application Support/Vixen Studio/Plugins/data/<id>/` (also `pluginDataDir` in
  `initialize`), mode 0700, kept when the plugin is uninstalled.
- **Process group:** its own. Stopping the plugin kills the whole group, so child processes (`npm start` → `node`)
  never outlive it.
- **Events and data:** only the events it declared; event data carries names and ids, never audio paths or show
  contents.

## The sandbox

Studio starts every plugin through `/usr/bin/sandbox-exec` with a profile generated from the manifest. It starts from
"allow everything" so python3, node and compiled plugins run as they would outside, then takes away:

- **Reading files in your home folder**, except the plugin's own folder, its data folder, the temporary folder and the
  install folder of the program in `entry.command` (an interpreter installed under your home folder keeps working).
  File names and sizes stay visible, so module lookups behave; only contents are refused. Files outside your home
  folder (the system, `/opt/homebrew`) stay readable.
- **Writing files** anywhere except the temporary folders (`TMPDIR`, `/tmp`), `/dev/null`-style devices and, with
  `"files": ["plugin-data"]`, the data folder. The plugin's own folder is read-only: Python skips writing `.pyc`
  caches there, which is harmless.
- **Network** without `permissions.network` rules: every socket is refused. With rules, each allows its protocol on
  its port (table above) plus host name lookups. macOS sandbox profiles can't name hosts or subnets, so the walls are
  protocols and ports: `https:api.example.com` allows HTTPS to any host, and `lan` allows any address.
- **Serial ports** without `"serial": true`.

A blocked call fails the way a permission problem does (`EPERM`, "Operation not permitted"), and the plugin log shows
the error. If a plugin works with `vixen-studio-mcp plugin run` outside Studio but not inside, check that its
permissions cover what it does. `vixen-studio-mcp plugin sandbox DIR` prints the exact profile; `validate` and `run`
use the same one, and `plugin_info` reports `sandboxProfile` for a running plugin. To rule the sandbox out while
diagnosing, start Studio (or the CLI) with `VIXEN_PLUGIN_SANDBOX=off` in its environment; a plugin can't change it,
since it never sees Studio's environment.

## Live output

Real lights are gated the same way for plugin controllers as for E1.31 and DDP:

- Frames and `controller.start` only reach a plugin while Live Output is on and Studio isn't muted.
- An agent can start a plugin controller or send it a test pattern only after the user turns on Settings ▸ Agents ▸
  Allow agents to control live output, which is off each time Studio starts. Turning it off stops what agents started.

## Agents managing plugins

Installing, uninstalling, enabling, disabling, linking, unlinking and reloading plugins through MCP need
`confirm: true` **and** Settings ▸ Plugins ▸ Allow agents to manage plugins, which is also off each time Studio starts
and has no MCP setter. Reading (`plugin_list`, `plugin_info`, `plugin_logs`), scaffolding into the development folder
and validating don't need it.

## Installing safely

Studio's installer copies or unpacks into a temporary folder first, refuses symlinks, absolute paths and `..` in a
package, keeps at most 20,000 files and 512 MiB, keeps only the executable bit of each file's mode (files become 0700 or
0600), validates the manifest there, and only then moves it into `Plugins/<id>/<version>/`. It records the package's
SHA-256 in `Plugins/registry.json` and clears the quarantine flag only on what you chose to install. A plugin folder
found on disk without a registry entry is listed as disabled and never started: nothing runs uninvited.
