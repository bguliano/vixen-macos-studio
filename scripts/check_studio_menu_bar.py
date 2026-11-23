#!/usr/bin/env python3
"""Verify the menu bar keeps one Tools menu and window menus only while their window is in front (backlog B69).

Usage: check_studio_menu_bar.py [Vixen.app] [show folder] [--foreground] [--before <earlier output directory>]
Defaults: build/Vixen.app and target/show-copy (APFS-cloned to a disposable folder under target/, never written).

Runs scripts/ui/studio-menu-bar.json on screen and reads the harness's menu dumps (the show window, Display Setup, the
preview designer, the prop editor, a sequencer, and the show window after each). `--foreground` makes the app active
for the run through scripts/foreground_test_guard.swift, so each window really becomes key, as it does for a user; the
guard stops the run if anything else comes to the front. Without it, the show window after the sequencer isn't checked:
offscreen no window is ever key, so the sequencer keeps the menu bar. Checks:
- the show window and Display Setup show Vixen Studio, File, Edit, View, Tools, Window, Help, and no System, Scheduler,
  Plugins, Prop or Settings menu; the designer adds its Settings menu, the prop editor its Prop menu, and both are gone
  again with the show window in front; the sequencer shows Sequence and its own Tools;
- no top-level menu shows twice or empty, and no menu lists the same title twice;
- Tools holds the former System, Tools, Scheduler and Plugins commands (Tools ▸ Plugins in the sequencer too), and
  Options... and Shutdown and Exit are the app menu's Settings… and Quit Vixen Studio, with their shortcuts;
- with --before (the menu dumps of an earlier build), every command any window showed is still in some window's menu
  bar, under Tools where it moved, with a shortcut it had.
"""
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import uuid

root = Path(__file__).resolve().parents[1]
SUFFIX = re.compile(r"^(?P<title>.*?)(?: \[(?P<key>[^\]]*)\])?(?: \(checked\))?(?: \(disabled\))?(?P<hidden> \(hidden\))?$")
WINDOWS = ["show", "setup", "designer", "after-designer", "prop-editor", "after-prop-editor", "sequencer", "show-again"]
SHOW_BAR = ["Vixen Studio", "File", "Edit", "View", "Tools", "Window", "Help"]
EXPECTED_BAR = {
    "show": SHOW_BAR, "setup": SHOW_BAR, "after-designer": SHOW_BAR, "after-prop-editor": SHOW_BAR, "show-again": SHOW_BAR,
    "designer": ["Vixen Studio", "File", "Edit", "View", "Tools", "Settings", "Window", "Help"],
    "prop-editor": ["Vixen Studio", "File", "Edit", "View", "Tools", "Prop", "Window", "Help"],
    "sequencer": ["Vixen Studio", "Sequence", "File", "Edit", "View", "Tools", "Window", "Help"],
}
# Where the old menus' commands live now: (old first titles) -> (new first titles).
MOVED = [(("System",), ("Tools",)), (("Scheduler",), ("Tools", "Scheduler")), (("Plugins",), ("Tools", "Plugins"))]
REPLACED = {("Tools", "Options..."): ("Vixen Studio", "Settings…"),
            ("System", "Shutdown and Exit"): ("Vixen Studio", "Quit Vixen Studio")}
# Lists that follow the show folder or the moment, not the menu bar's design.
VOLATILE = re.compile(r"\.(tim|log)$|^studio-heartbeat$|^Test |^Write Test State$|^Cmd |^Shift |^[A-Za-z0-9]$")


def visible_tree(path: Path):
    """The visible items of a harness `menus` dump: (path tuple, shortcut) in menu order, and the top-level titles."""
    items, tops, stack, hidden = [], [], [], []
    for line in path.read_text().splitlines():
        depth = (len(line) - len(line.lstrip(" "))) // 2
        text = line.strip()
        if text == "---":
            continue
        m = SUFFIX.match(text)
        stack[depth:] = [m.group("title")]
        hidden[depth:] = [bool(m.group("hidden"))]
        if any(hidden):
            continue
        if depth == 0:
            tops.append(m.group("title"))
        items.append((tuple(stack), m.group("key") or ""))
    return items, tops


def children(items, parent):
    return [p[-1] for p, _ in items if len(p) == len(parent) + 1 and p[:len(parent)] == parent]


