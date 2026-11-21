#!/usr/bin/env python3
"""Run native sequencer key checks on isolated copies, with output muted.

The supplied profile must be a small fixture with the Ruler layout used by
scripts/ui/sequencer-*.json; the optional sequence, a .tim with no effects, is
opened in place of a new one. Both are copied and never changed. Without a
sequence each case starts a new one, which Save writes to Sequence/Ruler.tim.
Keys are CGEvent-backed NSEvents posted only to the test application; these
checks do not claim physical keyboard or global WindowServer routing coverage.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess

from check_sequencer_commands import check as check_commands
from check_sequencer_distribution import check as check_distribution
from check_sequencer_transport import check as check_transport


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--sequence", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    assert not out.exists(), "Use a fresh output directory"
    out.mkdir(parents=True)
    scripts = Path(__file__).resolve().parent / "ui"
    for name, checker in [("transport", check_transport), ("commands", check_commands),
                          ("distribution", check_distribution)]:
        case = out / name
        case.mkdir()
        profile = case / "profile"
        shutil.copytree(args.profile, profile, symlinks=True)
        (profile / ".lock").unlink(missing_ok=True)
        sequence = profile / "Sequence/Ruler.tim"
        sequence.parent.mkdir(exist_ok=True)
        if args.sequence:
            shutil.copyfile(args.sequence, sequence)
        ui = case / "ui"
        env = {key: value for key, value in os.environ.items() if not key.startswith(("VIXEN_", "DYLD_"))}
        env.update(VIXEN_UI_TEST=str(scripts / f"sequencer-{name}.json"), VIXEN_UI_TEST_OUT=str(ui),
                   VIXEN_DATA=str(profile), VIXEN_MUTE="1")
        env["VIXEN_TEST_SEQUENCE" if args.sequence else "VIXEN_TEST_SAVE_AS"] = str(sequence)
        with (case / "process.log").open("w") as log:
            result = subprocess.run([str(args.app.resolve() / "Contents/MacOS/Vixen")], env=env,
                                    stdout=log, stderr=subprocess.STDOUT, timeout=60)
        assert result.returncode == 0, (name, result.returncode)
        checker(ui, sequence) if name == "commands" else checker(ui)
    (out / "summary.json").write_text(json.dumps({"passed": True, "nativeSteps": 183,
                                                  "app": str(args.app.resolve())}, indent=2))


if __name__ == "__main__":
    main()
