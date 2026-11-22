#!/usr/bin/env python3
"""Verify the plugin UI (P4, studio/PLUGINS-DESIGN.md) on a copy of target/show-copy, with a throwaway plugin folder.

Usage: check_studio_plugins_ui.py [Vixen.app]   (default build/Vixen.app; needs target/debug/studio-plugin-fixture,
built by `cargo build -p studio-plugins --bin studio-plugin-fixture`)
Harness script: scripts/ui/studio-plugins-ui.json. Screenshots land in target/ui-plugins/out; look at them.

Checks:
- a show whose StudioPlugins.json needs a plugin that isn't installed shows the missing-plugin banner;
- Settings ▸ Plugins links a development folder, which starts; the install sheet lists the package's permissions;
  Install installs and starts it; the log sheet opens;
- the selected running plugin's details show Sandboxed, and Copy Profile puts its sandbox-exec profile on the pasteboard;
- the Plugins menu lists the plugin's command under its name, then Manage Plugins…;
- Add ▸ Lamp adds a plugin controller; its inspector shows the configSchema form; an invalid address stays in the form
  with its error and isn't applied; valid edits apply; each edit is one Undo step.
The run never touches ~/Library/Application Support/Vixen Studio/Plugins (VIXEN_STUDIO_PLUGINS points elsewhere).
"""
import json
import os
import re
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WORK = ROOT / "target/ui-plugins"
FIXTURE = ROOT / "target/debug/studio-plugin-fixture"


def prepare() -> Path:
    shutil.rmtree(WORK, ignore_errors=True)
    for folder in ("out", "Plugins", "dev/lamp", "pkg"):
        (WORK / folder).mkdir(parents=True)
    # Media stays a link back to the original show (read only); nothing here writes to it.
    shutil.copytree(ROOT / "target/show-copy", WORK / "show", symlinks=True)
    (WORK / "show/SystemData/StudioPlugins.json").write_text(json.dumps({"plugins": [{"id": "io.github.someone.govee-output", "versionRange": "^1.2"}]}))
    schema = {"type": "object", "additionalProperties": False, "required": ["host"], "properties": {
        "host": {"type": "string", "format": "ipv4", "title": "Address", "description": "The lamp hub's IP address.", "x-order": 1},
        "port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 4048, "x-order": 2},
        "colorOrder": {"enum": ["rgb", "grb", "bgr"], "x-enumNames": ["RGB", "GRB", "BGR"], "default": "rgb", "x-order": 3},
        "brightnessLimit": {"type": "number", "minimum": 0, "maximum": 1, "default": 1, "x-order": 4},
        "fast": {"type": "boolean", "title": "Fast mode", "x-order": 5},
        "devices": {"type": "array", "maxItems": 8, "x-order": 6, "items": {"type": "object", "properties": {
            "name": {"type": "string"}, "universe": {"type": "integer", "minimum": 1}, "on": {"type": "boolean", "default": True}}}},
        "tags": {"type": "array", "items": {"type": "string"}, "x-order": 7}}}
    lamp = {"id": "com.example.lamp", "name": "Lamp Output", "version": "1.2.0", "apiVersion": "^1.0",
            "description": "Drives LAN lamps as a Vixen controller (test fixture).",
            "entry": {"command": str(FIXTURE), "args": []}, "permissions": {"network": ["udp:lan"], "files": ["plugin-data"]},
            "contributes": {"controllers": [{"typeId": "0a6c1f2e-3b4d-4c5e-8f90-123456789abc", "name": "Lamp", "configSchema": schema, "defaultOutputs": 3}],
                            "tools": [{"name": "echo", "description": "Echo the arguments.", "readOnly": True}],
                            "commands": [{"id": "say", "title": "Say Hello…", "tool": "echo"}]}}
    (WORK / "dev/lamp/vixen-plugin.json").write_text(json.dumps(lamp, indent=1))
    porch = {"id": "io.github.someone.porch-output", "name": "Porch Output", "version": "0.3.1", "apiVersion": "^1.0", "author": "Someone",
             "description": "Sends colours to a porch light over the LAN and a cloud API.", "entry": {"command": str(FIXTURE)},
             "permissions": {"network": ["udp:lan", "https:api.example.com"], "files": ["plugin-data"], "display": "read"},
             "contributes": {"controllers": [{"typeId": "1b7d2f3e-4c5d-4e6f-8a90-223456789abc", "name": "Porch Light"}],
                             "tools": [{"name": "discover", "description": "Find porch lights."}]}}
    with zipfile.ZipFile(WORK / "pkg/porch.vixenplugin", "w") as package:
        package.writestr("vixen-plugin.json", json.dumps(porch))
    script = (ROOT / "scripts/ui/studio-plugins-ui.json").read_text().replace("@WORK@", str(WORK))
    (WORK / "script.json").write_text(script)
    return WORK / "script.json"


