#!/usr/bin/env python3
"""Verify the Dialogs setup sheets have no OK/Cancel commit flows (backlog B9).

Usage: check_studio_dialogs_live.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/studio-dialogs-live.json on screen, in the show window's Display section. Adds a Launcher controller and
opens its Configure. Checks:
- the sheet has Done and no OK, Cancel or Save button; Esc closes it too;
- a change applies to Display Setup's working copy at once and is one step on the sheet's Edit > Undo (sent through the
  key window's responder chain, as the menu does), which Undo and Redo walk; nothing is published while the sheet is up;
- Done publishes the visit as one "Configure Controller" step on the shared history, and the controller has the change
  when the sheet opens again; closing it unchanged adds no step; the show window's Edit > Undo takes the step back.
Snapshots: 01-launcher.png, 02-launcher-changed.png, 03-elexol.png and 04-rds.png (two more controller sheets with Done).
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
work = root / "target" / ("studio-dialogs-live-" + uuid.uuid4().hex[:8])
out = work / "out"
profile = work / "show"
out.mkdir(parents=True)
subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)
script = work / "script.json"
script.write_text((root / "scripts/ui/studio-dialogs-live.json").read_text().replace("@OUT@", str(out)))
env = dict(os.environ, VIXEN_MUTE="1", VIXEN_DATA=str(profile), VIXEN_UI_TEST=str(script),
           VIXEN_UI_TEST_OUT=str(out), VIXEN_UI_TEST_ONSCREEN="1")

with (work / "app.log").open("w") as log:
    process = subprocess.Popen([str(app / "Contents/MacOS/Vixen")], env=env, stdout=log, stderr=log)
    try:
        process.wait(timeout=240)
    finally:
        if process.poll() is None:
            process.kill()

result = json.loads((out / "result.json").read_text())
steps = json.loads(script.read_text())["steps"]
# The sheet is asked for OK, Cancel and Save, which must not be there (the click finds visible text).
gone = [s for s in result if steps[s["step"]]["action"] == "click" and steps[s["step"]].get("label") in ("OK", "Cancel", "Save")]
assert len(gone) == 3 and all(not s.get("ok") for s in gone), "the sheet showed OK, Cancel or Save: " + str(gone)
failed = [s for s in result if not s.get("ok") and s not in gone]
assert not failed, failed


def details(action, name=None, verb=None):
    found = []
    for s in result:
        step = steps[s["step"]]
        if step["action"] != action or (name and step.get("name") != name):
            continue
        if verb and step.get("path", [None])[0] != verb:
            continue
        found.append(s.get("detail", ""))
    return found


def field(text, name):
    return re.search(rf"\b{name}=(.*?)(?= \w+=|$)", text).group(1)


opened, changed, undone, redone, reopened = details("hook", "studio.liveSheet", "state")
assert field(opened, "applied") == "false" and field(opened, "undo") == "-", opened
assert field(changed, "applied") == "true" and field(changed, "changed") == "true", "the change applied at once: " + changed
assert field(changed, "undo") == "Configure Controller", changed
assert field(undone, "applied") == "false" and field(undone, "redo") == "Configure Controller", undone
assert field(redone, "applied") == "true", redone
assert field(reopened, "applied") == "true" and field(reopened, "undo") == "-", "the controller kept the change: " + reopened

assert details("hook", "setup.dirty") == ["dirty"], "the change is in the working copy while the sheet is up"
added, during, after_done, after_unchanged, after_undo = details("displayPaneState")
assert field(added, "undo") == "Edit Controllers", added
assert field(during, "undo") == "Edit Controllers", "nothing publishes while the sheet is up: " + during
assert field(after_done, "undo") == "Configure Controller", "Done publishes one named step: " + after_done
assert field(after_unchanged, "undo") == "Configure Controller", after_unchanged
assert field(after_unchanged, "base") == field(after_done, "base"), "closing unchanged adds no step"
assert field(after_undo, "undo") == "Edit Controllers" and field(after_undo, "redo") == "Configure Controller", after_undo
assert details("hook", "setup.sheetOpen") == ["none", "none"], "Done and Esc close the sheet"
for name in ("03-elexol", "04-rds"):
    assert (out / f"{name}.png").exists(), name
print("dialogs live: OK", out)
