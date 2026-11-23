#!/usr/bin/env python3
"""Verify Single Bulbs from a plugin controller's lights (backlog B62) on a copy of target/show-copy, with a throwaway
plugin folder.

Usage: check_studio_plugin_lights_ui.py [Vixen.app]   (default build/Vixen.app; needs target/debug/studio-plugin-fixture,
built by `cargo build -p studio-plugins --bin studio-plugin-fixture`)
Harness script: scripts/ui/studio-plugin-lights-ui.json. Harness snapshots and Studio Capture shots land in
target/ui-plugin-lights/out; look at them.

Checks, with a fixture plugin whose type declares x-outputCount fromConfig and lists its lamps as lights (3 outputs each):
- five lamps make a 15-output controller;
- Display ▸ Add ▸ Single Bulbs… with that controller chosen offers Use Govee's Lights (5) (00), which fills in the five lamp
  names; Add Bulbs makes five elements patched 15 to 15 and shows them in the Kitchen preview;
- opened again, the sheet no longer offers the lights (every one is patched).
The run never touches ~/Library/Application Support/Vixen Studio/Plugins (VIXEN_STUDIO_PLUGINS points elsewhere).
"""
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WORK = ROOT / "target/ui-plugin-lights"
FIXTURE = ROOT / "target/debug/studio-plugin-fixture"
CAPTURE = Path.home() / "Applications/Studio Capture.app/Contents/MacOS/studio-capture"
LAMPS = ["Kitchen 1", "Kitchen 2", "Kitchen 3", "Kitchen 4", "Living Room Lamp"]


def agent(app: Path, requests):
    """One headless MCP connection on the show copy (the app isn't running yet): each (tool, arguments)."""
    lines = [{"jsonrpc": "2.0", "id": 0, "method": "initialize",
              "params": {"protocolVersion": "2024-11-05", "clientInfo": {"name": "lights-check"}, "capabilities": {}}}]
    lines += [{"jsonrpc": "2.0", "id": i + 1, "method": "tools/call", "params": {"name": n, "arguments": a}} for i, (n, a) in enumerate(requests)]
    reply = subprocess.run([str(app / "Contents/Helpers/vixen-studio-mcp"), "--profile", str(WORK / "show")],
                           input="".join(json.dumps(r) + "\n" for r in lines), text=True, capture_output=True,
                           env=dict(os.environ, VIXEN_MUTE="1", VIXEN_STUDIO_PLUGINS=str(WORK / "Plugins")), timeout=60)
    assert reply.returncode == 0, reply.stderr
    answers = []
    for response in [json.loads(line) for line in reply.stdout.splitlines()][1:]:
        assert response.get("result", {}).get("isError") is False, response
        answers.append(json.loads(response["result"]["content"][0]["text"]))
    return answers


def prepare(app: Path) -> Path:
    shutil.rmtree(WORK, ignore_errors=True)
    for folder in ("out", "Plugins", "dev/lamp"):
        (WORK / folder).mkdir(parents=True)
    # Media stays a link back to the original show (read only); nothing here writes to it.
    shutil.copytree(ROOT / "target/show-copy", WORK / "show", symlinks=True)
    schema = {"type": "object", "additionalProperties": False, "x-outputCount": "fromConfig",
              "properties": {"host": {"type": "string", "format": "ipv4", "title": "Address", "x-order": 1},
                             "listLights": {"type": "boolean", "title": "List Lights", "default": False, "x-order": 2},
                             "lamps": {"type": "array", "title": "Lamps", "x-order": 3, "default": [],
                                       "items": {"type": "object", "additionalProperties": False, "properties": {"name": {"type": "string", "title": "Name"}}}}}}
    lamp = {"id": "com.example.lamp", "name": "Lamp Output", "version": "1.2.0", "apiVersion": "^1.0",
            "entry": {"command": str(FIXTURE), "args": []}, "permissions": {"network": ["udp:lan"]},
            "contributes": {"controllers": [{"typeId": "0a6c1f2e-3b4d-4c5e-8f90-123456789abc", "name": "Lamp", "configSchema": schema, "defaultOutputs": 3}]}}
    (WORK / "dev/lamp/vixen-plugin.json").write_text(json.dumps(lamp, indent=1))
    agent(app, [("preview_create", {"name": "Kitchen"}), ("profile_save", {"confirm": True})])
    script = (ROOT / "scripts/ui/studio-plugin-lights-ui.json").read_text().replace("@WORK@", str(WORK))
    (WORK / "script.json").write_text(script)
    return WORK / "script.json"


def run(app: Path, script: Path) -> None:
    out = WORK / "out"
    env = dict(os.environ, VIXEN_STUDIO_PLUGINS=str(WORK / "Plugins"), VIXEN_UI_TEST=str(script), VIXEN_UI_TEST_OUT=str(out),
               VIXEN_DATA=str(WORK / "show"), VIXEN_UI_TEST_ONSCREEN="1", VIXEN_MUTE="1")
    process = subprocess.Popen([str(app / "Contents/MacOS/Vixen")], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    shots = ["00-sheet-offer", "01-sheet-lights", "02-after", "03-sheet-all-patched"]
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


def check(app: Path) -> None:
    out = WORK / "out"
    steps = json.loads((out / "result.json").read_text())
    failed = [s for s in steps if not s.get("ok")]
    assert not failed, failed
    stats = [s.get("detail", "") for s in steps if s["action"] == "hook" and s.get("detail", "").startswith("total ")]
    assert len(stats) == 2, stats
    assert "outputs 15 patched 0" in stats[0], stats[0]
    assert stats[1].startswith("total 15 connected 15 unconnected 0 items 5"), stats[1]
    # The names the lights filled in, the patch note and the missing Use … Lights button the second time are static text
    # the dump can't see: look at the captures (00: a "Use Govee's Lights (5)" button; 01: five lamp names, "Each bulb takes its own light's outputs on Govee";
    # 03: no "Use Govee's Lights" button).
    # The saved show: every light patched, each bulb on its own lamp's outputs, all five in the Kitchen preview.
    [overview] = agent(app, [("controller_overview", {})])
    govee = next(c for c in overview["controllers"] if c["name"] == "Govee")
    lights, *ranges = agent(app, [("controller_lights", {"id": govee["id"]})])
    # The headless helper may read the lights from the outputs' names (its plugin host need not start the plugin).
    assert [l["name"] for l in lights["lights"]] == LAMPS and lights["unpatched"] == 0, lights
    [props] = agent(app, [("preview_props", {})])
    assert sum(json.dumps(props).count(f'"{lamp}"') > 0 for lamp in LAMPS) == len(LAMPS), "not every bulb is in a preview"
    leftover = subprocess.run(["pgrep", "-f", str(FIXTURE)], capture_output=True, text=True).stdout.strip()
    assert not leftover, f"plugin processes left running: {leftover}"


if __name__ == "__main__":
    app = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "build/Vixen.app"
    assert FIXTURE.is_file(), "build the fixture first: cargo build -p studio-plugins --bin studio-plugin-fixture"
    run(app, prepare(app))
    check(app)
    print("plugin lights UI checks passed; screenshots in", WORK / "out")
