#!/usr/bin/env python3
"""Verify the show window's Previews pane (it replaced the Setup Previews sheet).

Usage: check_studio_previews.py <UI output directory> <profile the run used>
Harness script: scripts/ui/studio-previews.json, run on a copy of target/show-copy.

Checks, from the pane's state after each step and from the saved profile:
- Add Preview adds "New Preview" and starts renaming it; typing a name and Return renames it;
- Duplicate copies it as "<name> copy"; its On switch turns it off; Delete removes it; each is one undo step, labelled;
- Edit ▸ Undo (the window's undo manager), the pane's Undo button and Edit ▸ Redo walk those steps;
- System ▸ System Configuration ▸ Setup Previews shows this section, not a sheet;
- opening and closing the designer without changes adds no undo step;
- Live Output off closes every preview window, on opens the previews that are on, titled without "(OpenGL)", without FPS;
- the profile on disk has the renamed preview and the copy saved as off.

SwiftUI toolbar buttons act on the harness's press but report that they didn't, so step flags are not checked.
"""
import json
import re
import sys
from pathlib import Path


def check(out: Path, profile: Path) -> None:
    steps = json.loads((out / "result.json").read_text())
    states = [s["detail"] for s in steps if s["action"] == "studioPreviewState"]
    windows = [s["detail"] for s in steps if s["action"] == "windows"]
    section = [s["detail"] for s in steps if s["action"] == "studioSectionNow"]
    assert len(states) == 12, states

    def rows(state: str) -> str:
        return state.split(" | ")[0]

    def field(state: str, name: str) -> str:
        return re.search(rf"\b{name}=(.*?)(?= \w+=|$)", state).group(1)

    initial, added, renamed, duplicated, off, deleted, undo_menu, undo_button, redo, designed, output_off, output_on = states
    assert rows(initial) == "Christmas Tree 2019=off; Christmas Tree 2022 (OpenGL)=on,showing", initial
    assert rows(added).endswith("; New Preview=on,showing") and field(added, "renaming") == "yes", added
    assert field(added, "undo") == "Add Preview", added
    assert rows(renamed).endswith("; Roof Line=on,showing") and field(renamed, "undo") == "Rename Preview", renamed
    assert rows(duplicated).endswith("; Roof Line=on,showing; Roof Line copy=on,showing"), duplicated
    assert field(duplicated, "undo") == "Duplicate Preview", duplicated
    assert rows(off).endswith("; Roof Line copy=off") and field(off, "undo") == "Turn Off Preview", off
    assert "Roof Line copy" not in rows(deleted) and field(deleted, "menu") == "Undo Delete Preview/Redo", deleted
    assert rows(undo_menu) == rows(off) and field(undo_menu, "redo") == "Delete Preview", undo_menu
    assert rows(undo_button) == rows(duplicated) and field(undo_button, "redo") == "Turn Off Preview", undo_button
    assert rows(redo) == rows(off) and field(redo, "undo") == "Turn Off Preview", redo
    assert section == ["previews"], section
    assert "Setup Previews" not in windows[0], windows[0]
    assert "Preview Configuration" in windows[1], windows[1]
    assert field(designed, "undo") == "Turn Off Preview", designed  # the untouched designer added no step
    assert "Christmas Tree 2022" not in windows[2] and "Roof Line" not in windows[2], windows[2]
    assert "showing" not in rows(output_off), output_off
    for title in ("Christmas Tree 2022 ", "Roof Line "):
        assert title in windows[3] + " ", windows[3]
    assert "(OpenGL)" not in windows[3] and "Roof Line copy" not in windows[3], windows[3]
    assert rows(output_on) == rows(off), output_on
    live = (out / "live-window.txt").read_text()
    assert "Lights:" in live and "FPS" not in live, live

    config = (profile / "SystemData" / "SystemConfig.xml").read_text()
    previews = dict(re.findall(r'<Preview name="([^"]*)" typeId="[^"]*" instanceId="([^"]*)"', config))
    assert set(previews) == {"Christmas Tree 2019", "Christmas Tree 2022 (OpenGL)", "Roof Line", "Roof Line copy"}, previews
    disabled = set(re.findall(r'<Device id="([^"]*)"', config.split("<DisabledDevices>")[1]))
    assert previews["Roof Line copy"] in disabled and previews["Roof Line"] not in disabled, disabled
    print("PASS: Previews pane add, rename, duplicate, on/off, delete, undo/redo, designer, live windows and save")


if __name__ == "__main__":
    check(Path(sys.argv[1]), Path(sys.argv[2]))
