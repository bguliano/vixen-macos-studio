#!/usr/bin/env python3
"""Verify Import ▸ Timing from Sequence… in the Mark Collections panel (#61).

Usage: check_studio_timing_import.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Opens "Christmas Can-Can" with the Mark Collections tab showing and imports the timing of "Carol of the Bells.tim",
read from a copy outside the profile as another show's sequence would be, one collection per element with a -35 ms
offset. Studio Capture photographs the selection sheet (timing-selection.png). Checks:
- the toolbar's Import menu offers Timing from Sequence…, and its selection sheet adds the offered collections;
- every imported collection has marks, named "Carol of the Bells - <element>" (plus the file's own collections);
- the import is one step, "Undo Import Marks", and Edit ▸ Undo puts the collections back as they were.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid

root = Path(__file__).resolve().parents[1]
app = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "build/Vixen.app"
show = Path(sys.argv[2]) if len(sys.argv) > 2 else root / "target/show-copy"
work = root / "target" / ("studio-timing-import-" + uuid.uuid4().hex[:8])
out = work / "out"
profile = work / "show"
out.mkdir(parents=True)
subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)
source = work / "Old Show" / "Carol of the Bells.tim"
source.parent.mkdir()
shutil.copy(show / "Sequence/Carol of the Bells.tim", source)
script = work / "studio-timing-import.json"
script.write_text((root / "scripts/ui/studio-timing-import.json").read_text().replace("{TIMING_FILE}", str(source)))
capture = Path.home() / "Applications/Studio Capture.app/Contents/MacOS/studio-capture"
env = dict(os.environ, VIXEN_MUTE="1", VIXEN_DATA=str(profile), VIXEN_UI_TEST=str(script), VIXEN_UI_TEST_OUT=str(out),
           VIXEN_UI_TEST_ONSCREEN="1")


def shoot(name, title):
    status = work / f"{name}.status"
    subprocess.run([str(capture), "--owner", "Vixen", "--title", title, "--out", str(out / name), "--status", str(status)], timeout=20)
    print(name, status.read_text() if status.exists() else "no status")


with (work / "app.log").open("w") as log:
    process = subprocess.Popen([str(app / "Contents/MacOS/Vixen")], env=env, stdout=log, stderr=log)
    try:
        time.sleep(18)  # the selection sheet shows at about 16 s and stays 6 s
        shoot("timing-selection.png", "Christmas Can-Can")
        process.wait(timeout=90)
    finally:
        if process.poll() is None:
            process.kill()

result = json.loads((out / "result.json").read_text())
failed = [s for s in result if not s.get("ok")]
assert not failed, failed


def rows(name):
    return [(r["name"], r["marks"]) for r in json.loads((out / f"{name}.json").read_text())["rows"]]


def undo_item(name):
    menu = (out / f"{name}.txt").read_text()
    return next(line.strip() for line in menu.splitlines() if line.strip().startswith("Undo"))


start = rows("rows-start")
imported = rows("rows-after-timing")
added = imported[len(start):]
assert imported[:len(start)] == start, imported
assert any(name.startswith("Carol of the Bells - ") for name, _ in added), added
assert added and all(count > 0 for _, count in added), added
assert undo_item("03-after-timing").startswith("Undo Import Marks"), undo_item("03-after-timing")
assert rows("rows-after-undo-timing") == start
print(f"ok: {len(added)} collections imported from another sequence, one Undo step; screenshots in {out}")
