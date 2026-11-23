#!/usr/bin/env python3
"""Verify the preview designer's Edit menu items are in the menu bar while the designer is in front.

Usage: check_designer_menus.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/designer-menus.json on screen with the designer for "Christmas Tree 2019" and reads the harness's menu
dumps. The sequencer replaces Edit's pasteboard group, and SwiftUI then drops other scenes' groups placed after it, so
these items once went missing. Checks:
- with the designer in front, Edit lists Lock (⌘L), Unlock (⌘U), Unlock All (⌥⌘U), Hide Locked, Background
  Properties... and Templates (Legacy) once each, after Paste;
- with no designer open (before it opens and after it closes), Edit lists none of them;
- the designer's View items (Prop Information ... Float Properties Panel), its Settings menu and its Help items show,
  enabled, only with the designer in front, and Help lists Vixen YouTube Channel once either way;
- no top-level menu shows empty: the prop editor's Prop and the designer's Settings are hidden with neither in front.
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


def top_menus(name):
    """Each top-level menu: (hidden, [its item lines, without the indent])."""
    menus = {}
    current = None
    for line in (out / f"{name}.txt").read_text().splitlines():
        if not line.startswith(" "):
            current = line.removesuffix(" (hidden)")
            menus[current] = (line.endswith(" (hidden)"), [])
        elif line.startswith("  ") and not line.startswith("   "):
            menus[current][1].append(line.strip())
    return menus


def visible(lines):
    return [line for line in lines if line != "---" and not line.endswith(" (hidden)")]


view_items = ["Prop Information", "Prop Editor", "Elements", "Properties", "Float Elements Panel", "Float Properties Panel"]
settings_items = ["Update Location Properties", "Location Offset Setup", "Use OpenGL Preview", "Keep Insert Mode Active"]
help_items = ["View Help [?]", "Template Help", "Custom Prop Help"]
plain = lambda line: line.removesuffix(" (checked)")
for name in ("menus-admin", "menus-designer", "menus-closed"):
    menus = top_menus(name)
    empty = [title for title, (hidden, lines) in menus.items() if title and not hidden and not visible(lines)]
    assert not empty, (name, "empty menus show", empty)
    help_menu = visible(menus["Help"][1])
    assert [plain(line) for line in help_menu].count("Vixen YouTube Channel") == 1, (name, help_menu)
    designer = name == "menus-designer"
    view = [plain(line) for line in visible(menus["View"][1])]
    for item in view_items:
        assert (view.count(item) == 1) == designer, (name, item, view)
    help_titles = [plain(line) for line in help_menu]
    for item in help_items:
        assert (help_titles.count(item) == 1) == designer, (name, item, help_menu)
    settings = menus.get("Settings")
    if designer:
        assert settings and not settings[0], (name, settings)
        assert [plain(line) for line in visible(settings[1])] == settings_items, (name, settings)
        assert not [line for line in view + help_titles if line.endswith(" (disabled)") and line.removesuffix(" (disabled)") in view_items + help_items], (name, view, help_menu)
    else:
        assert settings is None or settings[0], (name, "Settings shows", settings)
        prop = menus.get("Prop")
        assert prop is None or prop[0], (name, "Prop shows", prop)
print("designer menus: OK", out)
