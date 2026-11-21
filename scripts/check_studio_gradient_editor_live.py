#!/usr/bin/env python3
"""Verify the Color Gradient Editor is live, with Undo and Done instead of OK/Cancel (backlog B27).

Usage: check_studio_gradient_editor_live.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/studio-gradient-editor-live.json on screen. Drops a Pulse effect in Christmas Can-Can and opens its
Color Gradient's editor from the effect editor. Checks:
- the editor has Done and no OK or Cancel button;
- each change (the first stop's colour, then its location) reaches the effect at once, each is one step on the sheet's
  Edit > Undo (sent through the key window's responder chain, as the menu does), and Undo and Redo walk them, with the
  effect following;
- the sequence's Undo holds the whole visit as one step; Done closes; reopening and pressing Esc unchanged closes without
  a step; the sequence's Undo then takes the effect back to its original gradient.
Snapshots: 01-gradient-editor.png, 02-gradient-editor-changed.png, 03-reopened.png.
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
work = root / "target" / ("studio-gradient-editor-live-" + uuid.uuid4().hex[:8])
out = work / "out"
profile = work / "show"
out.mkdir(parents=True)
subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)
script = work / "script.json"
script.write_text((root / "scripts/ui/studio-gradient-editor-live.json").read_text().replace("@OUT@", str(out)))
env = dict(os.environ, VIXEN_MUTE="1", VIXEN_DATA=str(profile), VIXEN_UI_TEST=str(script),
           VIXEN_UI_TEST_OUT=str(out), VIXEN_UI_TEST_ONSCREEN="1")

with (work / "app.log").open("w") as log:
    process = subprocess.Popen([str(app / "Contents/MacOS/Vixen")], env=env, stdout=log, stderr=log)
    try:
        process.wait(timeout=300)
    finally:
        if process.poll() is None:
            process.kill()

result = json.loads((out / "result.json").read_text())
steps = json.loads(script.read_text())["steps"]
# The editor is asked for OK and Cancel, which must not be there (the click finds visible text).
gone = [s for s in result if steps[s["step"]]["action"] == "click" and steps[s["step"]].get("label") in ("OK", "Cancel")]
assert len(gone) == 2 and all(not s.get("ok") for s in gone), "the gradient editor showed OK or Cancel: " + str(gone)
closed = [s for s in result if steps[s["step"]]["action"] == "hook" and s.get("detail") == "FAIL no gradient editor"]
assert len(closed) == 2, "Done and Esc close the editor: " + str(closed)
failed = [s for s in result if not s.get("ok") and s not in gone and s not in closed]
assert not failed, failed


def details(name, verb=None):
    found = []
    for s in result:
        step = steps[s["step"]]
        if step["action"] != "hook" or step.get("name") != name:
            continue
        if verb and step.get("path", [None])[0] != verb:
            continue
        found.append(s.get("detail", ""))
    return found


def field(text, name):
    return re.search(rf"\b{name}=(.*?)(?= \w+=|$)", text).group(1)


before, after_color, after_location, after_undo, after_done, after_sequence_undo = details("studio.effectGradient", "state")
original = field(before, "stops")
first = original.split(" ")[0]
recolored = original.replace(first, "FFFF0000@" + first.split("@")[1], 1)
assert field(after_color, "stops") == recolored, "the colour reached the effect at once: " + after_color + " from " + before
assert "FFFF0000@40" in field(after_location, "stops"), "the location reached the effect at once: " + after_location
assert field(after_undo, "stops") == recolored, "the sheet's Undo reached the effect: " + after_undo
assert field(after_done, "stops") == field(after_location, "stops"), after_done
assert field(after_sequence_undo, "stops") == original, "the sequence's Undo took the visit back: " + after_sequence_undo

opened, undone, redone, after_done_editor, after_esc_editor = details("studio.gradientEditor", "state")
assert field(opened, "changed") == "false" and field(opened, "undo") == "-", opened
assert field(undone, "undo") == "Edit Gradient" and field(undone, "redo") == "Edit Gradient", "two steps, Undo walked one: " + undone
assert "FFFF0000@40" in field(redone, "stops") and field(redone, "redo") == "-", redone
assert after_done_editor == after_esc_editor == "FAIL no gradient editor", "Done and Esc closed the editor"

added, during_1, during_2, after_done_seq, after_esc, after_undo_seq = details("sequencer.state")
assert field(added, "undo") == "Added 1 effect", added
step = field(during_1, "undo")
assert step != "Added 1 effect" and field(during_2, "undo") == step, "the visit is one sequence step: " + during_2
assert field(after_done_seq, "undo") == step, after_done_seq
assert field(after_esc, "undo") == step, "Esc unchanged adds no step: " + after_esc
assert field(after_undo_seq, "undo") == "Added 1 effect", "one Undo takes the whole visit back: " + after_undo_seq
for name in ("01-gradient-editor", "02-gradient-editor-changed", "03-reopened"):
    assert (out / f"{name}.png").exists(), name
print("gradient editor live: OK", out)
