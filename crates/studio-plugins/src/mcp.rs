//! The plugin developer tools, resources and `build_plugin` prompt the Studio MCP server offers (studio-mcp lists and
//! routes them; the plugin-controller test pattern lives there, since it needs the open show). Read-only tools,
//! `plugin_scaffold` and `plugin_validate` are always available; installing, enabling, linking, reloading and removing
//! need `confirm: true` and the local "Allow agents to manage plugins" switch ([`crate::agents_may_manage`]).
use crate::harness;
use crate::logs::Level;
use crate::PluginHost;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What the caller knows that this crate doesn't.
#[derive(Clone, Copy, Debug, Default)]
pub struct Context {
    /// The local "Allow agents to control live output" setting is on (the running app only).
    pub live_output_allowed: bool,
}

const MANAGE: [&str; 7] = ["plugin_install", "plugin_uninstall", "plugin_enable", "plugin_disable", "plugin_link_dev", "plugin_unlink_dev", "plugin_reload"];

const MANIFEST_SCHEMA: &str = include_str!("../../../studio/plugins/schema/manifest.schema.json");
const PROTOCOL_SCHEMA: &str = include_str!("../../../studio/plugins/schema/protocol.schema.json");
const DESIGN: &str = include_str!("../../../studio/PLUGINS-DESIGN.md");

fn tool(name: &str, description: &str, properties: Value, required: &[&str], read_only: bool) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {"type": "object", "properties": properties, "required": required, "additionalProperties": false},
        "annotations": {"readOnlyHint": read_only, "destructiveHint": !read_only && name != "plugin_scaffold", "openWorldHint": name == "plugin_validate" || !read_only},
    })
}

/// The plugin tools for `tools/list`.
pub fn tools() -> Vec<Value> {
    let id = json!({"type": "string", "description": "Plugin id, e.g. io.github.someone.lamp-output."});
    let confirm = json!({"type": "boolean", "description": "Must be true."});
    let manage = "Needs confirm:true and the local setting Allow agents to manage plugins (Settings ▸ Plugins), which only the person at this Mac can turn on and which is off at every launch.";
    vec![
        tool("plugin_list", "Installed and development plugins: id, name, version, source (installed/dev), enabled, state (running, stopped, starting, restarting, crashed, disabled, invalid, incompatible), pid, uptimeMs, restartCount, problem, lastError, contributed controller types/tools/commands/events; plus agentsMayManage (the local Allow agents to manage plugins switch) and each plugin tool's MCP name.", json!({}), &[], true),
        tool("plugin_info", "One plugin in detail: manifest, permissions, negotiated API version, capabilities, data folder, log file, lastLatencyMs of its last request, each started controller instance (status, live, framesSent, framesDropped, lastFrameAgeMs, counters, configErrors) and its MCP tool names.", json!({"id": id}), &["id"], true),
        tool("plugin_logs", "The newest log entries of one plugin, oldest first: its stderr, its log messages and the host's notes (starts, exits, restarts, protocol errors). Also written to Plugins/logs/<id>.log.", json!({"id": id, "limit": {"type": "integer", "minimum": 1, "maximum": 2000, "default": 100}, "level": {"enum": ["debug", "info", "warn", "error"], "default": "debug", "description": "Lowest level to include."}}), &["id"], true),
        tool("plugin_scaffold", "Create a working plugin from a template in a new folder under ~/Library/Application Support/Vixen Studio/Plugins/dev/ (only there; an existing folder is never touched): vixen-plugin.json, the main file, the language's SDK file next to it (it speaks the protocol; leave it as it is) and a README. kind controller adds an output type that logs the channel values it gets; tools adds an agent tool. Returns the folder, files and next steps (a Rust plugin needs ./build.sh before it runs). Doesn't link or start it. Same templates as `vixen-studio-mcp plugin new`.", json!({"id": id, "name": {"type": "string", "maxLength": 80, "description": "Display name: letters, digits, spaces and . _ ( ) -."}, "language": {"enum": crate::scaffold::LANGUAGES, "description": "python (python3, standard library only), typescript (Node 20+, no dependencies; plain ES module with .d.ts types) or rust (cargo; serde_json only)."}, "kind": {"enum": ["controller", "tools"]}, "folder": {"type": "string", "description": "Folder name under Plugins/dev (default: the last part of the id)."}}), &["id", "name", "language", "kind"], false),
        tool("plugin_validate", "Check a plugin: manifest and configSchema, then the conformance harness runs it on its own (never through the show or real output): initialize, per controller type create/configure with a valid and an invalid configuration/setOutputCount/destroy, then shutdown and exit. Returns passed, every check (pass, warn, fail, skip with ms and detail), what the plugin reported and its log. controller.start and 40 fps frame bursts in each encoding it accepts run only with liveOutput:true, which needs confirm:true, Live Output on and the local Allow agents to control live output setting, since a real output plugin lights real lights. path: a folder under Plugins/dev or an installed/linked plugin's folder; or give its id.", json!({"id": id, "path": {"type": "string", "description": "Absolute plugin folder."}, "liveOutput": {"type": "boolean", "default": false}, "confirm": confirm, "burstMs": {"type": "integer", "minimum": 100, "maximum": 10000, "default": 1000, "description": "Length of each encoding's frame burst."}}), &[], true),
        tool("plugin_install", &format!("Install a plugin from a folder or a .vixenplugin package (a zip of the folder, manifest at its root) into Plugins/<id>/<version>/, replacing any other version, enable and start it. Shows its permissions in the result. Symlinks and paths outside the package are refused; the download quarantine flag is cleared. {manage}"), json!({"path": {"type": "string", "description": "Absolute path."}, "confirm": confirm}), &["path", "confirm"], false),
        tool("plugin_uninstall", &format!("Stop an installed plugin and remove its files and registry entry. Its data folder and log stay. Shows that use its controllers keep them, with health \"Needs plugin …\". {manage}"), json!({"id": id, "confirm": confirm}), &["id", "confirm"], false),
        tool("plugin_enable", &format!("Enable and start a plugin; remembered across launches. {manage}"), json!({"id": id, "confirm": confirm}), &["id", "confirm"], false),
        tool("plugin_disable", &format!("Stop and disable a plugin; its controllers stay in the show with health \"Needs plugin … — it is disabled\". {manage}"), json!({"id": id, "confirm": confirm}), &["id", "confirm"], false),
        tool("plugin_link_dev", &format!("Link a development folder (absolute path) and start the plugin in it; Studio restarts it whenever a file in the folder changes. {manage}"), json!({"path": {"type": "string"}, "confirm": confirm}), &["path", "confirm"], false),
        tool("plugin_unlink_dev", &format!("Stop a linked development plugin and remove the link; the folder is left alone. {manage}"), json!({"id": id, "confirm": confirm}), &["id", "confirm"], false),
        tool("plugin_reload", &format!("Stop a plugin, read its manifest again, forget earlier crashes and start it when enabled. {manage}"), json!({"id": id, "confirm": confirm}), &["id", "confirm"], false),
    ]
}

