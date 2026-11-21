#!/usr/bin/env python3
"""Verify the native document shortcuts and the actual saved sequence.

Usage: check_sequencer_commands.py <UI output directory> <saved Ruler.tim>
Harness script: scripts/ui/sequencer-commands.json.
"""
import json
import sys
import xml.etree.ElementTree as ET
from pathlib import Path


def check(root: Path, saved: Path) -> None:
    rows = json.loads((root / "result.json").read_text())
    assert len(rows) == 45 and all(r["ok"] for r in rows), rows
    states = [json.loads(r["detail"]) for r in rows if r.get("detail", "").startswith("{")]
    assert len(states) == 9
    assert [len(s["elements"]) for s in states] == [3, 3, 3, 6, 3, 6, 3, 6, 6]
    assert len(states[1]["selected"]) == len(states[2]["selected"]) == 3
    assert all(s["modified"] for s in states[:-1]) and not states[-1]["modified"]
    assert states[4]["redo"] == states[6]["redo"] == ["Paste 3 effects"]
    assert not states[5]["redo"] and not states[7]["redo"]
    routes = [r["detail"] for r in rows if r.get("detail", "").startswith("posted ")]
    assert len(routes) == 8
    assert all("route=menu" in r for r in routes[:3])
    assert all("route=window undo/redo" in r for r in routes[3:6])
    assert "route=window redo" in routes[6] and "route=window save" in routes[7]
    assert all(e["start"] == 50_000_000 and e["duration"] == 20_000_000 for e in states[-1]["elements"])
    assert len({e["id"] for e in states[-1]["elements"]}) == 6

    document = ET.parse(saved).getroot()
    effects = next(x for x in document if x.tag.split("}")[-1] == "_effectNodeSurrogates")
    assert len(effects) == 6
    assert len({e.findtext("InstanceId") for e in effects}) == 6
    assert all(e.findtext("StartTime") == "PT5S" and e.findtext("TimeSpan") == "PT2S" for e in effects)
    assert all(e.findtext("TypeId") == "cbd76d3b-c924-40ff-bad6-d1437b3dbdc0" for e in effects)
    print("PASS: 45 native steps; Command-A/C/V/Z/Shift-Z/Y/S, exact effect counts, saved XML")


if __name__ == "__main__":
    check(Path(sys.argv[1]), Path(sys.argv[2]))
