"""vixen_plugin: the Vixen Studio plugin SDK for Python (plugin API 1.0). One file, standard library only, Python 3.9+.

Copy this file next to your plugin's main file (plugin new and plugin_scaffold do). It speaks the plugin protocol for
you: JSON-RPC 2.0 over stdin/stdout, one message per line, frame decoding, logging, errors and configuration checks.

    import vixen_plugin as vp

    plugin = vp.Plugin()

    @plugin.controller()
    class Lamp(vp.Controller):
        def configure(self, config):      # create and every edit; raise vp.ConfigError to refuse
            self.host = config["host"]
        def frame(self, frame):           # only while started; frame.values is a list of ints for u8/u16
            send_to_lamp(self.host, frame.values)

    @plugin.tool("discover")
    def discover(arguments):
        return "Found 2 lamps"

    plugin.run()

Only protocol messages go to stdout; print() anything else to stderr (Studio keeps it in the plugin's log). The SDK
checks each configuration against the controller type's configSchema in vixen-plugin.json before your configure()
sees it (the same subset Studio checks) and fills in defaults, so configure() only gets settings it declared.
Frames are latest-wins: a slow frame() skips frames rather than falling behind. Requests to Studio (storage_get,
storage_set) can be made from any thread, as can log, status, stats and message.
"""
import base64
import json
import os
import queue
import sys
import threading
import traceback

__version__ = "1.0.0"
API_VERSION = "1.0.0"

PARSE_ERROR = -32700
METHOD_NOT_FOUND = -32601
INVALID_PARAMS = -32602
INTERNAL_ERROR = -32603
INVALID_CONFIG = -32004


class ConfigError(Exception):
    """Refuses a configuration. errors: [{"field": "/host", "message": "is not reachable"}], or a single message."""

    def __init__(self, errors, field="/"):
        if isinstance(errors, str):
            errors = [{"field": field, "message": errors}]
        super().__init__("; ".join(f"{e['field']} {e['message']}" for e in errors) or "invalid configuration")
        self.errors = list(errors)


class ToolError(Exception):
    """A tool's refusal: answered as text with isError, not as a protocol error."""


class RpcError(Exception):
    """An error answer from Studio to one of the plugin's requests."""

    def __init__(self, code, message, data=None):
        super().__init__(message)
        self.code, self.message, self.data = code, message, data


class Frame:
    """One controller.frame: values (ints 0-255 for u8, 0-65535 for u16, the items as sent for json), seq, time_ms."""

    __slots__ = ("values", "seq", "time_ms", "encoding")

    def __init__(self, params):
        self.encoding = params.get("encoding", "u8")
        self.seq = params.get("seq", 0)
        self.time_ms = params.get("timeMs", 0)
        self.values = decode(self.encoding, params.get("values"))


def decode(encoding, values):
    if encoding == "u8":
        return list(base64.b64decode(values or ""))
    if encoding == "u16":
        raw = base64.b64decode(values or "")
        return [int.from_bytes(raw[i:i + 2], "little") for i in range(0, len(raw) - 1, 2)]
    return list(values or [])


def triplets(values, order="rgb"):
    """Groups channel values into (r, g, b) tuples, three outputs per light; order names the outputs' colour order."""
    order = order.lower()
    index = [order.index(c) for c in "rgb"]
    return [tuple(values[i + k] for k in index) for i in range(0, len(values) - 2, 3)]


_stats_lock = threading.Lock()


class Controller:
    """One controller instance. Override what you need; every method may raise (the error goes back to Studio)."""

    def __init__(self, plugin, instance_id, type_id, output_count):
        self.plugin = plugin
        self.instance_id = instance_id
        self.type_id = type_id
        self.output_count = output_count
        self.config = {}
        self.running = False
        self.paused = False

    def configure(self, config):
        """Called with the checked configuration on create and on every edit. Return a normalised config or None."""

    def set_output_count(self, count):
        self.output_count = count

    def start(self):
        """Live output began: connect to the device."""

    def stop(self):
        """Live output ended: disconnect."""

    def frame(self, frame):
        """New channel values; called only while started and not paused."""

    def destroy(self):
        """The controller was removed or the plugin is shutting down."""

    def output_names(self):
        """Optional names for the outputs (list of strings), sent with create's answer."""
        return None

    # Reports to Studio.
    def status(self, state, message=None):
        """state: running, unavailable or error; shown as the controller's health."""
        params = {"instanceId": self.instance_id, "state": state}
        if message is not None:
            params["message"] = message
        self.plugin.notify("controller.status", params)

    def stats(self, packets_sent=None, bytes_sent=None, errors=None):
        """Adds to the counters controller_overview shows: pass what you sent since the last call. The SDK keeps the
        totals and sends them, as controller.stats wants."""
        with _stats_lock:
            totals = self.__dict__.setdefault("_stats_totals", {"packetsSent": 0, "bytesSent": 0, "errors": 0})
            for key, value in (("packetsSent", packets_sent), ("bytesSent", bytes_sent), ("errors", errors)):
                if value is not None:
                    totals[key] += int(value)
            params = {"instanceId": self.instance_id, **totals}
        self.plugin.notify("controller.stats", params)

    def message(self, title, text, heading=None):
        """An alert for the person at Studio; never blocks the show."""
        params = {"instanceId": self.instance_id, "title": title, "text": text}
        if heading is not None:
            params["heading"] = heading
        self.plugin.notify("controller.message", params)


