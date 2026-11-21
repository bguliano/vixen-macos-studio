#!/usr/bin/env python3
"""Verify the prop editor's Edit > Paste Prop Elements as New… is in the menu bar while a prop editor is in front.

Usage: check_prop_editor_menus.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/prop-editor-menus.json on screen and reads the harness's menu dumps. The sequencer replaces Edit's
pasteboard group, and SwiftUI has dropped other scenes' groups placed after it (the preview designer's items went
missing that way), so this guards the prop editor's group after .pasteboard. Checks:
- with a prop editor in front, Edit lists Paste Prop Elements as New… (⇧⌘V) once, after Paste;
- with no prop editor open (before it opens and after it closes), Edit doesn't list it.
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
work = root / "target" / ("prop-editor-menus-" + uuid.uuid4().hex[:8])
out = work / "out"
profile = work / "show"
out.mkdir(parents=True)
subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)
env = dict(os.environ, VIXEN_MUTE="1", VIXEN_DATA=str(profile), VIXEN_UI_TEST=str(root / "scripts/ui/prop-editor-menus.json"),
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
    return [line.strip().removesuffix(" (disabled)") for line in lines[start:end] if line.startswith("  ") and not line.startswith("   ")]


item = "Paste Prop Elements as New… [v]"
edit = edit_menu("menus-prop-editor")
assert edit.count(item) == 1 and edit.index(item) > edit.index("Paste [v]"), edit
for name in ("menus-admin", "menus-closed"):
    assert item not in edit_menu(name), (name, edit_menu(name))
print("prop editor menus: OK", out)
