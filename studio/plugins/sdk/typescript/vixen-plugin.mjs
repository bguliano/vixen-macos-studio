// vixen-plugin: the Vixen Studio plugin SDK for Node (plugin API 1.1). One ES module, Node 20 or later, no dependencies.
// Types for TypeScript are in vixen-plugin.d.ts next to it.
//
// Copy this file (and the .d.ts) next to your plugin's main file (plugin new and plugin_scaffold do). It speaks the
// plugin protocol for you: JSON-RPC 2.0 over stdin/stdout, one message per line, frame decoding, logging, errors and
// configuration checks.
//
//   import { Plugin, Controller, ConfigError } from "./vixen-plugin.mjs";
//   const plugin = new Plugin();
//   plugin.controller(class Lamp extends Controller {
//     configure(config) { this.host = config.host; }      // create and every edit; throw ConfigError to refuse
//     frame(frame) { sendToLamp(this.host, frame.values); } // only while started
//   });
//   plugin.tool("discover", async (args) => "Found 2 lamps");
//   plugin.run();
//
// Only protocol messages go to stdout; write anything else with console.error (Studio keeps it in the plugin's log).
// The SDK checks each configuration against the type's configSchema in vixen-plugin.json before configure() sees it
// (the subset Studio checks) and fills in defaults. Handlers may be async: requests are answered in the order they
// arrive, one at a time. Frames are latest-wins: a slow frame() skips frames rather than falling behind.
import { readFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { join, dirname } from "node:path";

export const VERSION = "1.1.0";
export const API_VERSION = "1.1.0";
export const METHOD_NOT_FOUND = -32601;
export const INVALID_PARAMS = -32602;
export const INTERNAL_ERROR = -32603;
export const INVALID_CONFIG = -32004;

/** Refuses a configuration: errors is [{field, message}] or one message (for field "/"). */
export class ConfigError extends Error {
  constructor(errors, field = "/") {
    const list = typeof errors === "string" ? [{ field, message: errors }] : [...errors];
    super(list.map((e) => `${e.field} ${e.message}`).join("; ") || "invalid configuration");
    this.errors = list;
  }
}

/** A tool's refusal: answered as text with isError, not as a protocol error. */
export class ToolError extends Error {}

/** An error answer from Studio to one of the plugin's requests. */
export class RpcError extends Error {
  constructor(code, message, data) {
    super(message);
    this.code = code;
    this.data = data;
  }
}

class MethodNotFound extends Error {}
class UnknownInstance extends Error {}

/** A frame's values: numbers 0-255 (u8) or 0-65535 (u16), or the json items as sent. */
export function decode(encoding, values) {
  if (encoding === "u8") return Array.from(Buffer.from(values ?? "", "base64"));
  if (encoding === "u16") {
    const raw = Buffer.from(values ?? "", "base64");
    const out = [];
    for (let i = 0; i + 1 < raw.length; i += 2) out.push(raw.readUInt16LE(i));
    return out;
  }
  return Array.isArray(values) ? values : [];
}

/** Groups channel values into [r, g, b] per light; order names the outputs' colour order ("grb", "bgr", ...). */
export function triplets(values, order = "rgb") {
  const index = [..."rgb"].map((c) => order.toLowerCase().indexOf(c));
  const out = [];
  for (let i = 0; i + 2 < values.length; i += 3) out.push(index.map((k) => values[i + k]));
  return out;
}

/** One controller instance. Override what you need; anything may throw (or reject) and Studio gets the error. */
export class Controller {
  constructor(plugin, instanceId, typeId, outputCount) {
    this.plugin = plugin;
    this.instanceId = instanceId;
    this.typeId = typeId;
    this.outputCount = outputCount;
    this.config = {};
    this.running = false;
    this.paused = false;
  }
  /** The checked configuration, on create and every edit. Return a normalised config or nothing. */
  configure(config) {}
  setOutputCount(count) {
    this.outputCount = count;
  }
  /** Live output began: connect to the device. */
  start() {}
  /** Live output ended: disconnect. */
  stop() {}
  /** New channel values; called only while started and not paused. */
  frame(frame) {}
  destroy() {}
  /** Optional output names, sent with create's answer. */
  outputNames() {
    return undefined;
  }
  /**
   * For a type with "outputCount": "fromConfig": the outputs a configuration needs. No instance exists (Studio asks while
   * Live Output is off too); config is checked, with defaults filled in. Return a number, an array of output names (one
   * per output) or { outputCount, outputNames }. undefined (the default) means the type doesn't set its own count.
   */
  static outputsFor(config) {
    return undefined;
  }
  /** state: running, unavailable or error; shown as the controller's health. */
  status(state, message) {
    this.plugin.notify("controller.status", { instanceId: this.instanceId, state, ...(message === undefined ? {} : { message }) });
  }
  /** Adds to the counters controller_overview shows: pass what you sent since the last call. The SDK keeps the totals and sends them, as controller.stats wants. */
  stats({ packetsSent, bytesSent, errors } = {}) {
    const totals = (this.statsTotals ??= { packetsSent: 0, bytesSent: 0, errors: 0 });
    if (packetsSent !== undefined) totals.packetsSent += Math.trunc(packetsSent);
    if (bytesSent !== undefined) totals.bytesSent += Math.trunc(bytesSent);
    if (errors !== undefined) totals.errors += Math.trunc(errors);
    this.plugin.notify("controller.stats", { instanceId: this.instanceId, ...totals });
  }
  /** An alert for the person at Studio; never blocks the show. */
  message(title, text, heading) {
    this.plugin.notify("controller.message", { instanceId: this.instanceId, title, text, ...(heading === undefined ? {} : { heading }) });
  }
}

/** vixen-plugin.json from the working folder (Studio starts plugins in their folder), else next to the main script. */
export function loadManifest(path) {
  const candidates = path ? [path] : [join(process.cwd(), "vixen-plugin.json"), join(dirname(process.argv[1] ?? "."), "vixen-plugin.json")];
  for (const candidate of candidates) {
    try {
      return JSON.parse(readFileSync(candidate, "utf8"));
    } catch {
      // try the next one
    }
  }
  return {};
}

export class Plugin {
  constructor(manifestPath) {
    this.manifest = loadManifest(manifestPath);
    this.dataDir = process.env.VIXEN_PLUGIN_DATA_DIR;
    this.muted = true;
    this.profile = null;
    this.hostVersion = null;
    this.capabilities = {};
    this.types = []; // [typeId | null, class]
    this.tools = new Map();
    this.events = new Map();
    this.instances = new Map();
    this.pending = new Map();
    this.nextId = 0;
    this.frames = new Map();
    this.queue = Promise.resolve();
    this.done = false;
    this.released = false;
    this.initializeHandler = undefined;
    this.shutdownHandler = undefined;
  }

  // ---- Registration ------------------------------------------------------------------------------------------------

  /** A Controller subclass for typeId (the manifest's typeId or type name; leave out when there is one type). */
  controller(cls, { typeId } = {}) {
    if (!(cls.prototype instanceof Controller)) throw new TypeError("plugin.controller() takes a Controller subclass");
    this.types.push([this.resolveType(typeId), cls]);
    return cls;
  }

  /** fn(arguments) returns a string (text), {content, isError?} as it is, or anything else as JSON text. */
  tool(name, fn) {
    this.tools.set(name, fn);
  }

  /** fn(data) for a declared event (liveOutput.changed also updates plugin.muted). */
  event(name, fn) {
    this.events.set(name, fn);
  }

  onInitialize(fn) {
    this.initializeHandler = fn;
  }

  onShutdown(fn) {
    this.shutdownHandler = fn;
  }

  resolveType(typeId) {
    const types = this.manifest?.contributes?.controllers ?? [];
    if (typeId === undefined) return types.length === 1 ? types[0].typeId : null;
    return types.find((t) => t.typeId === typeId || t.name === typeId)?.typeId ?? typeId;
  }

  // ---- Talking to Studio -------------------------------------------------------------------------------------------

  send(message) {
    process.stdout.write(JSON.stringify({ jsonrpc: "2.0", ...message }) + "\n");
  }

  notify(method, params) {
    this.send({ method, params });
  }

  /** Asks Studio; resolves with the result or rejects with RpcError / a timeout Error. */
  request(method, params, timeoutMs = 5000) {
    const id = `p${++this.nextId}`;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`Studio didn't answer ${method} within ${timeoutMs} ms`));
      }, timeoutMs);
      this.pending.set(id, (reply) => {
        clearTimeout(timer);
        if (reply.error) reject(new RpcError(reply.error.code ?? INTERNAL_ERROR, reply.error.message ?? "error", reply.error.data));
        else resolve(reply.result);
      });
      this.send({ id, method, params });
    });
  }

  log(level, message) {
    this.notify("log", { level, message: String(message) });
  }
  debug(message) {
    this.log("debug", message);
  }
  info(message) {
    this.log("info", message);
  }
  warn(message) {
    this.log("warn", message);
  }
  error(message) {
    this.log("error", message);
  }

  /** A small JSON value kept in pluginDataDir/state.json. Needs "files": ["plugin-data"] in the manifest. */
  async storageGet(key, fallback = null) {
    const value = (await this.request("storage.get", { key }))?.value;
    return value ?? fallback;
  }

  async storageSet(key, value) {
    await this.request("storage.set", { key, value });
  }

  /**
   * The open show's elements, controllers and patches, every page gathered into one object (the shape of a
   * display.snapshot page, with more false). Needs "display": "read". Starts over if the show changes mid-way.
   */
  async displaySnapshot() {
    attempts: for (let attempt = 0; attempt < 5; attempt++) {
      const snapshot = await this.request("display.snapshot", {});
      while (snapshot.more) {
        const page = await this.request("display.snapshot", { elementOffset: snapshot.elementOffset, patchOffset: snapshot.patchOffset });
        if (page.revision !== snapshot.revision) continue attempts;
        snapshot.elements.push(...page.elements);
        snapshot.patches.push(...page.patches);
        Object.assign(snapshot, { more: page.more, elementOffset: page.elementOffset, patchOffset: page.patchOffset });
      }
      return snapshot;
    }
    throw new RpcError(-32603, "the show kept changing while it was read");
  }

  /** Replaces the agent tools Studio lists (same shape as contributes.tools); undefined goes back to the manifest's. */
  toolsChanged(tools) {
    this.notify("tools.changed", tools === undefined ? {} : { tools });
  }

  // ---- The loop ----------------------------------------------------------------------------------------------------

  /** Serves Studio until shutdown or stdin closes, then exits the process. */
  run() {
    const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
    lines.on("line", (line) => this.receive(line));
    lines.on("close", () => this.enqueue(() => this.finish(0)));
  }

  receive(line) {
    if (!line.trim() || this.done) return;
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      console.error(`vixen-plugin: ignored a line that isn't JSON: ${line.slice(0, 80)}`);
      return;
    }
    if (message === null || typeof message !== "object") return;
    if (message.method === undefined) {
      const resolve = this.pending.get(String(message.id));
      if (resolve) {
        this.pending.delete(String(message.id));
        resolve(message);
      }
      return;
    }
    if (message.method === "controller.frame" && message.id === undefined) {
      const instance = String(message.params?.instanceId);
      const waiting = this.frames.has(instance);
      this.frames.set(instance, message.params ?? {});
      if (!waiting) setImmediate(() => this.frame(instance));
      return;
    }
    this.enqueue(() => this.dispatch(message));
  }

  enqueue(task) {
    this.queue = this.queue.then(task, task);
  }

  async frame(instanceId) {
    const params = this.frames.get(instanceId);
    this.frames.delete(instanceId);
    const instance = this.instances.get(instanceId);
    if (!params || !instance || !instance.running || instance.paused) return;
    try {
      await instance.frame({ values: decode(params.encoding ?? "u8", params.values), seq: params.seq ?? 0, timeMs: params.timeMs ?? 0, encoding: params.encoding ?? "u8" });
    } catch (error) {
      this.error(`frame failed: ${error?.stack ?? error}`);
    }
  }

  async dispatch(message) {
    if (this.done) return;
    const { method, id } = message;
    const params = message.params ?? {};
    try {
      const result = await this.handle(method, params);
      if (id !== undefined && id !== null) this.send({ id, result: result ?? {} });
    } catch (error) {
      if (error instanceof ConfigError) this.answerError(id, INVALID_CONFIG, error.message, { errors: error.errors });
      else if (error instanceof MethodNotFound) this.answerError(id, METHOD_NOT_FOUND, `${method} is not implemented`);
      else if (error instanceof UnknownInstance) this.answerError(id, INVALID_PARAMS, `unknown instance ${error.message}`);
      else {
        console.error(error?.stack ?? error);
        this.answerError(id, INTERNAL_ERROR, `${error?.name ?? "Error"}: ${error?.message ?? error}`);
      }
    }
    if (method === "shutdown") await this.finish(0);
  }

  answerError(id, code, message, data) {
    if (id === undefined || id === null) {
      this.error(message);
      return;
    }
    this.send({ id, error: data === undefined ? { code, message } : { code, message, data } });
  }

  async handle(method, params) {
    switch (method) {
      case "initialize":
        this.muted = params.muted ?? true;
        this.profile = params.profile ?? null;
        this.hostVersion = params.hostVersion ?? null;
        this.capabilities = params.capabilities ?? {};
        this.dataDir = params.pluginDataDir ?? this.dataDir;
        if (this.initializeHandler) await this.initializeHandler(params);
        return { apiVersion: API_VERSION, capabilities: {} };
      case "shutdown":
        await this.release(); // before answering: Studio closes stdin once it has the answer
        return {};
      case "event": {
        if (params.name === "liveOutput.changed" && params.data && "muted" in params.data) this.muted = Boolean(params.data.muted);
        const handler = this.events.get(params.name);
        if (handler) await handler(params.data);
        return undefined;
      }
      case "tool.call":
        return this.callTool(params.name, params.arguments ?? {});
      case "controller.create":
        return this.create(params);
      case "controller.outputsFor":
        return this.outputsFor(params);
    }
    if (!method?.startsWith("controller.")) throw new MethodNotFound();
    const instance = this.instances.get(params.instanceId);
    if (!instance) throw new UnknownInstance(String(params.instanceId));
    switch (method) {
      case "controller.configure":
        instance.config = await this.configure(instance, params.config);
        return { config: instance.config };
      case "controller.setOutputCount":
        await instance.setOutputCount(Number(params.outputCount));
        return {};
      case "controller.start":
        await instance.start();
        instance.running = true;
        instance.paused = false;
        return {};
      case "controller.stop":
        instance.running = false;
        await instance.stop();
        return {};
      case "controller.pause":
        instance.paused = true;
        return {};
      case "controller.resume":
        instance.paused = false;
        return {};
      case "controller.destroy":
        this.instances.delete(params.instanceId);
        if (instance.running) {
          instance.running = false;
          await instance.stop();
        }
        await instance.destroy();
        return {};
    }
    throw new MethodNotFound();
  }

  /** True when this Studio resizes "outputCount": "fromConfig" controllers itself (plugin API 1.1). Before that, refuse a configuration that needs more outputs than outputCount, as plugins for API 1.0 did. */
  get hostSetsOutputCount() {
    return this.capabilities?.controllerOutputs === true;
  }

  classFor(typeId) {
    let cls = this.types.find(([t]) => t === typeId)?.[1];
    if (!cls) {
      const untyped = this.types.filter(([t]) => t === null);
      if (untyped.length === 1) cls = untyped[0][1];
    }
    if (!cls) throw new MethodNotFound();
    return cls;
  }

  async outputsFor(params) {
    const cls = this.classFor(params.typeId);
    let config = params.config && typeof params.config === "object" && !Array.isArray(params.config) ? params.config : {};
    const schema = (this.manifest?.contributes?.controllers ?? []).find((t) => t.typeId === params.typeId)?.configSchema;
    if (schema) {
      config = fillDefaults(schema, config);
      const errors = validate(schema, config);
      if (errors.length) throw new ConfigError(errors);
    }
    const answer = await cls.outputsFor(config);
    if (answer === undefined || answer === null) throw new MethodNotFound();
    if (typeof answer === "number") return { outputCount: Math.trunc(answer) };
    if (Array.isArray(answer)) return { outputCount: answer.length, outputNames: answer.map(String) };
    const result = { outputCount: Math.trunc(Number(answer.outputCount)) };
    if (answer.outputNames !== undefined) result.outputNames = answer.outputNames.map(String);
    return result;
  }

  async create(params) {
    const cls = this.classFor(params.typeId);
    const instance = new cls(this, params.instanceId, params.typeId, Number(params.outputCount ?? 0));
    instance.config = await this.configure(instance, params.config);
    this.instances.set(instance.instanceId, instance);
    const answer = { config: instance.config };
    const names = instance.outputNames();
    if (names !== undefined) answer.outputNames = [...names];
    return answer;
  }

  async configure(instance, config) {
    let checked = config && typeof config === "object" && !Array.isArray(config) ? config : {};
    const schema = (this.manifest?.contributes?.controllers ?? []).find((t) => t.typeId === instance.typeId)?.configSchema;
    if (schema) {
      checked = fillDefaults(schema, checked);
      const errors = validate(schema, checked);
      if (errors.length) throw new ConfigError(errors);
    }
    const normalised = await instance.configure(checked);
    return normalised ?? checked;
  }

  async callTool(name, args) {
    const handler = this.tools.get(name);
    if (!handler) return { content: [{ type: "text", text: `unknown tool ${name}` }], isError: true };
    let result;
    try {
      result = await handler(args);
    } catch (error) {
      if (error instanceof ToolError) return { content: [{ type: "text", text: error.message }], isError: true };
      throw error;
    }
    if (typeof result === "string") return { content: [{ type: "text", text: result }] };
    if (result && Array.isArray(result.content)) return result;
    return { content: [{ type: "text", text: JSON.stringify(result ?? null) }] };
  }

  async finish(code) {
    if (this.done) return;
    this.done = true;
    await this.release();
    // Let stdout drain before leaving.
    process.stdout.write("", () => process.exit(code));
  }

  /** Stops and destroys every instance, then runs the shutdown handler; once. */
  async release() {
    if (this.released) return;
    this.released = true;
    for (const instance of this.instances.values()) {
      try {
        if (instance.running) {
          instance.running = false;
          await instance.stop();
        }
        await instance.destroy();
      } catch (error) {
        console.error(error?.stack ?? error);
      }
    }
    this.instances.clear();
    if (this.shutdownHandler) await this.shutdownHandler();
  }
}

