#!/usr/bin/env python3
"""Verify the live Image Maps sheets and their Undo on the shared history (rest of UI roadmap R40).

Usage: check_studio_lipsync_live.py [Vixen.app] [show folder]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/studio-lipsync-live.json on screen on "Christmas Can-Can": Tools ▸ LipSync ▸ Edit Image Maps, New, then in
the map's editor a name, notes, a picture and a cleared picture, each applied at once. The script snapshots the library
(01-library.png, 04-library-after.png) and the editor (02-editor.png); Studio Capture photographs the editor in real
rendering (editor.png). Checks:
- the sheet's undo manager, which the sequencer's Edit ▸ Undo reaches while a sheet is in front, names the shared history's
  step (Undo Change Image Map), and undo: / redo: sent to the sheet (what the menu does; scripts run the app in the
  background, where no window has the keyboard) put the cleared picture back and take it away again;
- Clone, Make Default and Delete apply at once, and Edit ▸ Undo Delete Image Map brings the deleted map back;
- the map, its picture folder and notes really changed in the profile copy.
"""
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import time
import uuid
import zlib

root = Path(__file__).resolve().parents[1]
app = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "build/Vixen.app"
show = Path(sys.argv[2]) if len(sys.argv) > 2 else root / "target/show-copy"
work = root / "target" / ("studio-lipsync-live-" + uuid.uuid4().hex[:8])
out = work / "out"
profile = work / "show"
out.mkdir(parents=True)
subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)


def png(width, height, rgb):
    raw = b"".join(b"\0" + bytes(rgb) * width for _ in range(height))
    chunk = lambda kind, data: struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


picture = work / "mouth.png"
picture.write_bytes(png(64, 48, (200, 40, 40)))
script = work / "studio-lipsync-live.json"
script.write_text((root / "scripts/ui/studio-lipsync-live.json").read_text().replace("{PICTURE}", str(picture)))
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
        time.sleep(22)  # the editor shows from about 18 s to 25 s
        shoot("editor.png", "Christmas Can-Can")
        process.wait(timeout=90)
    finally:
        if process.poll() is None:
            process.kill()

result = json.loads((out / "result.json").read_text())
failed = [s for s in result if not s.get("ok")]
assert not failed, failed
hooks = [s.get("detail", "") for s in result if s.get("action") == "hook" and "studioImageMaps" in json.dumps(s)]
details = [s.get("detail") or "" for s in result]


def names(answer):
    """The map names in a state answer, `*` marking the default."""
    listed = answer[len("maps ["):answer.index("] selected")]
    return [n.rsplit("[", 1)[0] for n in listed.split(", ")]


def after(prefix, start=0):
    """The first hook answer starting with prefix at or after step index start, and its index."""
    for i in range(start, len(details)):
        if details[i].startswith(prefix):
            return details[i], i
    raise AssertionError(f"no answer starting {prefix!r}: {details}")


first, i = after("maps [")
assert "Singer" not in first, first
singer, i = after("notes ", i)
assert "notes Lead singer" in singer and "REST" not in singer and "AI" in singer, singer
menu, i = after("key ", i)
assert " undo Undo Change Image Map " in menu, f"the sheet in front names the step for Edit ▸ Undo: {menu}"
undone, i = after("notes ", i + 1)
assert "REST" in undone, f"Edit ▸ Undo puts the cleared picture back: {undone}"
menu, i = after("key ", i)
assert " redo Redo Change Image Map" in menu, menu
redone, i = after("notes ", i + 1)
assert "REST" not in redone, f"Edit ▸ Redo clears it again: {redone}"
cloned, i = after("maps [", i)
assert "Singer*" in names(cloned) and len([n for n in names(cloned) if n.startswith("Singer")]) == 2, cloned
deleted, i = after("maps [", i + 1)
assert [n for n in names(deleted) if n.startswith("Singer")] == ["Singer*"] and "undo Delete Image Map" in deleted, deleted
menu, i = after("key ", i)
assert " undo Undo Delete Image Map " in menu, menu
restored, i = after("maps [", i + 1)
assert names(restored) == names(cloned), f"Edit ▸ Undo brings the deleted map back: {restored}"
for name in ("01-library", "02-editor", "04-library-after"):
    assert (out / f"{name}.png").exists(), name

# The profile copy really holds the map, its notes and pictures (REST cleared, AI from the file).
store = (profile / "SystemData/ModuleStore.xml").read_text(errors="replace")
assert "Singer" in store and "Lead singer" in store, "the configuration is saved with each change"
folder = profile / "Module Data Files/LipSync/Singer"
assert (folder / "AI.bmp").exists() and not (folder / "REST.bmp").exists(), sorted(p.name for p in folder.iterdir())
print("PASS", work)