def run(app: Path, script: Path) -> None:
    env = dict(os.environ, VIXEN_STUDIO_PLUGINS=str(WORK / "Plugins"), VIXEN_UI_TEST=str(script),
               VIXEN_UI_TEST_OUT=str(WORK / "out"), VIXEN_DATA=str(WORK / "show"))
    subprocess.run([str(app / "Contents/MacOS/Vixen")], env=env, timeout=300, check=False,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def check() -> None:
    steps = json.loads((WORK / "out/result.json").read_text())
    failed = [s for s in steps if not s.get("ok")]
    assert not failed, failed
    hooks = [s.get("detail", "") for s in steps if s["action"] == "hook"]
    banner = next(s["detail"] for s in steps if s["action"] == "studioPluginBanner")
    assert banner == "Needs plugin io.github.someone.govee-output ^1.2 — not installed", banner
    lists = [h for h in hooks if h.startswith("com.example")]
    assert lists[0] == "com.example.lamp=running", lists
    assert lists[1] == "com.example.lamp=running,io.github.someone.porch-output=running", lists
    sandbox = next(h for h in hooks if h.startswith(("sandboxed", "unsandboxed", "none")))
    assert re.fullmatch(r"sandboxed profile=[1-9][0-9]*", sandbox), sandbox
    copied = subprocess.run(["pbpaste"], capture_output=True, text=True).stdout
    assert "sandbox for com.example.lamp" in copied and "(allow default)" in copied, copied[:200]
    # The install sheet's text isn't in the accessibility tree a test window gives; 03-install-sheet.png shows it.
    assert (WORK / "out/03-install-sheet.png").is_file()
    menus = (WORK / "out/06-menus.txt").read_text()
    assert re.search(r"^Plugins\n  Lamp Output\n    Say Hello…\n  ---\n  Manage Plugins…", menus, re.M), menus
    forms = [h for h in hooks if h.startswith("config=")]
    config = [json.loads(re.match(r"config=(\{.*?\}) draft=", f).group(1)) for f in forms]
    # state, bad host, good host, bad port, good port, fast, undo, undo
    assert len(forms) == 8, forms
    assert "host" not in config[1] and "/host must be an IPv4 address" in forms[1], forms[1]
    assert config[2]["host"] == "10.0.0.30", forms[2]
    assert config[3]["port"] == 4048 and "/port must be at most 65535" in forms[3], forms[3]
    assert config[4]["port"] == 5000 and config[5]["fast"] is True, forms[5]
    assert "fast" not in config[6] and config[6]["port"] == 5000, forms[6]
    assert config[7]["port"] == 4048 and config[7]["host"] == "10.0.0.30", forms[7]
    leftover = subprocess.run(["pgrep", "-f", str(FIXTURE)], capture_output=True, text=True).stdout.strip()
    assert not leftover, f"plugin processes left running: {leftover}"


if __name__ == "__main__":
    app = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "build/Vixen.app"
    assert FIXTURE.is_file(), f"build the fixture first: cargo build -p studio-plugins --bin studio-plugin-fixture"
    run(app, prepare())
    check()
    print("plugin UI checks passed; screenshots in", WORK / "out")
