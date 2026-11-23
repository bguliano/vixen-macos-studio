//! The show window's Schedule pane and the matching MCP tools: shows (their sequences and pauses) and schedules (when a show
//! runs), as live edits on the profile's shared undo history (`studio_display`), plus the pane's list with each schedule's next
//! run.
//!
//! Shows and schedules live only in the module data, which the output graph isn't built from, so an edit publishes without
//! `apply_display`: nothing playing stops, no controller restarts, and it works while Live Output is on. The running scheduler
//! (`scheduler::run::Scheduler`) keeps its own copy of the schedules; every publish hands it the new list, so a change applies
//! at once and a show it is running keeps running unless its schedule was deleted or changed.
//!
//! Studio never creates or edits a Launch item (a show step that runs another program); one made in Windows Vixen is listed,
//! and can be moved or removed. Turning the whole scheduler on stays a local switch (Tools ▸ Scheduler ▸ Enabled), not a command.
use crate::app::AppCore;
use crate::scheduler::{self, run::State, Date, ScheduleItem, SchedulerData, TimeOfDay};
use crate::shows::{self, ActionType, Show, ShowItem, ShowItemType};
use serde_json::{json, Value};
use std::path::Path;
use uuid::Uuid;
use vixen_io::datetime::LocalDateTime;
use vixen_io::SystemData;

/// The commands that publish through [`publish`] instead of rebuilding the display.
pub const COMMANDS: [&str; 13] = [
    "show_create",
    "show_rename",
    "show_delete",
    "show_item_add",
    "show_item_update",
    "show_item_move",
    "show_item_delete",
    "schedule_create",
    "schedule_update",
    "schedule_enable",
    "schedule_duplicate",
    "schedule_delete",
    "show_duplicate",
];

pub fn is_command(command: &str) -> bool {
    COMMANDS.contains(&command)
}

/// The undo history's label, in the words the pane and Edit ▸ Undo show.
pub fn label(command: &str, args: &Value) -> &'static str {
    match command {
        "show_create" => "Add Show",
        "show_rename" => "Rename Show",
        "show_duplicate" => "Duplicate Show",
        "show_delete" => "Delete Show",
        "show_item_add" => "Add to Show",
        "show_item_update" => "Change Show Step",
        "show_item_move" => "Reorder Show",
        "show_item_delete" => "Remove from Show",
        "schedule_create" => "Add Schedule",
        "schedule_update" => "Change Schedule",
        "schedule_duplicate" => "Duplicate Schedule",
        "schedule_delete" => "Delete Schedule",
        _ if args["enabled"] == true => "Turn On Schedule",
        _ => "Turn Off Schedule",
    }
}

/// The sections of a show, in the order a run takes them, with the names the pane and the tools use.
pub const SECTIONS: [(&str, ShowItemType); 4] = [
    ("startup", ShowItemType::Startup),
    ("sequential", ShowItemType::Sequential),
    ("background", ShowItemType::Background),
    ("shutdown", ShowItemType::Shutdown),
];

fn section(args: &Value) -> Result<ShowItemType, String> {
    let name = args["section"].as_str().unwrap_or_default();
    SECTIONS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| t.clone())
        .ok_or("section must be startup, sequential, background or shutdown".into())
}

fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or(format!("{key} must be a nonempty string of at most 256 bytes"))
}

fn index(args: &Value, key: &str) -> Result<usize, String> {
    args[key].as_u64().map(|n| n as usize).ok_or(format!("{key} must be a nonnegative integer"))
}

fn show_id(shows: &[Show], args: &Value, key: &str) -> Result<Uuid, String> {
    let id = Uuid::parse_str(text(args, key)?).map_err(|_| format!("{key} must be a show UUID"))?;
    shows.iter().any(|s| s.id == id).then_some(id).ok_or("Show not found".into())
}

/// `base`, or `base 2`, `base 3`… so a new show never repeats a name.
fn unique_name(shows: &[Show], base: &str) -> String {
    let taken = |n: &str| shows.iter().any(|s| s.name_text() == n);
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base} {n}")).find(|n| !taken(n)).unwrap()
}

/// A sequence a show may run: a `.tim` file under the profile's Sequence folder, named relative to it, that exists.
fn sequence_name(file: &str, sequence_dir: &Path) -> Result<String, String> {
    let parts: Vec<&str> = file.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    if parts.is_empty()
        || file.len() > 256
        || parts.iter().any(|p| *p == ".." || *p == "." || p.starts_with('.'))
        || !file.to_lowercase().ends_with(".tim")
    {
        return Err(format!("“{file}” isn't a .tim file in the Sequence folder"));
    }
    let path = parts.iter().fold(sequence_dir.to_path_buf(), |p, part| p.join(part));
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => Ok(parts.join("/")),
        _ => Err(format!("No sequence “{file}” in the Sequence folder")),
    }
}