pub fn is_tool(name: &str) -> bool {
    tools().iter().any(|t| t["name"] == name)
}

fn check_args(args: &Value, name: &str) -> Result<(), String> {
    let object = args.as_object().ok_or("arguments must be an object")?;
    let definition = tools().into_iter().find(|t| t["name"] == name).ok_or("Unknown tool")?;
    let allowed = definition["inputSchema"]["properties"].as_object().cloned().unwrap_or_default();
    if let Some(key) = object.keys().find(|k| !allowed.contains_key(*k)) {
        return Err(format!("Unknown argument {key}"));
    }
    for key in definition["inputSchema"]["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if !object.contains_key(key) {
            return Err(format!("{key} is required"));
        }
    }
    Ok(())
}

fn string<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or(format!("{key} must be a non-empty string"))
}

fn absolute(args: &Value) -> Result<PathBuf, String> {
    let path = PathBuf::from(string(args, "path")?);
    if !path.is_absolute() {
        return Err("path must be absolute".into());
    }
    Ok(path)
}

/// Runs one plugin tool.
pub fn execute(host: &PluginHost, name: &str, args: &Value, context: &Context) -> Result<Value, String> {
    check_args(args, name)?;
    if MANAGE.contains(&name) {
        if args.get("confirm") != Some(&Value::Bool(true)) {
            return Err(format!("{name} needs confirm:true"));
        }
        if !crate::agents_may_manage() {
            return Err("Allow agents to manage plugins is off. Only the person at this Mac can turn it on, in Settings ▸ Plugins; it resets at every launch.".into());
        }
    }
    host.refresh();
    match name {
        "plugin_list" => {
            let mut list = host.list();
            list["agentsMayManage"] = json!(crate::agents_may_manage());
            list["tools"] = Value::Array(host.contributed_tools().iter().map(|t| json!({"name": t.mcp_name, "pluginId": t.plugin_id, "tool": t.name, "readOnly": t.read_only})).collect());
            Ok(list)
        }
        "plugin_info" => {
            let id = string(args, "id")?;
            let mut info = host.info(id)?;
            info["mcpTools"] = Value::Array(host.contributed_tools().into_iter().filter(|t| t.plugin_id == id).map(|t| json!(t.mcp_name)).collect());
            Ok(info)
        }
        "plugin_logs" => {
            let limit = match args.get("limit") {
                None => 100,
                Some(v) => v.as_u64().filter(|n| (1..=2000).contains(n)).ok_or("limit must be 1..2000")? as usize,
            };
            let level = match args.get("level") {
                None => Level::Debug,
                Some(v) => v.as_str().and_then(Level::parse).ok_or("level must be debug, info, warn or error")?,
            };
            host.logs(string(args, "id")?, limit, level)
        }
        "plugin_scaffold" => {
            let folder = args.get("folder").map(|f| f.as_str().ok_or("folder must be a string")).transpose()?;
            crate::scaffold::scaffold(&host.root(), string(args, "id")?, string(args, "name")?, string(args, "language")?, string(args, "kind")?, folder)
        }
        "plugin_validate" => validate(host, args, context),
        "plugin_install" => host.install(&absolute(args)?),
        "plugin_uninstall" => host.uninstall(string(args, "id")?),
        "plugin_enable" | "plugin_disable" => host.set_enabled(string(args, "id")?, name == "plugin_enable"),
        "plugin_link_dev" => host.link_dev(&absolute(args)?),
        "plugin_unlink_dev" => {
            let id = string(args, "id")?;
            host.unlink_dev(id)?;
            Ok(json!({"id": id, "unlinked": true}))
        }
        "plugin_reload" => host.reload(string(args, "id")?),
        _ => Err("Unknown tool".into()),
    }
}