class Plugin:
    def __init__(self, manifest_path=None):
        self.manifest = load_manifest(manifest_path)
        self.data_dir = os.environ.get("VIXEN_PLUGIN_DATA_DIR")
        self.muted = True
        self.profile = None
        self.host_version = None
        self._types = []  # (type_id or None, class)
        self._tools = {}
        self._events = {}
        self._on_initialize = None
        self._on_shutdown = None
        self._instances = {}
        self._write = threading.Lock()
        self._pending = {}
        self._next_id = 0
        self._inbox = queue.Queue()
        self._frames = {}
        self._frames_lock = threading.Lock()
        self._finished = False

    # ---- Registration -------------------------------------------------------------------------------------------

    def controller(self, type_id=None):
        """Class decorator for a Controller subclass. type_id: the manifest typeId it serves (or the type's name);
        leave it out when the manifest declares one controller type."""

        def register(cls):
            if not (isinstance(cls, type) and issubclass(cls, Controller)):
                raise TypeError("@plugin.controller() decorates a vixen_plugin.Controller subclass")
            self._types.append((self._resolve_type(type_id), cls))
            return cls

        return register

    def tool(self, name):
        """Decorator for fn(arguments) -> str | dict | list. A str becomes text content; a dict with "content" is sent
        as it is; anything else is sent as JSON text. Raise ToolError to answer with isError."""

        def register(fn):
            self._tools[name] = fn
            return fn

        return register

    def event(self, name):
        """Decorator for fn(data), called for the declared event name (liveOutput.changed also updates plugin.muted)."""

        def register(fn):
            self._events[name] = fn
            return fn

        return register

    def on_initialize(self, fn):
        """Decorator for fn(params), called once Studio has introduced itself."""
        self._on_initialize = fn
        return fn

    def on_shutdown(self, fn):
        self._on_shutdown = fn
        return fn

    def _resolve_type(self, type_id):
        types = (self.manifest.get("contributes") or {}).get("controllers") or []
        if type_id is None:
            return types[0]["typeId"] if len(types) == 1 else None
        for t in types:
            if type_id in (t.get("typeId"), t.get("name")):
                return t["typeId"]
        return type_id

    # ---- Talking to Studio --------------------------------------------------------------------------------------

    def send(self, message):
        message["jsonrpc"] = "2.0"
        line = json.dumps(message, separators=(",", ":"), ensure_ascii=False) + "\n"
        with self._write:
            sys.stdout.write(line)
            sys.stdout.flush()

    def notify(self, method, params):
        self.send({"method": method, "params": params})

    def request(self, method, params, timeout=5.0):
        """Asks Studio and waits for the answer (from any thread). Raises RpcError or TimeoutError."""
        with self._write:
            self._next_id += 1
            request_id = f"p{self._next_id}"
            slot = self._pending[request_id] = [threading.Event(), None]
        self.send({"id": request_id, "method": method, "params": params})
        if not slot[0].wait(timeout):
            self._pending.pop(request_id, None)
            raise TimeoutError(f"Studio didn't answer {method} within {timeout} s")
        reply = slot[1]
        if "error" in reply:
            error = reply["error"] or {}
            raise RpcError(error.get("code", INTERNAL_ERROR), error.get("message", "error"), error.get("data"))
        return reply.get("result")

    def log(self, level, message):
        self.notify("log", {"level": level, "message": str(message)})

    def debug(self, message):
        self.log("debug", message)

    def info(self, message):
        self.log("info", message)

    def warn(self, message):
        self.log("warn", message)

    def error(self, message):
        self.log("error", message)

    def storage_get(self, key, default=None):
        """A small JSON value kept in pluginDataDir/state.json. Needs "files": ["plugin-data"] in the manifest."""
        value = (self.request("storage.get", {"key": key}) or {}).get("value")
        return default if value is None else value

    def storage_set(self, key, value):
        self.request("storage.set", {"key": key, "value": value})

    def display_snapshot(self):
        """The open show's elements, controllers and patches, every page gathered into one dict (the shape of a
        display.snapshot page, with more False). Needs "display": "read". Starts over if the show changes mid-way."""
        for _ in range(5):
            snapshot = self.request("display.snapshot", {})
            while snapshot.get("more"):
                page = self.request("display.snapshot", {"elementOffset": snapshot["elementOffset"], "patchOffset": snapshot["patchOffset"]})
                if page.get("revision") != snapshot.get("revision"):
                    break
                snapshot["elements"] += page["elements"]
                snapshot["patches"] += page["patches"]
                for key in ("more", "elementOffset", "patchOffset"):
                    snapshot[key] = page[key]
            else:
                return snapshot
        raise RpcError(-32603, "the show kept changing while it was read")

    def tools_changed(self, tools=None):
        """Replaces the agent tools Studio lists (same shape as contributes.tools); None goes back to the manifest's."""
        self.notify("tools.changed", {} if tools is None else {"tools": tools})

    # ---- The loop -----------------------------------------------------------------------------------------------

    def run(self):
        """Serves Studio until shutdown or stdin closes."""
        reader = threading.Thread(target=self._read, name="vixen-plugin-reader", daemon=True)
        reader.start()
        while True:
            item = self._inbox.get()
            if item is None:
                self._finish()
                return
            if isinstance(item, str):
                with self._frames_lock:
                    params = self._frames.pop(item, None)
                if params is not None:
                    self._frame(params)
                continue
            if self._dispatch(item):
                return

    def _read(self):
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue
            try:
                message = json.loads(line)
            except ValueError:
                print(f"vixen_plugin: ignored a line that isn't JSON: {line[:80]}", file=sys.stderr, flush=True)
                continue
            if not isinstance(message, dict):
                continue
            if "method" not in message:
                slot = self._pending.pop(str(message.get("id")), None)
                if slot is not None:
                    slot[1] = message
                    slot[0].set()
                continue
            if message["method"] == "controller.frame" and "id" not in message:
                params = message.get("params") or {}
                instance = str(params.get("instanceId"))
                with self._frames_lock:
                    waiting = instance in self._frames
                    self._frames[instance] = params
                if not waiting:
                    self._inbox.put(instance)
                continue
            self._inbox.put(message)
        self._inbox.put(None)

    def _frame(self, params):
        instance = self._instances.get(params.get("instanceId"))
        if instance is None or not instance.running or instance.paused:
            return
        try:
            instance.frame(Frame(params))
        except Exception as error:  # noqa: BLE001 - a bad frame must not end the plugin
            self.error(f"frame failed: {type(error).__name__}: {error}")
            traceback.print_exc(file=sys.stderr)

    def _dispatch(self, message):
        """Handles one request or notification. True once shutdown has been answered."""
        method, params, request_id = message["method"], message.get("params") or {}, message.get("id")
        try:
            result = self._handle(method, params)
            if request_id is not None:
                self.send({"id": request_id, "result": result if result is not None else {}})
        except ConfigError as error:
            self._answer_error(request_id, INVALID_CONFIG, str(error), {"errors": error.errors})
        except _MethodNotFound:
            self._answer_error(request_id, METHOD_NOT_FOUND, f"{method} is not implemented")
        except _UnknownInstance as error:
            self._answer_error(request_id, INVALID_PARAMS, f"unknown instance {error}")
        except Exception as error:  # noqa: BLE001 - report every failure to Studio instead of exiting
            traceback.print_exc(file=sys.stderr)
            self._answer_error(request_id, INTERNAL_ERROR, f"{type(error).__name__}: {error}")
        return method == "shutdown"

    def _answer_error(self, request_id, code, text, data=None):
        if request_id is None:
            self.error(text)
            return
        error = {"code": code, "message": text}
        if data is not None:
            error["data"] = data
        self.send({"id": request_id, "error": error})

    def _handle(self, method, params):
        if method == "initialize":
            self.muted = bool(params.get("muted", True))
            self.profile = params.get("profile")
            self.host_version = params.get("hostVersion")
            self.data_dir = params.get("pluginDataDir") or self.data_dir
            if self._on_initialize:
                self._on_initialize(params)
            return {"apiVersion": API_VERSION, "capabilities": {}}
        if method == "shutdown":
            self._finish()  # before answering: Studio closes stdin once it has the answer
            return {}
        if method == "event":
            name, data = params.get("name"), params.get("data")
            if name == "liveOutput.changed" and isinstance(data, dict) and "muted" in data:
                self.muted = bool(data["muted"])
            handler = self._events.get(name)
            if handler:
                handler(data)
            return None
        if method == "tool.call":
            return self._call_tool(params.get("name"), params.get("arguments") or {})
        if method == "controller.create":
            return self._create(params)
        if not method.startswith("controller."):
            raise _MethodNotFound()
        instance = self._instances.get(params.get("instanceId"))
        if instance is None:
            raise _UnknownInstance(params.get("instanceId"))
        if method == "controller.configure":
            instance.config = self._configure(instance, params.get("config"))
            return {"config": instance.config}
        if method == "controller.setOutputCount":
            instance.set_output_count(int(params["outputCount"]))
            return {}
        if method == "controller.start":
            instance.start()
            instance.running, instance.paused = True, False
            return {}
        if method == "controller.stop":
            instance.running = False
            instance.stop()
            return {}
        if method == "controller.pause":
            instance.paused = True
            return {}
        if method == "controller.resume":
            instance.paused = False
            return {}
        if method == "controller.destroy":
            self._instances.pop(params.get("instanceId"), None)
            if instance.running:
                instance.running = False
                instance.stop()
            instance.destroy()
            return {}
        raise _MethodNotFound()

    def _create(self, params):
        type_id = params.get("typeId")
        cls = next((c for t, c in self._types if t == type_id), None)
        if cls is None:
            untyped = [c for t, c in self._types if t is None]
            cls = untyped[0] if len(untyped) == 1 else None
        if cls is None:
            raise _MethodNotFound()
        instance = cls(self, params["instanceId"], type_id, int(params.get("outputCount", 0)))
        instance.config = self._configure(instance, params.get("config"))
        self._instances[instance.instance_id] = instance
        answer = {"config": instance.config}
        names = instance.output_names()
        if names is not None:
            answer["outputNames"] = list(names)
        return answer

    def _configure(self, instance, config):
        config = config if isinstance(config, dict) else {}
        schema = self._schema(instance.type_id)
        if schema:
            config = fill_defaults(schema, config)
            errors = validate(schema, config)
            if errors:
                raise ConfigError(errors)
        normalised = instance.configure(config)
        return config if normalised is None else normalised

    def _schema(self, type_id):
        for t in (self.manifest.get("contributes") or {}).get("controllers") or []:
            if t.get("typeId") == type_id:
                return t.get("configSchema")
        return None

    def _call_tool(self, name, arguments):
        handler = self._tools.get(name)
        if handler is None:
            return {"content": [{"type": "text", "text": f"unknown tool {name}"}], "isError": True}
        try:
            result = handler(arguments)
        except ToolError as error:
            return {"content": [{"type": "text", "text": str(error)}], "isError": True}
        if isinstance(result, str):
            return {"content": [{"type": "text", "text": result}]}
        if isinstance(result, dict) and isinstance(result.get("content"), list):
            return result
        return {"content": [{"type": "text", "text": json.dumps(result, ensure_ascii=False)}]}

    def _finish(self):
        if self._finished:
            return
        self._finished = True
        for instance in list(self._instances.values()):
            try:
                if instance.running:
                    instance.running = False
                    instance.stop()
                instance.destroy()
            except Exception:  # noqa: BLE001 - shutting down regardless
                traceback.print_exc(file=sys.stderr)
        self._instances.clear()
        if self._on_shutdown:
            self._on_shutdown()


