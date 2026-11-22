#!/usr/bin/env python3
"""Verify a collapsed sequencer drawer stops counting toward the window's minimum width (B54).

Usage: check_studio_dock_collapsed_minimum.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/studio-dock-collapsed-minimum.json on "Christmas Can-Can", shrinking the window to its minimum with the
`studio.layout` hook each time:
- with both drawers open the minimum fits Effects (180), the timeline (320) and the inspector (260);
- collapsing the Inspector drops its 260; collapsing the sidebar as well leaves the timeline's 320 and the two dividers
  the split view keeps for hidden panes, so the timeline never goes below its minimum;
- bringing each drawer back in that narrow window widens the window, so no pane, the timeline included, is left below
  its minimum.
Both drawer settings are cleared for the run and put back afterwards. Snapshots both-collapsed-min.png, both-reopened.png.
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
work = root / "target" / ("studio-dock-collapsed-minimum-" + uuid.uuid4().hex[:8])
out = work / "out"
profile = work / "show"
out.mkdir(parents=True)
subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)
script = root / "scripts/ui/studio-dock-collapsed-minimum.json"
env = dict(os.environ, VIXEN_MUTE="1", VIXEN_DATA=str(profile), VIXEN_UI_TEST=str(script), VIXEN_UI_TEST_OUT=str(out))

domain = "io.github.bguliano.vixen-macos-studio"
keys = ["studio.sequencerSidebarCollapsed", "studio.sequencerInspectorCollapsed"]
saved = {key: subprocess.run(["defaults", "read", domain, key], capture_output=True, text=True) for key in keys}
for key in keys:
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
    for key, value in saved.items():
        if value.returncode == 0:
            subprocess.run(["defaults", "write", domain, key, "-bool", "true" if value.stdout.strip() == "1" else "false"], check=True)
        else:
            subprocess.run(["defaults", "delete", domain, key], capture_output=True)

result = json.loads((out / "result.json").read_text())
failed = [s for s in result if not s.get("ok")]
assert not failed, failed
steps = json.loads(script.read_text())["steps"][:-1]
assert len(steps) == len(result), (len(steps), len(result))
layout = [(spec["path"][0], r["detail"]) for spec, r in zip(steps, result) if spec.get("name") == "studio.layout"]
minimums = [int(detail.split("x")[0]) for kind, detail in layout if kind == "min"]
panes = [[int(w) for w in detail.split(",")] for kind, detail in layout if kind == "panes"]

both, no_inspector, neither, reopened = minimums
open_panes, inspector_collapsed, both_collapsed, sidebar_back, both_back = panes
# Effects (180) + timeline (320) + inspector (260) + two dividers.
assert both >= 762, minimums
assert open_panes == [180, 320, 260], open_panes
# A hidden pane keeps its divider, so the timeline still gets its whole 320.
assert no_inspector == both - 260, ("the collapsed Inspector no longer counts", minimums)
assert inspector_collapsed == [180, 320, 0], inspector_collapsed
assert neither == 322, ("only the timeline and the dividers count with both drawers collapsed", minimums)
assert both_collapsed == [0, 320, 0], both_collapsed
assert sidebar_back[0] >= 180 and sidebar_back[1] >= 320 and sidebar_back[2] == 0, ("the sidebar widens the window", sidebar_back)
assert both_back[0] >= 180 and both_back[1] >= 320 and both_back[2] >= 260, ("the Inspector widens the window", both_back)
assert reopened == both, minimums
print("ok: collapsed drawers leave the minimum and widen the window when they come back;", minimums, panes, out)
