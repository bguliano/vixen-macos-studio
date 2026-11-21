#!/usr/bin/env python3
"""Verify the prop editor (Tools) has no OK/Cancel commit flows (backlog B8).

Usage: check_studio_prop_editor_live.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/studio-prop-editor-live.json on screen. Checks:
- Preferences applies and saves each change while the sheet is open, each one step on the sheet's Edit > Undo (sent
  through the sheet window's responder chain, as the menu does), Redo applies it again, Defaults is a step, Done closes;
- Scale Background Image confirms with Scale; the single-name prompts say Rename and Create; the rename sheets say
  Rename and the wire diagram sheet says Export; none of them shows an OK button;
- the Polygon Editor, opened from a Morph effect's Edit Polygons, writes a change to the sequence while it's open
  (saved mid-edit, the file has the new shape name) without an undo step, and Done records one "Polygons" step;
  its window has Done and no OK or Cancel.
Snapshots: 01-preferences.png, 02-scale.png, 05-rename-sheet.png, 09-polygon.png.
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
work = root / "target" / ("studio-prop-editor-live-" + uuid.uuid4().hex[:8])
out = work / "out"
profile = work / "show"
out.mkdir(parents=True)
subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)
script = work / "script.json"
script.write_text((root / "scripts/ui/studio-prop-editor-live.json").read_text().replace("@OUT@", str(out)))
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
# Each SwiftUI sheet is first asked for an OK button, which must not be there (the click finds visible text).
no_ok = [s for s in result if steps[s["step"]].get("label") == "OK"]
assert no_ok and all(not s.get("ok") for s in no_ok), "a sheet showed OK: " + str(no_ok)
failed = [s for s in result if not s.get("ok") and s not in no_ok]
assert not failed, failed


def probes(name, verb):
    return [s["detail"] for s in result if steps[s["step"]].get("name") == name and steps[s["step"]].get("path", [None])[0] == verb]


details = [s.get("detail", "") for s in result if s["action"] == "hook"]
prefs = probes("studio.propPreferences", "state")


def field(text, name):
    return re.search(rf"\b{name}=(\S+)", text).group(1)


start, changed, undone, redone, defaults = prefs
assert field(start, "size") == "3" and field(start, "undo") == "-", start
assert field(changed, "size") == "7" and field(changed, "saved") == "7", "a change applies and saves at once: " + changed
assert field(changed, "undo") == "Change", changed
assert field(undone, "size") == "3" and field(undone, "saved") == "3" and field(undone, "redo") == "Change", undone
assert field(redone, "size") == "7" and field(redone, "saved") == "7", redone
assert field(defaults, "size") == "3" and field(defaults, "undo") == "Change", defaults
assert "size=3" in details, "Done keeps the saved preferences"


def dump(name):
    return (out / f"{name}.txt").read_text()


# The name prompts are alerts, whose buttons the accessibility dump lists.
for name, verb in [("03-rename-prompt", "Rename"), ("04-group-prompt", "Create")]:
    text = dump(name)
    assert f'AXButton "{verb}"' in text and '"OK"' not in text, (name, text)
# Return presses each SwiftUI sheet's default button (Scale, Rename, Export), and what it did happened.
states = probes("props", "state")
assert "undo=Scale Background Image" in states[0], "Scale is one step on the prop's Undo: " + states[0]
assert "NewName-1" in probes("props", "rows")[-1], "the rename sheet's Rename renamed: " + probes("props", "rows")[-1]
assert "undo=Rename" in states[1], "the rename is one step: " + states[1]
notes = (out / "notes.txt").read_text()
assert "wire diagram " in notes, "Export exported the wire diagram"
for line in notes.splitlines():
    if line.startswith("wire diagram "):
        Path(line.removeprefix("wire diagram ")).unlink(missing_ok=True)

opened, edited, closed = probes("polygon", "source")
assert field(opened, "undo") == field(edited, "undo") == "Added", "no undo step while the editor is open: " + edited
assert "undo=1 Effect(s) Polygons modified|Added 1 effect" in closed, "Done is one step: " + closed
assert "Live Shape" in (out / "live-open.tim").read_text(errors="replace"), "the change reached the sequence while open"
print("prop editor live: OK", out)
