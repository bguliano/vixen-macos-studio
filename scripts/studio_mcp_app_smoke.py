#!/usr/bin/env python3
"""Prove the packaged app serves its own core; empty disposable profile, no sound/hardware."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import uuid

root = Path(__file__).resolve().parents[1]
app = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "target/build-mcp/Vixen.app"
work = root / "target" / ("studio-app-mcp-smoke-" + uuid.uuid4().hex[:12])
profile = work / "profile"
profile.mkdir(parents=True)
script = work / "script.json"
script.write_text(json.dumps({"steps": [{"action": "window", "id": "administration"}, {"action": "wait", "seconds": 15}, {"action": "closeWindow"}, {"action": "wait", "seconds": 20}]}))
env = dict(os.environ, VIXEN_MUTE="1", VIXEN_DATA=str(profile), VIXEN_UI_TEST=str(script), VIXEN_UI_TEST_OUT=str(work / "harness"))
hash_value = 0xcbf29ce484222325
for byte in os.fsencode(profile.resolve()):
    hash_value = ((hash_value ^ byte) * 0x100000001b3) & ((1 << 64) - 1)
socket = Path.home() / "Library/Application Support/Vixen Studio" / ("mcp-%016x.sock" % hash_value)
with (work / "app.log").open("w") as log:
    process = subprocess.Popen([str(app / "Contents/MacOS/Vixen")], env=env, stdout=log, stderr=log)
    try:
        for _ in range(200):
            if socket.exists():
                break
            if process.poll() is not None:
                raise RuntimeError("App exited before serving MCP; inspect " + str(work / "app.log"))
            time.sleep(0.1)
        else:
            raise RuntimeError("App did not expose its MCP socket; inspect " + str(work / "app.log"))
        requests = [
            {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05", "clientInfo": {}, "capabilities": {}}},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "studio_status"}},
            {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "element_create", "arguments": {"name": "Socket smoke"}}},
            {"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "display_undo"}},
            {"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "studio_status"}},
        ]
        reply = subprocess.run([str(app / "Contents/Helpers/vixen-studio-mcp"), "--profile", str(profile)], input="".join(json.dumps(r) + chr(10) for r in requests), text=True, capture_output=True, env=env, timeout=10)
        if reply.returncode:
            raise RuntimeError(reply.stderr)
        responses = [json.loads(line) for line in reply.stdout.splitlines()]
        assert len(responses) == 5, responses
        results = []
        for response in responses[1:]:
            assert response.get("result", {}).get("isError") is False, response
            results.append(json.loads(response["result"]["content"][0]["text"]))
        assert results[0]["mode"] == "liveSession", results
        assert results[1]["changed"] and results[2]["changed"], results
        assert results[0]["elements"] == results[3]["elements"], results
        assert results[3]["liveOutputAllowed"] is False, results
        process.wait(timeout=30)
        assert process.returncode == 0, process.returncode
        assert not socket.exists(), "App shutdown left its socket"
        (work / "mcp-result.json").write_text(json.dumps({"passed": True, "results": results}, indent=2))
        print("Packaged app live socket, shared mutation/undo, default-off output and socket cleanup: PASS")
        print("Evidence: " + str(work))
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
