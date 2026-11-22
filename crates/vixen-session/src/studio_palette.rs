//! The command palette (⌘K): every menu command of the front window, searchable, with its shortcut, why it is disabled and
//! which MCP tool does the same thing. The app enumerates its live menu bar (the only complete list: areas add their own
//! menus) and publishes it here; ranking, disabled reasons and agent equivalents live in Rust so the palette and the MCP
//! `command_palette` tool answer identically.
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// One menu command as the app found it: the menu path ending in its title, its shortcut as shown (⌘S), and its state.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: Vec<String>,
    pub shortcut: String,
    pub enabled: bool,
    pub checked: bool,
    /// The MCP tool the app says this command runs (a plugin's contributed command), when it isn't one of [`agent_tool`]'s.
    pub tool: Option<String>,
}

impl Entry {
    pub fn title(&self) -> &str {
        self.path.last().map(String::as_str).unwrap_or("")
    }
    fn top(&self) -> &str {
        self.path.first().map(String::as_str).unwrap_or("")
    }
    fn has(&self, menu: &str) -> bool {
        self.path[..self.path.len().saturating_sub(1)].iter().any(|m| m == menu)
    }
}

/// Entries from JSON `[{path:[..], shortcut, enabled, checked, tool?}]`. At most 5,000 entries of at most 8 levels.
pub fn parse_entries(value: &Value) -> Result<Vec<Entry>, String> {
    let list = value.as_array().ok_or("entries must be an array")?;
    if list.len() > 5_000 {
        return Err("at most 5000 entries".into());
    }
    list.iter()
        .map(|e| {
            let path: Vec<String> = e["path"]
                .as_array()
                .ok_or("each entry needs a path")?
                .iter()
                .map(|p| p.as_str().map(|s| s.chars().take(200).collect()).ok_or("path items must be strings"))
                .collect::<Result<_, _>>()?;
            if path.is_empty() || path.len() > 8 {
                return Err("a path has 1 to 8 items".to_string());
            }
            Ok(Entry {
                path,
                shortcut: e["shortcut"].as_str().unwrap_or("").chars().take(20).collect(),
                enabled: e["enabled"].as_bool().unwrap_or(true),
                checked: e["checked"].as_bool().unwrap_or(false),
                tool: e["tool"].as_str().filter(|t| !t.is_empty() && t.len() <= 64).map(str::to_string),
            })
        })
        .collect()
}

fn words(s: &str) -> Vec<String> {
    s.replace('…', " ")
        .replace("...", " ")
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn subsequence(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|h| h == c))
}

/// Extra words a command answers to beyond its title and menus, for what people search by rather than what the menu says.
fn keywords(e: &Entry) -> &'static str {
    match e.title() {
        "Start" | "Stop" if e.has("Execution Engine") => "execution engine output controllers",
        "Beat/Bar Detection" | "Detect Beats and Bars…" => "music tempo marks analyze audio rhythm",
        "Associate Audio..." => "song music mp3 add audio media",
        "Remove Audio" => "song music media",
        "Create Evenly Divided Marks" => "marks grid beats",
        "Sequence Length..." => "duration time length",
        "Export" => "fseq falcon fpp export render",
        "Play" | "Stop" | "Pause" | "Loop" => "playback transport",
        "Setup Display" => "elements props controllers patching display",
        "Setup Previews" => "preview layout designer",
        "Profiles..." => "show folder data profile",
        "Logs" => "errors log",
        "Shows" | "Schedules" => "scheduler",
        "Options..." | "Settings…" => "preferences settings options",
        "Curve Editor" => "curves library",
        "Color Gradient Editor" => "gradients library colour",
        "Color Collection Editor" => "colors colours palette library",
        "Web Server" => "remote http web",
        _ => "",
    }
}

