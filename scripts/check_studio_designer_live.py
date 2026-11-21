#!/usr/bin/env python3
"""Verify the preview designer's dialogs are live with Undo, without OK/Cancel (UI roadmap R41, Preview designer).

Usage: check_studio_designer_live.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/studio-designer-live.json on screen in the designer for "Christmas Tree 2019", snapshotting each sheet
(01-offset-sheet.png, 02-resize-sheet.png, 04-linker-sheet.png, 05-single-sheet.png). Checks:
- Location Offset applies as it's typed, while the sheet is open, as one designer Undo step; Done closes it;
- Resize Background (Return presses its Resize button) is one step, and Undo puts the old background size back;
- Linked Elements changes the shape as it's used (two double-click assignments), all one Undo step, and Done closes it;
- Done closes Location Offset and Linked Elements; the snapshots show no OK button on any of the four sheets
  (Single Bulb Configuration's says Draw Bulbs).
"""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import uuid

root = Path(__file__).resolve().parents[1]
app = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "build/Vixen.app"
show = Path(sys.argv[2]) if len(sys.argv) > 2 else root / "target/show-copy"
work = root / "target" / ("studio-designer-live-" + uuid.uuid4().hex[:8])
out = work / "out"
profile = work / "show"
out.mkdir(parents=True)
subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)
env = dict(os.environ, VIXEN_MUTE="1", VIXEN_DATA=str(profile), VIXEN_UI_TEST=str(root / "scripts/ui/studio-designer-live.json"),
           VIXEN_UI_TEST_OUT=str(out), VIXEN_UI_TEST_ONSCREEN="1")

with (work / "app.log").open("w") as log:
    process = subprocess.Popen([str(app / "Contents/MacOS/Vixen")], env=env, stdout=log, stderr=log)
    try:
        process.wait(timeout=180)
    finally:
        if process.poll() is None:
            process.kill()

result = json.loads((out / "result.json").read_text())
failed = [s for s in result if not s.get("ok")]
assert not failed, failed
info = [s["detail"] for s in result if s["action"] == "designerInfo"]
linker = [s["detail"] for s in result if s["action"] == "designerLinkerInfo"]


def field(text, name):
    return re.search(rf"\b{name}=(\S+)", text).group(1)


def undo(text):
    return re.search(r"undo=(\[.*?\])", text).group(1)


start = info[0]
assert field(start, "offset") == "0.0,0.0", start
assert field(info[1], "offset") == "25.0,0.0" and undo(info[1]).startswith('["Location Offset"'), info[1]
assert field(info[2], "offset") == "0.0,0.0", "Undo puts the offset back"
assert field(info[3], "offset") == "25.0,0.0", "Redo applies it again"
original = field(start, "bg")
assert field(info[4], "bg").startswith("400x") and undo(info[4]).startswith('["Resize Background"'), info[4]
assert field(info[5], "bg") == original, (info[5], original)
assert undo(info[6]).startswith('["Link Elements", "Location Offset"'), "three live changes are one step: " + info[6]
assert linker[0] != linker[1], "the sheet's Reverse reached the links"
assert undo(info[7]).startswith('["Location Offset"'), info[7]
clicks = [s for s in result if s["action"] == "click"]
assert [c["step"] for c in clicks] and all(c["ok"] for c in clicks), "Done and Cancel were where the sheets show them"
print("designer live dialogs: OK", out)
