#!/usr/bin/env python3
"""Console Output (example): the tutorial plugin from docs/plugins/GETTING_STARTED.md.

A controller type that prints the channel values it receives to its log (Settings ▸ Plugins ▸ Log…, or plugin_logs),
as 0-255 levels or as one #RRGGBB colour per three outputs. It shows each part of the SDK: settings checked against
configSchema with an extra check of its own, start/stop, frames, status, an agent tool with a menu command, an event,
and storage that survives restarts.
"""
import sys
import time

import vixen_plugin as vp

plugin = vp.Plugin()
received = {}  # instance id -> frames received since the plugin started


@plugin.controller()
class Console(vp.Controller):
    def configure(self, config):
        # Studio and the SDK have already checked the schema (types, ranges, the colours/levels choice) and filled in
        # defaults. Anything the schema can't express is checked here.
        if config["show"] == "colours" and self.output_count % 3:
            raise vp.ConfigError(f"colours need a multiple of 3 outputs; this controller has {self.output_count}", field="/show")
        self.label = config["label"]
        self.show = config["show"]
        self.interval = 1.0 / config["perSecond"]
        self.max_channels = config["maxChannels"]
        self.last_print = 0.0

    def start(self):
        self.status("running", "printing to the plugin log")
        plugin.info(f"{self.label}: started with {self.output_count} outputs")

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


if __name__ == "__main__":
    plugin.run()
