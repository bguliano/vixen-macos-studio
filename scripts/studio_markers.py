#!/usr/bin/env python3
"""Inventory of Studio-marked changes, so merges from vanilla can't silently drop them.

    scripts/studio_markers.py record   # write studio/markers.lock from the working tree
    scripts/studio_markers.py check    # compare the working tree against studio/markers.lock

Blocks are `STUDIO-BEGIN <name>: <why>` ... `STUDIO-END <name>`, behind any comment leader. One-line changes carry a
trailing `STUDIO: <why>`. See STUDIO.md.
"""
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LOCK = ROOT / "studio" / "markers.lock"
BEGIN = re.compile(r"STUDIO-BEGIN ([a-z0-9][a-z0-9-]*)\s*:")
END = re.compile(r"STUDIO-END ([a-z0-9][a-z0-9-]*)")
LINE = re.compile(r"(?<![-A-Z])STUDIO: ")
SKIP_PREFIXES = ("upstream/", "studio/markers.lock")
# This file and the docs describe the markers rather than carry them.
SKIP_FILES = {"scripts/studio_markers.py", "STUDIO.md"}


def tracked_files():
    out = subprocess.run(["git", "ls-files", "-z"], cwd=ROOT, check=True, capture_output=True).stdout
    for name in out.decode().split("\0"):
        if name and not name.startswith(SKIP_PREFIXES) and name not in SKIP_FILES:
            yield name


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()[:16]


def scan():
    """Returns (markers, errors). Markers map a stable key to its file and content hash."""
    markers, errors = {}, []
    for name in tracked_files():
        path = ROOT / name
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, FileNotFoundError, IsADirectoryError):
            continue
        if "STUDIO" not in text:
            continue
        open_blocks = {}
        for number, line in enumerate(text.splitlines(), 1):
            if m := BEGIN.search(line):
                block = m.group(1)
                if block in open_blocks:
                    errors.append(f"{name}:{number}: block '{block}' opened twice")
                open_blocks[block] = (number, [])
                continue
            if m := END.search(line):
                block = m.group(1)
                if block not in open_blocks:
                    errors.append(f"{name}:{number}: STUDIO-END '{block}' without a STUDIO-BEGIN")
                    continue
                start, body = open_blocks.pop(block)
                key = f"block:{block}"
                if key in markers:
                    errors.append(f"{name}:{start}: block name '{block}' is also used in {markers[key]['file']}")
                markers[key] = {"file": name, "hash": digest("\n".join(body))}
                continue
            for _, body in open_blocks.values():
                body.append(line)
            if LINE.search(line) and not open_blocks:
                key = f"line:{name}:{digest(line.strip())}"
                markers[key] = {"file": name, "hash": digest(line.strip())}
        for block, (start, _) in open_blocks.items():
            errors.append(f"{name}:{start}: block '{block}' is never closed")
    return markers, errors


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else "check"
    markers, errors = scan()
    if errors:
        print("Malformed Studio markers:", *errors, sep="\n  ")
        return 1
    if mode == "record":
        LOCK.parent.mkdir(exist_ok=True)
        LOCK.write_text(json.dumps(dict(sorted(markers.items())), indent=1) + "\n")
        print(f"Recorded {len(markers)} Studio markers in {LOCK.relative_to(ROOT)}")
        return 0
    recorded = json.loads(LOCK.read_text()) if LOCK.exists() else {}
    missing = [k for k in recorded if k not in markers]
    changed = [k for k in recorded if k in markers and markers[k]["hash"] != recorded[k]["hash"]]
    unrecorded = [k for k in markers if k not in recorded]
    for label, keys in (("Missing (dropped by the merge?)", missing), ("Changed", changed), ("Not yet recorded", unrecorded)):
        if keys:
            print(f"{label}:")
            for k in keys:
                print(f"  {k}  ({(recorded.get(k) or markers[k])['file']})")
    if missing or changed:
        print("Restore the Studio side of these blocks, or run scripts/studio_sync.sh --record if the change is intended.")
        return 1
    if unrecorded:
        print("Run scripts/studio_sync.sh --record to add new markers to the inventory.")
        return 1
    print(f"All {len(markers)} Studio markers intact.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
