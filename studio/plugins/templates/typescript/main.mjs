#!/usr/bin/env node
// {{NAME}}: a Vixen Studio plugin (plugin API 1.0), made by plugin_scaffold. Node 20 or later, no dependencies.
//
// Studio starts this file and speaks JSON-RPC 2.0 with it over stdin/stdout, one message per line. Only protocol
// messages go to stdout; write anything else to stderr (console.error), which Studio keeps in the plugin's log
// (plugin_logs). The full protocol: the MCP resources plugins://api and plugins://schema/protocol.
//
// Replace the parts marked TODO.
import { createInterface } from "node:readline";

const API_VERSION = "1.0.0";
const INVALID_CONFIG = -32004;
// The settings in the manifest's configSchema. TODO: keep in step with it.
const SETTINGS = new Set(["prefix"]);

/** instanceId -> { config, outputs, running, lastLog } */
const instances = new Map();

function send(message) {
  process.stdout.write(JSON.stringify({ jsonrpc: "2.0", ...message }) + "\n");
}

function log(level, message) {
  send({ method: "log", params: { level, message } });
}

/** Refuses a configuration; Studio shows each {field, message} next to that field. */
class ConfigError extends Error {
  constructor(errors) {
    super("invalid configuration");
    this.errors = errors;
  }
}

class NotImplemented extends Error {}

function checkConfig(config) {
  // TODO: check your settings here. The manifest's configSchema has already been checked by Studio.
  const errors = Object.keys(config)
    .filter((key) => !SETTINGS.has(key))
    .map((key) => ({ field: `/${key}`, message: "is not a setting of this plugin" }));
  if ((config.prefix ?? "").length > 40) errors.push({ field: "/prefix", message: "is longer than 40 characters" });
  if (errors.length) throw new ConfigError(errors);
  return config;
}

/** A frame's values: ints 0-255 for u8, 0-65535 for u16, or the json items as they are. */
function valuesOf({ encoding, values }) {
  if (encoding === "u8") return [...Buffer.from(values, "base64")];
  if (encoding === "u16") {
    const raw = Buffer.from(values, "base64");
    const out = [];
    for (let i = 0; i + 1 < raw.length; i += 2) out.push(raw.readUInt16LE(i));
    return out;
  }
  return values;
}

function onFrame(instance, values) {
  // TODO: send the values to your device. This example prints them to the log at most twice a second.
  const now = Date.now();
  if (now - instance.lastLog >= 500) {
    instance.lastLog = now;
    console.error((instance.config.prefix ?? "") + values.slice(0, 32).join(" "));
  }
}

function callTool(name, args) {
  // TODO: your agent tools, as listed in the manifest's contributes.tools.
  if (name === "hello") {
    return { content: [{ type: "text", text: `Hello, ${args.name || "there"}, from {{NAME}}.` }] };
  }
  return { content: [{ type: "text", text: `unknown tool ${name}` }], isError: true };
}

function handle(method, params) {
  if (method === "initialize") {
    log("info", `{{NAME}} ready (muted=${params.muted})`);
    return { apiVersion: API_VERSION, capabilities: {} };
  }
  if (method === "shutdown") return {};
  if (method === "tool.call") return callTool(params.name, params.arguments ?? {});
  const id = params.instanceId;
  if (method === "controller.create") {
    const config = checkConfig(params.config ?? {});
    instances.set(id, { config, outputs: params.outputCount ?? 0, running: false, lastLog: 0 });
    return { config };
  }
  const instance = instances.get(id);
  if (!instance) throw new Error(`unknown instance ${id}`);
  switch (method) {
    case "controller.configure":
      instance.config = checkConfig(params.config ?? {});
      return { config: instance.config };
    case "controller.setOutputCount":
      instance.outputs = params.outputCount;
      return {};
    case "controller.start":
    case "controller.resume":
      instance.running = true; // TODO: connect to your device here.
      return {};
    case "controller.stop":
    case "controller.pause":
      instance.running = false;
      return {};
    case "controller.destroy":
      instances.delete(id);
      return {};
  }
  throw new NotImplemented(method);
}

const input = createInterface({ input: process.stdin });
input.on("line", (line) => {
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    return;
  }
  const { method, id } = message;
  const params = message.params ?? {};
  if (method === undefined) return; // an answer to a request of ours; this plugin sends none
  if (method === "controller.frame") {
    const instance = instances.get(params.instanceId);
    if (instance?.running) onFrame(instance, valuesOf(params));
    return;
  }
  if (id === undefined) return; // other notifications, such as events
  try {
    send({ id, result: handle(method, params) });
  } catch (error) {
    if (error instanceof ConfigError) {
      send({ id, error: { code: INVALID_CONFIG, message: error.message, data: { errors: error.errors } } });
    } else if (error instanceof NotImplemented) {
      send({ id, error: { code: -32601, message: `${method} is not implemented` } });
    } else {
      send({ id, error: { code: -32603, message: String(error?.message ?? error) } });
    }
  }
  // Writes to a pipe finish later on macOS: exit once the answer is out.
  if (method === "shutdown") process.stdout.write("", () => process.exit(0));
});
input.on("close", () => process.exit(0));
