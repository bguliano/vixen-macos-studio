#!/usr/bin/env python3
"""Verify VoiceOver labels and keyboard navigation on the custom canvases (UI roadmap R43).

Usage: check_studio_access.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/studio-access.json, which drives each canvas with keys only, through the `studio.access` hook:
- the Display pane's graphical patching (Relays and the Arduino): a layout area of labelled shapes; Tab reaches
  "White Bottom, element, to Output 1"; its menu's Connect To lists the Arduino's outputs and connecting it to Output 2
  takes Output 2 from White Middle, as a drag would (selecting the new line, so Tab Tab reaches White Bottom again);
  Edit ▸ Undo (⌘Z, sent as the menu sends it with the show window key, since a script's window never is) puts it back
  as one step (the pane reloads into the Simple view, so the script picks Graphical again); ⇧Tab moves the selection;
  ⌃Tab leaves;
- the timeline of "Christmas Can-Can": effects read as "Set Level on White Bottom, 0:00.476 to 0:01.497"; ↓ gives the
  grid a row, → and Tab step along it, ⇧→ extends, ⇧Tab steps back, ↑ and ⌥← read the row, ⌃Tab leaves;
- the preview designer for "Christmas Tree 2019": thirteen labelled props; Tab selects the first, Tab Tab ⇧Tab the
  second ("linked to White Bottom") with its properties shown; → → moves it two points; ⌃Tab leaves.
Snapshots graph-keyboard.png, timeline-keyboard.png (designer-keyboard.png is Metal: capture it on screen).
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
work = root / "target" / ("studio-access-" + uuid.uuid4().hex[:8])
out = work / "out"
profile = work / "show"
out.mkdir(parents=True)
subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)
env = dict(os.environ, VIXEN_MUTE="1", VIXEN_DATA=str(profile), VIXEN_UI_TEST=str(root / "scripts/ui/studio-access.json"),
           VIXEN_UI_TEST_OUT=str(out), VIXEN_UI_TEST_ONSCREEN="1")

with (work / "app.log").open("w") as log:
    process = subprocess.Popen([str(app / "Contents/MacOS/Vixen")], env=env, stdout=log, stderr=log)
    try:
        process.wait(timeout=300)
    finally:
        if process.poll() is None:
            process.kill()

result = json.loads((out / "result.json").read_text())
failed = [s for s in result if not s.get("ok")]
assert not failed, failed
answers = [s["detail"] for s in result if s["action"] == "hook" and (s.get("detail") or "").startswith(("focused=", "Connect To", "chose"))]
reports = iter(answers)


def report():
    text = next(reports)
    fields = dict(re.findall(r"(\w+)=(.*?)(?= \w+=|$)", text))
    return text, fields


# The graph
text, r = report()
assert r["focused"] == "1" and r["role"] == "AXLayoutArea" and r["label"] == "Patching graph" and r["items"] == "34", text
text, r = report()
assert r["selected"] == "White Bottom, element, to Output 1" and r["said"] == r["selected"], text
menu = next(reports)
assert menu.startswith("Connect To > Output 1, Output 2,") and "Output 16" in menu, menu
assert next(reports) == "chose Output 2"
undo = [s["detail"] for s in result if s["action"] == "displayMenuUndoRedo"]
assert undo == ["Undo Patching"], undo
text, r = report()
assert r["selected"] == "White Bottom, element, to Output 1, Output 2", "Connect To connected it: " + text
text, r = report()
assert r["focused"] == "1" and r["items"] == "34", "the graph again after Undo: " + text
undone_selection = r["selected"]
text, r = report()
assert r["selected"] and r["selected"] != undone_selection and r["said"] == r["selected"], "⇧Tab moves the selection: " + text
text, r = report()
assert r["focused"] == "0", "⌃Tab leaves the graph: " + text
labels = [line[2:].split(" @ ")[0] for line in (out / "access-graph.txt").read_text().splitlines()]
assert "White Bottom, element, to Output 1" in labels and "White Middle, element, to Output 2" in labels, ("Undo took the connection back", labels[:4])

# The timeline
text, r = report()
assert r["focused"] == "1" and r["label"] == "Timeline" and int(r["items"]) > 0 and r["selected"] == "", text
text, r = report()
assert r["selected"] == "Set Level on White Bottom, 0:00.476 to 0:01.497" and r["said"] == r["selected"], text
text, r = report()
two = r["selected"].split(" | ")
assert len(two) == 2 and all(t.startswith("Set Level on White Bottom, ") for t in two), "Tab, then ⇧→ extends: " + text
text, r = report()
assert r["selected"] == two[0], "⇧Tab steps back to one effect: " + text
text, r = report()
assert r["said"].startswith("Relays, expanded"), "↑ reads the row: " + text
text, r = report()
assert r["said"].startswith("Relays, collapsed"), "⌥← closes it: " + text
text, r = report()
assert r["focused"] == "0", "⌃Tab leaves the timeline: " + text

# The designer
text, r = report()
assert r["focused"] == "1" and r["label"] == "Preview layout" and r["items"] == "13", text
text, r = report()
assert r["selected"] == "Poly Line, 2 lights, not linked", text
text, r = report()
assert r["selected"] == "Poly Line, 161 lights, linked to White Bottom", text
before = [int(v) for v in r["frames"].split(",")]
text, r = report()
after = [int(v) for v in r["frames"].split(",")]
assert after[0] == before[0] + 2 and after[1:] == before[1:], ("→ → moved it two points", before, after)
info = [s["detail"] for s in result if s["action"] == "designerInfo"][-1]
assert "panel=Poly Line Properties" in info, info
text, r = report()
assert r["focused"] == "0", "⌃Tab leaves the designer: " + text
assert next(reports, None) is None

for name in ["graph-tree", "timeline-tree", "designer-tree"]:
    tree = (out / f"{name}.txt").read_text()
    assert "AXLayoutArea" in tree and "AXLayoutItem" in tree, name
print("canvas accessibility: OK", out)