/// The folder `plugin_validate` may run: one under `Plugins/dev`, or a known plugin's.
fn validation_folder(host: &PluginHost, args: &Value) -> Result<PathBuf, String> {
    match (args.get("id"), args.get("path")) {
        (Some(_), Some(_)) => Err("give id or path, not both".into()),
        (Some(_), None) => {
            let info = host.info(string(args, "id")?)?;
            Ok(PathBuf::from(info["folder"].as_str().unwrap_or_default()))
        }
        (None, Some(_)) => {
            let path = absolute(args)?.canonicalize().map_err(|e| format!("path: {e}"))?;
            let dev = host.root().join("dev").canonicalize().ok();
            let known = host.list()["plugins"].as_array().into_iter().flatten().filter_map(|p| p["folder"].as_str()).any(|f| Path::new(f).canonicalize().is_ok_and(|f| f == path));
            if dev.is_some_and(|dev| path.starts_with(dev)) || known {
                Ok(path)
            } else {
                Err(format!("plugin_validate runs plugins in {}/dev (plugin_scaffold puts them there) or installed and linked plugins only", host.root().display()))
            }
        }
        (None, None) => Err("give the plugin's id or its folder's path".into()),
    }
}

fn validate(host: &PluginHost, args: &Value, context: &Context) -> Result<Value, String> {
    let dir = validation_folder(host, args)?;
    let live_output = args.get("liveOutput").and_then(Value::as_bool).unwrap_or(false);
    if live_output {
        if args.get("confirm") != Some(&Value::Bool(true)) {
            return Err("liveOutput:true needs confirm:true: the plugin may light real lights".into());
        }
        if !context.live_output_allowed {
            return Err("liveOutput:true needs the local setting Allow agents to control live output, in the running app".into());
        }
        if host.muted() {
            return Err("liveOutput:true needs Live Output on; Studio is muted".into());
        }
    }
    let burst = match args.get("burstMs") {
        None => 1000,
        Some(v) => v.as_u64().filter(|n| (100..=10_000).contains(n)).ok_or("burstMs must be 100..10000")?,
    };
    let options = harness::Options { live_output, burst: Duration::from_millis(burst), ..harness::Options::default() };
    Ok(harness::validate(&dir, &options))
}

// ---- Resources and the prompt ------------------------------------------------------------------------------------