fn score(e: &Entry, query: &[String], whole: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let title = words(e.title());
    let menus: Vec<String> = e.path[..e.path.len() - 1].iter().flat_map(|m| words(m)).collect();
    let extra = words(keywords(e));
    let joined = title.join(" ");
    let initials: String = title.iter().filter_map(|w| w.chars().next()).collect();
    let mut total = 0;
    for t in query {
        let best = title
            .iter()
            .enumerate()
            .map(|(i, w)| {
                if w == t {
                    100 - 2 * i as i64
                } else if w.starts_with(t.as_str()) {
                    80 - 2 * i as i64
                } else if w.contains(t.as_str()) {
                    45
                } else {
                    0
                }
            })
            .max()
            .unwrap_or(0)
            .max(if t.len() >= 2 && initials.starts_with(t.as_str()) { 60 } else { 0 })
            .max(if extra.iter().any(|w| w.starts_with(t.as_str())) { 40 } else { 0 })
            .max(if menus.iter().any(|w| w.starts_with(t.as_str())) { 30 } else { 0 })
            .max(if t.len() >= 3 && title.iter().any(|w| subsequence(t, w)) { 15 } else { 0 });
        if best == 0 {
            return None;
        }
        total += best;
    }
    if joined == whole {
        total += 200;
    } else if joined.starts_with(whole) {
        total += 100;
    }
    if !e.enabled {
        total -= 5;
    }
    Some(total - e.path.len() as i64)
}

/// Why a disabled command is off, in words the user can act on. `window` is "show" or "sequencer" (the front window).
pub fn reason(e: &Entry, window: &str) -> Option<String> {
    if e.enabled {
        return None;
    }
    let t = e.title();
    let designer = "Open a preview in the designer first (Previews section ▸ Edit Layout…).";
    let text = match (e.top(), t) {
        (_, "Undo") | (_, "Redo") => format!("Nothing to {} yet.", t.to_lowercase()),
        (_, "Cut") | (_, "Copy") | (_, "Delete") => {
            if window == "sequencer" { "Select one or more effects first." } else { "Select something first." }.into()
        }
        (_, "Paste") | (_, "Paste Special") | (_, "Invert") | (_, "Visible Marks") => "Copy effects first.".into(),
        ("Edit", _) if e.has("Writing Tools") || e.has("AutoFill") || t == "Start Dictation…" => {
            "Click into a text field first.".into()
        }
        ("Sequence", "Play") => "Already playing, or the sequence has nothing to play.".into(),
        ("Sequence", "Pause") => "Pause with Play (F5), as Windows Vixen does.".into(),
        ("Sequence", "Stop") => "Nothing is playing.".into(),
        ("Sequence", "Export") => "Save the sequence once before exporting it.".into(),
        ("Tools", "Remove Audio") => "This sequence has no audio.".into(),
        ("Tools", "Beat/Bar Detection") => "Associate audio with the sequence first (Tools ▸ Audio ▸ Associate Audio…).".into(),
        ("Tools", "Speed / Tempo") => "Associate audio with the sequence first.".into(),
        ("Tools", "Edit Image Maps") | ("Tools", "Edit Element Face Mapping") => {
            "This profile has no lip-sync image maps yet.".into()
        }
        ("File", "Save") | ("File", "Import Prop") => designer.into(),
        // The preview designer's menus stay in the menu bar with other windows in front.
        ("Settings", _) => designer.into(),
        ("View", "Prop Information" | "Prop Editor" | "Elements" | "Properties" | "Float Elements Panel" | "Float Properties Panel") => {
            designer.into()
        }
        ("Help", "View Help") | ("Help", "Vixen YouTube Channel") | ("Help", "Template Help") | ("Help", "Custom Prop Help") => {
            designer.into()
        }
        ("Window", "Turn Off Preview Transparency") => "No preview window is transparent.".into(),
        ("Window", "Minimize") | ("Window", "Zoom") => "The front window can't be resized this way.".into(),
        ("System", "Start") => "The execution engine is already running.".into(),
        ("System", "Stop") => "The execution engine isn't running.".into(),
        ("Scheduler", _) | ("System", _) | ("Tools", _) if window == "show" => {
            "Busy: wait for the current profile operation to finish.".into()
        }
        _ if window == "sequencer" && matches!(e.top(), "Sequence" | "Tools" | "Edit" | "View") => {
            "Wait for the sequence to finish loading, or close the system configuration session.".into()
        }
        _ => "Not available in the front window right now.".into(),
    };
    Some(text)
}

