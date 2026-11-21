#!/usr/bin/env python3
"""Verify the Lyric Converter's live unknown-word mapping and its Undo on the shared history (rest of UI roadmap R40).

Usage: check_studio_lyric_map_word.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/studio-lyric-map-word.json on "Christmas Can-Can": Tools ▸ LipSync ▸ Lyric Converter, "hello zorbleplex"
converted to the clipboard. The Text Conversion Failure sheet asks about ZORBLEPLEX (01-failure-sheet.png); ETC, MBP and WQ are
clicked (02-mapped.png) and Map Word maps it as one step, after which the conversion delivers (its Convert Text message is
dismissed). Checks:
- the sheet's buttons are verbs (Skip Word, Map Word), not Cancel and OK;
- the converter window's undo manager, which Edit ▸ Undo reaches while it is in front, names the step (Undo Map Word
  “ZORBLEPLEX”), and undo: / redo: sent to it take the mapping out of the profile's user_dictionary file and put it back;
- after the final Undo the file is gone again, the word is asked about again, and Skip Word goes on without writing anything.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import uuid

root = Path(__file__).resolve().parents[1]
app = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "build/Vixen.app"
show = Path(sys.argv[2]) if len(sys.argv) > 2 else root / "target/show-copy"
work = root / "target" / ("studio-lyric-map-word-" + uuid.uuid4().hex[:8])
out = work / "out"
profile = work / "show"
out.mkdir(parents=True)
subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)
(profile / ".lock").unlink(missing_ok=True)
dictionary = profile / "user_dictionary"
dictionary.unlink(missing_ok=True)
env = dict(os.environ, VIXEN_MUTE="1", VIXEN_DATA=str(profile), VIXEN_UI_TEST=str(root / "scripts/ui/studio-lyric-map-word.json"),
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
details = [s.get("detail") or "" for s in result]
lyric = [d for d in details if d.startswith(("outcome ", "undo ", "sendUndo", "sendRedo"))]
print("\n".join(lyric))

assert lyric[0] == "outcome needsMapping zorbleplex asking zorbleplex", lyric[0]
after_map = json.loads((out / "03-after-map.json").read_text())
assert after_map["lastOutcome"].startswith("delivered") and after_map["failure"] == "", after_map
assert lyric[1] == "undo Undo Map Word “ZORBLEPLEX” redo -", f"the converter names the step for Edit ▸ Undo: {lyric[1]}"
assert lyric[3] == "undo - redo Redo Map Word “ZORBLEPLEX”", lyric[3]
assert lyric[5] == "undo Undo Map Word “ZORBLEPLEX” redo -", lyric[5]
assert lyric[7] == "outcome needsMapping zorbleplex asking zorbleplex", f"after Undo the word is unknown again: {lyric[7]}"
after_skip = json.loads((out / "04-after-skip.json").read_text())
assert after_skip["failure"] == "", after_skip
assert not dictionary.exists(), f"Undo took the file away and Skip Word wrote nothing: {dictionary.read_text()!r}"
for name in ("01-failure-sheet", "02-mapped", "03-asked-again"):
    assert (out / f"{name}.png").exists(), name
print("PASS", work)
