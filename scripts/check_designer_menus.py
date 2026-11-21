#!/usr/bin/env python3
"""Verify the preview designer's Edit menu items are in the menu bar while the designer is in front.

Usage: check_designer_menus.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/designer-menus.json on screen with the designer for "Christmas Tree 2019" and reads the harness's menu
dumps. The sequencer replaces Edit's pasteboard group, and SwiftUI then drops other scenes' groups placed after it, so
these items once went missing. Checks:
- with the designer in front, Edit lists Lock (⌘L), Unlock (⌘U), Unlock All (⌥⌘U), Hide Locked, Background
  Properties... and Templates (Legacy) once each, after Paste;
- with no designer open (before it opens and after it closes), Edit lists none of them.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import uuid

root = Path(__file__).resolve().parents[1]
app = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "build/Vixen.app"
show = Path(sys.argv[2]) if len(sys.argv) > 2 else root / "target/show-copy"
work = root / "target" / ("designer-menus-" + uuid.uuid4().hex[:8])
out = work / "out"
profile = work / "show"
out.mkdir(parents=True)
subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)
env = dict(os.environ, VIXEN_MUTE="1", VIXEN_DATA=str(profile), VIXEN_UI_TEST=str(root / "scripts/ui/designer-menus.json"),
           VIXEN_UI_TEST_OUT=str(out), VIXEN_UI_TEST_ONSCREEN="1")

with (work / "app.log").open("w") as log:
    process = subprocess.Popen([str(app / "Contents/MacOS/Vixen")], env=env, stdout=log, stderr=log)
    try:
        process.wait(timeout=120)
    finally:
        if process.poll() is None:
            process.kill()

result = json.loads((out / "result.json").read_text())
failed = [s for s in result if not s.get("ok")]
assert not failed, failed


def edit_menu(name):
    lines = (out / f"{name}.txt").read_text().splitlines()
    start = lines.index("Edit") + 1
    end = next(i for i in range(start, len(lines)) if not lines[i].startswith(" "))
    return [line.strip() for line in lines[start:end] if line.startswith("  ") and not line.startswith("   ")]


items = ["Lock [l]", "Unlock [u]", "Unlock All [u]", "Hide Locked", "Background Properties...", "Templates (Legacy)"]
edit = edit_menu("menus-designer")
titles = [line.removesuffix(" (disabled)") for line in edit]
for item in items:
    assert titles.count(item) == 1, (item, edit)
positions = [titles.index(item) for item in items]
assert positions == sorted(positions) and positions[0] > titles.index("Paste [v]"), edit
for name in ("menus-admin", "menus-closed"):
    other = [line.removesuffix(" (disabled)") for line in edit_menu(name)]
    assert not set(items) & set(other), (name, other)
print("designer menus: OK", out)