/// The MCP tool that does what this menu command does, so a user can see agents can do it too.
pub fn agent_tool(e: &Entry, window: &str) -> Option<&'static str> {
    let sequencer = window == "sequencer";
    Some(match (e.top(), e.title()) {
        ("Edit", "Undo") => if sequencer { "sequence_undo" } else { "display_undo" },
        ("Edit", "Redo") => if sequencer { "sequence_redo" } else { "display_redo" },
        ("Edit", "Delete") if sequencer => "effect_delete",
        ("Edit", "Copy") if sequencer => "effect_copy",
        ("Sequence", "Save") => "sequence_save",
        ("Sequence", "Play") => "sequence_play",
        ("Sequence", "Pause") => "sequence_pause",
        ("Sequence", "Stop") => "sequence_stop",
        ("Sequence", "Export") => "sequence_export_fseq",
        ("Sequence", "Close") => "sequence_close",
        ("File", "New Sequence...") => "sequence_create",
        ("File", "Open Sequence...") => "sequence_open",
        ("Tools", "Beat/Bar Detection") => "sequence_music_marks",
        ("Tools", "Create Evenly Divided Marks") => "marks_create",
        ("Tools", "Sequence Length...") => "sequence_length",
        ("System", "Logs") => "recent_logs",
        ("System", "Setup Display") => "display_elements",
        ("System", "Setup Previews") => "preview_list",
        ("System", "View Installed Modules") => "studio_status",
        ("Scheduler", "Shows") => "show_list",
        ("Scheduler", "Schedules") => "schedule_list",
        _ => return None,
    })
}

/// Ranked matches for `query`: every entry for an empty query in menu order, otherwise entries matching every word,
/// best first (ties keep menu order). Each result carries its index into `entries`.
pub fn search(entries: &[Entry], query: &str, window: &str, limit: usize) -> Vec<Value> {
    let q = words(query);
    let whole = q.join(" ");
    let mut scored: Vec<(i64, usize)> =
        entries.iter().enumerate().filter_map(|(i, e)| score(e, &q, &whole).map(|s| (s, i))).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored
        .into_iter()
        .take(limit)
        .map(|(s, i)| {
            let e = &entries[i];
            let mut v = json!({"index":i,"title":e.title(),"path":e.path,"shortcut":e.shortcut,"enabled":e.enabled,"checked":e.checked,"score":s});
            if let Some(r) = reason(e, window) {
                v["reason"] = json!(r);
            }
            if let Some(t) = agent_tool(e, window).map(str::to_string).or_else(|| e.tool.clone()) {
                v["agentTool"] = json!(t);
            }
            v
        })
        .collect()
}

fn store() -> &'static Mutex<HashMap<String, Vec<Entry>>> {
    static STORE: OnceLock<Mutex<HashMap<String, Vec<Entry>>>> = OnceLock::new();
    STORE.get_or_init(Default::default)
}

/// What the app last found in its menu bar with each kind of window in front ("show" or "sequencer").
pub fn publish(window: &str, entries: Vec<Entry>) {
    store().lock().unwrap_or_else(|p| p.into_inner()).insert(window.to_string(), entries);
}

