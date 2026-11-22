# Getting started: your first plugin in 15 minutes

You'll build **Console Output**, a controller type that prints the channel values Studio sends it, in Python. It's
the same plugin as [`studio/plugins/examples/console-output`](../../studio/plugins/examples/console-output), and it
shows every part you need for a real output plugin: settings, starting and stopping, frames, status, a tool with a
menu command, an event, and storage.

You need Vixen Studio and `python3` (3.9 or later; macOS's own is fine). In the commands below, `vixen-studio-mcp` is
the helper inside the app; make a shortcut first:

```sh
alias vixen-studio-mcp="/Applications/Vixen Studio.app/Contents/Helpers/vixen-studio-mcp"
```

## 1. Make the plugin

```sh
vixen-studio-mcp plugin new --id com.example.console-output --name "Console Output" --language python --dir ~/Plugins
cd ~/Plugins/console-output
```

You get four files:

- `vixen-plugin.json`, the manifest: who the plugin is and what it adds ([MANIFEST.md](MANIFEST.md)).
- `main.py`, your plugin.
- `vixen_plugin.py`, the SDK. It speaks the protocol for you; leave it as it is.
- `README.md`.

It already works. Check it:

```sh
vixen-studio-mcp plugin validate .
```

Every check should say `PASS`, except `controller.start` and `frames`, which say `skip`: they need `--live`.

## 2. Describe the settings

Open `vixen-plugin.json`. Under `contributes.controllers` is one controller type with a `typeId` (a UUID made for you:
keep it forever, shows store it) and a `configSchema`. Replace the `configSchema` with the settings Console Output
needs:

```json
"configSchema": {
  "type": "object",
  "x-order": ["label", "show", "perSecond", "maxChannels"],
  "properties": {
    "label": { "type": "string", "title": "Label", "description": "Printed in front of each line.", "default": "console", "maxLength": 40 },
    "show": { "type": "string", "title": "Show values as", "enum": ["levels", "colours"], "x-enumNames": ["Levels (0-255)", "Colours (#RRGGBB per 3 outputs)"], "default": "levels" },
    "perSecond": { "type": "number", "title": "Lines per second", "default": 2, "minimum": 0.1, "maximum": 40 },
    "maxChannels": { "type": "integer", "title": "Channels per line", "default": 32, "minimum": 1, "maximum": 512 }
  },
  "additionalProperties": false
}
```

This schema becomes the controller's inspector form in Studio: a text field, a choice, two number fields, in that
order. Studio and the SDK check every configuration against it and fill in the defaults, so your code only sees valid
settings ([CONTROLLERS.md](CONTROLLERS.md#settings-configschema)).

## 3. Write the controller

Replace `main.py` with:

```python
#!/usr/bin/env python3
import sys
import time

import vixen_plugin as vp

plugin = vp.Plugin()
received = {}  # instance id -> frames received since the plugin started


@plugin.controller()
class Console(vp.Controller):
    def configure(self, config):
        # Called on create and after every edit, with checked settings. Check here what the schema can't say.
        if config["show"] == "colours" and self.output_count % 3:
            raise vp.ConfigError(f"colours need a multiple of 3 outputs; this controller has {self.output_count}", field="/show")
        self.label = config["label"]
        self.show = config["show"]
        self.interval = 1.0 / config["perSecond"]
        self.max_channels = config["maxChannels"]
        self.last_print = 0.0

    def start(self):
        self.status("running", "printing to the plugin log")

    def stop(self):
        plugin.info(f"{self.label}: stopped after {received.get(self.instance_id, 0)} frames")

    def frame(self, frame):
        received[self.instance_id] = received.get(self.instance_id, 0) + 1
        now = time.monotonic()
        if now - self.last_print < self.interval:
            return
        self.last_print = now
        values = frame.values[:self.max_channels]
        if self.show == "colours":
            text = " ".join("#%02X%02X%02X" % rgb for rgb in vp.triplets(values))
        else:
            text = " ".join(str(v) for v in values)
        print(f"{self.label} #{frame.seq}: {text}", file=sys.stderr, flush=True)


if __name__ == "__main__":
    plugin.run()
```

Three rules this follows, which every plugin must:

- **Only protocol messages go to stdout.** Print everything else to stderr; Studio keeps it in the plugin's log.
- **`frame` must be quick.** It's called up to 40 times a second. This one only prints a couple of times a second.
- **Refuse bad settings with `ConfigError`,** naming the field. Studio shows the message under that field.

Validate again, then watch it work with a test pattern:

```sh
vixen-studio-mcp plugin validate . --live
vixen-studio-mcp plugin run . --frames chase --seconds 3
```

`plugin run` prints the plugin's log as it arrives: you'll see lines like `console #12: 0 0 255 0 0 0 …` move along.

## 4. See it in a show

1. In Studio, open **Settings ▸ Plugins**, click **Link Development Folder…** and choose `~/Plugins/console-output`.
   It appears as Development, running. From now on Studio restarts it whenever you save a file in the folder.
2. In the show window's **Controllers** section, **Add ▸ Console Output**, name it, give it 9 outputs.
3. Select it: the inspector shows your form. Choose "Colours", type a label. Each edit is one step on Undo.
4. In the **Display** section, patch an element to it.
5. Turn on **Live Output**, then right-click the element or the controller in Display and choose **Test ▸ Color Bars
   (R, G, B, W)**, or play a sequence.
6. **Settings ▸ Plugins ▸ Console Output ▸ Log…** shows the colours going by.

Try breaking it: set "Colours" on a controller with 10 outputs. The inspector refuses it with your message under the
field, and the controller keeps its last good settings.

## 5. Add a tool, a menu command, an event and storage

Add to `main.py`, before `if __name__ == "__main__":`:

```python
@plugin.event("liveOutput.changed")
def live_output_changed(data):
    plugin.info("live output is " + ("off" if plugin.muted else "on"))


@plugin.tool("frames_seen")
def frames_seen(arguments):
    total = plugin.storage_get("framesBeforeRestart", 0) + sum(received.values())
    lines = [f"{instance}: {count} frames" for instance, count in received.items()] or ["No frames since the plugin started."]
    return "\n".join(lines + [f"Total including earlier runs: {total}"])


@plugin.on_shutdown
def remember():
    try:
        plugin.storage_set("framesBeforeRestart", plugin.storage_get("framesBeforeRestart", 0) + sum(received.values()))
    except (vp.RpcError, TimeoutError):
        pass
```

And declare them in `vixen-plugin.json`'s `contributes`, next to `controllers`:

```json
"tools": [
  { "name": "frames_seen", "description": "How many frames each console controller has received since the plugin started (also kept across restarts).", "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }, "readOnly": true }
],
"commands": [{ "id": "frames_seen", "title": "Frames Seen", "tool": "frames_seen" }],
"events": ["liveOutput.changed"]
```

Storage needs `"permissions": { "files": ["plugin-data"] }`, which `plugin new` already put in the manifest.

Save. Studio restarts the plugin, and now:

- **Plugins ▸ Console Output ▸ Frames Seen** runs the tool and shows its answer.
- An AI agent connected to Studio sees the tool as `plugin__console_output__frames_seen`.
- The log says when Live Output turns on and off.
- The frame count survives restarts, in the plugin's data folder.

## 6. Share it

Set `"version": "1.0.0"` in the manifest, then:

```sh
vixen-studio-mcp plugin pack .
```

This writes `com.example.console-output-1.0.0.vixenplugin` and its `.sha256` next to the folder. Anyone can install it
with Settings ▸ Plugins ▸ Install from File…. See [PACKAGING.md](PACKAGING.md).

## Next

- A real device: [CONTROLLERS.md](CONTROLLERS.md), and the `udp-pixel-output` (TypeScript) and `rust-http-output`
  (Rust) examples.
- Every message in detail: [PROTOCOL.md](PROTOCOL.md).
- When something goes wrong: [TESTING.md](TESTING.md) and [TROUBLESHOOTING.md](TROUBLESHOOTING.md).
