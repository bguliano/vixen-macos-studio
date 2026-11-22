# Permissions and security

## The honest version

A plugin is a program running as you, the same user as Studio, like the MCP helper or any app you install. **It is
not sandboxed.** It can do anything your user account can. Install plugins from people you trust, the way you would any
other software.

What Studio does on top of that:

- **It tells you what a plugin says it does** before anything is installed: the install sheet reads the package
  without installing it and lists "This plugin can: …" from the manifest's `permissions`.
- **It enforces the permissions wherever it is the one handing something out**: the plugin's storage, the events it
  receives, the show data it can read, and when live output may flow.
- **It keeps the plugin away from Studio's own state**: a clean environment, its own process group, its own data folder.

A future version may also launch plugins under a macOS sandbox profile derived from their permissions; until then, the
network and serial permissions are declarations the user reads, not walls.

## Declaring permissions

In `vixen-plugin.json` ([MANIFEST.md](MANIFEST.md#permissions)):

| Permission | The install sheet says | Studio enforces |
|---|---|---|
| `"network": ["udp:lan"]` | Send and receive UDP on your local network | Nothing (shown to the user). |
| `"network": ["tcp:hub.local:8080"]` | Open TCP connections to hub.local:8080 | Nothing (shown to the user). |
| `"network": ["http:…"]` / `"https:…"` | Make HTTP requests / Make HTTPS requests to … | Nothing (shown to the user). |
| `"network": ["ws:…"]` / `"wss:…"` | Open WebSocket connections to … | Nothing (shown to the user). |
| `"serial": true` | Use serial ports (USB adapters and boards) | Nothing (shown to the user). |
| `"files": ["plugin-data"]` | Keep its own settings in its data folder | `storage.get` / `storage.set` answer `-32003` without it. |
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
