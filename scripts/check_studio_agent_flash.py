#!/usr/bin/env python3
"""Verify that agent edits flash on an open sequence's timeline, and the user's own edits don't (UI roadmap R39).

Usage: check_studio_agent_flash.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/studio-agent-flash.json on screen and plays the agent itself, through the app's MCP helper: once the
sequence is open and the user has dropped a Pulse on the first row, it adds a Pulse on the same row, in view, and moves the earliest other effect.
Studio Capture photographs the window while the flash shows and after it has faded. Checks, from `sequencer.agentFlash`:
- opening the sequence flashes nothing, and neither does the user's own drop;
- the agent's edit flashes once: two effects, with tiles on the grid;
- the user undoing it flashes nothing more.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid

root = Path(__file__).resolve().parents[1]
app = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "build/Vixen.app"
show = Path(sys.argv[2]) if len(sys.argv) > 2 else root / "target/show-copy"
work = root / "target" / ("studio-agent-flash-" + uuid.uuid4().hex[:8])
out = work / "out"
profile = work / "show"
out.mkdir(parents=True)
subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)
capture = Path.home() / "Applications/Studio Capture.app/Contents/MacOS/studio-capture"
env = dict(os.environ, VIXEN_MUTE="1", VIXEN_DATA=str(profile), VIXEN_UI_TEST=str(root / "scripts/ui/studio-agent-flash.json"),
           VIXEN_UI_TEST_OUT=str(out), VIXEN_UI_TEST_ONSCREEN="1")


def agent(requests):
    """One MCP connection: initialize, then each (tool, arguments). Returns each tool's result."""
    lines = [{"jsonrpc": "2.0", "id": 0, "method": "initialize",
              "params": {"protocolVersion": "2024-11-05", "clientInfo": {"name": "flash-check"}, "capabilities": {}}}]
    lines += [{"jsonrpc": "2.0", "id": i + 1, "method": "tools/call", "params": {"name": n, "arguments": a}} for i, (n, a) in enumerate(requests)]
    reply = subprocess.run([str(app / "Contents/Helpers/vixen-studio-mcp"), "--profile", str(profile)],
                           input="".join(json.dumps(r) + "\n" for r in lines), text=True, capture_output=True, env=env, timeout=30)
    assert reply.returncode == 0, reply.stderr
    results = []
    for response in [json.loads(line) for line in reply.stdout.splitlines()][1:]:
        assert response.get("result", {}).get("isError") is False, response
        results.append(json.loads(response["result"]["content"][0]["text"]))
    return results


def shoot(name):
    subprocess.run([str(capture), "--owner", "Vixen", "--title", "Christmas Can-Can", "--out", str(out / name)], timeout=20)


with (work / "app.log").open("w") as log:
    process = subprocess.Popen([str(app / "Contents/MacOS/Vixen")], env=env, stdout=log, stderr=log)
    try:
        # The script has the sequence open, the user's drop made and checked by about 11 s; it then waits 30 s.
        time.sleep(18)
        [opened] = agent([("sequence_open", {"file": "Christmas Can-Can.tim"})])
        assert opened["alreadyOpen"], opened
        sequence = opened["sequenceId"]
        [listed] = agent([("sequence_effects", {"sequenceId": sequence})])
        effects = listed["effects"]
        dropped = max(effects, key=lambda e: e["id"])  # the user's Pulse on the first row
        other = next(e for e in sorted(effects, key=lambda e: e["startMs"]) if e["id"] != dropped["id"])
        agent([("effects_batch", {"sequenceId": sequence, "ops": [
            {"op": "add", "typeId": dropped["typeId"], "targetId": dropped["targets"][0], "startMs": 1000, "durationMs": 1500},
            {"op": "timing", "effectId": other["id"], "startMs": other["startMs"] + 100, "durationMs": other["durationMs"]}]})])
        time.sleep(0.45)
        shoot("flashing.png")
        time.sleep(2.5)
        shoot("faded.png")
        process.wait(timeout=60)
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()

steps = json.loads((out / "result.json").read_text())
failed = [s for s in steps if not s.get("ok")]
assert not failed, failed
flashes = [json.loads(s["detail"]) for s in steps if s["action"] == "hook" and '"flashes"' in s.get("detail", "")]
assert len(flashes) == 4, flashes
opened, dropped, agent_edit, undone = flashes
assert opened["flashes"] == 0, opened
assert dropped["flashes"] == 0, "the user's own drop flashed"
assert agent_edit["flashes"] == 1 and agent_edit["lastEffects"] == 2 and agent_edit["lastTiles"] >= 2, agent_edit
assert undone["flashes"] == 1, "Undo flashed"
chrome = [json.loads(s["detail"]) for s in steps if s["action"] == "hook" and '"renderHelp"' in s.get("detail", "")]
assert len(chrome) == 2 and chrome[1]["effects"] == chrome[0]["effects"] - 1, ("Undo didn't take the agent's edit back", chrome)
print("Agent edits flash once, user edits and Undo don't: PASS", json.dumps(agent_edit))
print("Evidence: " + str(out))
shutil.rmtree(profile)