def check_window(name: str, items, tops) -> list:
    problems = []
    if tops != EXPECTED_BAR[name]:
        problems.append(f"{name}: menu bar {tops}, expected {EXPECTED_BAR[name]}")
    for top in set(tops):
        if not children(items, (top,)):
            problems.append(f"{name}: {top} shows empty")
    parents = {p[:-1] for p, _ in items}
    for parent in parents:
        if parent[:2] == ("Window", "Move & Resize"):
            continue  # the system's own window tiling items, with section headings
        titles = [t for t in children(items, parent) if not VOLATILE.search(t)]
        dupes = sorted({t for t in titles if titles.count(t) > 1})
        if dupes:
            problems.append(f"{name}: {' ▸ '.join(parent) or 'menu bar'} lists {dupes} twice")
    paths = {p for p, _ in items}
    keys = dict(items)
    if name in ("show", "setup", "show-again"):
        for path in [("Tools", "Custom Prop Editor"), ("Tools", "Web Server"), ("Tools", "Logs"), ("Tools", "Profiles..."),
                     ("Tools", "View Installed Modules"), ("Tools", "System Configuration", "Setup Display"),
                     ("Tools", "System Configuration", "Setup Previews"), ("Tools", "Execution Engine", "Start"),
                     ("Tools", "Execution Engine", "Stop"), ("Tools", "Scheduler", "Shows"), ("Tools", "Scheduler", "Schedules"),
                     ("Tools", "Scheduler", "Enabled"), ("Tools", "Plugins", "Manage Plugins…")]:
            if path not in paths:
                problems.append(f"{name}: no {' ▸ '.join(path)}")
        for gone in [("Tools", "Options..."), ("Tools", "Shutdown and Exit")]:
            if gone in paths:
                problems.append(f"{name}: {' ▸ '.join(gone)} is still there")
        if keys.get(("Vixen Studio", "Settings…")) != "," or keys.get(("Vixen Studio", "Quit Vixen Studio")) != "q":
            problems.append(f"{name}: Settings… (⌘,) or Quit Vixen Studio (⌘Q) is missing")
    if name == "sequencer":
        for path in [("Tools", "Audio"), ("Tools", "LipSync"), ("Tools", "Plugins", "Manage Plugins…")]:
            if path not in paths:
                problems.append(f"{name}: no {' ▸ '.join(path)}")
    return problems


def new_path(old: tuple) -> tuple:
    if old[:2] in REPLACED:
        return REPLACED[old[:2]]
    for before, after in MOVED:
        if old[:len(before)] == before:
            return after + old[len(before):]
    return old


def compare(before: Path, after: Path) -> list:
    """Every command any window showed before is in some window's menu bar now, where it moved, with a shortcut it had."""
    def leaves(directory: Path) -> dict:
        found = {}
        for dump in directory.glob("menus-*.txt"):
            items, _ = visible_tree(dump)
            parents = {p[:-1] for p, _ in items}
            for path, key in items:
                # A one-title path is a top-level menu that showed empty (the stray Prop menu).
                if path not in parents and len(path) > 1 and not any(VOLATILE.search(t) for t in path):
                    found.setdefault(path, set()).add(key)
        return found
    old, new = leaves(before), leaves(after)
    problems = []
    for path, keys in sorted(old.items()):
        target = new_path(path)
        if target not in new:
            problems.append(f"{' ▸ '.join(path)} is gone (looked for {' ▸ '.join(target)})")
        elif path[:2] not in REPLACED and keys - {""} and not keys & new[target]:
            problems.append(f"{' ▸ '.join(path)} shortcut {sorted(keys)} is now {sorted(new[target])}")
    return problems


def run(app: Path, show: Path, foreground: bool) -> Path:
    work = root / "target" / ("studio-menu-bar-" + uuid.uuid4().hex[:8])
    out, profile = work / "out", work / "show"
    out.mkdir(parents=True)
    subprocess.run(["cp", "-cR", str(show), str(profile)], check=True)
    env = dict(os.environ, VIXEN_MUTE="1", VIXEN_DATA=str(profile), VIXEN_UI_TEST=str(root / "scripts/ui/studio-menu-bar.json"),
               VIXEN_UI_TEST_OUT=str(out), VIXEN_UI_TEST_ONSCREEN="1")
    binary = work / "foreground-guard"
    if foreground:
        subprocess.run(["swiftc", "-O", str(root / "scripts/foreground_test_guard.swift"), "-o", str(binary)], check=True,
                       stderr=subprocess.DEVNULL)
    proc = subprocess.Popen([str(app / "Contents/MacOS/Vixen")], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    guard = None
    try:
        if foreground:
            time.sleep(1.5)  # until the app is registered as a running application
            guard = subprocess.Popen([str(binary), str(proc.pid), str(work / "guard.json"), "120"])
        proc.wait(timeout=150)
        if guard:
            guard.wait(timeout=30)
    finally:
        if proc.poll() is None:
            proc.kill()
        if guard and guard.poll() is None:
            guard.kill()
        shutil.rmtree(profile, ignore_errors=True)
    if foreground:
        report = (work / "guard.json").read_text()
        assert '"blocked" : false' in report or '"blocked": false' in report, report
    return out


def main() -> None:
    args = sys.argv[1:]
    foreground = "--foreground" in args
    before = None
    if "--before" in args:
        i = args.index("--before")
        before = Path(args[i + 1])
        del args[i:i + 2]
    args = [a for a in args if a != "--foreground"]
    app = Path(args[0]) if args else root / "build/Vixen.app"
    show = Path(args[1]) if len(args) > 1 else root / "target/show-copy"
    out = run(app, show, foreground)
    result = (out / "result.json").read_text()
    assert '"ok" : false' not in result and '"ok": false' not in result, result
    problems = []
    for name in WINDOWS:
        if name == "show-again" and not foreground:
            continue  # a test window is never key offscreen, so the sequencer keeps the menu bar; --foreground checks it
        items, tops = visible_tree(out / f"menus-{name}.txt")
        problems += check_window(name, items, tops)
    if before:
        problems += compare(before, out)
    for problem in problems:
        print("FAIL", problem)
    print(f"output: {out}")
    if problems:
        sys.exit(1)
    print("PASS: one Tools menu; window menus only with their window in front; no duplicate or empty menus"
          + ("; every earlier command still reachable with its shortcut" if before else ""))


if __name__ == "__main__":
    main()
