#!/usr/bin/env python3
"""Window resizes leave the timeline's row-name pane alone; only a divider move changes and saves its width.

Runs the app muted on a private copy of a show profile (`--profile`, e.g. a copy of target/show-copy) with one
sequence, resizes the sequencer window through the harness, moves the divider once, and reads the pane widths
and the saved splitter distance after each step. Never saves the sequence.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, default=Path("target/build-debug/Vixen.app"))
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--sequence", required=True, help="file name of a sequence in the profile's Sequence folder")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    source = args.profile.resolve()
    assert not out.exists() and not out.is_relative_to(source), "Use a fresh output outside the source profile"
    out.mkdir(parents=True)
    profile = out / "profile"
    shutil.copytree(source, profile, symlinks=True)
    (profile / ".lock").unlink(missing_ok=True)
    sequence = profile / "Sequence" / args.sequence
    title = sequence.stem
    hook = lambda name, *path: {"action": "hook", "name": name, "path": list(path)}
    wait = lambda seconds=.6: {"action": "wait", "seconds": seconds}
    split = lambda: [wait(), hook("timelineSplit")]
    steps = [{"action": "window", "id": "administration"}, wait(2),
             hook("admin.openSequence", str(sequence)), wait(4),
             {"action": "focus", "window": title}, hook("sequencer.activate", title),
             hook("sequencer.resize", "1400", "900"), *split(),
             hook("sequencer.resize", "762", "600"), *split(),
             hook("sequencer.resize", "1400", "900"), *split(),
             hook("timelineSplitMove", "260"), *split(),
             hook("sequencer.resize", "762", "600"), *split(),
             {"action": "snapshot", "name": "narrow-after-move"},
             hook("sequencer.resize", "1400", "900"), *split(),
             {"action": "snapshot", "name": "wide-after-move"},
             {"action": "quit"}]
    script = out / "steps.json"
    script.write_text(json.dumps({"steps": steps}, indent=2) + "\n")
    ui = out / "ui"
    ui.mkdir()
    env = {k: v for k, v in os.environ.items() if not k.startswith(("VIXEN_", "DYLD_"))}
    env.update(VIXEN_DATA=str(profile), VIXEN_MUTE="1", VIXEN_UI_TEST=str(script), VIXEN_UI_TEST_OUT=str(ui))
    executable = args.app.resolve() / "Contents/MacOS/Vixen"
    with (out / "process.log").open("w") as log:
        process = subprocess.Popen([str(executable)], env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            process.wait(timeout=120)
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
    rows = json.loads((ui / "result.json").read_text())
    assert all(row["ok"] for row in rows), [row for row in rows if not row["ok"]]
    reads = [dict((k, int(v)) for k, v in re.findall(r"(\w+)=(-?\d+)", row["detail"]))
             for row in rows if str(row.get("detail", "")).startswith("row=")]
    assert len(reads) == 6, reads
    rows_seen = [r["row"] for r in reads]
    assert rows_seen == [200, 200, 200, 260, 260, 260], rows_seen
    assert all(r["gridLeft"] == r["row"] + 1 for r in reads), reads
    assert [r["saved"] for r in reads[:3]] == [200] * 3 or [r["saved"] for r in reads[:3]] == [0] * 3, reads
    assert [r["saved"] for r in reads[3:]] == [260] * 3, reads
    assert reads[1]["width"] < reads[0]["width"], reads
    saved = re.search(r"<SplitterDistance>(-?\d+)<", (ui / "Settings.xml").read_text())
    assert saved is None or saved.group(1) == "260", "Settings.xml should keep the dragged width"
    report = {"passed": True, "reads": reads, "out": str(out)}
    (out / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
