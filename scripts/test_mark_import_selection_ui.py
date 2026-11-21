#!/usr/bin/env python3
"""Bounded private native check of imported-mark Space selection; no app hooks added."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import xml.etree.ElementTree as ET

from PIL import Image

from test_video_cache_ui import hook, wait, capture


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, default=Path("target/build-release/Vixen.app"))
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--mode", choices=["accept", "cancel"], default="accept")
    args = parser.parse_args()
    profile, out = args.profile.resolve(), args.out.resolve()
    assert "target" in profile.parts and not out.exists()
    fixture = json.loads((profile / "fixture.json").read_text())
    assert fixture["controllers"] == 0 and fixture["muted"]
    sequence = Path(fixture["sequence"]).resolve()
    source = Path(fixture["input"]).resolve()
    assert sequence.is_relative_to(profile) and source.is_relative_to(profile)
    app = args.app.resolve() / "Contents/MacOS/Vixen"
    digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    hashes = {str(p): digest(p) for p in (sequence, source, app)}
    out.mkdir(parents=True)
    click = lambda label: {"action": "click", "label": label}
    key = lambda value: [{"action": "key", "key": value}, wait()]
    opening = [{"action": "window", "id": "administration"}, wait(2),
        hook("admin.openSequence", str(sequence)), wait(3),
        {"action": "focus", "window": "Mark Import Verification"},
        hook("sequencer.activate", "Mark Import Verification"), hook("sequencer.resize", "1400", "900"),
        hook("sequencer.panel", "Mark Collections"), wait(), hook("marksRows", "initial")]
    # STUDIO-BEGIN marks-import-menu-check: Studio's Import is a menu of file types (R40), not the Import Type dialog
    def begin_import():
        # The panel toolbar's Import menu, where scripts/ui/studio-marks-live.json finds it in a 1400x900 sequencer.
        return [hook("marksNextFile", str(source)), {"action": "popup", "x": 1107, "y": 117, "label": "Vixen 3"},
                wait(), {"action": "focus", "window": "Mark Import Verification"}]
    # STUDIO-END marks-import-menu-check
    space = lambda: [hook("sequencer.keyEvent", "space"), wait()]
    select = lambda name: [{"action": "selectRow", "label": name}, wait()]
    steps = opening + begin_import() + capture("all-checked") + space() + capture("first-space")
    if args.mode == "cancel":
        steps += [hook("timelineState")] + key("escape") + [hook("marksRows", "after-cancel")]
    else:
        steps += select("Alpha Source") + space() + capture("second-space") \
            + select("Middle Source") + space() + capture("all-unchecked") \
            + key("return") + capture("unchecked-return") + [hook("marksRows", "empty-return"), hook("timelineState")] \
            + select("Middle Source") + space() + capture("last-included") \
            + select("Zebra Source") + space() + capture("subset") \
            + key("return") + [wait(), hook("marksRows", "committed"), {"action": "menu", "path": ["Sequence", "Save"]}, wait(), hook("marksRows", "saved"), hook("timelineState")]
    steps += [{"action": "quit"}]
    script = out / "steps.json"
    script.write_text(json.dumps({"steps": steps}, indent=2) + "\n")
    ui, runtime = out / "ui", out / "runtime"
    ui.mkdir(); runtime.mkdir()
    env = {k: v for k, v in os.environ.items() if not k.startswith(("VIXEN_", "DYLD_"))}
    env.update(VIXEN_DATA=str(profile), VIXEN_MUTE="1", VIXEN_UI_TEST=str(script),
               VIXEN_UI_TEST_OUT=str(ui), VIXEN_UI_TEST_TRACE="1", TMPDIR=str(runtime)+"/")
    with (out / "process.log").open("w") as log:
        process = subprocess.Popen([str(app)], env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        try: process.wait(timeout=110)
        except BaseException:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            raise
    rows = json.loads((ui / "result.json").read_text())
    report = {"passed": False, "mode":args.mode, "exitCode": process.returncode, "steps": len(rows), "inputHashes": hashes}
    (out / "result.json").write_text(json.dumps(report, indent=2)+"\n")
    assert process.returncode == 0 and len(rows) == len(steps)-1 and all(r["ok"] for r in rows), [r for r in rows if not r["ok"]]
    assert digest(source) == hashes[str(source)] and digest(app) == hashes[str(app)]
    for row in rows:
        if '"transport"' in row.get("detail", ""):
            assert json.loads(row["detail"])["transport"] == "stopped", row
    def state(name): return json.loads((ui / (name+".json")).read_text())
    def bitmap(name):
        im = Image.open(ui / (name+".png")).convert("RGB")
        assert im.size in ((600,400),(1200,800)), (name,im.size)
        return im.resize((600,400), Image.Resampling.BOX)
    def picture(name, checks, ok):
        im = bitmap(name)
        counts = [sum(min(p)>150 for p in im.crop((32,y-8,49,y+9)).get_flattened_data()) for y in (90,114,138)]
        assert [n>15 for n in counts] == checks, (name,counts)
        assert all(n==0 if not checked else n>15 for n,checked in zip(counts,checks)), (name,counts)
        ok_count = sum(min(p)>150 for p in im.crop((474,365,503,381)).get_flattened_data())
        assert (ok_count>20) == ok, (name,ok_count)
        return {"checkmarkPixels":counts,"okTextPixels":ok_count}
    pictures = {"all-checked":picture("all-checked",[True,True,True],True),
                "first-space":picture("first-space",[False,True,True],True)}
    # Initial selection is visible before any test selection: first row only.
    initial = bitmap("all-checked")
    assert initial.getpixel((300,90))[0] > initial.getpixel((300,114))[0]+20
    if args.mode == "cancel":
        assert digest(sequence) == hashes[str(sequence)]
        assert state("initial") == state("after-cancel")
    else:
        for name,checks,ok in [("second-space",[False,False,True],True),
                               ("all-unchecked",[False,False,False],False),
                               ("unchecked-return",[False,False,False],False),
                               ("last-included",[False,False,True],True),
                               ("subset",[True,False,True],True)]:
            pictures[name] = picture(name,checks,ok)
        assert state("empty-return") == state("initial")
        selected = [fixture["candidates"][i] for i in (0,2)]
        committed = state("committed")
        assert committed["modified"] and [r["name"] for r in committed["rows"]] == [c["name"] for c in selected]
        assert not state("saved")["modified"]
        assert state("saved")["rows"] == state("committed")["rows"]
        root = ET.parse(sequence).getroot()
        local = lambda tag: tag.split("}")[-1]
        children = lambda node: {local(e.tag):e for e in node}
        collections = list(next(e for e in root if local(e.tag)=="LabeledMarkCollections"))
        assert len(collections)==2
        for element, wanted in zip(collections,selected):
            values = children(element)
            assert values["Name"].text==wanted["name"] and values["Id"].text==wanted["id"]
            marks = list(values["Marks"])
            assert len(marks)==1
            mark = children(marks[0])
            assert mark["Text"].text==wanted["text"]
            assert mark["StartTime"].text == f"PT{wanted['startTicks']//10_000_000}S"
            assert mark["Duration"].text == "PT0.45S"
        report["savedOrder"] = [c["name"] for c in selected]
    report.update(passed=True, pictures=pictures, scope="CG-posted Space, native table selection and Return/Escape; queued file result, no system file-panel/physical pointer claim")
    (out / "result.json").write_text(json.dumps(report, indent=2)+"\n")
    print(json.dumps({"passed": True, "steps": len(rows), "report": str(out / "result.json")}))


if __name__ == "__main__": main()