/// Every sequence a show can run, relative to the Sequence folder, sorted by name. Backups (`.tim.6`, `_backup.…`) don't end
/// in `.tim`; the hidden scratch folder is skipped.
pub fn sequence_files(sequence_dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() && depth < 3 {
                walk(&entry.path(), &format!("{prefix}{name}/"), depth + 1, out);
            } else if kind.is_file() && name.to_lowercase().ends_with(".tim") {
                out.push(format!("{prefix}{name}"));
            }
        }
    }
    let mut out = Vec::new();
    walk(sequence_dir, "", 0, &mut out);
    out.sort_by_key(|n| n.to_lowercase());
    out
}

/// A section's items in run order, as positions in `show.items`.
fn positions(show: &Show, kind: &ShowItemType) -> Vec<usize> {
    let mut found: Vec<usize> = (0..show.items.len()).filter(|i| show.items[*i].item_type == *kind).collect();
    found.sort_by_key(|i| show.items[*i].item_order);
    found
}

/// Replaces a section's items with `items`, in this order, numbered 0, 1, 2… as the Show Editor numbers a tab.
fn set_section(show: &mut Show, kind: &ShowItemType, mut items: Vec<ShowItem>) {
    let first = show.items.iter().position(|i| i.item_type == *kind).unwrap_or(show.items.len());
    let before = show.items[..first].iter().filter(|i| i.item_type != *kind).count();
    show.items.retain(|i| i.item_type != *kind);
    for (order, item) in items.iter_mut().enumerate() {
        item.item_order = order as i32;
    }
    show.items.splice(before..before, items);
}

/// Sets an item to run `sequence` or pause `pauseSeconds`, named as the Show Editor names it.
fn set_action(item: &mut ShowItem, args: &Value, sequence_dir: &Path) -> Result<(), String> {
    match (args.get("sequence"), args.get("pauseSeconds")) {
        (Some(_), None) => {
            let name = sequence_name(text(args, "sequence")?, sequence_dir)?;
            item.action = ActionType::Sequence;
            item.sequence_file_name = Some(name);
        }
        (None, Some(seconds)) => {
            let seconds = seconds
                .as_i64()
                .filter(|s| (i64::from(shows::PAUSE_MIN)..=i64::from(shows::PAUSE_MAX)).contains(s))
                .ok_or(format!("pauseSeconds must be {}…{}", shows::PAUSE_MIN, shows::PAUSE_MAX))?;
            item.action = ActionType::Pause;
            item.pause_seconds = seconds as i32;
        }
        _ => return Err("Give either sequence or pauseSeconds".into()),
    }
    item.name = item.editor_name();
    Ok(())
}

