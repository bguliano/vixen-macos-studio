//! P3: contributed tools, the conformance harness, scaffolding, install and the MCP tool layer, against real plugin
//! processes (the fixture in src/bin/studio-plugin-fixture.rs and the scaffold templates under python3 and node).
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use studio_plugins::harness::{self, Options};
use studio_plugins::mcp::{self, Context};
use studio_plugins::{HostOptions, PluginHost, State};

const FIXTURE: &str = env!("CARGO_BIN_EXE_studio-plugin-fixture");
const TYPE_ID: &str = "0a6c1f2e-3b4d-4c5e-8f90-123456789abc";

fn options() -> HostOptions {
    HostOptions {
        request_timeout: Duration::from_secs(2),
        init_timeout: Duration::from_secs(2),
        shutdown_grace: Duration::from_millis(400),
        backoff: [Duration::from_millis(50), Duration::from_millis(100), Duration::from_millis(150)],
        restart_window: Duration::from_secs(30),
        tick: Duration::from_millis(20),
        dev_poll: Duration::from_millis(60),
    }
}

fn quick() -> Options {
    Options { burst: Duration::from_millis(300), request_timeout: Duration::from_secs(3), start_timeout: Duration::from_secs(3), shutdown_grace: Duration::from_millis(800), ..Options::default() }
}

