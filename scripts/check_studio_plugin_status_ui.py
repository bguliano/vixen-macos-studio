#!/usr/bin/env python3
"""Look at a plugin controller's health in the Controllers list and its inspector (backlog B68), on a copy of
target/show-copy with every other controller deleted and a throwaway plugin folder.

Usage: check_studio_plugin_status_ui.py [Vixen.app]   (default build/Vixen.app; needs target/debug/studio-plugin-fixture,
built by `cargo build -p studio-plugins --bin studio-plugin-fixture`)
Harness scripts: scripts/ui/studio-plugin-status-*.json. Harness snapshots and Studio Capture shots land in
target/ui-plugin-status/out; look at them.

The harness always runs muted, so the live states (unavailable while the plugin looks for its devices, then sending; a
refused configuration as an error) are checked by the studio-plugins test a_passing_unavailable_status_is_not_a_send_error
on the same controller_overview the list shows. Here: the Output column wraps a long reason to two lines instead of
"Nothing p…" (00-unpatched), and the plugin controller's inspector has a status line (01-patched).
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
WORK = ROOT / "target/ui-plugin-status"
FIXTURE = ROOT / "target/debug/studio-plugin-fixture"
CAPTURE = Path.home() / "Applications/Studio Capture.app/Contents/MacOS/studio-capture"
TYPE_ID = "0a6c1f2e-3b4d-4c5e-8f90-123456789abc"


def agent(app: Path, requests):
    """One headless MCP connection on the show copy (the app isn't running): each (tool, arguments)."""
    lines = [{"jsonrpc": "2.0", "id": 0, "method": "initialize",
              "params": {"protocolVersion": "2024-11-05", "clientInfo": {"name": "status-check"}, "capabilities": {}}}]
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


def prepare(app: Path) -> str:
    """The show copy, the fixture plugin's folder, and the id of the element to patch."""
    shutil.rmtree(WORK, ignore_errors=True)
    for folder in ("out", "Plugins", "dev/lamp"):
        (WORK / folder).mkdir(parents=True)
    shutil.copytree(ROOT / "target/show-copy", WORK / "show", symlinks=True)
    schema = {"type": "object", "additionalProperties": False,
              "properties": {"host": {"type": "string", "format": "ipv4", "title": "Address", "default": "10.0.0.30", "x-order": 1},
                             "bad": {"type": "boolean", "title": "Refuse This", "x-order": 2}}}
    lamp = {"id": "com.example.lamp", "name": "Lamp Output", "version": "1.2.0", "apiVersion": "^1.0",
            "entry": {"command": str(FIXTURE), "args": ["--find-devices-after=150"]}, "permissions": {"network": ["udp:lan"]},
            "contributes": {"controllers": [{"typeId": TYPE_ID, "name": "Lamp", "configSchema": schema, "defaultOutputs": 3}]}}
    (WORK / "dev/lamp/vixen-plugin.json").write_text(json.dumps(lamp, indent=1))
    # Only the fixture's controller: nothing else in the list.
    [listed] = agent(app, [("controller_list", {})])
    controllers = listed["items"]
    answers = agent(app, [("controller_delete", {"id": c["id"], "confirm": True}) for c in controllers] + [
        ("element_template", {"kind": "megatree", "name": "Status Tree", "count": 1, "pixels": 1, "color": "rgb"}),
        ("profile_save", {"confirm": True})])
    return answers[-2]["result"]["id"]


def script(name: str) -> Path:
    path = WORK / f"{name}.json"
    path.write_text((ROOT / f"scripts/ui/studio-plugin-status-{name}.json").read_text().replace("@WORK@", str(WORK)))
    return path


def run(app: Path, name: str, shots) -> None:
    out = WORK / "out"
    env = dict(os.environ, VIXEN_STUDIO_PLUGINS=str(WORK / "Plugins"), VIXEN_UI_TEST=str(script(name)), VIXEN_UI_TEST_OUT=str(out),
               VIXEN_DATA=str(WORK / "show"), VIXEN_UI_TEST_ONSCREEN="1", VIXEN_MUTE="1")
    (out / "result.json").unlink(missing_ok=True)
    process = subprocess.Popen([str(app / "Contents/MacOS/Vixen")], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    shots = list(shots)
    deadline = time.monotonic() + 300
    try:
        while process.poll() is None and time.monotonic() < deadline:
            if shots and (out / f"{shots[0]}.png").is_file():
                time.sleep(1)
                if CAPTURE.is_file():
                    subprocess.run([str(CAPTURE), "--owner", "Vixen", "--title", "Vixen Studio", "--out", str(out / f"capture-{shots[0]}.png")],
                                   timeout=20, check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                shots.pop(0)
            time.sleep(0.2)
        if process.poll() is None:
            raise AssertionError(f"the {name} run didn't finish")
    finally:
        if process.poll() is None:
            process.kill()
    steps = json.loads((out / "result.json").read_text())
    failed = [s for s in steps if not s.get("ok")]
    assert not failed, failed
    (out / f"result-{name}.json").write_text(json.dumps(steps, indent=1))


def states(name: str):
    steps = json.loads((WORK / "out" / f"result-{name}.json").read_text())
    return [s.get("detail", "") for s in steps if s["action"] == "studioControllersState"]


if __name__ == "__main__":
    app = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "build/Vixen.app"
    assert FIXTURE.is_file(), "build the fixture first: cargo build -p studio-plugins --bin studio-plugin-fixture"
    tree = prepare(app)
    # 1. Link the plugin and add its controller, unpatched: the Output column's warning.
    run(app, "add", ["00-unpatched"])
    [overview] = agent(app, [("controller_overview", {})])
    rows = overview["controllers"]
    assert [r["name"] for r in rows] == ["Govee"], rows
    agent(app, [("patch_elements", {"elementId": tree, "controllerId": rows[0]["id"]}), ("profile_save", {"confirm": True})])
    # 2. Patched, selected: the inspector's status line.
    run(app, "patched", ["01-patched"])
    added, patched = states("add")[-1], states("patched")[-1]
    print(added, patched, sep="\n")
    assert added.startswith("Govee=running,-,,0/3,warning"), added
    assert patched.startswith("Govee=running,-,,3/3,off") and "selected=Govee" in patched, patched
    leftover = subprocess.run(["pgrep", "-f", str(FIXTURE)], capture_output=True, text=True).stdout.strip()
    assert not leftover, f"plugin processes left running: {leftover}"
    print("plugin status UI checks passed; screenshots in", WORK / "out")