/// Static resources, then each plugin's manifest.
pub fn resources(host: &PluginHost) -> Vec<Value> {
    let mut list = vec![
        json!({"uri": "plugins://api", "name": "Plugin API and design", "description": "How plugins work: manifest, protocol (every method), controllers, tools, permissions, lifecycle.", "mimeType": "text/markdown"}),
        json!({"uri": "plugins://schema/manifest", "name": "vixen-plugin.json JSON Schema", "mimeType": "application/schema+json"}),
        json!({"uri": "plugins://schema/protocol", "name": "Plugin protocol JSON Schema (every method's params and result)", "mimeType": "application/schema+json"}),
    ];
    for (language, example) in EXAMPLES {
        list.push(json!({"uri": format!("plugins://examples/{language}"), "name": format!("Example {language} plugin: {}", example.folder), "description": example.about, "mimeType": "text/markdown"}));
        list.push(json!({"uri": format!("plugins://sdk/{language}"), "name": format!("The {language} plugin SDK ({})", sdk(language).map(|(file, _)| file).unwrap_or_default()), "description": "The one file a plugin copies next to its main file to speak the protocol.", "mimeType": "text/plain"}));
    }
    for plugin in host.list()["plugins"].as_array().into_iter().flatten() {
        if let Some(id) = plugin["id"].as_str().filter(|_| plugin.get("version").is_some()) {
            list.push(json!({"uri": format!("plugins://{id}/manifest"), "name": format!("{} manifest", plugin["name"].as_str().unwrap_or(id)), "mimeType": "application/json"}));
        }
    }
    list
}

/// `resources/read` for a `plugins://` URI: `Some(Ok(contents))`, `Some(Err)` for a known shape that failed, `None` when
/// the URI isn't a plugin resource.
pub fn read_resource(host: &PluginHost, uri: &str) -> Option<Result<Value, String>> {
    let rest = uri.strip_prefix("plugins://")?;
    let text = |mime: &str, text: String| Ok(json!([{"uri": uri, "mimeType": mime, "text": text}]));
    Some(match rest {
        "api" => text("text/markdown", DESIGN.to_string()),
        "schema/manifest" => text("application/schema+json", MANIFEST_SCHEMA.to_string()),
        "schema/protocol" => text("application/schema+json", PROTOCOL_SCHEMA.to_string()),
        _ if rest.starts_with("examples/") => match example(&rest["examples/".len()..]) {
            Some(markdown) => text("text/markdown", markdown),
            None => Err(format!("no example for that language; there are {}", crate::scaffold::LANGUAGES.join(", "))),
        },
        _ if rest.starts_with("sdk/") => match sdk(&rest["sdk/".len()..]) {
            Some((_, source)) => text("text/plain", source.to_string()),
            None => Err(format!("no SDK for that language; there are {}", crate::scaffold::LANGUAGES.join(", "))),
        },
        _ => match rest.strip_suffix("/manifest") {
            Some(id) => host.info(id).and_then(|info| match info.get("manifest") {
                Some(manifest) => text("application/json", serde_json::to_string_pretty(manifest).expect("serializes")),
                None => Err(format!("{id} has no valid manifest: {}", info["problem"].as_str().unwrap_or("unknown problem"))),
            }),
            None => return None,
        },
    })
}

/// An example plugin from `studio/plugins/examples/`, served as Markdown (its SDK file is `plugins://sdk/<language>`).
pub struct Example {
    pub folder: &'static str,
    pub about: &'static str,
    pub files: &'static [(&'static str, &'static str)],
}

pub const EXAMPLES: [(&str, Example); 3] = [
    ("python", Example {
        folder: "console-output",
        about: "The tutorial plugin: a controller type that prints channel values to its log; settings, status, a tool with a menu command, an event and storage.",
        files: &[
            ("vixen-plugin.json", include_str!("../../../studio/plugins/examples/console-output/vixen-plugin.json")),
            ("main.py", include_str!("../../../studio/plugins/examples/console-output/main.py")),
        ],
    }),
    ("typescript", Example {
        folder: "udp-pixel-output",
        about: "A controller type sending raw RGB over UDP to host:port, with a discover tool.",
        files: &[
            ("vixen-plugin.json", include_str!("../../../studio/plugins/examples/udp-pixel-output/vixen-plugin.json")),
            ("main.mjs", include_str!("../../../studio/plugins/examples/udp-pixel-output/main.mjs")),
        ],
    }),
    ("rust", Example {
        folder: "rust-http-output",
        about: "A controller type POSTing colours as JSON to a URL, rate-limited from a worker thread: the shape a cloud-API light needs.",
        files: &[
            ("vixen-plugin.json", include_str!("../../../studio/plugins/examples/rust-http-output/vixen-plugin.json")),
            ("Cargo.toml", include_str!("../../../studio/plugins/examples/rust-http-output/Cargo.toml")),
            ("build.sh", include_str!("../../../studio/plugins/examples/rust-http-output/build.sh")),
            ("src/main.rs", include_str!("../../../studio/plugins/examples/rust-http-output/src/main.rs")),
        ],
    }),
];

