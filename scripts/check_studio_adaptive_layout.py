#!/usr/bin/env python3
"""Verify every Studio window lays out at its minimum size, in light and dark (UI roadmap R42).

Usage: check_studio_adaptive_layout.py <UI output directory>
Harness script: scripts/ui/studio-adaptive-layout.json, run on a copy of target/show-copy.

The script opens each window (the show window's five sections, About, Installed Modules, Release Notes,
Instrumentation, Display Setup, the sequence package and export wizards, a sequencer, the preview designer, the Custom
Prop Editor, the Dummy Lighting and Debugging Values output windows, and Settings) and the sheets (Place Effect on Marks, Export Sequence, a controller's and an element
property's setup), shrinks each to its minimum with the `studio.layout` hook (a sheet keeps the size its content asks for),
snapshots it in dark and light, and has the hook check its accessibility tree. Checks:
- every step passed, so no window has a control or scroll view reaching past its edge, overlapping a sibling, or a
  one-line label drawn narrower than its text;
- the sequencer's minimum leaves its timeline room beside both side panes (before R42 it was 640 points, with the
  timeline pushed under the inspector); its panes never go below their minimums, come back to the user's widths when
  the window grows, and only a dragged divider changes those widths (a window resize used to save squeezed panes);
- the show window's Display sections keep their wider minimum;
- About no longer insists on 900 x 800, and fits a small display;
- the Controller Output monitors open on sample data through the `studio.monitor` hook (the harness keeps output muted,
  so no controller runs by itself; the hook starts only controllers with no hardware) and close when stopped; the Dummy
  Lighting window keeps a minimum (it had none and shrank to 1 x 32, its title-bar buttons outside it) and the Debugging
  Values window keeps upstream's 538 x 405;
- the Custom Prop Editor keeps its 740-point minimum, and there its toolbar folds the arranging tools into one menu
  rather than scrolling Undo and Redo out of sight;
- every window was checked in both appearances, and the user's own appearance setting was put back.
Look at the PNGs as well: the hook can't see text a view truncates without saying so, nor most controls in a SwiftUI
sheet.
"""
import json
import sys
from pathlib import Path

WINDOWS = [
    "Vixen Studio", "About Vixen Studio", "Installed Modules", "Vixen Release Notes", "Instrumentation",
    "Display Setup", "Sequence Package Export", "Sequence Package Import", "Export Wizard", "Christmas Can-Can",
    "Preview Configuration", "Custom Prop Editor", "Dummy Lighting Controller", "Debug Controller Output", "@settings",
]
# Sheets have no titles: the run names each by its snapshot.
SHEETS = ["place-on-marks", "effect-gallery", "export-sequence", "controller-setup", "property-setup"]


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
    shot = None
    sheets = {}
    monitors = []
    for spec, result in zip(script, steps):
        path = spec.get("path") or []
        if spec.get("name") == "studio.monitor":
            monitors.append(result["detail"])
        elif spec["action"] == "windows" and monitors and monitors[-1].startswith("stopped"):
            monitors.append("closed" if "Controller" not in result["detail"] else result["detail"])
        if spec.get("name") != "studio.layout" or not path:
            continue
        if path[0] == "appearance":
            appearance = path[1]
        elif path[0] == "snapshot":
            shot = path[1].rsplit("-", 1)[0]
            if path[2:] == ["@sheet"]:
                sheets[shot] = size(result["detail"].split()[0])
        elif path[0] == "min":
            minimums.setdefault(path[1], set()).add(size(result["detail"]))
        elif path[0] == "panes":
            panes.append([int(w) for w in result["detail"].split(",")])
        elif path[0] == "check":
            assert result["detail"].startswith("ok"), result
            window = f"@sheet {shot}" if path[1] == "@sheet" else path[1]
            checked.setdefault(window, set()).add(appearance)

    for window in WINDOWS + [f"@sheet {sheet}" for sheet in SHEETS]:
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
    # A timeline row was selected first, so Place Effect on Marks shows its whole form, not "No Rows Selected".
    assert sheets["place-on-marks"][1] >= 500, sheets
    # The same row, so the Effect Gallery shows its effect list and thumbnails.
    assert sheets["effect-gallery"][1] >= 560, sheets
    prop = max(minimums["Custom Prop Editor"])
    assert prop[0] <= 760, prop
    toolbar = (out / "layout-Custom Prop Editor.txt").read_text()
    # ViewThatFits picked the compact row, so all of it fits (SwiftUI leaves the icon-only buttons out of the tree).
    assert '"Arrange selected lights"' in toolbar and '"Align selected lights"' not in toolbar, toolbar
    # Both monitors opened under their own titles, and stopping their controllers closed both windows.
    assert monitors == ["Dummy Lighting Controller", "Debug Controller Output", "stopped 2", "closed"], monitors
    dummy = minimums["Dummy Lighting Controller"]
    assert all(w >= 160 and h >= 120 for w, h in dummy), dummy
    debug = minimums["Debug Controller Output"]
    # Upstream's 538 x 405 content, under a 32-point title bar.
    assert debug == {(538, 437)}, debug
    show = minimums["Vixen Studio"]
    assert {w for w, _ in show} == {860, 1060}, show

    assert script[-1].get("path") == ["restore"] and steps[-1]["ok"], "the user's appearance wasn't restored"
    for name in ["show-controllers-dark", "show-display-light", "sequencer-light", "sequencer-regrown", "settings-light",
                 "place-on-marks-dark", "effect-gallery-light", "export-sequence-light", "controller-setup-dark", "property-setup-light",
                 "preview-designer-dark", "prop-editor-light", "prop-editor-wide", "monitor-dummy-dark",
                 "monitor-debug-light"]:
        assert (out / f"{name}.png").stat().st_size > 10_000, name


if __name__ == "__main__":
    check(Path(sys.argv[1]))
    print("ok")
