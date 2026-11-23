//! Studio local MCP adapter. Mutations dispatch through shared vixen-session commands.
pub mod lock;
pub mod socket;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::Arc;
use vixen_session::app::AppCore;
use vixen_session::setup::SetupSession;
use vixen_session::studio_preview::geometry;

pub const MAX_MESSAGE: usize = 1024 * 1024;
const TOOLS: &[(&str, &str)] = &[
    ("studio_status", "Profile identity, safety state and implementation capabilities. build is the commit the serving app or session was built from ({commit, date, dirty}); bridge is the build of the vixen-studio-mcp bridge relaying this connection (it can be older than the app), null for a direct socket client or when the bridge hosts the session itself (then build is the bridge's)."),
    ("recent_logs", "Read up to limit lines from the newest profile log, bounded to 64 KiB; never follows a log symlink."),
    ("controller_monitor", "Actual execution/frame/controller run statistics; no startup or polling side effects."),
    ("show_list", "List shows with their steps by section (startup, sequential, background, shutdown) in run order, and how many schedules run each. Never runs anything."),
    ("schedule_list", "List schedules: show, first and last day, start and end time, days (Sunday first), enabled, state (off, upcoming, due, playing, ended, never) and nextRun; schedulerOn is the user's Scheduler > Enabled switch. index plus revision address schedule edits."),
    ("image_map_list", "List the LipSync image maps (Tools > LipSync > Edit Image Maps): name, notes, isDefault, editable (picture maps; string maps from Windows Vixen aren't), usingDefaults (still on the bundled mouths) and pictures (phonemes with a picture). image_map_* edit them, one undo step each."),
    ("face_mapping_list", "List the elements with a Face property (Tools > LipSync > Edit Element Face Mapping): id, name, the mouth shapes (phonemes) and Outlines/EyesOpen/EyesClosed (components) each draws, and its color. face_mapping_set edits them, one undo step per call."),
    ("phoneme_mapping_list", "List the profile's user phoneme dictionary (the words the Lyric Converter's Text Conversion Failure sheet or phoneme_mapping_set mapped): each word, upper case as the dictionary keys it, and its mouth shapes in order (an empty list: the word is skipped). phoneme_mapping_set and phoneme_mapping_remove edit it, one undo step each."),
    ("preview_props", "List native prop IDs and property metadata for every preview. Use returned revision for edits."),
    ("preview_list", "List configured preview IDs, names and whether each is on (shown whenever Live Output is on)."),
    ("controller_types", "Available controller types with stable UUIDs. outputsFromConfig: a plugin type that sets its own output count from its settings (controller_configure resizes it; outputs is refused)."),
    ("display_elements", "List element nodes with stable GUIDs and child GUIDs; paginated."),
    ("controller_list", "List configured controllers and output counts. Does not start output."),
    ("sequence_list", "List sequence filenames in the selected profile."),
    ("media_list", "List media filenames, read-only; does not decode or play audio."),
    ("effect_catalog", "List registered effects and JSON parameter schemas. Unsupported native value kinds have a rejecting schema."),
    ("diagnostics_report", "Report patches, controller/universe errors, and missing media/targets/native color corrections for open sequences."),
];

