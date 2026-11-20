#!/usr/bin/env python3
"""Verify the show window's Sequences library: badges, New Sequence from Audio, Duplicate with Undo, Export FSEQ.

Usage: check_studio_library.py <UI output directory> <profile the run used>
Harness script: scripts/ui/studio-library.json, run on a copy of target/show-copy whose Media folder holds real files
(no links, so a song can be copied in) without "Carey Grinch.mp3", with no Export/Carol of the Bells.fseq, and with an
audio file named "Test Song.mp3" beside the profile folder.

Checks, from the library's state after each step and from the profile on disk:
- Carey Grinch shows Audio Missing and sequences with an up-to-date FSEQ export show FSEQ;
- dropping Test Song.mp3 copies it into Media, saves Sequence/Test Song.tim as long as the song and opens it;
- the context menu's Duplicate makes "Cameron-Zoom copy", Undo removes it, Redo makes it again;
- Export FSEQ writes Export/Carol of the Bells.fseq and the row gains FSEQ;
- opening Wizards in Winter marks it open with every effect rendered.
"""
import json
import sys
from pathlib import Path


def check(out: Path, profile: Path) -> None:
    steps = json.loads((out / "result.json").read_text())
    failed = [s for s in steps if not s.get("ok")]
    assert not failed, failed
    states = [s["detail"] for s in steps if s["action"] == "studioLibraryState"]
    windows = [s["detail"] for s in steps if s["action"] == "windows"]
    assert len(states) == 7, states

    def rows(state: str) -> dict[str, str]:
        parts = state.split(" | ")
        return dict(p.split(": ", 1) for p in parts if not p.startswith(("status: ", "undo: ")))

    def field(state: str, name: str) -> str:
        return next(p for p in state.split(" | ") if p.startswith(name + ": "))[len(name) + 2:]

    initial, dropped, duplicated, undone, redone, exported, opened = states
    assert "audioMissing" in rows(initial)["Carey Grinch"], initial
    assert "fseq" in rows(initial)["Cameron-Zoom"], initial
    assert "Test Song" not in rows(initial) and "fseq" not in rows(initial)["Carol of the Bells"], initial

    assert "Test Song" in rows(dropped), dropped
    assert field(dropped, "status") == "Created Test Song.tim." and field(dropped, "undo") == "Undo New Sequence from Audio", dropped
    assert "Test Song" in windows[0], windows
    tim = (profile / "Sequence" / "Test Song.tim").read_text()
    assert "Test Song.mp3" in tim and "<Length" in tim and "PT2M36" in tim, tim[:400]
    assert (profile / "Media" / "Test Song.mp3").stat().st_size == (profile.parent / "Test Song.mp3").stat().st_size

    assert "Cameron-Zoom copy" in rows(duplicated) and field(duplicated, "undo") == "Undo Duplicate Sequence", duplicated
    assert "Cameron-Zoom copy" not in rows(undone) and field(undone, "status") == "Removed Cameron-Zoom copy.tim.", undone
    assert "Cameron-Zoom copy" in rows(redone), redone
    assert (profile / "Sequence" / "Cameron-Zoom copy.tim").read_bytes() == (profile / "Sequence" / "Cameron-Zoom.tim").read_bytes()

    assert "fseq" in rows(exported)["Carol of the Bells"], exported
    assert field(exported, "status") == "Exported Carol of the Bells.fseq to the Export folder.", exported
    assert (profile / "Export" / "Carol of the Bells.fseq").stat().st_size > 0

    wizards = rows(opened)["Wizards in Winter"].split(",")
    assert "open" in wizards and "unsaved" not in wizards, opened
    rendered = next(w for w in wizards if w.startswith("rendered "))[len("rendered "):].split("/")
    assert rendered[0] == rendered[1] and int(rendered[1]) > 0, opened


if __name__ == "__main__":
    check(Path(sys.argv[1]), Path(sys.argv[2]))
    print("studio library: ok")
