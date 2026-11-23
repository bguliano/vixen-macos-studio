#!/usr/bin/env python3
"""Verify the show window's Schedule pane (it replaced the Shows and Scheduler windows).

Usage: check_studio_schedule.py <UI output directory> <profile the run used>
Harness script: scripts/ui/studio-schedule.json, run on a copy of target/show-copy.

Checks, from the pane's state after each step and from the saved profile:
- the show's existing show and schedule are listed, the schedule as ended (its last day was in 2019);
- Add Show adds "New Show" and starts renaming it; typing a name and Return renames it;
- steps added to two sections and reordered list in run order, each edit one labelled undo step;
- Add Schedule schedules the selected show from today, on and upcoming (or due, if the check runs inside its hours);
  its end time and days change in place;
- its On switch turns it off ("Turn Off Schedule");
- Edit ▸ Undo (the window's undo manager), the pane's Undo button and Edit ▸ Redo walk those steps;
- deleting the show deletes its schedule in the same step, and Undo brings both back;
- Tools ▸ Scheduler ▸ Schedules shows this section, not the Scheduler window;
- the profile on disk has the show with its steps and the schedule as left.

SwiftUI toolbar buttons act on the harness's press but report that they didn't, so step flags are not checked.
"""
import json
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path


def check(out: Path, profile: Path) -> None:
    steps = json.loads((out / "result.json").read_text())
    states = [s["detail"] for s in steps if s["action"] == "studioScheduleState"]
    section = [s["detail"] for s in steps if s["action"] == "studioSectionNow"]
    windows = [s["detail"] for s in steps if s["action"] == "windows"]
    assert len(states) == 12, len(states)

    def part(state: str, index: int) -> str:
        return state.split(" | ")[index]

    def field(state: str, name: str) -> str:
        return re.search(rf"\b{name}=(.*?)(?= \w+=|$)", part(state, 2)).group(1)

    def front_yard(state: str) -> str:
        found = [s for s in part(state, 1).removeprefix("schedules: ").split("; ") if s.startswith("Front Yard ")]
        return found[0] if found else ""

    # An enabled schedule that hasn't ended reads "upcoming", or "due" while the clock is inside its days and hours.
    def active(state: str, ending: str) -> bool:
        return re.fullmatch(rf".*{re.escape(ending)} on (upcoming|due)", front_yard(state)) is not None

    (initial, added, renamed, steps_state, scheduled, edited, toggled, undo_menu, undo_button, redo, deleted, restored) = states
    original = "Christmas Lights 2018[1]: sequential=Wizards in Winter,Christmas Eve Sarajevo,Carey Grinch shutdown=Cameron-Zoom"
    assert part(initial, 0) == f"shows: {original}", initial
    assert "Christmas Lights 2018 2018-12-25..2019-01-01 19:07-19:11 Every day on ended" in part(initial, 1), initial
    assert part(added, 0).endswith("; New Show[0]: ") and field(added, "renaming") == "yes", added
    assert field(added, "undo") == "Add Show", added
    assert "Front Yard[0]" in part(renamed, 0) and field(renamed, "undo") == "Rename Show", renamed
    assert "Front Yard[0]: startup=Cameron-Zoom sequential=Pause for 5 s,Wizards in Winter" in part(steps_state, 0), steps_state
    assert field(steps_state, "undo") == "Reorder Show", steps_state
    assert re.fullmatch(r"Front Yard \d{4}-\d\d-\d\d\.\.\d{4}-\d\d-\d\d 17:30-22:00 Every day on (upcoming|due)", front_yard(scheduled)), scheduled
    assert field(scheduled, "undo") == "Add Schedule" and field(scheduled, "selected") == "schedule:1", scheduled
    assert active(edited, "17:30-23:15 Fri, Sat"), edited
    assert field(edited, "undo") == "Change Schedule", edited
    assert front_yard(toggled).endswith("Fri, Sat off off"), toggled
    assert field(toggled, "undo") == "Turn Off Schedule" and field(toggled, "menu").startswith("Undo Turn Off Schedule/"), toggled
    assert active(undo_menu, " Fri, Sat") and field(undo_menu, "redo") == "Turn Off Schedule", undo_menu
    assert active(undo_button, "17:30-23:15 Every day"), undo_button
    assert active(redo, "17:30-23:15 Fri, Sat"), redo
    assert part(deleted, 0) == f"shows: {original}" and front_yard(deleted) == "", deleted
    assert field(deleted, "undo") == "Delete Show", deleted
    assert active(restored, " Fri, Sat") and "Front Yard[1]" in part(restored, 0), restored
    assert field(restored, "redo") == "Delete Show", restored
    assert section == ["schedule"], section
    assert windows and "Scheduler" not in windows[0] and "Shows " not in windows[0], windows

    # The profile on disk: the restored show and schedule, saved after the last step.
    root = ET.parse(profile / "SystemData" / "ModuleStore.xml").getroot()
    local = lambda e: e.tag.rsplit("}", 1)[-1]
    def child(e, name):
        return next((c for c in e if local(c) == name), None)
    shows = [e for e in root.iter() if local(e) == "Show"]
    yard = [s for s in shows if child(s, "Name") is not None and child(s, "Name").text == "Front Yard"]
    assert len(yard) == 1, [child(s, "Name").text for s in shows]
    items = [(child(i, "ItemType").text, int(child(i, "ItemOrder").text), child(i, "Name").text) for i in child(yard[0], "Items")]
    assert sorted(items) == [("Sequential", 0, "Pause for 5"), ("Sequential", 1, "Run sequence: Wizards in Winter.tim"),
                             ("Startup", 0, "Run sequence: Cameron-Zoom.tim")], items
    yard_id = child(yard[0], "ID").text
    schedules = [e for e in root.iter() if local(e) == "ScheduleItem" and child(e, "ShowID").text == yard_id]
    assert len(schedules) == 1, len(schedules)
    s = schedules[0]
    days = [child(s, d).text for d in ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"]]
    assert days == ["false"] * 5 + ["true"] * 2, days
    assert child(s, "Enabled").text == "true", child(s, "Enabled").text
    assert child(s, "StartTime").text.endswith("T17:30:00") and child(s, "EndTime").text.endswith("T23:15:00")
    assert child(s, "EndDate").text.endswith("-01-06T00:00:00"), child(s, "EndDate").text
    print("PASS")


if __name__ == "__main__":
    check(Path(sys.argv[1]), Path(sys.argv[2]))