class _MethodNotFound(Exception):
    pass


class _UnknownInstance(Exception):
    pass


def load_manifest(path=None):
    """vixen-plugin.json from the working folder (Studio starts plugins in their folder), else next to the script."""
    candidates = [path] if path else [os.path.join(os.getcwd(), "vixen-plugin.json"),
                                      os.path.join(os.path.dirname(os.path.abspath(sys.argv[0])), "vixen-plugin.json")]
    for candidate in candidates:
        try:
            with open(candidate, encoding="utf-8") as file:
                return json.load(file)
        except (OSError, ValueError):
            continue
    return {}


# ---- Configuration checks: the configSchema subset Studio checks (crates/studio-plugins/src/schema.rs) --------------

def _types(schema):
    t = schema.get("type")
    return [t] if isinstance(t, str) else [x for x in t if isinstance(x, str)] if isinstance(t, list) else []


def _type_of(value):
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int) or (isinstance(value, float) and value.is_integer()):
        return "integer"
    if isinstance(value, float):
        return "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    return "object"


def _escape(name):
    return name.replace("~", "~0").replace("/", "~1")


def _number(n):
    return str(int(n)) if float(n).is_integer() else str(n)


def _ipv4(text):
    parts = text.split(".")
    return len(parts) == 4 and all(p.isdigit() and len(p) <= 3 and int(p) <= 255 and (p == "0" or not p.startswith("0")) for p in parts)