pub struct Server {
    root: std::path::PathBuf,
    setup: SetupSession,
    initialized: bool,
    client_id: Option<String>,
    core: Option<Arc<AppCore>>,
    /// Serving the running app's core. A headless bridge opens its own muted core on demand, which the app never sees.
    shared: bool,
    /// The plugin host; `None` uses the process's shared one (`studio_plugins::shared`), opened on first use.
    plugins: Option<&'static studio_plugins::PluginHost>,
    /// The plugin tools as of the client's last `tools/list`, to tell it when they change.
    listed_plugin_tools: Option<Vec<Value>>,
    /// The build of the vixen-studio-mcp bridge relaying this client, as it announced it (`BRIDGE_NOTIFICATION`).
    bridge: Option<Value>,
}
impl Server {
    /// The live display when serving a core, otherwise the snapshot loaded at startup, with its revision.
    fn with_system<T>(&self, f: impl FnOnce(&vixen_io::SystemData, u64) -> T) -> T {
        match &self.core {
            Some(core) => f(&core.system.read(), core.display.revision()),
            None => f(&self.setup.system, 0),
        }
    }
    /// Shows and schedules as the Schedule pane lists them, with the running scheduler's state when serving the app.
    fn schedules(&self) -> Value {
        let sequences = vixen_io::DataPaths::new(self.root.clone()).sequence_dir();
        let running = self.core.as_ref().and_then(|c| c.studio_display.scheduler());
        self.with_system(|system, revision| {
            vixen_session::studio_schedule::describe(system, revision, &sequences, running.as_deref())
        })
    }
    pub fn open(root: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        // Load only: unlike AppCore::open this creates no folders/cache and starts no services.
        let system = vixen_io::load_system(&vixen_io::DataPaths::new(root.clone()))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            root,
            setup: SetupSession::from_system(system, &Default::default(), false),
            initialized: false,
            client_id: None,
            core: None,
            shared: false,
            plugins: None,
            listed_plugin_tools: None,
            bridge: None,
        })
    }
    pub fn from_core(core: Arc<AppCore>) -> Self {
        Self::attach(core, true)
    }
    /// A client of `core`: the running app's (`shared`), or a headless bridge's own muted core, whose documents, sequence IDs
    /// and undo every bridge and script on the profile shares while the app never sees them.
    pub fn attach(core: Arc<AppCore>, shared: bool) -> Self {
        let setup =
            SetupSession::from_system(core.system.read().clone(), &Default::default(), false);
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let client = format!(
            "client-{}",
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        core.studio_display.activity("clientConnected", &client);
        Self {
            root: core.paths.root().to_path_buf(),
            setup,
            initialized: false,
            client_id: Some(client),
            core: Some(core),
            shared,
            plugins: None,
            listed_plugin_tools: None,
            bridge: None,
        }
    }
    /// Uses `host` instead of the shared plugin host (tests).
    pub fn with_plugin_host(mut self, host: &'static studio_plugins::PluginHost) -> Self {
        self.plugins = Some(host);
        self
    }
    fn plugin_host(&self) -> Result<&'static studio_plugins::PluginHost, String> {
        match self.plugins {
            Some(host) => Ok(host),
            None => studio_plugins::shared(),
        }
    }
    /// Every plugin tool as `tools/list` shows it; none when the plugin folder can't be opened.
    fn plugin_tools(&self) -> Vec<Value> {
        self.plugin_host().map(|h| h.contributed_tools().iter().map(|t| t.to_mcp()).collect()).unwrap_or_default()
    }
    /// Notifications to send before the next response: `notifications/tools/list_changed` once the plugin tools differ
    /// from what the client last listed (a plugin started, stopped, was enabled or sent `tools.changed`).
    pub fn notifications(&mut self) -> Vec<Value> {
        let Some(listed) = &self.listed_plugin_tools else { return Vec::new() };
        let now = self.plugin_tools();
        if &now == listed {
            return Vec::new();
        }
        self.listed_plugin_tools = Some(now);
        vec![json!({"jsonrpc":"2.0","method":"notifications/tools/list_changed"})]
    }
    pub fn dispatch(&mut self, request: Value) -> Option<Value> {
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(Value::as_str);
        if request.get("jsonrpc") != Some(&json!("2.0"))
            || method.is_none()
            || id
                .as_ref()
                .is_some_and(|v| !v.is_string() && !v.is_number() && !v.is_null())
        {
            return Some(error(Value::Null, -32600, "Invalid Request"));
        }
        let Some(id) = id else {
            if method == Some(BRIDGE_NOTIFICATION) {
                self.bridge = request.get("params").and_then(|p| p.get("build")).and_then(announced_build);
            }
            return None;
        };
        let p = request.get("params").cloned().unwrap_or(json!({}));
        if !p.is_object() {
            return Some(error(id, -32602, "params must be an object"));
        }
        let method = method.unwrap();
        if !self.initialized && method != "initialize" && method != "ping" {
            return Some(error(id, -32002, "Initialize first"));
        }
        let result = match method {
            "initialize" => {
                if !p["protocolVersion"].is_string()
                    || !p["clientInfo"].is_object()
                    || !p["capabilities"].is_object()
                {
                    return Some(error(id, -32602, "Missing initialize fields"));
                }
                self.initialized = true;
                json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{"listChanged":true},"resources":{},"prompts":{}},"serverInfo":{"name":"vixen-studio","version":env!("CARGO_PKG_VERSION"),"build":build(),"bridge":self.bridge},"instructions":"Shared undoable display and sequence authoring. Hardware requires local permission and explicit confirmation; headless stays muted. Save/export is never implicit."})
            }
            "ping" => json!({}),
            "tools/list" => {
                let mut tools=TOOLS.iter().map(|(name,description)|json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":{"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":500}},"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}})).collect::<Vec<_>>();
                tools.extend(vixen_session::studio_display::command_tools());
                tools.extend(vixen_session::studio_sequence::tools());
                tools.extend(vixen_session::studio_live::tools());
                tools.push(geometry::tool());
                tools.push(vixen_session::studio_controller_network::tool());
                tools.push(vixen_session::studio_controller_network::overview_tool());
                tools.push(vixen_session::studio_patch::ranges_tool());
                tools.push(vixen_session::studio_single_bulbs::lights_tool());
                tools.push(vixen_session::studio_palette::tool());
                tools.extend(vixen_session::studio_updates::tools());
                tools.extend(studio_plugins::mcp::tools());
                tools.push(send_test_frame_tool());
                tools.push(required_tool());
                tools.push(controller_action_tool());
                let plugin_tools = self.plugin_tools();
                tools.extend(plugin_tools.iter().cloned());
                self.listed_plugin_tools = Some(plugin_tools);
                json!({"tools":tools})
            }
            "tools/call" => {
                let name = p["name"].as_str().unwrap_or("");
                if let Some(answer) = self.call_plugin_tool(name, &p) {
                    return Some(json!({"jsonrpc":"2.0","id":id,"result":answer}));
                }
                if !vixen_session::studio_live::tools()
                    .iter()
                    .any(|t| t["name"] == name)
                    && !vixen_session::studio_sequence::tools()
                        .iter()
                        .any(|t| t["name"] == name)
                    && !TOOLS.iter().any(|(n, _)| *n == name)
                    && name != "display_geometry"
                    && name != "controller_network"
                    && name != "controller_overview"
                    && name != "patch_ranges"
                    && name != "controller_lights"
                    && name != "command_palette"
                    && !vixen_session::studio_updates::is_tool(name)
                    && !vixen_session::studio_display::command_tools()
                        .iter()
                        .any(|t| t["name"] == name)
                {
                    return Some(error(id, -32602, "Unknown tool"));
                }
                let args = p.get("arguments").cloned().unwrap_or(json!({}));
                if let Some(c) = &self.core {
                    c.studio_display.activity("commandStarted", name);
                }
                let outcome = self.call(name, &args);
                if let Some(c) = &self.core {
                    c.studio_display.activity(
                        if outcome.is_ok() {
                            "commandFinished"
                        } else {
                            "commandFailed"
                        },
                        name,
                    );
                }
                match outcome {
                    Ok(value) => {
                        json!({"content":[{"type":"text","text":value.to_string()}],"isError":false})
                    }
                    Err(message) => {
                        json!({"content":[{"type":"text","text":message}],"isError":true})
                    }
                }
            }
            "resources/list" => {
                let mut resources = vec![json!({"uri":"studio://contract","name":"Studio MCP contract","mimeType":"text/markdown"}),json!({"uri":"display://summary","name":"Display prop orientation summary","description":"Every preview prop's type, size, Px1 end, S1 position and numbering direction; read once at the start of a session.","mimeType":"application/json"})];
                if let Ok(host) = self.plugin_host() {
                    resources.extend(studio_plugins::mcp::resources(host));
                }
                json!({"resources":resources})
            }
            "resources/read" if p["uri"].as_str().is_some_and(|u| u.starts_with("plugins://")) => {
                let read = self.plugin_host().and_then(|host| studio_plugins::mcp::read_resource(host, p["uri"].as_str().unwrap_or("")).unwrap_or(Err("Unknown resource".into())));
                match read {
                    Ok(contents) => json!({"contents":contents}),
                    Err(message) => return Some(error(id, -32602, &message)),
                }
            }
            "resources/read" if p["uri"] == "studio://contract" => {
                json!({"contents":[{"uri":"studio://contract","mimeType":"text/markdown","text":include_str!("../../../studio/MCP.md")}]})
            }
            "resources/read" if p["uri"] == "display://summary" => {
                let summary = self.with_system(|system, revision| geometry::summary(system, revision));
                json!({"contents":[{"uri":"display://summary","mimeType":"application/json","text":summary.to_string()}]})
            }
            "resources/read" => return Some(error(id, -32602, "Unknown resource")),
            "prompts/list" => {
                json!({"prompts":[{"name":"build_show","description":"Safe music-synchronized show workflow and current capability limits"},{"name":studio_plugins::mcp::PROMPT_NAME,"description":studio_plugins::mcp::PROMPT_DESCRIPTION,"arguments":[{"name":"goal","description":"What the plugin should do, e.g. drive LAN lamps as a controller.","required":false}]}]})
            }
            "prompts/get" if p["name"] == studio_plugins::mcp::PROMPT_NAME => {
                let text = studio_plugins::mcp::prompt(p["arguments"]["goal"].as_str());
                json!({"description":studio_plugins::mcp::PROMPT_DESCRIPTION,"messages":[{"role":"user","content":{"type":"text","text":text}}]})
            }
            "prompts/get" if p["name"] == "build_show" => {
                json!({"messages":[{"role":"user","content":{"type":"text","text":"Inspect studio_status, display_elements, controller_list, preview_list and diagnostics_report, and read display://summary (or display_geometry) for which way each prop runs: Px1 end, S1 position and numbering direction. Analyze real profile audio with music_analyze. Create a sequence with media, place effects on detected beats, add marks (sequence_onset_marks writes the hits as editable mark collections; effects_on_marks places an effect on every mark in one undo step; motif_place puts a multi-effect motif (march, train, swoosh_glow, slam, hop, pingpong, stab_run, climb, or one saved with motif_save) on every mark, varied per hit; effect_step turns staccato stabs into discrete segment walks, alternations, stacks or jumps), then sequence_render and preview_frame; review whole passages with preview_sheet (one tiled PNG) or preview_video (MP4 with the song) instead of many preview_frame calls, and check for dead air with sequence_activity (dark stretches from the exact intensities) before showing anyone a preview. Validate diagnostics and render results before confirmed sequence_save, profile_save and sequence_export_fseq. Use shared undo to correct edits. Never enable hardware merely to author a show. New schedules start turned off; schedules run only while the user has the scheduler on (Scheduler > Enabled). Unsupported parameter codecs and heuristic onset/section estimates must not be misrepresented as exact."}}]})
            }
            _ => return Some(error(id, -32601, "Method not found")),
        };
        Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
    /// `tools/call` for the plugin developer tools, `plugin_send_test_frame` and plugin-contributed tools; `None` for
    /// any other tool.
    fn call_plugin_tool(&mut self, name: &str, p: &Value) -> Option<Value> {
        let known = studio_plugins::mcp::is_tool(name) || matches!(name, "plugin_send_test_frame" | "plugin_required" | "controller_action") || name.starts_with(studio_plugins::tools::PREFIX);
        if !known {
            return None;
        }
        let args = p.get("arguments").cloned().unwrap_or(json!({}));
        if let Some(c) = &self.core {
            c.studio_display.activity("commandStarted", name);
        }
        let outcome = match self.plugin_host() {
            Err(why) => Err(format!("The plugin folder can't be opened: {why}")),
            Ok(host) if name.starts_with(studio_plugins::tools::PREFIX) => host.call_tool(name, args),
            Ok(host) if name == "plugin_required" => host.requirements(&self.root).map(|v| json!({"content":[{"type":"text","text":v.to_string()}],"isError":false})),
            Ok(_) if name == "plugin_send_test_frame" => self.send_test_frame(&args).map(|v| json!({"content":[{"type":"text","text":v.to_string()}],"isError":false})),
            Ok(_) if name == "controller_action" => self.controller_action(&args).map(|v| json!({"content":[{"type":"text","text":v.to_string()}],"isError":false})),
            Ok(host) => {
                let context = studio_plugins::mcp::Context { live_output_allowed: self.core.as_ref().is_some_and(|c| vixen_session::studio_live::allowed(c)) };
                studio_plugins::mcp::execute(host, name, &args, &context).map(|v| json!({"content":[{"type":"text","text":v.to_string()}],"isError":false}))
            }
        };
        let answer = outcome.unwrap_or_else(|message| json!({"content":[{"type":"text","text":message}],"isError":true}));
        if let Some(c) = &self.core {
            let failed = answer["isError"] == json!(true);
            c.studio_display.activity(if failed { "commandFailed" } else { "commandFinished" }, name);
        }
        Some(answer)
    }
    /// `controller_action`: runs a plugin controller's settings action (its tool answers new settings), then applies them
    /// with `controller_configure`, so they're checked the same way and are one undo step.
    fn controller_action(&mut self, args: &Value) -> Result<Value, String> {
        let object = args.as_object().ok_or("arguments must be an object")?;
        if let Some(key) = object.keys().find(|k| !matches!(k.as_str(), "id" | "action" | "removePatchedOutputs")) {
            return Err(format!("Unknown argument {key}"));
        }
        let id = args["id"].as_str().ok_or("id is required")?;
        let action = args["action"].as_str().ok_or("action is required")?;
        let setup = match &self.core {
            Some(core) => SetupSession::open(core),
            None => SetupSession::from_system(self.setup.system.clone(), &Default::default(), false),
        };
        let controller = setup.controllers.iter().find(|c| c.alive && c.id.to_string() == id).ok_or("No such controller")?;
        let Some(vixen_output::ControllerConfig::External(config)) = &controller.config else {
            return Err(format!("{} is not a plugin controller", controller.name));
        };
        let settings = vixen_output::modules::studio_external::describe(config)
            .and_then(|d| serde_json::from_str::<Value>(&d).ok())
            .map(|d| d["config"].clone())
            .filter(Value::is_object)
            .ok_or("The controller's saved settings can't be read")?;
        let answer: Value = serde_json::from_str(&vixen_output::modules::studio_external::run_action(config, &settings.to_string(), action)?).map_err(|e| e.to_string())?;
        let message = answer["message"].clone();
        let Some(new) = answer.get("config").filter(|c| c.is_object()) else {
            return Ok(json!({"applied":false,"message":message}));
        };
        if *new == settings {
            return Ok(json!({"applied":false,"unchanged":true,"message":message}));
        }
        let mut configure = json!({"id":id,"config":new});
        if let Some(remove) = args.get("removePatchedOutputs") {
            configure["removePatchedOutputs"] = remove.clone();
        }
        let result = self.call("controller_configure", &configure)?;
        Ok(json!({"applied":true,"message":message,"result":result}))
    }
    /// A commissioning test pattern on one plugin controller: `output_test_pattern` with its gating, after checking the
    /// controller belongs to a plugin.
    fn send_test_frame(&mut self, args: &Value) -> Result<Value, String> {
        let object = args.as_object().ok_or("arguments must be an object")?;
        let allowed = ["controllerId", "pattern", "brightnessPct", "stepMs", "durationMs", "confirm"];
        if let Some(key) = object.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(format!("Unknown argument {key}"));
        }
        let core = self.core.clone().ok_or("Test patterns need the running app with Live Output on, not a headless snapshot")?;
        let id = args["controllerId"].as_str().ok_or("controllerId is required")?;
        let plugin = {
            let setup = SetupSession::open(&core);
            let controller = setup.controllers.iter().find(|c| c.alive && c.id.to_string() == id).ok_or("No such controller")?;
            match &controller.config {
                Some(vixen_output::ControllerConfig::External(config)) => vixen_output::modules::studio_external::describe(config)
                    .and_then(|d| serde_json::from_str::<Value>(&d).ok())
                    .map(|d| d["pluginId"].clone())
                    .unwrap_or(Value::Null),
                _ => return Err(format!("{} is not a plugin controller; use output_test_pattern", controller.name)),
            }
        };
        let mut forwarded = args.clone();
        if forwarded.get("pattern").is_none() {
            forwarded["pattern"] = json!("movingPixel");
        }
        if forwarded["pattern"] == "pulse" {
            return Err("pulse needs an element; use movingPixel, colorBars or allOn".into());
        }
        let mut result = vixen_session::studio_live::execute(&core, "output_test_pattern", &forwarded)?;
        result["pluginId"] = json!(plugin);
        Ok(result)
    }
    fn call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        if vixen_session::studio_live::tools()
            .iter()
            .any(|t| t["name"] == name)
        {
            let core = self
                .core
                .as_ref()
                .ok_or("Live output requires the running app, not a headless snapshot")?;
            return vixen_session::studio_live::execute(core, name, args);
        }
        if vixen_session::studio_sequence::tools()
            .iter()
            .any(|t| t["name"] == name)
        {
            if self.core.is_none() {
                self.core = Some(Arc::new(
                    AppCore::open(self.root.clone(), true).map_err(|e| e.to_string())?,
                ));
            }
            let mut result = vixen_session::studio_sequence::execute(
                self.core.as_ref().unwrap(),
                name,
                args,
                true,
            )?;
            if matches!(
                name,
                "sequence_open" | "sequence_create" | "sequence_status"
            ) {
                result["sharedWithApp"] = json!(self.shared);
            }
            return Ok(result);
        }

        if vixen_session::studio_display::command_tools()
            .iter()
            .any(|t| t["name"] == name)
        {
            if self.core.is_none() {
                self.core = Some(Arc::new(
                    AppCore::open(self.root.clone(), true).map_err(|e| e.to_string())?,
                ));
            }
            let core = self.core.as_ref().unwrap();
            return core.studio_display.execute(core, name, args, true);
        }
        if name == "command_palette" {
            return vixen_session::studio_palette::execute(args);
        }
        if vixen_session::studio_updates::is_tool(name) {
            return vixen_session::studio_updates::execute(name, args);
        }
        if name == "display_geometry" {
            return self.with_system(|system, revision| geometry::execute(system, revision, args));
        }
        if let Some(core) = &self.core {
            self.setup =
                SetupSession::from_system(core.system.read().clone(), &Default::default(), false);
        }
        if name == "controller_network" {
            return vixen_session::studio_controller_network::execute(&self.setup, self.core.as_deref(), args);
        }
        if name == "controller_overview" {
            // The working copy above doesn't know which controllers run; one opened from the core does.
            return match self.core.as_deref() {
                Some(core) => vixen_session::studio_controller_network::overview_execute(&SetupSession::open(core), Some(core), args),
                None => vixen_session::studio_controller_network::overview_execute(&self.setup, None, args),
            };
        }
        if name == "patch_ranges" {
            return vixen_session::studio_patch::ranges_of(&self.setup, args);
        }
        if name == "controller_lights" {
            return vixen_session::studio_single_bulbs::controller_lights(&self.setup, args);
        }
        let obj = args.as_object().ok_or("arguments must be an object")?;
        if obj.keys().any(|k| k != "offset" && k != "limit") {
            return Err("Unknown argument".into());
        }
        let number = |key: &str, default: u64| -> Result<usize, String> {
            match args.get(key) {
                None => Ok(default as usize),
                Some(v) => v
                    .as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or(format!("{key} must be a nonnegative integer")),
            }
        };
        let offset = number("offset", 0)?;
        let limit = number("limit", 100)?;
        if !(1..=500).contains(&limit) {
            return Err("limit must be 1..500".into());
        }
        let s = &self.setup;
        let items:Vec<Value> = match name {
            "studio_status" => return Ok(json!({"profile":self.root,"mode":if self.shared {"liveSession"} else {"headlessSnapshot"},"sharedWithApp":self.shared,"liveOutputAllowed":self.core.as_ref().is_some_and(|c|vixen_session::studio_live::allowed(c)),"mutationsAvailable":true,"revision":self.core.as_ref().map_or(0,|c|c.display.revision()),"version":env!("CARGO_PKG_VERSION"),"build":build(),"bridge":self.bridge,"elements":s.tree.len(),"controllers":s.controllers.len()})),
            "display_elements" => s.tree.live_nodes().map(|(_,n)|json!({"id":n.id.to_string(),"name":n.name,"elementId":n.element_id.map(|i|i.to_string()),"children":n.children.iter().map(|id|s.tree.node(*id).id.to_string()).collect::<Vec<_>>()})).collect(),
            "show_list" => self.schedules()["shows"].as_array().cloned().unwrap_or_default(),
            "schedule_list" => {let mut all=self.schedules();return Ok(json!({"schedulerOn":all["schedulerOn"].take(),"status":all["status"].take(),"revision":all["revision"].take(),"now":all["now"].take(),"items":all["schedules"].take()}));},
            "image_map_list" => return Ok(self.with_system(|system, revision| vixen_session::studio_lipsync::describe(system, &vixen_io::DataPaths::new(self.root.clone()).module_data_dir().join("LipSync"), revision))),
            "face_mapping_list" => return Ok(self.with_system(vixen_session::studio_face_mapping::describe)),
            "phoneme_mapping_list" => return Ok(vixen_session::studio_phoneme_mappings::describe(&self.root.join("user_dictionary"))),
            "preview_props" => {let mut props=Vec::new();for entry in vixen_session::preview::preview_entries(&s.system){if let Ok(data)=vixen_session::preview::load_preview_data(&s.system.module_store,&entry){let design=vixen_session::render_support::design::DesignSession::new(data,Some(Arc::new(s.system.tree.clone())));for item in &design.data().display_items{props.push(json!({"previewId":entry.id.to_string(),"propId":item.id.0,"name":item.shape.name(),"kind":item.shape.type_name(),"properties":design.shape_properties(item.id).iter().map(|p|json!({"id":p.id,"name":p.name,"kind":format!("{:?}",p.kind),"value":format!("{:?}",p.value),"readOnly":p.read_only})).collect::<Vec<_>>()}));}}}props},
            "preview_list" => vixen_session::preview::preview_entries(&s.system).iter().map(|p|{let links=vixen_session::preview::load_preview_data(&s.system.module_store,p).ok().map(|data|vixen_session::render_support::Scene::build(&data,Some(&s.system.tree)).nodes().iter().take(500).map(|id|id.to_string()).collect::<Vec<_>>()).unwrap_or_default();json!({"id":p.id.to_string(),"name":p.name,"enabled":!s.system.disabled_devices.contains(&p.id),"linkedNodeIds":links})}).collect(),
            "controller_types" => vixen_session::setup::controllers::controller_types().iter().map(|(id,name)|json!({"id":id.to_string(),"name":name,"outputsFromConfig":vixen_output::modules::studio_external::outputs_from_config(id)})).collect(),
            "controller_list" => s.controllers.iter().filter(|c|c.alive).map(|c|json!({"id":c.id.to_string(),"name":c.name,"type":c.type_name(),"typeId":c.type_id.to_string(),"outputs":c.outputs.len(),"configuration":vixen_session::studio_diagnostics::controller_configuration(c),"running":self.core.as_ref().and_then(|core|core.playback.running_controllers()).is_some_and(|ids|ids.contains(&c.id))})).collect(),
            "sequence_list" | "media_list" => {
                let folder=self.root.join(if name=="sequence_list" {"Sequence"} else {"Media"});
                let mut paths=std::fs::read_dir(folder).map_err(|e|e.to_string())?.map(|e|e.map(|e| e.file_name().to_string_lossy().into_owned())).collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
                paths.sort(); paths.into_iter().map(|p|json!({"name":p})).collect()
            }
            "effect_catalog" => vixen_effect_registry::all().iter().map(|d|json!({"id":d.type_id.to_string(),"name":d.name,"description":d.description,"parameters":d.params.iter().map(|p|{let mut o=json!({"name":p.name,"label":p.label,"description":p.description,"schema":vixen_session::studio_params::param_schema(p)});if let Some(n)=vixen_session::studio_params::note(d.name,p.name){o["note"]=json!(n);}o}).collect::<Vec<_>>()})).collect(),
            "diagnostics_report" => return Ok(vixen_session::studio_diagnostics::report(s,self.core.as_deref())),
            "recent_logs" => return vixen_session::studio_diagnostics::recent_logs(&self.root,limit),
            "controller_monitor" => return Ok(self.core.as_ref().map(|c|vixen_session::studio_diagnostics::monitor(c)).unwrap_or(json!({"execution":"Not started","controllers":[]}))),
            _ => return Err("Unknown tool".into()),
        };
        let total = items.len();
        let page: Vec<_> = items.into_iter().skip(offset).take(limit).collect();
        Ok(
            json!({"revision":self.core.as_ref().map_or(0,|c|c.display.revision()),"items":page,"total":total,"nextOffset":if offset.saturating_add(limit)<total {Some(offset+limit)} else {None}}),
        )
    }
}
/// What the show window's missing-plugin banner reads.
fn required_tool() -> Value {
    json!({"name":"plugin_required","description":"The plugins this show's controllers need (its SystemData/StudioPlugins.json, rewritten on every save) and whether each is ready: id, versionRange, installedVersion, state, satisfied and, when not, message (\"Needs plugin … — not installed\"). The show window shows a banner while any isn't satisfied.","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}})
}

fn controller_action_tool() -> Value {
    json!({"name":"controller_action","description":"Run one of a plugin controller's settings actions (controller_network plugin.actions, e.g. detect: Detect Lights), as the controller inspector's button does: the plugin's tool works out new settings (it may scan the network), then they're applied like controller_configure {config} (checked against the configSchema; a plugin that sets its own output count resizes the controller), one undo step. applied:false when the action changed nothing; message is what the plugin said. New settings that would drop patched outputs are refused unless removePatchedOutputs is true.","inputSchema":{"type":"object","properties":{"id":{"type":"string","description":"Controller id."},"action":{"type":"string","description":"Action id from controller_network plugin.actions."},"removePatchedOutputs":{"type":"boolean"}},"required":["id","action"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"openWorldHint":true}})
}

fn send_test_frame_tool() -> Value {
    json!({"name":"plugin_send_test_frame","description":"Drive one plugin controller with a commissioning test pattern, like output_test_pattern on that controller: pattern movingPixel (default; one output at a time, stepMs each), colorBars or allOn, at brightnessPct, for durationMs or until output_test_stop. Same gating: the local Allow agents to control live output setting, Live Output on (the controller running) and confirm:true. Muted Studio sends nothing.","inputSchema":{"type":"object","properties":{"controllerId":{"type":"string"},"pattern":{"enum":["movingPixel","colorBars","allOn"]},"brightnessPct":{"type":"integer","minimum":1,"maximum":100},"stepMs":{"type":"integer","minimum":vixen_session::studio_commission::STEP_RANGE_MS.0,"maximum":vixen_session::studio_commission::STEP_RANGE_MS.1},"durationMs":{"type":"integer","minimum":25,"maximum":vixen_session::studio_commission::MAX_DURATION_MS},"confirm":{"type":"boolean"}},"required":["controllerId","confirm"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":true,"openWorldHint":true}})
}
/// The notification a bridge sends before relaying its client, so the session can report the bridge's own build: a bridge
/// from an older install can relay to a newer app. Sessions that predate it ignore it, as they do any notification.
pub const BRIDGE_NOTIFICATION: &str = "notifications/studio/bridge";

/// The commit this binary was built from (`build.rs`): {commit, date (the commit's), dirty (tracked files differed from it)}.
/// Null fields when it was built outside a git checkout.
pub fn build() -> Value {
    let text = |s: &str| if s.is_empty() { Value::Null } else { json!(s) };
    json!({
        "commit": text(env!("STUDIO_BUILD_COMMIT")),
        "date": text(env!("STUDIO_BUILD_DATE")),
        "dirty": match env!("STUDIO_BUILD_DIRTY") { "true" => json!(true), "false" => json!(false), _ => Value::Null },
    })
}

/// `BRIDGE_NOTIFICATION`'s line for this binary.
pub fn bridge_notification() -> String {
    json!({"jsonrpc":"2.0","method":BRIDGE_NOTIFICATION,"params":{"build":build()}}).to_string()
}

/// A bridge's announced build, kept only in `build()`'s shape: a hex commit, a short date and a flag.
fn announced_build(build: &Value) -> Option<Value> {
    let commit = build.get("commit")?;
    let date = build.get("date")?;
    let dirty = build.get("dirty")?;
    let commit_ok = commit.is_null() || commit.as_str().is_some_and(|c| (7..=64).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_hexdigit()));
    let date_ok = date.is_null() || date.as_str().is_some_and(|d| d.len() <= 40 && d.bytes().all(|b| b.is_ascii_alphanumeric() || b"-:+.".contains(&b)));
    (commit_ok && date_ok && (dirty.is_null() || dirty.is_boolean())).then(|| json!({"commit":commit,"date":date,"dirty":dirty}))
}

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
/// Serves one client's lines until its input ends. Nothing a request does closes the connection: an oversized request or
/// response, or a tool that panics, gets an error reply and the next line is served, so one bad call never drops a bridge (and
/// every MCP client sharing it) mid-session. Only I/O failures, the idle limit and the end of input end it.
pub fn serve(
    server: &mut Server,
    mut input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    loop {
        let mut bytes = Vec::new();
        let read = std::io::Read::take(&mut input, (MAX_MESSAGE + 1) as u64)
            .read_until(b'\n', &mut bytes)?;
        if read == 0 {
            return Ok(());
        }
        let response = if bytes.len() > MAX_MESSAGE {
            if bytes.last() != Some(&b'\n') {
                skip_line(&mut input)?;
            }
            Some(error(Value::Null, -32600, "MCP message exceeds 1 MiB; split it, or pass a large batch as opsFile"))
        } else {
            match serde_json::from_slice::<Value>(&bytes) {
                Ok(v) => dispatch_guarded(server, v),
                Err(_) => Some(error(Value::Null, -32700, "Parse error")),
            }
        };
        for note in server.notifications() {
            writeln!(output, "{note}")?;
        }
        if let Some(value) = response {
            let mut encoded = value.to_string();
            if encoded.len() > 8 * MAX_MESSAGE {
                encoded = error(value.get("id").cloned().unwrap_or(Value::Null), -32603, "Response exceeds 8 MiB; reduce limit").to_string();
            }
            writeln!(output, "{encoded}")?;
            output.flush()?;
        }
    }
}

/// Discards the rest of an oversized line, without buffering it, up to and including its newline.
fn skip_line(input: &mut impl BufRead) -> std::io::Result<()> {
    loop {
        let buffer = input.fill_buf()?;
        if buffer.is_empty() {
            return Ok(());
        }
        match buffer.iter().position(|&b| b == b'\n') {
            Some(end) => {
                input.consume(end + 1);
                return Ok(());
            }
            None => {
                let all = buffer.len();
                input.consume(all);
            }
        }
    }
}

/// `Server::dispatch`, answering a panic with an internal error instead of ending the connection's thread. The panic is logged
/// (the crash hook writes it to the profile log); session state uses non-poisoning locks, so the next call works as before.
fn dispatch_guarded(server: &mut Server, request: Value) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str).unwrap_or("").to_string();
    let call = match request.pointer("/params/name").and_then(Value::as_str) {
        Some(tool) => format!("{method} {tool}"),
        None => method,
    };
    guarded(id, &call, || server.dispatch(request))
}

fn guarded(id: Option<Value>, call: &str, dispatch: impl FnOnce() -> Option<Value>) -> Option<Value> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(dispatch)) {
        Ok(response) => response,
        Err(panic) => {
            let what = panic.downcast_ref::<&str>().map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_string());
            vixen_session::logging::error("Vixen.Studio.MCP", &format!("{call} panicked; the connection stays open"), Some(&what));
            id.map(|id| error(id, -32603, &format!("Internal error in {call}: {what}. The connection stays open; read the state back before retrying.")))
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let (Some(core), Some(id)) = (&self.core, &self.client_id) {
            core.studio_display.activity("clientDisconnected", id);
        }
    }
}

#[cfg(test)]
mod guard_tests {
    use super::*;

    #[test]
    fn a_panicking_call_is_answered_with_an_internal_error() {
        let reply = guarded(Some(json!(7)), "tools/call x", || panic!("boom")).unwrap();
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["error"]["code"], -32603);
        let message = reply["error"]["message"].as_str().unwrap();
        assert!(message.contains("tools/call x") && message.contains("boom"), "{message}");
        assert!(guarded(None, "notifications/x", || panic!("boom")).is_none(), "a notification gets no reply");
        assert_eq!(guarded(Some(json!(1)), "ping", || Some(json!({"ok":true}))).unwrap(), json!({"ok":true}));
    }

    #[test]
    fn skip_line_drops_only_the_rest_of_the_line() {
        let mut input = std::io::Cursor::new(b"aaaa\nnext\n".to_vec());
        skip_line(&mut input).unwrap();
        let mut rest = String::new();
        input.read_line(&mut rest).unwrap();
        assert_eq!(rest, "next\n");
    }
}
