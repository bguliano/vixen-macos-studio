#!/usr/bin/env python3
"""Verify the preview designer's Edit menu items show in the Previews pane and in the designer's own window (backlog B17).

Usage: check_studio_designer_menus.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/studio-designer-menus.json on screen and reads the harness's menu dumps. Vanilla's
scripts/check_designer_menus.py covers the designer window alone. Checks:
- with the designer showing in the Previews pane, and in its own window, Edit lists Lock (⌘L), Unlock (⌘U), Unlock All
  (⌥⌘U), Hide Locked, Background Properties... and Templates (Legacy) once each, after Paste;
- before the designer opens and after the pane leaves it, Edit lists none of them.
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
work = root / "target" / ("studio-designer-menus-" + uuid.uuid4().hex[:8])
out = work / "out"
profile = work / "show"
out.mkdir(parents=True)
subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)
env = dict(os.environ, VIXEN_MUTE="1", VIXEN_DATA=str(profile), VIXEN_UI_TEST=str(root / "scripts/ui/studio-designer-menus.json"),
           VIXEN_UI_TEST_OUT=str(out), VIXEN_UI_TEST_ONSCREEN="1")

with (work / "app.log").open("w") as log:
    process = subprocess.Popen([str(app / "Contents/MacOS/Vixen")], env=env, stdout=log, stderr=log)
    try:
        process.wait(timeout=150)
    finally:
        if process.poll() is None:
            process.kill()

result = json.loads((out / "result.json").read_text())
failed = [s for s in result if not s.get("ok")]
assert not failed, failed
states = [s["detail"] for s in result if s["action"] == "studioPreviewState"]
assert "designing=Christmas Tree 2019" in states[0] and "designing=-" in states[1], states


def edit_menu(name):
    lines = (out / f"{name}.txt").read_text().splitlines()
    start = lines.index("Edit") + 1
    end = next(i for i in range(start, len(lines)) if not lines[i].startswith(" "))
    return [line.strip().removesuffix(" (disabled)") for line in lines[start:end] if line.startswith("  ") and not line.startswith("   ")]


items = ["Lock [l]", "Unlock [u]", "Unlock All [u]", "Hide Locked", "Background Properties...", "Templates (Legacy)"]
for name in ("menus-pane", "menus-window"):
    titles = edit_menu(name)
    for item in items:
        assert titles.count(item) == 1, (name, item, titles)
    positions = [titles.index(item) for item in items]
    assert positions == sorted(positions) and positions[0] > titles.index("Paste [v]"), (name, titles)
for name in ("menus-admin", "menus-left"):
    titles = edit_menu(name)
    assert not set(items) & set(titles), (name, titles)
print("studio designer menus: OK", out)
