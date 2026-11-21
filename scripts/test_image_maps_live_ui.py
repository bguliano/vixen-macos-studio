#!/usr/bin/env python3
"""Read-only live-render observations around actual Image Maps/Face UI edits.

Requires the guarded effectEditor.renderProbe and existing imageMaps hooks.
The only authoring hook enters rename; Return/Escape still travel through AppKit.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess

from test_image_maps_ui import opening, maps, key
from test_video_cache_ui import wait, hook, capture


def summary(value):
    parsed = {}
    for effect in value["effects"]:
        match = re.fullmatch(r"generation (\d+) rendered (\d+) elements (\d+) intents (\d+) sampleMs 50 inspected (\d+) active (\d+) nonColor (\d+) truncated false colors \[(.*)\]", effect["summary"])
        assert match, effect
        fields = dict(zip(("generation","rendered","elements","intents","inspected","active","nonColor"), map(int, match.groups()[:7])))
        fields["colors"] = {key:int(count) for key,count in (item.split(":") for item in match[8].split(",") if item)}
        assert fields["generation"] == fields["rendered"]
        parsed[str(effect["id"])] = fields
    assert set(parsed) == {"1","2","3"}, parsed
    return parsed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--stage", choices=["map","face-cancel","face-accept","rename"], required=True)
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    profile, out = args.profile.resolve(), args.out.resolve()
    fixture = json.loads((profile / "fixture.json").read_text())
    assert "target" in profile.parts and fixture["controllers"]==0 and fixture["muted"] and fixture["live"]
    assert not out.exists()
    out.mkdir(parents=True)
    click = lambda text: {"action":"click","label":text}
    select = lambda text: {"action":"selectRow","label":text}
    observe = lambda: [hook("effectEditor.renderProbe")]
    sequence = profile / "Sequence/Image Maps Verification.tim"
    start = opening(sequence) + [{"action":"menu","path":["Edit","Select All Elements"]}, hook("sequencer.panel","Effect Editor"), wait()] + observe()
    finish = [hook("timelineState"),hook("sequencer.state"),{"action":"quit"}]
    if args.stage == "map":
        edit = lambda: [select("Active Map"),click("Edit"),wait(),hook("marksNextFile",str(profile / "Inputs/green.png")),click("File"),wait()]
        steps = start + maps() + edit() + [click("Cancel"),wait()] + observe() \
            + edit() + [click("OK"),wait()] + observe() + [click("OK"),wait(2)] + observe() \
            + capture("accepted") + [{"action":"menu","path":["Sequence","Save"]},wait()] + finish
    elif args.stage.startswith("face-"):
        # STUDIO-BEGIN face-mapping-check: OK is Set Up Faces… (R40), clicked where it sits; Studio's own check is check_studio_face_mapping_live.py
        steps = start + [{"action":"menu","path":["Tools","LipSync","Edit Element Face Mapping"]},wait(),
                hook("faceNodes","selectTree","Face Mouth"),hook("faceNodes","add"),wait(),
                {"action":"mouse","x":485,"y":292},wait()] \
            + capture("before-face") + [{"action":"mouse","x":165,"y":172},wait()] + capture("face-checked") \
            + [{"action":"mouse","x":810 if args.stage=="face-cancel" else 748,"y":388},wait(2),hook("faceNodes","state")] + observe() + capture("after-face") + finish
        # STUDIO-END face-mapping-check
    else:
        steps = start + maps() + key("escape") + [{"action":"windows"}] + maps() + key("return") + [{"action":"windows"}] + maps() \
            + [hook("imageMaps","beginRename","Rename Me"),wait(),{"action":"key","key":"a","modifiers":["cmd"]},
               {"action":"type","text":"Discarded Rename"}] + key("escape") + [hook("imageMaps","state")] + capture("rename-cancel") \
            + [{"action":"selectRow","label":"Rename Me"},wait(),hook("imageMaps","state")] + capture("rename-reselected") \
            + [hook("imageMaps","beginRename","Rename Me"),wait(),{"action":"key","key":"a","modifiers":["cmd"]},
               {"action":"type","text":"Renamed Map"}] + key("return") + [hook("imageMaps","state")] + capture("rename-commit") \
            + [{"action":"mouse","x":338,"y":272},wait(2),{"action":"windows"}] + observe() + finish
    script = out / "steps.json"
    script.write_text(json.dumps({"steps":steps},indent=2)+"\n")
    if args.prepare_only:
        print(script)
        return
    app = args.app.resolve() / "Contents/MacOS/Vixen"
    digest = hashlib.sha256(app.read_bytes()).hexdigest()
    original = {str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (profile / "SystemData").glob("*.xml")}
    ui = out / "ui"
    ui.mkdir()
    runtime = out / "runtime"
    runtime.mkdir()
    env = {k:v for k,v in os.environ.items() if not k.startswith(("VIXEN_","DYLD_"))}
    env.update(VIXEN_UI_TEST=str(script),VIXEN_UI_TEST_OUT=str(ui),VIXEN_UI_TEST_TRACE="1",VIXEN_DATA=str(profile),VIXEN_MUTE="1",TMPDIR=str(runtime)+"/")
    with (out / "process.log").open("w") as log:
        process = subprocess.Popen([str(app)],env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
        try: process.wait(timeout=110)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid,signal.SIGKILL)
            process.wait()
            raise
    rows = json.loads((ui / "result.json").read_text())
    report = {"passed":False,"stage":args.stage,"exitCode":process.returncode,"steps":len(rows),"appSha256":digest,"profile":str(profile)}
    (out / "result.json").write_text(json.dumps(report,indent=2)+"\n")
    assert process.returncode==0 and len(rows)==len(steps)-1 and all(row["ok"] for row in rows), [r for r in rows if not r["ok"]]
    probes = [summary(json.loads(row["detail"])) for row in rows if '"effects":' in row.get("detail","")]
    assert len(probes)==(4 if args.stage=="map" else 2), probes
    before, after = probes[0], probes[-1]
    if args.stage in ("map","face-accept","rename"):
        assert all(after[k]["generation"]>before[k]["generation"] for k in ("1","2")), probes
        assert after["3"]==before["3"], probes
    if args.stage=="map":
        assert probes[1]==before and probes[2]==before, probes
        old,new = before["1"]["colors"],after["1"]["colors"]
        assert old.get("FFFF0000",0)>0 and "FF00FF00" not in old
        assert new.get("FF00FF00",0)==old["FFFF0000"] and "FFFF0000" not in new
        assert after["2"]["colors"]==before["2"]["colors"]
    elif args.stage=="face-cancel":
        assert before==after
        assert any(row.get("detail")=="(closed) lastClose false" or row.get("detail","").endswith("faceSetup=false rendered=-") for row in rows)
        assert original=={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in (profile / "SystemData").glob("*.xml")}
    elif args.stage=="face-accept":
        assert any(row.get("detail")=="(closed) lastClose true" or row.get("detail","").endswith("faceSetup=false rendered=2") for row in rows)
        assert before["2"]["colors"]=={} and after["2"]["colors"]=={"FFFFFFFF":1}, probes
        assert before["1"]["colors"]==after["1"]["colors"]
    else:
        states = [row["detail"] for row in rows if row.get("detail","").startswith("rows [")]
        assert len(states)==3 and "Rename Me" in states[0] and "Discarded Rename" not in states[0], states
        assert "Rename Me" in states[1] and states[1].endswith("new=true edit=true remove=true clone=true"), states
        assert "Renamed Map" in states[2] and "Rename Me" not in states[2], states
        assert all(states[i].endswith("new=true edit=false remove=false clone=false") for i in (0,2)), states
        windows = [row["detail"] for row in rows if row["action"]=="windows"]
        assert len(windows)==3 and all("Untitled" not in value and "Image Maps Verification" in value for value in windows)
        assert all(before[k]["colors"]==after[k]["colors"] for k in before)
        report["mapStates"] = states
    assert hashlib.sha256(app.read_bytes()).hexdigest()==digest
    report.update(passed=True,probes=probes,limits=["Rename-start and node selection supplied by existing hooks; commit/cancel keys and Face checkbox use native events",
        "Live cached intents sampled read-only at50ms; no physical hardware/drag/file-panel or external-editor claim"])
    if args.stage=="rename":
        report["limits"].append("Reselection uses actual NSTableView.selectRowIndexes; physical same-row click remains unverified offscreen")
    (out / "result.json").write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps({"passed":True,"steps":len(rows),"report":str(out / "result.json")}))


if __name__ == "__main__": main()
