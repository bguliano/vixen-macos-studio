#!/usr/bin/env python3
"""{{NAME}}: a Vixen Studio plugin (plugin API 1.0), made by plugin_scaffold.

Studio starts this file and speaks JSON-RPC 2.0 with it over stdin/stdout, one message per line. Only protocol
messages go to stdout; print anything else to stderr, which Studio keeps in the plugin's log (plugin_logs).
The full protocol: the MCP resource plugins://api and plugins://schema/protocol.

Standard library only. Replace the parts marked TODO.
"""
import base64
import json
import sys
import time

API_VERSION = "1.0.0"
INVALID_CONFIG = -32004
# The settings in the manifest's configSchema. TODO: keep in step with it.
SETTINGS = {"prefix"}

# instanceId -> {"config": {...}, "outputs": int, "running": bool, "last_log": float}
instances = {}


def send(message):
    message["jsonrpc"] = "2.0"
    sys.stdout.write(json.dumps(message, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def log(level, text):
    send({"method": "log", "params": {"level": level, "message": text}})


class ConfigError(Exception):
    """Refuses a configuration; Studio shows each {field, message} next to that field."""

    def __init__(self, errors):
        super().__init__("invalid configuration")
        self.errors = errors


def check_config(config):
    # TODO: check your settings here. The manifest's configSchema has already been checked by Studio.
    errors = [{"field": "/" + key, "message": "is not a setting of this plugin"} for key in config if key not in SETTINGS]
    if len(config.get("prefix", "")) > 40:
        errors.append({"field": "/prefix", "message": "is longer than 40 characters"})
    if errors:
        raise ConfigError(errors)
    return config


def values_of(params):
    """A frame's values as a list: ints 0-255 for u8, 0-65535 for u16, or the json items as they are."""
    encoding, values = params["encoding"], params["values"]
    if encoding == "u8":
        return list(base64.b64decode(values))
    if encoding == "u16":
        raw = base64.b64decode(values)
        return [int.from_bytes(raw[i:i + 2], "little") for i in range(0, len(raw), 2)]
    return values


def on_frame(instance, values):
    # TODO: send the values to your device. This example prints them to the log at most twice a second.
    now = time.monotonic()
    if now - instance["last_log"] >= 0.5:
        instance["last_log"] = now
        print(instance["config"].get("prefix", "") + " ".join(str(v) for v in values[:32]), file=sys.stderr, flush=True)


def call_tool(name, arguments):
    # TODO: your agent tools, as listed in the manifest's contributes.tools.
    if name == "hello":
        who = arguments.get("name") or "there"
        return {"content": [{"type": "text", "text": f"Hello, {who}, from {{NAME}}."}]}
    return {"content": [{"type": "text", "text": f"unknown tool {name}"}], "isError": True}


def handle(method, params):
    """Answers one request; raising ConfigError or KeyError becomes a JSON-RPC error."""
    if method == "initialize":
        log("info", f"{{NAME}} ready (muted={params.get('muted')})")
        return {"apiVersion": API_VERSION, "capabilities": {}}
    if method == "shutdown":
        return {}
    if method == "tool.call":
        return call_tool(params["name"], params.get("arguments") or {})
    instance_id = params.get("instanceId")
    if method == "controller.create":
        config = check_config(params.get("config") or {})
        instances[instance_id] = {"config": config, "outputs": params.get("outputCount", 0), "running": False, "last_log": 0.0}
        return {"config": config}
    instance = instances[instance_id]
    if method == "controller.configure":
        instance["config"] = check_config(params.get("config") or {})
        return {"config": instance["config"]}
    if method == "controller.setOutputCount":
        instance["outputs"] = params["outputCount"]
        return {}
    if method in ("controller.start", "controller.resume"):
        instance["running"] = True  # TODO: connect to your device here.
        return {}
    if method in ("controller.stop", "controller.pause"):
        instance["running"] = False
        return {}
    if method == "controller.destroy":
        instances.pop(instance_id, None)
        return {}
    raise NotImplementedError(method)


def main():
    for line in sys.stdin:
        try:
            message = json.loads(line)
        except ValueError:
            continue
        method, params, request_id = message.get("method"), message.get("params") or {}, message.get("id")
        if method is None:
            continue  # an answer to a request of ours; this plugin sends none
        if method == "controller.frame":
            instance = instances.get(params.get("instanceId"))
            if instance and instance["running"]:
                on_frame(instance, values_of(params))
            continue
        if request_id is None:
            continue  # other notifications, such as events
        try:
            send({"id": request_id, "result": handle(method, params)})
        except ConfigError as error:
            send({"id": request_id, "error": {"code": INVALID_CONFIG, "message": str(error), "data": {"errors": error.errors}}})
        except NotImplementedError:
            send({"id": request_id, "error": {"code": -32601, "message": f"{method} is not implemented"}})
        except Exception as error:  # noqa: BLE001 - report every failure to Studio instead of exiting
            send({"id": request_id, "error": {"code": -32603, "message": f"{type(error).__name__}: {error}"}})
        if method == "shutdown":
            return


if __name__ == "__main__":
    main()