fn parse_date(args: &Value, key: &str) -> Result<Option<Date>, String> {
    let Some(value) = args.get(key) else { return Ok(None) };
    let text = value.as_str().unwrap_or_default();
    let parts: Vec<&str> = text.split('-').collect();
    let parsed = (parts.len() == 3)
        .then(|| Some((parts[0].parse::<i32>().ok()?, parts[1].parse::<u8>().ok()?, parts[2].parse::<u8>().ok()?)))
        .flatten()
        .filter(|(y, m, d)| (2000..=2999).contains(y) && (1..=12).contains(m) && *d >= 1 && *d <= days_in_month(*y, *m));
    parsed
        .map(|(year, month, day)| Some(Date { year, month, day }))
        .ok_or(format!("{key} must be a date like 2026-12-01"))
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn parse_time(args: &Value, key: &str) -> Result<Option<TimeOfDay>, String> {
    let Some(value) = args.get(key) else { return Ok(None) };
    let parts: Vec<&str> = value.as_str().unwrap_or_default().split(':').collect();
    let number = |i: usize, max: u8| parts.get(i).and_then(|p| p.parse::<u8>().ok()).filter(|n| *n <= max);
    match (parts.len(), number(0, 23), number(1, 59)) {
        (2 | 3, Some(hour), Some(minute)) => {
            let second = if parts.len() == 3 { number(2, 59) } else { Some(0) };
            second.map(|second| Some(TimeOfDay { hour, minute, second })).ok_or(format!("{key} must be a time like 17:30"))
        }
        _ => Err(format!("{key} must be a time like 17:30")),
    }
}

/// Applies the fields given to a schedule; the rest keep their values.
fn apply_fields(item: &mut ScheduleItem, shows: &[Show], args: &Value) -> Result<(), String> {
    if args.get("showId").is_some() {
        item.show_id = show_id(shows, args, "showId")?;
    }
    if let Some(date) = parse_date(args, "startDate")? {
        item.start_date = date;
    }
    if let Some(date) = parse_date(args, "endDate")? {
        item.end_date = date;
    }
    if let Some(time) = parse_time(args, "startTime")? {
        item.start_time = time;
    }
    if let Some(time) = parse_time(args, "endTime")? {
        item.end_time = time;
    }
    if let Some(days) = args.get("days") {
        let days = days.as_array().filter(|d| d.len() == 7).ok_or("days must be seven booleans, Sunday first")?;
        for (slot, day) in item.days.iter_mut().zip(days) {
            *slot = day.as_bool().ok_or("days must be seven booleans, Sunday first")?;
        }
    }
    if let Some(on) = args.get("enabled") {
        item.enabled = on.as_bool().ok_or("enabled must be true or false")?;
    }
    if item.start_date > item.end_date {
        return Err("The last day can't come before the first day".into());
    }
    Ok(())
}

fn schedule_index(data: &SchedulerData, args: &Value) -> Result<usize, String> {
    let i = index(args, "index")?;
    (i < data.items.len()).then_some(i).ok_or("Schedule not found".into())
}

fn save_schedules(system: &mut SystemData, data: &SchedulerData) {
    scheduler::save_scheduler(&mut system.module_store, data, &LocalDateTime::now());
}

/// One show or schedule edit on `system`. Returns the result and whether anything changed.
pub fn edit(system: &mut SystemData, sequence_dir: &Path, command: &str, args: &Value) -> Result<(Value, bool), String> {
    let mut list = shows::load_shows(&system.module_store);
    let mut data = scheduler::load_scheduler(&system.module_store);
    let result = match command {
        "show_create" => {
            let name = match args.get("name") {
                Some(_) => text(args, "name")?.to_string(),
                None => unique_name(&list, "New Show"),
            };
            let mut show = Show { id: Uuid::new_v4(), name: Some(name.clone()), items: Vec::new() };
            let files = match args.get("sequences") {
                Some(files) => files.as_array().filter(|f| f.len() <= 100).ok_or("sequences must list at most 100 files")?.clone(),
                None => Vec::new(),
            };
            let mut items = Vec::new();
            for file in files {
                let mut item = ShowItem::new(ShowItemType::Sequential, "");
                set_action(&mut item, &json!({ "sequence": file }), sequence_dir)?;
                items.push(item);
            }
            set_section(&mut show, &ShowItemType::Sequential, items);
            let id = show.id;
            list.push(show);
            json!({"showId":id.to_string(),"name":name})
        }
        "show_rename" => {
            let id = show_id(&list, args, "showId")?;
            let name = text(args, "name")?;
            let show = list.iter_mut().find(|s| s.id == id).unwrap();
            if show.name_text() == name {
                return Ok((json!({"showId":id.to_string(),"name":name}), false));
            }
            show.name = Some(name.to_string());
            json!({"showId":id.to_string(),"name":name})
        }
        "show_duplicate" => {
            let id = show_id(&list, args, "showId")?;
            let index = list.iter().position(|s| s.id == id).unwrap();
            let mut copy = list[index].clone();
            copy.id = Uuid::new_v4();
            let name = unique_name(&list, &format!("{} copy", list[index].name_text()));
            copy.name = Some(name.clone());
            let new_id = copy.id;
            list.insert(index + 1, copy);
            json!({"showId":new_id.to_string(),"name":name})
        }
        "show_delete" => {
            if args["confirm"] != true {
                return Err("Explicit confirm:true is required".into());
            }
            let id = show_id(&list, args, "showId")?;
            let used = data.items.iter().filter(|s| s.show_id == id).count();
            if used > 0 && args["deleteSchedules"] != true {
                return Err(format!(
                    "{} schedule{} run{} this show: pass deleteSchedules:true to delete {} too",
                    used,
                    if used == 1 { "" } else { "s" },
                    if used == 1 { "s" } else { "" },
                    if used == 1 { "it" } else { "them" }
                ));
            }
            data.items.retain(|s| s.show_id != id);
            list.retain(|s| s.id != id);
            json!({"showId":id.to_string(),"deletedSchedules":used})
        }
        "show_item_add" | "show_item_update" | "show_item_move" | "show_item_delete" => {
            let id = show_id(&list, args, "showId")?;
            let kind = section(args)?;
            let show = list.iter_mut().find(|s| s.id == id).unwrap();
            let mut items: Vec<ShowItem> = positions(show, &kind).into_iter().map(|i| show.items[i].clone()).collect();
            let before = items.clone();
            match command {
                "show_item_add" => {
                    let mut item = ShowItem::new(kind.clone(), "");
                    set_action(&mut item, args, sequence_dir)?;
                    let at = match args.get("index") {
                        Some(_) => index(args, "index")?.min(items.len()),
                        None => items.len(),
                    };
                    items.insert(at, item);
                }
                "show_item_update" => {
                    let at = index(args, "index")?;
                    let item = items.get_mut(at).ok_or("Show step not found")?;
                    if item.action == ActionType::Launch {
                        return Err("Studio doesn't edit Launch steps; remove it and add a sequence or pause".into());
                    }
                    set_action(item, args, sequence_dir)?;
                }
                "show_item_move" => {
                    let from = index(args, "from")?;
                    if from >= items.len() {
                        return Err("Show step not found".into());
                    }
                    let item = items.remove(from);
                    let to = index(args, "to")?.min(items.len());
                    items.insert(to, item);
                }
                _ => {
                    let at = index(args, "index")?;
                    if at >= items.len() {
                        return Err("Show step not found".into());
                    }
                    items.remove(at);
                }
            }
            if items == before {
                return Ok((json!({"showId":id.to_string()}), false));
            }
            set_section(show, &kind, items);
            json!({"showId":id.to_string()})
        }
        "schedule_create" => {
            let today = Date::of(&LocalDateTime::now());
            let mut item = ScheduleItem {
                show_id: show_id(&list, args, "showId")?,
                days: [true; 7],
                enabled: false,
                start_date: today,
                end_date: today,
                start_time: TimeOfDay { hour: 17, minute: 30, second: 0 },
                end_time: TimeOfDay { hour: 22, minute: 0, second: 0 },
            };
            apply_fields(&mut item, &list, args)?;
            data.items.push(item);
            json!({"index":data.items.len() - 1})
        }
        "schedule_update" | "schedule_enable" => {
            let i = schedule_index(&data, args)?;
            let mut item = data.items[i].clone();
            apply_fields(&mut item, &list, args)?;
            if item == data.items[i] {
                return Ok((json!({"index":i}), false));
            }
            data.items[i] = item;
            json!({"index":i})
        }
        "schedule_duplicate" => {
            let i = schedule_index(&data, args)?;
            let mut copy = data.items[i].clone();
            copy.enabled = false;
            data.items.insert(i + 1, copy);
            json!({"index":i + 1,"enabled":false})
        }
        "schedule_delete" => {
            if args["confirm"] != true {
                return Err("Explicit confirm:true is required".into());
            }
            let i = schedule_index(&data, args)?;
            data.items.remove(i);
            json!({"index":i})
        }
        _ => return Err("Unknown show or schedule command".into()),
    };
    shows::save_shows(&mut system.module_store, &list);
    save_schedules(system, &data);
    Ok((result, true))
}

/// Swaps in `system` after a show or schedule change, without rebuilding the output graph, and hands the running scheduler its
/// new schedule list. Whether the scheduler as a whole is on, and where its status window sits, are the scheduler's own
/// settings, changed outside the history: an Undo restores the schedules and keeps those as they are now.
pub fn publish(core: &AppCore, mut system: SystemData) {
    let live = scheduler::load_scheduler(&core.system.read().module_store);
    let mut data = scheduler::load_scheduler(&system.module_store);
    if data.is_enabled != live.is_enabled || data.status_position != live.status_position {
        data.is_enabled = live.is_enabled;
        data.status_position = live.status_position;
        save_schedules(&mut system, &data);
    }
    *core.system.write() = system;
    core.display.studio_bump();
    if let Some(running) = core.studio_display.scheduler() {
        running.studio_replace_items(data.items);
    }
}

/// When a schedule next starts, or `Running` when it is inside its window now. Days are checked as the scheduler checks them:
/// from the first to the last day (both included), on the ticked weekdays, from the start time to the end time (overnight when
/// the end comes first). The status line's own "next start" stops looking once today's start time has passed; this doesn't.
#[derive(Debug, PartialEq, Eq)]
pub enum NextRun {
    Running,
    At(LocalDateTime),
    /// The last day has passed.
    Ended,
    /// No ticked weekday falls between the first and last day.
    Never,
}

pub fn next_run(item: &ScheduleItem, now: &LocalDateTime) -> NextRun {
    let today = Date::of(now);
    if item.end_date < today {
        return NextRun::Ended;
    }
    let probe = ScheduleItem { enabled: true, ..item.clone() };
    if probe.should_start(now) {
        return NextRun::Running;
    }
    // Eight days reach every weekday, today's again included, so finding none means no day is ticked.
    let mut day = if item.start_date > today { item.start_date } else { today };
    for _ in 0..8 {
        if day > item.end_date {
            // Past the last day: it has ended if it ever had a day to run on.
            let mut d = item.start_date;
            for _ in 0..7 {
                if d > item.end_date {
                    break;
                }
                if item.days[d.weekday()] {
                    return NextRun::Ended;
                }
                d = d.plus_days(1);
            }
            return NextRun::Never;
        }
        let start = day.at(item.start_time);
        if item.days[day.weekday()] && start > *now {
            return NextRun::At(start);
        }
        day = day.plus_days(1);
    }
    NextRun::Never
}

fn date_text(d: Date) -> Option<String> {
    (d.year >= 2000).then(|| format!("{:04}-{:02}-{:02}", d.year, d.month, d.day))
}

fn time_text(t: TimeOfDay) -> String {
    format!("{:02}:{:02}", t.hour, t.minute)
}

fn iso(t: &LocalDateTime) -> String {
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
}

fn item_json(item: &ShowItem, sequence_dir: &Path) -> Value {
    let action = match item.action {
        ActionType::Sequence => "sequence",
        ActionType::Pause => "pause",
        ActionType::Launch => "launch",
        ActionType::Other(_) => "other",
    };
    let mut value = json!({"action":action,"name":item.name_text()});
    match item.action {
        ActionType::Sequence => {
            let file = item.sequence_file_name.as_deref().unwrap_or("").replace('\\', "/");
            value["sequence"] = json!(file);
            value["missing"] = json!(!shows::sequence_path(item, sequence_dir).is_file());
        }
        ActionType::Pause => value["pauseSeconds"] = json!(item.pause_shown()),
        ActionType::Launch => value["program"] = json!(item.launch_program_name.as_deref().unwrap_or("")),
        ActionType::Other(_) => {}
    }
    value
}

/// Shows, schedules and the sequences a show can run, as the pane and `show_list`/`schedule_list` report them. `running`
/// gives the running scheduler's state for each schedule and its status line, when there is one.
pub fn describe(system: &SystemData, revision: u64, sequence_dir: &Path, running: Option<&scheduler::run::Scheduler>) -> Value {
    let list = shows::load_shows(&system.module_store);
    let data = scheduler::load_scheduler(&system.module_store);
    let now = LocalDateTime::now();
    let shows: Vec<Value> = list
        .iter()
        .map(|show| {
            let mut sections = serde_json::Map::new();
            for (name, kind) in &SECTIONS {
                let items: Vec<Value> = positions(show, kind).iter().map(|i| item_json(&show.items[*i], sequence_dir)).collect();
                sections.insert(name.to_string(), json!(items));
            }
            json!({
                "id": show.id.to_string(),
                "name": show.name_text(),
                "sections": sections,
                "schedules": data.items.iter().filter(|s| s.show_id == show.id).count(),
            })
        })
        .collect();
    let schedules: Vec<Value> = data
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let show = list.iter().find(|s| s.id == item.show_id);
            let next = next_run(item, &now);
            let playing = running.and_then(|r| r.item_state(index)).is_some_and(|s| s != State::Waiting);
            let state = if playing {
                "playing"
            } else if !item.enabled {
                "off"
            } else {
                match next {
                    NextRun::Running => "due",
                    NextRun::At(_) => "upcoming",
                    NextRun::Ended => "ended",
                    NextRun::Never => "never",
                }
            };
            json!({
                "index": index,
                "showId": show.map(|s| s.id.to_string()),
                "showName": show.map(|s| s.name_text().to_string()),
                "enabled": item.enabled,
                "startDate": date_text(item.start_date),
                "endDate": date_text(item.end_date),
                "startTime": time_text(item.start_time),
                "endTime": time_text(item.end_time),
                "days": item.days,
                "state": state,
                "nextRun": match &next { NextRun::At(t) => Some(iso(t)), _ => None },
            })
        })
        .collect();
    json!({
        "revision": revision,
        "schedulerOn": data.is_enabled,
        "status": running.map(|r| r.status().text),
        "now": iso(&now),
        "shows": shows,
        "schedules": schedules,
        "sequences": sequence_files(sequence_dir),
    })
}