// ---- Configuration checks: the configSchema subset Studio checks (crates/studio-plugins/src/schema.rs) ----------------

function typesOf(schema) {
  const t = schema.type;
  if (typeof t === "string") return [t];
  return Array.isArray(t) ? t.filter((x) => typeof x === "string") : [];
}

function typeOf(value) {
  if (value === null || value === undefined) return "null";
  if (typeof value === "boolean") return "boolean";
  if (typeof value === "number") return Number.isInteger(value) ? "integer" : "number";
  if (typeof value === "string") return "string";
  if (Array.isArray(value)) return "array";
  return "object";
}

const escape = (name) => name.replaceAll("~", "~0").replaceAll("/", "~1");

function deepEqual(a, b) {
  return JSON.stringify(a) === JSON.stringify(b);
}

function ipv4(text) {
  const parts = text.split(".");
  return parts.length === 4 && parts.every((p) => /^\d{1,3}$/.test(p) && Number(p) <= 255 && (p === "0" || !p.startsWith("0")));
}

function hostname(text) {
  return text.length > 0 && text.length <= 253 && text.split(".").every((l) => l.length > 0 && l.length <= 63 && /^[A-Za-z0-9-]+$/.test(l) && !l.startsWith("-") && !l.endsWith("-"));
}

/** [{field, message}] for every way value breaks schema (at most 50). */
export function validate(schema, value, at = "") {
  const errors = [];
  check(schema, value, at, errors);
  return errors.slice(0, 50);
}

