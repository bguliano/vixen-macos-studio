#!/usr/bin/env python3
"""{{NAME}}: a Vixen Studio plugin (plugin API 1.0), made by plugin new / plugin_scaffold.

vixen_plugin.py (next to this file) speaks the protocol; this file is only your plugin. Only protocol messages go to
stdout: print() anything else to stderr, which Studio keeps in the plugin's log (plugin_logs). Standard library only.
The protocol reference: the MCP resource plugins://api. Replace the parts marked TODO.
"""
import sys
import time

import vixen_plugin as vp

plugin = vp.Plugin()


@plugin.controller()
class Output(vp.Controller):
    """One controller of this type in the show. vixen-plugin.json declares its settings (configSchema)."""

    def configure(self, config):
        # Called on create and after every edit, with settings already checked against configSchema and defaults
        # filled in. TODO: check anything the schema can't say; raise vp.ConfigError("why", field="/prefix") to refuse.
        self.prefix = config.get("prefix", "")
        self.last_log = 0.0

    def start(self):
        # Live output began. TODO: connect to your device here.
        self.status("running")

    def frame(self, frame):
        # TODO: send frame.values (one 0-255 level per output) to your device. This prints them twice a second.
        now = time.monotonic()
        if now - self.last_log >= 0.5:
            self.last_log = now
            print(self.prefix + " ".join(str(v) for v in frame.values[:32]), file=sys.stderr, flush=True)

    def stop(self):
        # Live output ended. TODO: disconnect.
        pass


@plugin.tool("hello")
def hello(arguments):
    # TODO: your agent tools, as listed in vixen-plugin.json's contributes.tools.
    return f"Hello, {arguments.get('name') or 'there'}, from {{NAME}}."


if __name__ == "__main__":
    plugin.run()