def _hostname(text):
    labels = text.split(".")
    return 0 < len(text) <= 253 and all(
        0 < len(l) <= 63 and not l.startswith("-") and not l.endswith("-") and all(c.isascii() and (c.isalnum() or c == "-") for c in l)
        for l in labels)


def validate(schema, value, at=""):
    """[{field, message}] for every way value breaks schema (at most 50)."""
    errors = []
    _check(schema, value, at, errors)
    return errors[:50]


def _check(schema, value, at, errors):
    def push(message):
        errors.append({"field": at or "/", "message": message})

    types = _types(schema)
    actual = _type_of(value)
    if types and not any(t == actual or (t == "number" and actual == "integer") for t in types):
        push("must be " + " or ".join(types))
        return
    if isinstance(schema.get("enum"), list) and value not in schema["enum"]:
        push("must be one of " + ", ".join(json.dumps(v) for v in schema["enum"]))
    if "const" in schema and schema["const"] != value:
        push("must be " + json.dumps(schema["const"]))
    if actual in ("integer", "number"):
        if isinstance(schema.get("minimum"), (int, float)) and value < schema["minimum"]:
            push("must be at least " + _number(schema["minimum"]))
        if isinstance(schema.get("maximum"), (int, float)) and value > schema["maximum"]:
            push("must be at most " + _number(schema["maximum"]))
    elif actual == "string":
        if isinstance(schema.get("minLength"), int) and len(value) < schema["minLength"]:
            push(f"must be at least {schema['minLength']} characters")
        if isinstance(schema.get("maxLength"), int) and len(value) > schema["maxLength"]:
            push(f"must be at most {schema['maxLength']} characters")
        if schema.get("format") == "ipv4" and not _ipv4(value):
            push("must be an IPv4 address such as 192.168.1.50")
        if schema.get("format") == "hostname" and not _hostname(value):
            push("must be a host name")
    elif actual == "array":
        if isinstance(schema.get("minItems"), int) and len(value) < schema["minItems"]:
            push(f"must have at least {schema['minItems']} items")
        if isinstance(schema.get("maxItems"), int) and len(value) > schema["maxItems"]:
            push(f"must have at most {schema['maxItems']} items")
        if isinstance(schema.get("items"), dict):
            for i, item in enumerate(value):
                _check(schema["items"], item, f"{at}/{i}", errors)
    elif actual == "object":
        properties = schema.get("properties") if isinstance(schema.get("properties"), dict) else {}
        for name in schema.get("required") or []:
            if isinstance(name, str) and name not in value:
                errors.append({"field": f"{at}/{_escape(name)}", "message": "is required"})
        for name, item in value.items():
            field = f"{at}/{_escape(name)}"
            if name in properties:
                _check(properties[name], item, field, errors)
            elif schema.get("additionalProperties") is False:
                errors.append({"field": field, "message": "is not a known setting"})
            elif isinstance(schema.get("additionalProperties"), dict):
                _check(schema["additionalProperties"], item, field, errors)


def fill_defaults(schema, config):
    """config with each missing property that has a default (or is an object with defaults inside) filled in."""
    out = dict(config)
    for name, prop in (schema.get("properties") or {}).items():
        if not isinstance(prop, dict):
            continue
        if name not in out:
            if "default" in prop:
                out[name] = json.loads(json.dumps(prop["default"]))
            elif "object" in _types(prop) and prop.get("properties"):
                inner = fill_defaults(prop, {})
                if inner:
                    out[name] = inner
        elif isinstance(out[name], dict) and prop.get("properties"):
            out[name] = fill_defaults(prop, out[name])
    return out
