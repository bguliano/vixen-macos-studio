#!/usr/bin/env python3
"""Assert real native key/dialog results, including exact TimeSpan tick rounding.

Usage: scripts/check_sequencer_distribution.py <UI output directory>
The corresponding harness script is scripts/ui/sequencer-distribution.json.
See docs/VERIFICATION.md for the private Ruler fixture and launch settings.
"""
import json
import sys
from pathlib import Path


def check(root: Path) -> None:
    rows = json.loads((root / "result.json").read_text())
    assert len(rows) == 78 and all(row["ok"] for row in rows), rows
    states = []
    for row in rows:
        detail = row.get("detail", "")
        if detail.startswith("{"):
            value = json.loads(detail)
            if "elements" in value:
                states.append(value)
    assert len(states) == 6, len(states)
    before, equal, canceled, specified, undone, redone = states

    def times(state):
        return [(e["start"], e["duration"]) for e in sorted(state["elements"], key=lambda e: e["id"])]

    assert times(before) == [(50_000_000, 20_000_000)] * 3
    assert times(equal) == [(50_000_000, 6_666_666), (56_666_666, 6_666_666), (63_333_332, 6_666_666)]
    assert times(canceled) == times(equal)
    assert canceled["undo"] == equal["undo"]
    assert times(specified) == [(61_999_999, 5_000_000), (56_000_000, 5_000_000), (50_000_000, 5_000_000)]
    assert times(undone) == times(equal)
    assert times(redone) == times(specified)
    assert equal["undo"][0] == "Distributed 3 effects"
    assert specified["undo"][:2] == ["Distributed 3 effects"] * 2
    assert undone["redo"] == ["Distributed 3 effects"] and not redone["redo"]

    routes = [r["detail"] for r in rows if r.get("detail", "").startswith("posted ")]
    assert len(routes) == 3 and all("route=table" in r and "handled=true" in r for r in routes), routes
    assert [r.split()[1] for r in routes] == ["d", "i", "i"]
    assert any("text=00:00.500" in r.get("detail", "") for r in rows)
    assert any("text=00:00.100" in r.get("detail", "") for r in rows)
    print("PASS: 78 native steps; D/I routing, exact effect timing, Cancel, reverse-order options, Undo/Redo")


if __name__ == "__main__":
    check(Path(sys.argv[1]))
