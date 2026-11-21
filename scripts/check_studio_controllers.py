#!/usr/bin/env python3
"""Verify the show window's Controllers section (the controllers had only Display's right column before).

Usage: check_studio_controllers.py <UI output directory> <profile the run used>
Harness script: scripts/ui/studio-controllers.json, run on a copy of target/show-copy.

Checks, from the section's state after each step and from the saved profile:
- the list shows every controller with run state, address, universes, patched outputs and a health level;
- selecting a row selects that controller in Display's controller tree, and the inspector follows it;
- Start in the inspector marks it running; Display shows the same selection, and coming back keeps it;
- Add ▸ Streaming ACN (E1.31) adds a controller that needs an address (a warning); typing the address, the output
  count and choosing the WLED RGBW preset update the list's address and universes; Undo takes back the preset;
- Move Down from the context menu reorders the list and Display; Find Elements moves to Display;
- Undo and 25 more reopens of the working copy each leave the tree under the section with its rows;
- the profile on disk has the new order and the Tree controller.
"""
import json
import re
import sys
from pathlib import Path


def check(out: Path, profile: Path) -> None:
    steps = json.loads((out / "result.json").read_text())
    failed = [s for s in steps if not s.get("ok") and s["action"] not in ("click",)]
    assert not failed, failed
    states = [s["detail"] for s in steps if s["action"] == "studioControllersState"]
    sections = [s["detail"] for s in steps if s["action"] == "studioSectionNow"]
    assert len(states) == 10, states

    def rows(state: str) -> list[str]:
        return state.split(" | ")[0].split("; ")

    def row(state: str, name: str) -> str:
        return next(r for r in rows(state) if r.startswith(name + "="))

    def field(state: str, name: str) -> str:
        return re.search(rf"\b{name}=(.*?)(?= \w+=|$)", state).group(1)

    initial, selected, started, back, added, addressed, outputs, preset, undone, moved = states
    assert [r.split("=")[0] for r in rows(initial)] == ["Arduino", "Light Strip Controller"], initial
    assert row(initial, "Light Strip Controller") == "Light Strip Controller=stopped,-,,3351/3351,off", initial
    assert int(field(initial, "treeRows")) == 2, initial
    assert field(selected, "selected") == "Light Strip Controller" and field(selected, "tree") == "c1", selected
    assert field(selected, "one") == "true", selected
    assert row(started, "Light Strip Controller").startswith("Light Strip Controller=running"), started
    assert sections[0] == "display", sections
    assert field(back, "selected") == "Light Strip Controller" and field(back, "tree") == "c1", back
    assert row(added, "Tree") == "Tree=running,-,1 universe,0/1,warning", added
    assert field(added, "selected") == "Tree" and "1 needs attention" in field(added, "summary"), added
    assert row(addressed, "Tree") == "Tree=running,10.0.0.60,1 (510 ch),0/1,warning", addressed
    assert row(outputs, "Tree") == "Tree=running,10.0.0.60,1–4 (510 ch),0/1536,warning", outputs
    assert row(preset, "Tree") == "Tree=running,10.0.0.60,1–3 (512 ch),0/1536,warning", preset
    assert row(undone, "Tree") == row(outputs, "Tree"), undone
    # Undo reopens the working copy and rebuilds the tree under the section; it must come back with its rows.
    assert int(field(undone, "treeRows")) == 3, undone
    assert [r.split("=")[0] for r in rows(moved)] == ["Light Strip Controller", "Arduino", "Tree"], moved
    assert field(moved, "selected") == "Arduino" and field(moved, "tree") == "c0", moved
    assert int(field(moved, "treeRows")) == 3, moved
    assert sections[1] == "display", sections
    reopened = next(s["detail"] for s in steps if s["action"] == "studioControllersReopen")
    assert reopened.startswith("treeRows ") and "0" not in reopened.split(" ")[1].split(","), reopened

    config = (profile / "SystemData" / "SystemConfig.xml").read_text()
    names = re.findall(r'<Controller name="([^"]*)"', config)
    assert names == ["Light Strip Controller", "Arduino", "Tree"], names
    assert "10.0.0.60" in (profile / "SystemData" / "ModuleStore.xml").read_text()
    print("PASS: Controllers section list, selection sync with Display, start, add, address, outputs, preset, undo, move and find")


if __name__ == "__main__":
    check(Path(sys.argv[1]), Path(sys.argv[2]))
