#!/usr/bin/env node
// {{NAME}}: a Vixen Studio plugin (plugin API 1.0), made by plugin new / plugin_scaffold. Node 20 or later.
//
// vixen-plugin.mjs (next to this file, with its types in vixen-plugin.d.ts) speaks the protocol; this file is only your
// plugin. Only protocol messages go to stdout: use console.error for anything else, which Studio keeps in the plugin's
// log (plugin_logs). The protocol reference: the MCP resource plugins://api. Replace the parts marked TODO.
import { Plugin, Controller } from "./vixen-plugin.mjs";

const plugin = new Plugin();

/** One controller of this type in the show. vixen-plugin.json declares its settings (configSchema). */
class Output extends Controller {
  configure(config) {
    // Called on create and after every edit, with settings already checked against configSchema and defaults filled
    // in. TODO: check anything the schema can't say; throw new ConfigError("why", "/prefix") to refuse.
    this.prefix = config.prefix ?? "";
    this.lastLog = 0;
  }

  start() {
    // Live output began. TODO: connect to your device here.
    this.status("running");
  }

  frame(frame) {
    // TODO: send frame.values (one 0-255 level per output) to your device. This prints them twice a second.
    const now = Date.now();
    if (now - this.lastLog >= 500) {
      this.lastLog = now;
      console.error(this.prefix + frame.values.slice(0, 32).join(" "));
    }
  }

  stop() {
    // Live output ended. TODO: disconnect.
  }
}

plugin.controller(Output);

// TODO: your agent tools, as listed in vixen-plugin.json's contributes.tools.
plugin.tool("hello", (args) => `Hello, ${args.name || "there"}, from {{NAME}}.`);

plugin.run();
