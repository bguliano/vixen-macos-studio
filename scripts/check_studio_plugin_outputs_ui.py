#!/usr/bin/env python3
"""Verify plugin controllers that set their own output count and run settings actions (backlog B61) on a copy of
target/show-copy, with a throwaway plugin folder.

Usage: check_studio_plugin_outputs_ui.py [Vixen.app]   (default build/Vixen.app; needs target/debug/studio-plugin-fixture,
built by `cargo build -p studio-plugins --bin studio-plugin-fixture`)
Harness script: scripts/ui/studio-plugin-outputs-ui.json. Harness snapshots and Studio Capture shots land in
target/ui-plugin-outputs/out; look at them.

Checks, with a fixture plugin whose type declares x-outputCount fromConfig (3 outputs per lamp, named per lamp) and a
Detect Lamps action:
- a new controller starts at its 3 default outputs and the inspector shows the count read-only;
- settings with five lamps resize it to 15 outputs named "Ceiling R", "Ceiling G", …;
- five RGB bulbs and the controller patch 15 to 15;
- settings that drop patched outputs wait for Remove Outputs (Keep Current Settings changes nothing); Remove Outputs
  shrinks to 12, and one Undo brings back 15 outputs and their patches;
- Detect Lamps applies the plugin's merge patch through the same check and resize (the stub's two lamps drop patched
  outputs, so it too waits for Remove Outputs).
The run never touches ~/Library/Application Support/Vixen Studio/Plugins (VIXEN_STUDIO_PLUGINS points elsewhere).
"""
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WORK = ROOT / "target/ui-plugin-outputs"
FIXTURE = ROOT / "target/debug/studio-plugin-fixture"
CAPTURE = Path.home() / "Applications/Studio Capture.app/Contents/MacOS/studio-capture"


def agent(app: Path, requests):
    """One headless MCP connection on the show copy (the app isn't running yet): each (tool, arguments)."""
    lines = [{"jsonrpc": "2.0", "id": 0, "method": "initialize",
              "params": {"protocolVersion": "2024-11-05", "clientInfo": {"name": "outputs-check"}, "capabilities": {}}}]
    lines += [{"jsonrpc": "2.0", "id": i + 1, "method": "tools/call", "params": {"name": n, "arguments": a}} for i, (n, a) in enumerate(requests)]
    reply = subprocess.run([str(app / "Contents/Helpers/vixen-studio-mcp"), "--profile", str(WORK / "show")],
                           input="".join(json.dumps(r) + "\n" for r in lines), text=True, capture_output=True,
                           env=dict(os.environ, VIXEN_MUTE="1", VIXEN_STUDIO_PLUGINS=str(WORK / "Plugins")), timeout=60)
    assert reply.returncode == 0, reply.stderr
    for response in [json.loads(line) for line in reply.stdout.splitlines()][1:]:
        assert response.get("result", {}).get("isError") is False, response


def prepare(app: Path) -> Path:
    shutil.rmtree(WORK, ignore_errors=True)
    for folder in ("out", "Plugins", "dev/lamp"):
        (WORK / folder).mkdir(parents=True)
    # Media stays a link back to the original show (read only); nothing here writes to it.
    shutil.copytree(ROOT / "target/show-copy", WORK / "show", symlinks=True)
    schema = {"type": "object", "additionalProperties": False, "x-outputCount": "fromConfig",
              "x-actions": [{"id": "detect", "title": "Detect Lamps", "tool": "detect", "description": "Find lamps (a stub)."}],
              "properties": {"host": {"type": "string", "format": "ipv4", "title": "Address", "x-order": 1},
                             "lamps": {"type": "array", "title": "Lamps", "x-order": 2, "default": [],
                                       "items": {"type": "object", "additionalProperties": False, "properties": {"name": {"type": "string", "title": "Name"}}}}}}
    lamp = {"id": "com.example.lamp", "name": "Lamp Output", "version": "1.2.0", "apiVersion": "^1.0",
            "entry": {"command": str(FIXTURE), "args": []}, "permissions": {"network": ["udp:lan"]},
            "contributes": {"controllers": [{"typeId": "0a6c1f2e-3b4d-4c5e-8f90-123456789abc", "name": "Lamp", "configSchema": schema, "defaultOutputs": 3}],
                            "tools": [{"name": "detect", "description": "Find lamps.", "readOnly": True}]}}
    (WORK / "dev/lamp/vixen-plugin.json").write_text(json.dumps(lamp, indent=1))
    agent(app, [("element_single_bulbs", {"names": [f"Bulb {n}" for n in range(1, 6)]}), ("profile_save", {"confirm": True})])
    script = Path(os.environ.get("B61_SCRIPT", ROOT / "scripts/ui/studio-plugin-outputs-ui.json")).read_text().replace("@WORK@", str(WORK))
    (WORK / "script.json").write_text(script)
    return WORK / "script.json"


