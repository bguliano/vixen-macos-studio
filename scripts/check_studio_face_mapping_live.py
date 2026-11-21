#!/usr/bin/env python3
"""Verify Edit Element Face Mapping's live Face Setup visit and its Undo on the shared history (rest of UI roadmap R40).

Usage: check_studio_face_mapping_live.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/studio-face-mapping-live.json on "Christmas Can-Can": Tools ▸ LipSync ▸ Edit Element Face Mapping, Relays/White
Bottom chosen (01-chosen.png), Set Up Faces… (02-face-setup.png), the first mouth checkbox clicked (03-face-checked.png) and Face
Setup closed with Done (Esc). Checks:
- the node select sheet's buttons are verbs (Set Up Faces…, Done), not Cancel and OK, and it stays up after Face Setup
  (04-back-to-node-select.png);
- Face Setup's change reached the profile at once, and the visit is one step on the shared history (Edit Face Mapping) that the
  sheet's undo manager names for Edit ▸ Undo;
- Undo takes the element's new Face property away, Redo puts it back, and Done closes reporting the change.
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
work = root / "target" / ("studio-face-mapping-live-" + uuid.uuid4().hex[:8])
out = work / "out"
profile = work / "show"
out.mkdir(parents=True)
subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)
(profile / ".lock").unlink(missing_ok=True)
env = dict(os.environ, VIXEN_MUTE="1", VIXEN_DATA=str(profile), VIXEN_UI_TEST=str(root / "scripts/ui/studio-face-mapping-live.json"),
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
details = [(s.get("action"), (s.get("detail") or "")[:300]) for s in result]
print("\n".join(f"{a}: {d}" for a, d in details if d))
assert not failed, failed
faces = [d for a, d in details if a == "hook" and (d == "(none)" or ":" in d and not d.startswith(("chosen", "undo", "changed", "applied", "{", "(")))]
menus = [d for a, d in details if d.startswith("undo ")]
before, during, after_undo, after_redo, after_last_undo = faces[0], faces[1], faces[2], faces[3], faces[4]
assert "White Bottom" not in before, before
assert "White Bottom:" in during, f"Face Setup wrote the profile as it was used: {during}"
state = next(d for a, d in details if d.startswith("changed "))
assert state == "changed true label Edit Face Mapping", state
assert menus[0] == "undo Undo Edit Face Mapping redo -", f"the sheet names the visit for Edit ▸ Undo: {menus[0]}"
assert after_undo == before, f"Undo took the Face property away: {after_undo}"
assert menus[1] == "undo - redo Redo Edit Face Mapping", menus[1]
assert after_redo == during, after_redo
assert after_last_undo == before, after_last_undo
closed = [d for a, d in details if d.startswith("lastClose ")]
assert closed == ["lastClose true"], f"Done closed the sheet reporting the change: {closed}"
for name in ("01-chosen", "02-face-setup", "03-face-checked", "04-back-to-node-select"):
    assert (out / f"{name}.png").exists(), name
print("PASS", work)
