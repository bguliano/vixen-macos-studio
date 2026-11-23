#!/usr/bin/env python3
"""Verify the command palette (⌘K) reaches every menu command in the show window and the sequencer.

Usage: check_studio_command_palette.py <UI output directory>
Harness script: scripts/ui/studio-command-palette.json.

Checks, from the harness's own menu dumps (written by code independent of the palette):
- the palette's check step passed in both windows: every visible command found by searching its own title, every
  disabled one with a reason;
- every palette entry is a real menu item at that path, and the sequencer lists its own Sequence menu, the show window not;
- palette runs reach Studio's panes: Tools ▸ Scheduler ▸ Schedules shows the show window's Schedule pane, no Scheduler window;
- ⌘K opens the palette, typing and Return run the best match (Tools ▸ Scheduler ▸ Shows, the Schedule pane again) and close it;
- running a command from the palette really runs it (Sequence ▸ Playback ▸ Loop becomes checked).
"""
import json
import re
import sys
from pathlib import Path

SUFFIX = re.compile(r"^(?P<title>.*?)(?: \[[^\]]*\])?(?: \(checked\))?(?: \(disabled\))?$")


def menu_paths(dump: Path) -> dict:
    """Every item path in a harness `menus` dump, mapped to its line."""
    paths, stack = {}, []
    for line in dump.read_text().splitlines():
        depth = (len(line) - len(line.lstrip(" "))) // 2
        text = line.strip()
        if text == "---":
            continue
        title = SUFFIX.match(text).group("title")
        stack[depth:] = [title]
        paths[" ▸ ".join(stack)] = text
    return paths


def palette(root: Path, kind: str) -> list:
    rows = []
    for line in (root / f"palette-{kind}.txt").read_text().splitlines():
        path, shortcut, state, reason, tool = line.split(" | ")
        rows.append({"path": path, "shortcut": shortcut, "enabled": state == "on", "reason": reason, "tool": tool})
    return rows


def check(root: Path) -> None:
    steps = json.loads((root / "result.json").read_text())
    failed = [s for s in steps if not s["ok"]]
    assert not failed, failed
    details = [s.get("detail", "") for s in steps]
    passes = [d for d in details if d.startswith("PASS ")]
    assert len(passes) == 2 and passes[0].startswith("PASS show:") and passes[1].startswith("PASS sequencer:"), passes

    windows = [d for s, d in zip(steps, details) if s["action"] == "windows"]
    sections = [d for s, d in zip(steps, details) if s["action"] == "studioSectionNow"]
    assert sections == ["schedule", "schedule"], sections  # both runs left Sequences for the Schedule pane
    assert "Scheduler" not in windows[0] and "Shows " not in windows[0], windows[0]  # palette run: Tools ▸ Scheduler ▸ Schedules
    assert "Command Palette…" in windows[1], windows[1]  # ⌘K opened it
    assert "Command Palette…" not in windows[2] and "Shows " not in windows[2], windows[2]  # typed "shows", Return

    for kind, dump in (("show", "menus-show.txt"), ("sequencer", "menus-after-run.txt")):
        rows = palette(root, kind)
        menus = menu_paths(root / dump)
        assert len(rows) >= 80, (kind, len(rows))
        missing = [r["path"] for r in rows if r["path"] not in menus]
        assert not missing, (kind, missing[:10])
        assert all(r["reason"] for r in rows if not r["enabled"]), kind
        assert not any(r["reason"] for r in rows if r["enabled"]), kind
        assert not any(r["path"].endswith("Command Palette…") for r in rows), "the palette lists itself"
        has_sequence = any(r["path"].startswith("Sequence ▸ ") for r in rows)
        assert has_sequence == (kind == "sequencer"), (kind, "Sequence menu")
        assert any(r["tool"] for r in rows), (kind, "no agent tools named")

    show = {r["path"] for r in palette(root, "show")}
    assert "Window ▸ Vixen Studio" in show, "Window ▸ Vixen Studio"
    retired = ["Shows", "Scheduler", "About Vixen Studio", "Export Wizard", "Instrumentation", "Display Setup"]
    assert not [t for t in retired if f"Window ▸ {t}" in show], "the palette offers a retired Window menu opener (B83)"

    seq = {r["path"]: r for r in palette(root, "sequencer")}
    assert seq["Tools ▸ Audio ▸ Beat/Bar Detection"]["tool"] == "sequence_music_marks"
    assert "Associate audio" in seq["Tools ▸ Audio ▸ Beat/Bar Detection"]["reason"]
    assert seq["Sequence ▸ Save"]["shortcut"] == "⌘S" and seq["Sequence ▸ Save"]["tool"] == "sequence_save"
    assert seq["Sequence ▸ Playback ▸ Play"]["shortcut"] == "F5"
    assert menu_paths(root / "menus-after-run.txt")["Sequence ▸ Playback ▸ Loop"].endswith("(checked)")
    assert (root / "palette-show.png").stat().st_size > 10_000 and (root / "palette-sequencer.png").stat().st_size > 10_000
    show_count, seq_count = len(palette(root, "show")), len(seq)
    print(f"PASS: {show_count} show-window and {seq_count} sequencer commands reachable; ⌘K, type, Return and palette runs work")


if __name__ == "__main__":
    check(Path(sys.argv[1]))