def run(app: Path, script: Path) -> None:
    out = WORK / "out"
    env = dict(os.environ, VIXEN_STUDIO_PLUGINS=str(WORK / "Plugins"), VIXEN_UI_TEST=str(script), VIXEN_UI_TEST_OUT=str(out),
               VIXEN_DATA=str(WORK / "show"), VIXEN_UI_TEST_ONSCREEN="1", VIXEN_MUTE="1")
    process = subprocess.Popen([str(app / "Contents/MacOS/Vixen")], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    shots = ["01-before", "02-fifteen", "03-remove-warning", "05-detect-warning", "06-detected"]
    deadline = time.monotonic() + 300
    try:
        while process.poll() is None and time.monotonic() < deadline:
            # Each harness snapshot is followed by a pause: capture the real window (glass and all) then.
            if shots and (out / f"{shots[0]}.png").is_file():
                time.sleep(1)
                if CAPTURE.is_file():
                    subprocess.run([str(CAPTURE), "--owner", "Vixen", "--title", "Vixen Studio", "--out", str(out / f"capture-{shots[0]}.png")],
                                   timeout=20, check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                shots.pop(0)
            time.sleep(0.2)
        if process.poll() is None:
            raise AssertionError("the run didn't finish")
    finally:
        if process.poll() is None:
            process.kill()


def check() -> None:
    steps = json.loads((WORK / "out/result.json").read_text())
    failed = [s for s in steps if not s.get("ok")]
    assert not failed, failed
    hooks = [s.get("detail", "") for s in steps if s["action"] == "hook"]
    forms = [h for h in hooks if h.startswith("config=")]
    outputs = [int(re.search(r" outputs=(-?\d+)", f).group(1)) for f in forms]
    # state, five lamps, four (refused), keep, four (refused), remove, state after undo, detect (waits), remove
    assert len(forms) == 9, forms
    assert outputs[0] == 3 and outputs[1] == 15, outputs
    assert "These settings remove 3 patched outputs (outputs 13–15 of 15): they need 12 outputs. Their patches go with them. Undo brings them back." in forms[2] and outputs[2] == 15, forms[2]
    assert "pending= message=" in forms[3], forms[3]
    assert outputs[3] == 15 and outputs[5] == 12, outputs
    assert outputs[6] == 15 and '"Lamp"' in forms[6], forms[6]
    assert outputs[7] == 15 and "These settings remove 9 patched outputs" in forms[7] and "message=Found 2 lamps" in forms[7], forms[7]
    assert outputs[8] == 6 and '"lamps":[{"name":"Ceiling"},{"name":"Porch"}]' in forms[8], forms[8]
    stats = [h for h in hooks if h.startswith("total ")]
    assert len(stats) == 3, stats
    assert any(h.startswith("Patching Complete") for h in hooks), hooks
    assert "patched 15 unpatched 0" in stats[1] and "patched 15 unpatched 0" in stats[2], stats
    print("patch summary before patching:", stats[0].split(" | ")[2])
    # The read-only count, Detect Lamps and the warning are static text the dump can't see: look at the captures.
    leftover = subprocess.run(["pgrep", "-f", str(FIXTURE)], capture_output=True, text=True).stdout.strip()
    assert not leftover, f"plugin processes left running: {leftover}"


if __name__ == "__main__":
    app = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "build/Vixen.app"
    assert FIXTURE.is_file(), "build the fixture first: cargo build -p studio-plugins --bin studio-plugin-fixture"
    run(app, prepare(app))
    check()
    print("plugin output-count UI checks passed; screenshots in", WORK / "out")