/// The pane's snapshot: [`describe`] for the open profile.
pub fn list(core: &AppCore) -> Value {
    let running = core.studio_display.scheduler();
    let mut value = {
        let system = core.system.read();
        describe(&system, core.display.revision(), &core.paths.sequence_dir(), running.as_deref())
    };
    value["liveOutput"] = json!(core.execution().is_open());
    value
}

pub fn tools() -> Vec<Value> {
    let string = json!({"type":"string","minLength":1,"maxLength":256});
    let boolean = json!({"type":"boolean"});
    let integer = json!({"type":"integer","minimum":0});
    let section = json!({"type":"string","enum":["startup","sequential","background","shutdown"]});
    let pause = json!({"type":"integer","minimum":shows::PAUSE_MIN,"maximum":shows::PAUSE_MAX});
    let date = json!({"type":"string","pattern":"^\\d{4}-\\d{2}-\\d{2}$"});
    let time = json!({"type":"string","pattern":"^\\d{2}:\\d{2}(:\\d{2})?$"});
    let days = json!({"type":"array","items":boolean,"minItems":7,"maxItems":7});
    let schedule = |extra: Value| {
        let mut p = json!({"showId":string,"startDate":date,"endDate":date,"startTime":time,"endTime":time,"days":days,"enabled":boolean});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    vec![
        ("show_create","Add a show. name defaults to a numbered \"New Show\"; sequences are .tim files relative to the Sequence folder, run in order. One undo step.",json!({"name":string,"sequences":{"type":"array","items":string,"maxItems":100}}),vec![],false),
        ("show_rename","Rename a show.",json!({"showId":string,"name":string}),vec!["showId","name"],false),
        ("show_duplicate","Copy a show and all its steps, placed after it as \"<name> copy\".",json!({"showId":string}),vec!["showId"],false),
        ("show_delete","Delete a show (confirm). A show that schedules run needs deleteSchedules:true, which deletes those schedules in the same step. Undo brings both back.",json!({"showId":string,"confirm":boolean,"deleteSchedules":boolean}),vec!["showId","confirm"],true),
        ("show_item_add","Add a step to a show section (startup, sequential, background, shutdown): give sequence (a .tim file relative to the Sequence folder) or pauseSeconds. index defaults to the end. Launch steps (running programs) are never created.",json!({"showId":string,"section":section,"sequence":string,"pauseSeconds":pause,"index":integer}),vec!["showId","section"],false),
        ("show_item_update","Change a show step to run another sequence or pause; index counts within the section from 0, as show_list lists it. Launch steps can't be edited.",json!({"showId":string,"section":section,"index":integer,"sequence":string,"pauseSeconds":pause}),vec!["showId","section","index"],false),
        ("show_item_move","Move a show step within its section, from one position to another.",json!({"showId":string,"section":section,"from":integer,"to":integer}),vec!["showId","section","from","to"],false),
        ("show_item_delete","Remove a step from a show section. Undo brings it back.",json!({"showId":string,"section":section,"index":integer}),vec!["showId","section","index"],false),
        ("schedule_create","Schedule a show: from startDate to endDate (both included, YYYY-MM-DD), on the ticked days (seven booleans, Sunday first; default every day), from startTime to endTime (HH:MM, 24-hour; default 17:30-22:00; an end before the start runs past midnight). enabled defaults to false. A schedule only runs while the scheduler is on, which only the user can switch (Scheduler > Enabled).",schedule(json!({})),vec!["showId","startDate","endDate"],false),
        ("schedule_update","Change a schedule's show, dates, times, days or enabled; omitted fields keep their values. index is from schedule_list at expectedRevision.",schedule(json!({"index":integer})),vec!["index","expectedRevision"],false),
        ("schedule_enable","Turn a schedule on or off. It runs only while the scheduler is on (Scheduler > Enabled, the user's switch).",json!({"index":integer,"enabled":boolean}),vec!["index","enabled","expectedRevision"],false),
        ("schedule_duplicate","Copy a schedule, placed after it and turned off.",json!({"index":integer}),vec!["index","expectedRevision"],false),
        ("schedule_delete","Delete a schedule (confirm). Undo brings it back.",json!({"index":integer,"confirm":boolean}),vec!["index","confirm","expectedRevision"],true),
    ].into_iter().map(|(name,description,mut properties,required,destructive)|{properties["expectedRevision"]=json!({"type":"integer","minimum":0});json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":destructive,"openWorldHint":false}})}).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> LocalDateTime {
        LocalDateTime { year, month, day, hour, minute, second, millisecond: 0 }
    }

    fn temp() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("studio-schedule-{}", Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("Songs")).unwrap();
        for f in ["A.tim", "B.tim", "Songs/C.tim", "A.tim.6"] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        dir
    }

    fn run(system: &mut SystemData, dir: &Path, command: &str, args: Value) -> Value {
        let (value, changed) = edit(system, dir, command, &args).unwrap_or_else(|e| panic!("{command}: {e}"));
        assert!(changed, "{command} changed nothing");
        value
    }

    fn item(days: [bool; 7], start: Date, end: Date, from: (u8, u8), to: (u8, u8)) -> ScheduleItem {
        ScheduleItem {
            show_id: Uuid::new_v4(),
            days,
            enabled: true,
            start_date: start,
            end_date: end,
            start_time: TimeOfDay { hour: from.0, minute: from.1, second: 0 },
            end_time: TimeOfDay { hour: to.0, minute: to.1, second: 0 },
        }
    }

    #[test]
    fn shows_and_steps_edit_in_run_order() {
        let dir = temp();
        let mut system = SystemData::default();
        let show = run(&mut system, &dir, "show_create", json!({"sequences":["A.tim"]}))["showId"].as_str().unwrap().to_string();
        assert_eq!(shows::load_shows(&system.module_store)[0].name_text(), "New Show");
        run(&mut system, &dir, "show_item_add", json!({"showId":show,"section":"sequential","pauseSeconds":5}));
        run(&mut system, &dir, "show_item_add", json!({"showId":show,"section":"sequential","sequence":"Songs/C.tim","index":0}));
        run(&mut system, &dir, "show_item_add", json!({"showId":show,"section":"startup","sequence":"B.tim"}));
        run(&mut system, &dir, "show_item_move", json!({"showId":show,"section":"sequential","from":2,"to":0}));
        run(&mut system, &dir, "show_item_update", json!({"showId":show,"section":"sequential","index":1,"pauseSeconds":30}));
        let listed = describe(&system, 0, &dir, None);
        let names: Vec<&str> =
            listed["shows"][0]["sections"]["sequential"].as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Pause for 5", "Pause for 30", "Run sequence: A.tim"]);
        assert_eq!(listed["shows"][0]["sections"]["startup"][0]["sequence"], "B.tim");
        assert_eq!(listed["sequences"], json!(["A.tim", "B.tim", "Songs/C.tim"]));
        // Saved order numbers match what is listed, so a run takes them in that order.
        let stored = shows::load_shows(&system.module_store);
        let orders: Vec<i32> = stored[0].ordered(&ShowItemType::Sequential).iter().map(|i| i.item_order).collect();
        assert_eq!(orders, [0, 1, 2]);
        run(&mut system, &dir, "show_item_delete", json!({"showId":show,"section":"sequential","index":0}));
        assert_eq!(describe(&system, 0, &dir, None)["shows"][0]["sections"]["sequential"].as_array().unwrap().len(), 2);
        for bad in [json!({"sequence":"../x.tim"}), json!({"sequence":"Missing.tim"}), json!({"sequence":"A.tim.6"}), json!({"pauseSeconds":0}), json!({})] {
            let mut args = json!({"showId":show,"section":"sequential"});
            args.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
            assert!(edit(&mut system, &dir, "show_item_add", &args).is_err(), "{args}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn schedules_create_edit_enable_and_delete_with_their_show() {
        let dir = temp();
        let mut system = SystemData::default();
        let show = run(&mut system, &dir, "show_create", json!({"name":"Evening","sequences":["A.tim"]}))["showId"].as_str().unwrap().to_string();
        run(&mut system, &dir, "schedule_create", json!({"showId":show,"startDate":"2026-11-27","endDate":"2027-01-06","startTime":"17:30","endTime":"22:00"}));
        let listed = describe(&system, 0, &dir, None);
        assert_eq!(listed["schedules"][0]["enabled"], false);
        assert_eq!(listed["schedules"][0]["showName"], "Evening");
        assert_eq!(listed["schedules"][0]["state"], "off");
        run(&mut system, &dir, "schedule_enable", json!({"index":0,"enabled":true}));
        assert!(!edit(&mut system, &dir, "schedule_enable", &json!({"index":0,"enabled":true})).unwrap().1);
        run(&mut system, &dir, "schedule_update", json!({"index":0,"endTime":"01:00","days":[false,false,false,false,false,true,true]}));
        let stored = scheduler::load_scheduler(&system.module_store).items[0].clone();
        assert!(stored.enabled);
        assert_eq!(stored.end_time, TimeOfDay { hour: 1, minute: 0, second: 0 });
        assert_eq!(stored.start_date, Date { year: 2026, month: 11, day: 27 });
        assert!(edit(&mut system, &dir, "schedule_update", &json!({"index":0,"endDate":"2026-01-01"})).is_err());
        assert!(edit(&mut system, &dir, "schedule_update", &json!({"index":0,"startTime":"25:00"})).is_err());
        assert!(edit(&mut system, &dir, "schedule_update", &json!({"index":3,"enabled":false})).is_err());
        run(&mut system, &dir, "schedule_duplicate", json!({"index":0}));
        assert!(!scheduler::load_scheduler(&system.module_store).items[1].enabled);
        // A show that schedules run is deleted only with them.
        assert!(edit(&mut system, &dir, "show_delete", &json!({"showId":show,"confirm":true})).unwrap_err().contains("2 schedules run"));
        assert_eq!(run(&mut system, &dir, "show_delete", json!({"showId":show,"confirm":true,"deleteSchedules":true}))["deletedSchedules"], 2);
        assert!(scheduler::load_scheduler(&system.module_store).items.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    struct NoRunner;
    impl crate::scheduler::actions::ActionRunner for NoRunner {
        fn execute(&self, _: &ShowItem, _: crate::scheduler::actions::Done) -> Box<dyn crate::scheduler::actions::RunningAction> {
            unreachable!("nothing is due in this test")
        }
    }

    #[test]
    fn edits_undo_on_the_shared_history_and_reach_the_running_scheduler() {
        use std::sync::Arc;
        let root = std::env::temp_dir().join(format!("studio-schedule-core-{}", Uuid::new_v4()));
        let core = Arc::new(AppCore::open(root.clone(), true).unwrap());
        std::fs::create_dir_all(core.paths.sequence_dir()).unwrap();
        std::fs::write(core.paths.sequence_dir().join("A.tim"), "x").unwrap();
        let deps = scheduler::run::Deps::threaded(Arc::new(NoRunner), Arc::new(|_| None));
        let running = scheduler::run::Scheduler::new(scheduler::load_scheduler(&core.system.read().module_store), deps);
        core.studio_display.attach_scheduler(&running);
        let exec = |command: &str, args: Value| core.studio_display.execute(&core, command, &args, false).unwrap();
        let show = exec("show_create", json!({"sequences":["A.tim"]}))["result"]["showId"].as_str().unwrap().to_string();
        exec("schedule_create", json!({"showId":show,"startDate":"2030-01-01","endDate":"2030-01-31"}));
        let revision = core.display.revision();
        exec("schedule_enable", json!({"index":0,"enabled":true,"expectedRevision":revision}));
        assert!(running.data().items[0].enabled);
        assert_eq!(core.studio_display.history()["undo"], json!(["Turn On Schedule", "Add Schedule", "Add Show"]));
        // A schedule step undoes without waiting for Live Output, like a preview step.
        assert_eq!(core.studio_display.history()["undoPreviewOnly"], true);
        // A stale index refuses.
        assert!(core.studio_display.execute(&core, "schedule_enable", &json!({"index":0,"enabled":false,"expectedRevision":revision}), false).is_err());
        // The user turns the scheduler on outside the history; Undo leaves that alone.
        running.set_enabled(true);
        scheduler::save_scheduler(&mut core.system.write().module_store, &running.data(), &LocalDateTime::now());
        exec("display_undo", json!({}));
        assert!(!running.data().items[0].enabled);
        assert!(running.data().is_enabled);
        assert!(scheduler::load_scheduler(&core.system.read().module_store).is_enabled);
        exec("display_undo", json!({}));
        assert!(running.data().items.is_empty());
        exec("display_redo", json!({}));
        assert_eq!(running.data().items.len(), 1);
        assert_eq!(list(&core)["schedules"][0]["showName"], "New Show");
        running.quit();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn next_run_looks_past_today() {
        let every = [true; 7];
        let nov = |d| Date { year: 2026, month: 11, day: d };
        let window = item(every, nov(1), nov(30), (17, 30), (22, 0));
        // Before today's start: today. After it has passed (where the status line gives up): tomorrow.
        assert_eq!(next_run(&window, &at(2026, 11, 20, 12, 0, 0)), NextRun::At(at(2026, 11, 20, 17, 30, 0)));
        assert_eq!(next_run(&window, &at(2026, 11, 20, 18, 0, 0)), NextRun::Running);
        assert_eq!(next_run(&window, &at(2026, 11, 20, 23, 0, 0)), NextRun::At(at(2026, 11, 21, 17, 30, 0)));
        // The last day is included; after it the schedule has ended.
        assert_eq!(next_run(&window, &at(2026, 11, 30, 12, 0, 0)), NextRun::At(at(2026, 11, 30, 17, 30, 0)));
        assert_eq!(next_run(&window, &at(2026, 11, 30, 23, 0, 0)), NextRun::Ended);
        assert_eq!(next_run(&window, &at(2026, 12, 1, 12, 0, 0)), NextRun::Ended);
        // Not started yet: its first day. Weekends only: the coming Saturday (2026-11-21).
        assert_eq!(next_run(&window, &at(2026, 10, 1, 12, 0, 0)), NextRun::At(at(2026, 11, 1, 17, 30, 0)));
        let weekends = item([true, false, false, false, false, false, true], nov(1), nov(30), (17, 30), (22, 0));
        assert_eq!(next_run(&weekends, &at(2026, 11, 18, 12, 0, 0)), NextRun::At(at(2026, 11, 21, 17, 30, 0)));
        let none = item([false; 7], nov(1), nov(30), (17, 30), (22, 0));
        assert_eq!(next_run(&none, &at(2026, 11, 18, 12, 0, 0)), NextRun::Never);
        // A Monday-only window inside one week with no Monday never runs.
        let monday = item([false, true, false, false, false, false, false], nov(3), nov(7), (17, 30), (22, 0));
        assert_eq!(next_run(&monday, &at(2026, 11, 1, 12, 0, 0)), NextRun::Never);
    }
}