pub fn tool() -> Value {
    json!({"name":"command_palette","description":"Search the running app's menu commands, as its ⌘K command palette does: each match's menu path, shortcut, whether it's enabled (and why not), and the MCP tool that does the same thing (agentTool). window picks the front window the menus were read with: show (the show window) or sequencer. Lists what the app last published when that window was in front; needs the running app, not a headless bridge. Read-only: it never runs a command.","inputSchema":{"type":"object","properties":{"query":{"type":"string","maxLength":200},"window":{"type":"string","enum":["show","sequencer"]},"limit":{"type":"integer","minimum":1,"maximum":500}},"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}})
}

pub fn execute(args: &Value) -> Result<Value, String> {
    let obj = args.as_object().ok_or("arguments must be an object")?;
    if obj.keys().any(|k| !matches!(k.as_str(), "query" | "window" | "limit")) {
        return Err("Unknown argument".into());
    }
    let query = args.get("query").map(|q| q.as_str().ok_or("query must be a string")).transpose()?.unwrap_or("");
    let window = args.get("window").map(|w| w.as_str().ok_or("window must be a string")).transpose()?.unwrap_or("show");
    if !matches!(window, "show" | "sequencer") {
        return Err("window must be show or sequencer".into());
    }
    let limit = args.get("limit").map(|l| l.as_u64().filter(|n| (1..=500).contains(n)).ok_or("limit must be 1 to 500")).transpose()?.unwrap_or(50) as usize;
    let all = store().lock().unwrap_or_else(|p| p.into_inner());
    let entries = all.get(window).ok_or_else(|| {
        format!("No menus published for the {window} window: the command palette reads the running app's menu bar, so attach to the app and bring that window to the front once.")
    })?;
    let matches = search(entries, query, window, limit);
    Ok(json!({"window":window,"total":entries.len(),"disabled":entries.iter().filter(|e| !e.enabled).count(),"matches":matches}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(path: &[&str], enabled: bool) -> Entry {
        Entry { path: path.iter().map(|s| s.to_string()).collect(), shortcut: String::new(), enabled, checked: false, tool: None }
    }

    fn menu() -> Vec<Entry> {
        vec![
            e(&["File", "New Sequence..."], true),
            e(&["Edit", "Undo"], false),
            e(&["Edit", "Select All Elements"], true),
            e(&["Sequence", "Save"], true),
            e(&["Sequence", "Save As..."], true),
            e(&["Sequence", "Playback", "Play"], true),
            e(&["Tools", "Audio", "Beat/Bar Detection"], false),
            e(&["Tools", "Audio", "Associate Audio..."], true),
            e(&["Tools", "Sequence Length..."], true),
            e(&["Window", "Bring All to Front"], true),
        ]
    }

    fn titles(results: &[Value]) -> Vec<&str> {
        results.iter().map(|r| r["title"].as_str().unwrap()).collect()
    }

    #[test]
    fn empty_query_lists_everything_in_menu_order() {
        let m = menu();
        let r = search(&m, "  ", "sequencer", 500);
        assert_eq!(r.len(), m.len());
        assert_eq!(r[0]["index"], 0);
        assert_eq!(r[9]["index"], 9);
    }

    #[test]
    fn every_title_finds_its_own_entry_first() {
        let m = menu();
        for (i, entry) in m.iter().enumerate() {
            let r = search(&m, entry.title(), "sequencer", 500);
            assert_eq!(r[0]["index"], i, "{} ranked {:?}", entry.title(), titles(&r));
        }
    }

    #[test]
    fn prefixes_initials_menus_and_keywords_match() {
        let m = menu();
        assert_eq!(titles(&search(&m, "beat", "sequencer", 5))[0], "Beat/Bar Detection");
        assert_eq!(titles(&search(&m, "bar det", "sequencer", 5))[0], "Beat/Bar Detection");
        assert_eq!(titles(&search(&m, "sae", "sequencer", 5))[0], "Select All Elements");
        assert_eq!(titles(&search(&m, "tempo", "sequencer", 5))[0], "Beat/Bar Detection");
        assert_eq!(titles(&search(&m, "audio", "sequencer", 5))[0], "Associate Audio...");
        assert_eq!(titles(&search(&m, "save as", "sequencer", 5))[0], "Save As...");
        assert_eq!(titles(&search(&m, "save", "sequencer", 5))[..2], ["Save", "Save As..."]);
        assert!(search(&m, "zzzz", "sequencer", 5).is_empty());
        // Letters in order within one word, not scattered across a title.
        assert_eq!(titles(&search(&m, "lngth", "sequencer", 5)), ["Sequence Length..."]);
        let recent = [e(&["File", "Open Recent", "The Night Before Christmas.tim"], true)];
        assert!(search(&recent, "beat", "sequencer", 5).is_empty());
        // Every word must match something.
        assert!(search(&m, "save zzzz", "sequencer", 5).is_empty());
    }

    #[test]
    fn plugin_commands_name_the_tool_the_app_gave_them() {
        let list = parse_entries(&json!([
            {"path": ["Plugins", "Lamp Output", "Discover Lamps…"], "tool": "plugin__lamp__discover"},
            {"path": ["Plugins", "Lamp Output", "Help"], "tool": ""}
        ]))
        .unwrap();
        let found = search(&list, "discover lamps", "show", 5);
        assert_eq!(found[0]["agentTool"], "plugin__lamp__discover");
        assert!(search(&list, "help", "show", 5)[0].get("agentTool").is_none());
    }

    #[test]
    fn disabled_commands_say_why_and_name_their_agent_tool() {
        let m = menu();
        let beat = &search(&m, "beat", "sequencer", 1)[0];
        assert_eq!(beat["enabled"], false);
        assert!(beat["reason"].as_str().unwrap().contains("Associate audio"));
        assert_eq!(beat["agentTool"], "sequence_music_marks");
        let undo = &search(&m, "undo", "show", 1)[0];
        assert_eq!(undo["reason"], "Nothing to undo yet.");
        assert_eq!(undo["agentTool"], "display_undo");
        assert_eq!(search(&m, "undo", "sequencer", 1)[0]["agentTool"], "sequence_undo");
        let save = &search(&m, "save", "sequencer", 1)[0];
        assert!(save.get("reason").is_none());
        assert_eq!(save["agentTool"], "sequence_save");
        // The designer's menus explain themselves whichever window is in front.
        let props = e(&["View", "Prop Information"], false);
        assert!(reason(&props, "sequencer").unwrap().contains("designer"));
        assert!(reason(&e(&["Settings", "Use OpenGL Preview"], false), "sequencer").unwrap().contains("designer"));
        // Every disabled entry gets some reason.
        for entry in [e(&["Prop", "Something"], false), e(&["Help", "Anything"], false)] {
            assert!(reason(&entry, "show").is_some());
            assert!(reason(&entry, "sequencer").is_some());
        }
    }

    #[test]
    fn mcp_tool_reads_what_the_app_published() {
        publish("sequencer", menu());
        let r = execute(&json!({"window":"sequencer","query":"length"})).unwrap();
        assert_eq!(r["total"], 10);
        assert_eq!(r["disabled"], 2);
        assert_eq!(r["matches"][0]["title"], "Sequence Length...");
        assert_eq!(r["matches"][0]["agentTool"], "sequence_length");
        assert!(execute(&json!({"window":"other"})).is_err());
        assert!(execute(&json!({"bogus":1})).is_err());
        assert!(execute(&json!({"limit":0})).is_err());
    }

    #[test]
    fn parses_published_entries() {
        let v = json!([{"path":["Edit","Undo"],"shortcut":"⌘Z","enabled":false},{"path":["Help"]}]);
        let parsed = parse_entries(&v).unwrap();
        assert_eq!(parsed[0].shortcut, "⌘Z");
        assert!(!parsed[0].enabled && parsed[1].enabled);
        assert!(parse_entries(&json!([{"path":[]}])).is_err());
        assert!(parse_entries(&json!({})).is_err());
    }
}
