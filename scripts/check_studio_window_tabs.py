#!/usr/bin/env python3
"""Verify that sequences tab natively and the legacy "Timeline - song" tab strip is gone (UI roadmap R37).

Usage: check_studio_window_tabs.py <UI output directory>
Harness script: scripts/ui/studio-window-tabs.json, run on a copy of target/show-copy.

Checks, from the `sequencer.tabs` hook after each step and the accessibility dump:
- one open sequence is one window with tabbing preferred, and no "timeline-tab" or "Timeline - " strip;
- opening a second sequence joins the first window's tabs, selected, with the tab bar showing;
- the system's tab commands are on (`automaticTabbing`; AppKit only puts them in the Window menu for a key window,
  and harness windows are never key);
- closing the second tab leaves the first sequence alone in its window.
"""
import json
import sys
from pathlib import Path


def check(out: Path) -> None:
    steps = json.loads((out / "result.json").read_text())
    failed = [s for s in steps if not s.get("ok")]
    assert not failed, failed
    # result.json doesn't record a hook's name; `sequencer.tabs` is the only hook answering with this JSON.
    tabs = [json.loads(s["detail"]) for s in steps
            if s["action"] == "hook" and '"sequencerWindows"' in s.get("detail", "")]
    assert len(tabs) == 3, tabs
    one, two, closed = tabs

    assert one["sequencerWindows"] == 1 and len(one["groups"]) == 1, one
    assert one["groups"][0]["tabs"] == ["Christmas Can-Can"] and one["groups"][0]["preferred"], one
    dump = (out / "one-sequence.txt").read_text()
    assert "timeline-tab" not in dump and "Timeline - " not in dump, "legacy tab strip still drawn"

    assert two["sequencerWindows"] == 2 and len(two["groups"]) == 1, two
    group = two["groups"][0]
    assert sorted(group["tabs"]) == ["Carey Grinch", "Christmas Can-Can"], group
    assert group["selected"] == "Carey Grinch" and group["tabBarVisible"], group

    assert two["automaticTabbing"], "system tab commands are turned off"

    assert closed["sequencerWindows"] == 1 and closed["groups"][0]["tabs"] == ["Christmas Can-Can"], closed


if __name__ == "__main__":
    check(Path(sys.argv[1]))
    print("ok")