/// The SDK file a plugin in `language` copies next to its main file.
pub fn sdk(language: &str) -> Option<(&'static str, &'static str)> {
    match language {
        "python" => Some(("vixen_plugin.py", crate::scaffold::PYTHON_SDK)),
        "typescript" => Some(("vixen-plugin.mjs", crate::scaffold::TYPESCRIPT_SDK)),
        "rust" => Some(("src/vixen_plugin_sdk.rs", crate::scaffold::RUST_SDK)),
        _ => None,
    }
}

fn example(language: &str) -> Option<String> {
    let (_, example) = EXAMPLES.iter().find(|(l, _)| *l == language)?;
    let (sdk_file, _) = sdk(language)?;
    let mut out = format!(
        "# Example {language} plugin: {}\n\n{}\n\nIt lives in studio/plugins/examples/{} in the Vixen Studio repository. Next to these files it has \
         {sdk_file}, the {language} SDK (MCP resource plugins://sdk/{language}); `plugin_scaffold` writes the same SDK file.\n",
        example.folder, example.about, example.folder
    );
    for (name, contents) in example.files {
        let fence = match name.rsplit('.').next() {
            Some("json") => "json",
            Some("py") => "python",
            Some("mjs") => "javascript",
            Some("rs") => "rust",
            Some("toml") => "toml",
            Some("sh") => "sh",
            _ => "",
        };
        out.push_str(&format!("\n## {name}\n\n```{fence}\n{}```\n", contents.trim_end().to_string() + "\n"));
    }
    Some(out)
}

pub const PROMPT_NAME: &str = "build_plugin";
pub const PROMPT_DESCRIPTION: &str = "Build, test and ship a Vixen Studio plugin (an output controller type or agent tools) through MCP";

/// The `build_plugin` prompt's text.
pub fn prompt(goal: Option<&str>) -> String {
    let goal = goal.map(|g| format!("Goal: {g}\n\n")).unwrap_or_default();
    format!(
        "{goal}Build a Vixen Studio plugin. A plugin is a folder with vixen-plugin.json and a program in any language that Studio \
         runs and talks to with JSON-RPC 2.0 over stdin/stdout, one message per line (logs to stderr, never stdout).\n\
         1. Read plugins://api (design and every protocol method), plugins://schema/manifest and plugins://schema/protocol, and \
         plugins://examples/python, plugins://examples/typescript or plugins://examples/rust.\n\
         2. plugin_scaffold {{id (reverse-DNS, lowercase), name, language (python, typescript or rust), kind: controller (an \
         output type, e.g. a network light protocol) or tools (agent tools)}}. It writes a working plugin under Plugins/dev/, \
         with the language's SDK file next to the main file (it speaks the protocol; don't edit it), and returns the folder. \
         A Rust plugin needs ./build.sh before it runs.\n\
         3. Implement it in that folder with your own file tools: configSchema in the manifest (it becomes the controller \
         inspector's form; the SDK checks configurations against it and fills in defaults), then the Controller class in the \
         main file: configure (refuse what the schema can't express with ConfigError, which Studio shows next to the field), \
         start/stop (connect and disconnect), frame (one 0-255 level per output; never block it for long: hand slow device \
         I/O to a worker), and the tool functions. Keep the typeId forever: shows store it. Declare permissions honestly. \
         Without an SDK, plugins://api has every protocol method.\n\
         4. plugin_validate {{path}} and fix every fail and warning. Frames and controller.start are only exercised with \
         liveOutput:true (the user must allow agents to control live output); never point a test at real lights without the \
         user's say-so.\n\
         5. Ask the user to turn on Allow agents to manage plugins in Settings ▸ Plugins, then plugin_link_dev {{path, \
         confirm:true}}. Studio restarts it whenever you save a file. Read plugin_logs and plugin_info (state, lastError, \
         instances) to debug.\n\
         6. Controllers: controller_types lists the new type; controller_create and controller_configure {{config}} add it to the \
         show (undoable). Patch a test element to it, then plugin_send_test_frame (or output_test_pattern) once the user has \
         allowed live output; muted Studio sends nothing. Tools appear as plugin__<short-id>__<tool> after tools/list.\n\
         7. Bump version in the manifest for each release; `vixen-studio-mcp plugin pack <folder>` makes <id>-<version>.vixenplugin \
         (a zip with the manifest at its root, plus its SHA-256) to share, which plugin_install installs.\n\
         Don't build vendor-specific support into Studio itself: everything device-specific belongs in the plugin."
    )
}
