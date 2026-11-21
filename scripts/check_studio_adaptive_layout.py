#!/usr/bin/env python3
"""Verify every Studio window lays out at its minimum size, in light and dark (UI roadmap R42).

Usage: check_studio_adaptive_layout.py <UI output directory>
Harness script: scripts/ui/studio-adaptive-layout.json, run on a copy of target/show-copy.

The script opens each window (the show window's five sections, About, Installed Modules, Release Notes,
Instrumentation, Display Setup, the sequence package and export wizards, a sequencer and Settings), shrinks it to
its minimum with the `studio.layout` hook, snapshots it in dark and light, and has the hook check its accessibility
tree. Checks:
- every step passed, so no window has a control or scroll view reaching past its edge, overlapping a sibling, or a
  one-line label drawn narrower than its text;
- the sequencer's minimum leaves its timeline room beside both side panes (before R42 it was 640 points, with the
  timeline pushed under the inspector); its panes never go below their minimums, come back to the user's widths when
  the window grows, and only a dragged divider changes those widths (a window resize used to save squeezed panes);
- the show window's Display sections keep their wider minimum;
- About no longer insists on 900 x 800, and fits a small display;
- every window was checked in both appearances, and the user's own appearance setting was put back.
Look at the PNGs as well: the hook can't see text a view truncates without saying so.
"""
import json
import sys
from pathlib import Path

WINDOWS = [
    "Vixen Studio", "About Vixen Studio", "Installed Modules", "Vixen Release Notes", "Instrumentation",
    "Display Setup", "Sequence Package Export", "Sequence Package Import", "Export Wizard", "Christmas Can-Can",
    "@settings",
]


def size(detail: str) -> tuple:
    w, h = detail.split("x")
    return int(w), int(h)


def check(out: Path) -> None:
    steps = json.loads((out / "result.json").read_text())
    failed = [s for s in steps if not s.get("ok")]
    assert not failed, failed

    script = json.loads((Path(__file__).parent / "ui" / "studio-adaptive-layout.json").read_text())["steps"]
    # `quit` exits before it's recorded.
    assert script[-1]["action"] == "quit", script[-1]
    script = script[:-1]
    assert len(script) == len(steps), (len(script), len(steps))
    minimums, checked, panes = {}, {}, []
    appearance = None
    for spec, result in zip(script, steps):
        path = spec.get("path") or []
        if spec.get("name") != "studio.layout" or not path:
            continue
        if path[0] == "appearance":
            appearance = path[1]
        elif path[0] == "min":
            minimums.setdefault(path[1], set()).add(size(result["detail"]))
        elif path[0] == "panes":
            panes.append([int(w) for w in result["detail"].split(",")])
        elif path[0] == "check":
            assert result["detail"].startswith("ok"), result
            checked.setdefault(path[1], set()).add(appearance)

    for window in WINDOWS:
        assert checked.get(window, set()) >= {"light", "dark"}, (window, checked.get(window))

    sequencer = max(minimums["Christmas Can-Can"])
    # Effects (180) + timeline (320) + inspector (260) + two dividers.
    assert sequencer[0] >= 762, sequencer
    # The sequencer's panes: at the minimum; the saved 200 and 340 back in a wide window; the left divider dragged to 260;
    # squeezed to the minimum again; and both preferences back in the wide window, the drag remembered and the right
    # pane's squeeze not.
    at_min, regrown, dragged, squeezed, again = panes
    assert at_min == [180, 320, 260], at_min
    assert regrown[0] == 200 and regrown[2] == 340, regrown
    assert dragged[0] == 260 and dragged[2] == 340, dragged
    assert squeezed == [180, 320, 260], squeezed
    assert again[0] == 260 and again[2] == 340, again
    about = max(minimums["About Vixen Studio"])
    assert about[0] <= 700 and about[1] <= 560, about
    show = minimums["Vixen Studio"]
    assert {w for w, _ in show} == {860, 1060}, show

    assert script[-1].get("path") == ["restore"] and steps[-1]["ok"], "the user's appearance wasn't restored"
    for name in ["show-controllers-dark", "show-display-light", "sequencer-light", "sequencer-regrown", "settings-light"]:
        assert (out / f"{name}.png").stat().st_size > 10_000, name


if __name__ == "__main__":
    check(Path(sys.argv[1]))
    print("ok")
