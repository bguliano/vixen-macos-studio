#!/usr/bin/env python3
"""Verify the sequencer's Effects button collapses the whole left-hand drawer (B50).

Usage: check_studio_sidebar_drawer.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/studio-sidebar-drawer.json on "Christmas Can-Can", reading the `sequencer.sidebarDrawer` hook:
- the sidebar opens with Effects; View ▸ Layer Editor Window, closed and shown again, reopens it on the left (KEEP F71);
- the toolbar's Effects button (the hook's `toggle`, the same call) then hides the left pane while both tabs stay docked,
  and View offers Show Sidebar (⌃⌘S);
- View ▸ Show Sidebar and Hide Sidebar bring it back at its width and collapse it again;
- closing a left-hand panel from the View menu leaves it collapsed, and showing it again opens the sidebar;
- a sequence opened after the sidebar was collapsed opens collapsed, and the button brings it back.
The user's own setting is put back afterwards. Snapshots sidebar-open.png, sidebar-collapsed.png, reopened-collapsed.png.
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
work = root / "target" / ("studio-sidebar-drawer-" + uuid.uuid4().hex[:8])
out = work / "out"
profile = work / "show"
out.mkdir(parents=True)
subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)
env = dict(os.environ, VIXEN_MUTE="1", VIXEN_DATA=str(profile), VIXEN_UI_TEST=str(root / "scripts/ui/studio-sidebar-drawer.json"),
           VIXEN_UI_TEST_OUT=str(out))

domain, key = "io.github.bguliano.vixen-macos-studio", "studio.sequencerSidebarCollapsed"
saved = subprocess.run(["defaults", "read", domain, key], capture_output=True, text=True)
subprocess.run(["defaults", "delete", domain, key], capture_output=True)
try:
    with (work / "app.log").open("w") as log:
        process = subprocess.Popen([str(app / "Contents/MacOS/Vixen")], env=env, stdout=log, stderr=log)
        try:
            process.wait(timeout=300)
        finally:
            if process.poll() is None:
                process.kill()
finally:
    if saved.returncode == 0:
        subprocess.run(["defaults", "write", domain, key, "-bool", "true" if saved.stdout.strip() == "1" else "false"], check=True)
    else:
        subprocess.run(["defaults", "delete", domain, key], capture_output=True)

result = json.loads((out / "result.json").read_text())
failed = [s for s in result if not s.get("ok")]
assert not failed, failed
reports = iter(s["detail"] for s in result if s["action"] == "hook" and (s.get("detail") or "").startswith("shown="))


def report():
    text = next(reports)
    return text, dict(re.findall(r"(\w+)=(\S+)", text))


def expect(shown, why):
    text, r = report()
    assert r["shown"] == str(shown).lower() and r["paneHidden"] == str(not shown).lower(), why + ": " + text
    return r


start = expect(True, "the sidebar starts open")
opened = expect(True, "the Layer Editor reopens in the sidebar")
tabs = opened["leftTabs"]
assert int(tabs) == int(start["leftTabs"]) + 1 and int(opened["paneWidth"]) >= 180, (start, opened)
for why in ("the Effects button collapses the whole sidebar", "it stays collapsed"):
    r = expect(False, why)
    assert r["collapsed"] == "true" and r["leftTabs"] == tabs, "collapsing keeps every tab docked: " + str(r)
menus = (out / "menus-collapsed.txt").read_text()
assert re.search(r"Show Sidebar \[s\]", menus), "View offers Show Sidebar (⌃⌘S) while collapsed"
r = expect(True, "View ▸ Show Sidebar brings the sidebar back")
assert r["paneWidth"] == opened["paneWidth"], "it comes back at its width: " + str(r)
expect(False, "View ▸ Hide Sidebar collapses it")
r = expect(False, "closing a left-hand panel leaves the sidebar collapsed")
assert int(r["leftTabs"]) == int(tabs) - 1, r
r = expect(True, "showing a left-hand panel opens the sidebar")
assert r["leftTabs"] == tabs, r
expect(False, "the button collapses it again")
expect(False, "a sequence opened afterwards opens collapsed")
expect(True, "the button brings it back")
print("ok: sidebar collapses, comes back and is remembered;", out)