struct Sandbox {
    base: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Sandbox {
        let base = std::env::temp_dir().join(format!("studio-plugins-mcp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Sandbox { base: base.canonicalize().unwrap() }
    }

    fn root(&self) -> PathBuf {
        self.base.join("Plugins")
    }

    fn host(&self) -> PluginHost {
        PluginHost::new(&self.root(), options()).unwrap()
    }

    /// A fixture plugin folder (under Plugins/dev so plugin_validate may run it).
    fn plugin(&self, folder: &str, args: &[&str], contributes: Value) -> PathBuf {
        let dir = self.root().join("dev").join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = json!({
            "id": format!("com.example.{folder}"),
            "name": format!("Fixture {folder}"),
            "version": "1.2.0",
            "apiVersion": "^1.0",
            "entry": {"command": FIXTURE, "args": args},
            "contributes": contributes,
        });
        std::fs::write(dir.join("vixen-plugin.json"), manifest.to_string()).unwrap();
        dir
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn lamp_controller() -> Value {
    json!({"controllers": [{
        "typeId": TYPE_ID,
        "name": "Lamp",
        "configSchema": {"type": "object", "properties": {"label": {"type": "string", "default": "porch"}, "seen": {"type": "boolean"}}, "additionalProperties": false},
        "defaultOutputs": 3,
        "frameEncodings": ["u8", "json", "u16"],
    }]})
}

fn check<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["checks"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap_or_else(|| panic!("no check {name} in {report:#}"))
}

#[test]
fn contributed_tools_are_proxied_and_follow_tools_changed() {
    let sandbox = Sandbox::new("tools");
    let tools = json!({"tools": [
        {"name": "echo", "description": "Echo the arguments.", "inputSchema": {"type": "object", "properties": {"x": {"type": "integer"}}}, "readOnly": true},
        {"name": "discover", "description": "Find lamps."},
        {"name": "malformed", "description": "Answers badly."},
    ]});
    let dir = sandbox.plugin("lamp-tools", &[], tools);
    let host = sandbox.host();
    host.link_dev(&dir).unwrap();
    let names: Vec<String> = host.contributed_tools().iter().map(|t| t.mcp_name.clone()).collect();
    assert_eq!(names, ["plugin__lamp_tools__echo", "plugin__lamp_tools__discover", "plugin__lamp_tools__malformed"]);
    let listed = host.contributed_tools()[0].to_mcp();
    assert_eq!(listed["annotations"]["readOnlyHint"], json!(true));
    assert_eq!(listed["inputSchema"]["properties"]["x"]["type"], json!("integer"));

    // A stopped plugin is started for the call.
    host.stop("com.example.lamp-tools").unwrap();
    let answer = host.call_tool("plugin__lamp_tools__echo", json!({"x": 4})).unwrap();
    assert_eq!(answer, json!({"content": [{"type": "text", "text": "{\"x\":4}"}], "isError": false}));
    assert_eq!(host.state("com.example.lamp-tools"), Some(State::Running));

    let err = host.call_tool("plugin__lamp_tools__malformed", json!({})).unwrap_err();
    assert!(err.contains("without a content array"), "{err}");

    host.call_tool("plugin__lamp_tools__discover", json!({})).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while host.contributed_tools().len() != 1 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let after: Vec<String> = host.contributed_tools().iter().map(|t| t.mcp_name.clone()).collect();
    assert_eq!(after, ["plugin__lamp_tools__lamp_on"]);

    // A restart goes back to the manifest's tools; a disabled plugin lists none.
    host.reload("com.example.lamp-tools").unwrap();
    assert_eq!(host.contributed_tools().len(), 3);
    host.set_enabled("com.example.lamp-tools", false).unwrap();
    assert!(host.contributed_tools().is_empty());
    assert!(host.call_tool("plugin__lamp_tools__echo", json!({})).is_err());
}

#[test]
fn the_harness_passes_a_good_plugin_and_runs_frames_only_with_live_output() {
    let sandbox = Sandbox::new("harness");
    let dir = sandbox.plugin("lamp", &[], lamp_controller());
    let report = harness::validate(&dir, &quick());
    assert_eq!(report["passed"], json!(true), "{report:#}");
    for name in ["manifest", "configSchema Lamp", "entry.command", "launch", "initialize", "controller.create Lamp", "controller.configure Lamp", "controller.configure (invalid) Lamp", "controller.setOutputCount Lamp", "controller.destroy Lamp", "protocol", "shutdown", "exit"] {
        assert_eq!(check(&report, name)["status"], json!("pass"), "{name}: {report:#}");
    }
    assert_eq!(check(&report, "controller.start Lamp")["status"], json!("skip"));
    assert!(report.get("frames").is_none());
    assert!(report["logs"].as_array().unwrap().iter().any(|e| e["message"].as_str().unwrap_or("").contains("muted=true")), "{report:#}");

    let live = harness::validate(&dir, &Options { live_output: true, ..quick() });
    assert_eq!(live["passed"], json!(true), "{live:#}");
    for encoding in ["u8", "json", "u16"] {
        assert_eq!(check(&live, &format!("frames {encoding} Lamp"))["status"], json!("pass"), "{live:#}");
    }
    let frames = live["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 3);
    assert!(frames.iter().all(|f| f["sent"].as_u64().unwrap() >= 8), "{frames:?}");
}

#[test]
fn the_harness_reports_broken_plugins() {
    let sandbox = Sandbox::new("broken");
    let garbage = harness::validate(&sandbox.plugin("garbage", &["garbage"], lamp_controller()), &quick());
    assert_eq!(garbage["passed"], json!(false));
    assert!(garbage["checks"].as_array().unwrap().iter().any(|c| c["status"] == "fail" && c["detail"].as_str().unwrap_or("").contains("other than JSON-RPC")), "{garbage:#}");

    let silent = harness::validate(&sandbox.plugin("silent", &["silent"], json!({})), &quick());
    assert_eq!(check(&silent, "initialize")["status"], json!("fail"), "{silent:#}");

    let stubborn = harness::validate(&sandbox.plugin("stubborn", &["--stubborn"], json!({})), &quick());
    assert_eq!(check(&stubborn, "exit")["status"], json!("fail"), "{stubborn:#}");

    let missing = sandbox.plugin("missing", &[], json!({}));
    let mut manifest: Value = serde_json::from_str(&std::fs::read_to_string(missing.join("vixen-plugin.json")).unwrap()).unwrap();
    manifest["entry"]["command"] = json!("no-such-runtime-xyz");
    std::fs::write(missing.join("vixen-plugin.json"), manifest.to_string()).unwrap();
    let report = harness::validate(&missing, &quick());
    assert_eq!(check(&report, "entry.command")["status"], json!("fail"));

    std::fs::write(missing.join("vixen-plugin.json"), "{\"id\": \"Bad Id\"}").unwrap();
    let report = harness::validate(&missing, &quick());
    assert_eq!(check(&report, "manifest")["status"], json!("fail"));
    assert_eq!(report["checks"].as_array().unwrap().len(), 1);
}

#[test]
fn scaffolded_plugins_pass_validation_in_every_language_and_kind() {
    let sandbox = Sandbox::new("scaffold");
    let host = sandbox.host();
    for language in ["python", "typescript"] {
        for kind in ["controller", "tools"] {
            let id = format!("com.example.{language}-{kind}");
            let made = mcp::execute(&host, "plugin_scaffold", &json!({"id": id, "name": "My Lamp (LAN)", "language": language, "kind": kind}), &Context::default()).unwrap();
            let folder = PathBuf::from(made["folder"].as_str().unwrap());
            assert!(folder.starts_with(sandbox.root().join("dev")), "{folder:?}");
            assert_eq!(made["files"].as_array().unwrap().len(), 3);
            let report = harness::validate(&folder, &Options { live_output: true, ..quick() });
            assert_eq!(report["passed"], json!(true), "{language} {kind}: {report:#}");
            assert_eq!(report["warnings"], json!(0), "{language} {kind}: {report:#}");
            if kind == "controller" {
                assert_eq!(check(&report, "controller.configure (invalid) My Lamp (LAN)")["status"], json!("pass"), "{report:#}");
                assert_eq!(check(&report, "frames u8 My Lamp (LAN)")["status"], json!("pass"), "{report:#}");
            }
        }
    }
    // The tools template's tool works through the host once linked.
    let dir = sandbox.root().join("dev/python-tools");
    host.link_dev(&dir).unwrap();
    let answer = host.call_tool("plugin__python_tools__hello", json!({"name": "Braden"})).unwrap();
    assert_eq!(answer["content"][0]["text"], json!("Hello, Braden, from My Lamp (LAN)."));

    let again = mcp::execute(&host, "plugin_scaffold", &json!({"id": "com.example.python-tools", "name": "X", "language": "python", "kind": "tools"}), &Context::default());
    assert!(again.unwrap_err().contains("already exists"));
    for bad in [json!({"folder": "../out"}), json!({"language": "cobol"}), json!({"name": "a\"b"}), json!({"id": "Not An Id"})] {
        let mut args = json!({"id": "com.example.other", "name": "Other", "language": "python", "kind": "tools"});
        for (k, v) in bad.as_object().unwrap() {
            args[k] = v.clone();
        }
        assert!(mcp::execute(&host, "plugin_scaffold", &args, &Context::default()).is_err(), "{args}");
    }
}

#[test]
fn management_needs_confirm_and_the_local_switch_and_validate_stays_in_bounds() {
    let sandbox = Sandbox::new("manage");
    let dir = sandbox.plugin("lamp", &[], lamp_controller());
    let host = sandbox.host();
    let ctx = Context::default();
    let link = json!({"path": dir.to_string_lossy(), "confirm": true});
    let refused = mcp::execute(&host, "plugin_link_dev", &json!({"path": dir.to_string_lossy()}), &ctx).unwrap_err();
    assert!(refused.contains("confirm"), "{refused}");
    let refused = mcp::execute(&host, "plugin_link_dev", &link, &ctx).unwrap_err();
    assert!(refused.contains("Allow agents to manage plugins"), "{refused}");
    assert_eq!(mcp::execute(&host, "plugin_list", &json!({}), &ctx).unwrap()["agentsMayManage"], json!(false));

    studio_plugins::set_agents_may_manage(true);
    let linked = mcp::execute(&host, "plugin_link_dev", &link, &ctx);
    let disabled = mcp::execute(&host, "plugin_disable", &json!({"id": "com.example.lamp", "confirm": true}), &ctx);
    let enabled = mcp::execute(&host, "plugin_enable", &json!({"id": "com.example.lamp", "confirm": true}), &ctx);
    studio_plugins::set_agents_may_manage(false);
    assert_eq!(linked.unwrap()["state"], json!("running"));
    assert_eq!(disabled.unwrap()["state"], json!("disabled"));
    assert_eq!(enabled.unwrap()["state"], json!("running"));

    let info = mcp::execute(&host, "plugin_info", &json!({"id": "com.example.lamp"}), &ctx).unwrap();
    assert_eq!(info["manifest"]["contributes"]["controllers"][0]["typeId"], json!(TYPE_ID));
    let logs = mcp::execute(&host, "plugin_logs", &json!({"id": "com.example.lamp", "limit": 5, "level": "warn"}), &ctx).unwrap();
    assert!(logs["entries"].as_array().unwrap().len() <= 5);
    assert!(mcp::execute(&host, "plugin_logs", &json!({"id": "com.example.lamp", "bogus": 1}), &ctx).unwrap_err().contains("Unknown argument"));

    // By id or by a dev path; never an arbitrary folder.
    assert_eq!(mcp::execute(&host, "plugin_validate", &json!({"id": "com.example.lamp", "burstMs": 100}), &ctx).unwrap()["passed"], json!(true));
    let outside = sandbox.base.join("elsewhere");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::copy(dir.join("vixen-plugin.json"), outside.join("vixen-plugin.json")).unwrap();
    let err = mcp::execute(&host, "plugin_validate", &json!({"path": outside.to_string_lossy()}), &ctx).unwrap_err();
    assert!(err.contains("Plugins/dev") || err.contains("/dev"), "{err}");
    let err = mcp::execute(&host, "plugin_validate", &json!({"id": "com.example.lamp", "liveOutput": true, "confirm": true}), &ctx).unwrap_err();
    assert!(err.contains("Allow agents to control live output"), "{err}");
    let err = mcp::execute(&host, "plugin_validate", &json!({"id": "com.example.lamp", "liveOutput": true, "confirm": true}), &Context { live_output_allowed: true }).unwrap_err();
    assert!(err.contains("muted"), "{err}");
    host.set_muted(false);
    let live = mcp::execute(&host, "plugin_validate", &json!({"id": "com.example.lamp", "liveOutput": true, "confirm": true, "burstMs": 100}), &Context { live_output_allowed: true }).unwrap();
    assert_eq!(live["passed"], json!(true), "{live:#}");
}

fn package(path: &Path, entries: &[(&str, &[u8], u32)]) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    for (name, bytes, mode) in entries {
        let options = zip::write::SimpleFileOptions::default().unix_permissions(*mode);
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn packages_install_into_their_version_folder_and_uninstall_keeps_data() {
    let sandbox = Sandbox::new("install");
    let host = sandbox.host();
    let manifest = |version: &str| json!({"id": "com.example.packaged", "name": "Packaged", "version": version, "apiVersion": "^1.0", "entry": {"command": FIXTURE}}).to_string();
    let v1 = sandbox.base.join("packaged-1.vixenplugin");
    package(&v1, &[("vixen-plugin.json", manifest("1.0.0").as_bytes(), 0o644), ("lib/helper.sh", b"#!/bin/sh\n", 0o755)]);
    let installed = host.install(&v1).unwrap();
    assert_eq!(installed["state"], json!("running"), "{installed:#}");
    assert_eq!(installed["source"], json!("installed"));
    assert_eq!(installed["installed"]["files"], json!(2));
    assert_eq!(installed["installed"]["sha256"].as_str().unwrap().len(), 64);
    let folder = sandbox.root().join("com.example.packaged/1.0.0");
    assert!(folder.join("vixen-plugin.json").is_file());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(folder.join("lib/helper.sh")).unwrap().permissions().mode() & 0o777, 0o700);

    // A new version replaces the old one.
    let v2 = sandbox.base.join("packaged-2.vixenplugin");
    package(&v2, &[("vixen-plugin.json", manifest("1.1.0").as_bytes(), 0o644)]);
    assert_eq!(host.install(&v2).unwrap()["version"], json!("1.1.0"));
    assert!(!folder.exists());
    assert!(sandbox.root().join("com.example.packaged/1.1.0").is_dir());

    // Unsafe packages are refused and leave nothing behind.
    let evil = sandbox.base.join("evil.vixenplugin");
    package(&evil, &[("vixen-plugin.json", manifest("9.0.0").as_bytes(), 0o644), ("../escape.txt", b"x", 0o644)]);
    assert!(host.install(&evil).unwrap_err().contains("unsafe path"));
    let leftovers: Vec<_> = std::fs::read_dir(sandbox.root()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with(".install")).collect();
    assert!(leftovers.is_empty());
    assert!(!sandbox.base.join("escape.txt").exists());

    std::fs::create_dir_all(sandbox.root().join("data/com.example.packaged")).unwrap();
    let removed = host.uninstall("com.example.packaged").unwrap();
    assert_eq!(removed["removed"], json!(true));
    assert!(!sandbox.root().join("com.example.packaged").exists());
    assert!(sandbox.root().join("data/com.example.packaged").exists());
    assert!(host.state("com.example.packaged").is_none());
}

#[test]
fn resources_serve_the_api_schemas_examples_and_manifests() {
    let sandbox = Sandbox::new("resources");
    let dir = sandbox.plugin("lamp", &[], lamp_controller());
    let host = sandbox.host();
    host.link_dev(&dir).unwrap();
    let uris: Vec<String> = mcp::resources(&host).iter().map(|r| r["uri"].as_str().unwrap().to_string()).collect();
    for uri in ["plugins://api", "plugins://schema/manifest", "plugins://schema/protocol", "plugins://examples/python", "plugins://examples/typescript", "plugins://com.example.lamp/manifest"] {
        assert!(uris.iter().any(|u| u == uri), "{uri} in {uris:?}");
        let contents = mcp::read_resource(&host, uri).unwrap().unwrap();
        assert!(!contents[0]["text"].as_str().unwrap().is_empty(), "{uri}");
    }
    let schema: Value = serde_json::from_str(mcp::read_resource(&host, "plugins://schema/protocol").unwrap().unwrap()[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(schema["x-hostToPlugin"]["tool.call"]["available"], json!("P3"));
    assert!(mcp::read_resource(&host, "plugins://examples/cobol").unwrap().is_err());
    assert!(mcp::read_resource(&host, "plugins://com.example.nothing/manifest").unwrap().is_err());
    assert!(mcp::read_resource(&host, "studio://contract").is_none());
    assert!(mcp::prompt(Some("a UDP lamp")).contains("plugin_scaffold"));
}