function check(schema, value, at, errors) {
  const push = (message) => errors.push({ field: at || "/", message });
  const types = typesOf(schema);
  const actual = typeOf(value);
  if (types.length && !types.some((t) => t === actual || (t === "number" && actual === "integer"))) {
    push(`must be ${types.join(" or ")}`);
    return;
  }
  if (Array.isArray(schema.enum) && !schema.enum.some((v) => deepEqual(v, value))) push(`must be one of ${schema.enum.map((v) => JSON.stringify(v)).join(", ")}`);
  if ("const" in schema && !deepEqual(schema.const, value)) push(`must be ${JSON.stringify(schema.const)}`);
  if (actual === "integer" || actual === "number") {
    if (typeof schema.minimum === "number" && value < schema.minimum) push(`must be at least ${schema.minimum}`);
    if (typeof schema.maximum === "number" && value > schema.maximum) push(`must be at most ${schema.maximum}`);
  } else if (actual === "string") {
    const chars = [...value].length;
    if (Number.isInteger(schema.minLength) && chars < schema.minLength) push(`must be at least ${schema.minLength} characters`);
    if (Number.isInteger(schema.maxLength) && chars > schema.maxLength) push(`must be at most ${schema.maxLength} characters`);
    if (schema.format === "ipv4" && !ipv4(value)) push("must be an IPv4 address such as 192.168.1.50");
    if (schema.format === "hostname" && !hostname(value)) push("must be a host name");
  } else if (actual === "array") {
    if (Number.isInteger(schema.minItems) && value.length < schema.minItems) push(`must have at least ${schema.minItems} items`);
    if (Number.isInteger(schema.maxItems) && value.length > schema.maxItems) push(`must have at most ${schema.maxItems} items`);
    if (schema.items && typeof schema.items === "object" && !Array.isArray(schema.items)) value.forEach((item, i) => check(schema.items, item, `${at}/${i}`, errors));
  } else if (actual === "object") {
    const properties = schema.properties && typeof schema.properties === "object" ? schema.properties : {};
    for (const name of Array.isArray(schema.required) ? schema.required : []) {
      if (typeof name === "string" && !(name in value)) errors.push({ field: `${at}/${escape(name)}`, message: "is required" });
    }
    for (const [name, item] of Object.entries(value)) {
      const field = `${at}/${escape(name)}`;
      if (Object.hasOwn(properties, name)) check(properties[name], item, field, errors);
      else if (schema.additionalProperties === false) errors.push({ field, message: "is not a known setting" });
      else if (schema.additionalProperties && typeof schema.additionalProperties === "object") check(schema.additionalProperties, item, field, errors);
    }
  }
}

/** config with each missing property that has a default (or an object with defaults inside) filled in. */
export function fillDefaults(schema, config) {
  const out = { ...config };
  for (const [name, prop] of Object.entries(schema.properties ?? {})) {
    if (!prop || typeof prop !== "object") continue;
    if (!(name in out)) {
      if ("default" in prop) out[name] = structuredClone(prop.default);
      else if (typesOf(prop).includes("object") && prop.properties) {
        const inner = fillDefaults(prop, {});
        if (Object.keys(inner).length) out[name] = inner;
      }
    } else if (out[name] && typeof out[name] === "object" && !Array.isArray(out[name]) && prop.properties) {
      out[name] = fillDefaults(prop, out[name]);
    }
  }
  return out;
}
