//! Plugin controllers against a real child process (the fixture plugin) and, at the end, a real Studio profile: types,
//! defaults, the instance lifecycle, frames, configuration, plugin restarts, status/stats/messages, a missing plugin,
//! lossless module data, StudioPlugins.json, live-output gating and a commissioning test on a plugin controller.
//!
//! The provider is process-wide (`studio_external::register`), so these tests take turns.
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use studio_plugins::logs::Level;
use studio_plugins::{HostOptions, PluginHost};
use uuid::Uuid;
use vixen_output::modules::studio_external::{self, ExternalConfig};
use vixen_output::{create_module, ControllerConfig, ModuleStatus, OutputCommand, OutputModule};
use vixen_xml::XElement;

const FIXTURE: &str = env!("CARGO_BIN_EXE_studio-plugin-fixture");
const TYPE_ID: &str = "6f1c2a8e-0000-4000-8000-00000000c0de";

static TURN: Mutex<()> = Mutex::new(());

fn turn() -> MutexGuard<'static, ()> {
    TURN.lock().unwrap_or_else(|p| p.into_inner())
}

fn options() -> HostOptions {
    HostOptions {
        request_timeout: Duration::from_secs(2),
        init_timeout: Duration::from_secs(2),
        shutdown_grace: Duration::from_millis(400),
        backoff: [Duration::from_millis(50), Duration::from_millis(100), Duration::from_millis(150)],
        restart_window: Duration::from_secs(30),
        tick: Duration::from_millis(20),
        dev_poll: Duration::from_secs(3600),
    }
}

struct Sandbox {
    base: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Sandbox {
        let base = std::env::temp_dir().join(format!("studio-plugins-ctl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Sandbox { base: base.canonicalize().unwrap() }
    }

    fn host(&self) -> PluginHost {
        PluginHost::new(&self.base.join("Plugins"), options()).unwrap()
    }

    /// A development plugin offering one controller type ("Lamp") and running the fixture with `args`.
    fn plugin(&self, folder: &str, version: &str, args: &[&str], encodings: &[&str]) -> PathBuf {
        let dir = self.base.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = json!({
            "id": "com.example.lamp",
            "name": "Lamp Output",
            "version": version,
            "apiVersion": "^1.0",
            "entry": {"command": FIXTURE, "args": args},
            "contributes": {"controllers": [{
                "typeId": TYPE_ID,
                "name": "Lamp (LAN)",
                "configSchema": {"type": "object", "properties": {
                    "host": {"type": "string", "format": "ipv4", "default": "10.0.0.2"},
                    "port": {"type": "integer", "minimum": 1, "maximum": 65535, "default": 4048},
                    "bad": {"type": "boolean"}
                }, "additionalProperties": false},
                "defaultOutputs": 3,
                "maxOutputs": 300,
                "dataPolicy": "highestWins8Bit",
                "frameEncodings": encodings
            }]}
        });
        std::fs::write(dir.join("vixen-plugin.json"), manifest.to_string()).unwrap();
        dir
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        studio_external::register(None);
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn type_id() -> Uuid {
    Uuid::parse_str(TYPE_ID).unwrap()
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn log_text(host: &PluginHost) -> String {
    let logs = host.logs("com.example.lamp", 2000, Level::Debug).unwrap();
    logs["entries"].as_array().unwrap().iter().map(|e| format!("{}\n", e["message"].as_str().unwrap())).collect()
}

fn count(host: &PluginHost, needle: &str) -> usize {
    log_text(host).matches(needle).count()
}

fn register(host: &PluginHost) {
    studio_external::register(Some(host.controllers()));
}

fn new_module(instance: Uuid) -> (ControllerConfig, Box<dyn OutputModule>) {
    let config = ControllerConfig::new_default(&type_id(), instance).expect("an installed plugin type has defaults");
    (config.clone(), create_module(config))
}

fn settings(config: &ControllerConfig) -> Value {
    let ControllerConfig::External(c) = config else { panic!("not a plugin controller: {config:?}") };
    serde_json::from_str(&c.data.child_text("Config").unwrap()).unwrap()
}

fn with_settings(config: &ControllerConfig, value: Value) -> ControllerConfig {
    let ControllerConfig::External(c) = config else { panic!("not a plugin controller") };
    let mut data = c.data.clone();
    studio_plugins::controllers::set_config_text(&mut data, &value);
    ControllerConfig::External(ExternalConfig { ids: c.ids, data })
}

#[test]
fn plugin_types_list_and_get_schema_defaults() {
    let _turn = turn();
    let sandbox = Sandbox::new("types");
    let host = sandbox.host();
    host.link_dev(&sandbox.plugin("lamp", "1.2.3", &[], &["u8"])).unwrap();
    assert!(ControllerConfig::new_default(&type_id(), Uuid::new_v4()).is_none(), "no provider, no plugin types");
    register(&host);
    assert_eq!(studio_external::types(), vec![(type_id(), "Lamp (LAN)")]);
    let (config, module) = new_module(Uuid::new_v4());
    assert_eq!(settings(&config), json!({"host": "10.0.0.2", "port": 4048}));
    let ControllerConfig::External(c) = &config else { unreachable!() };
    assert_eq!(c.data.child_text("PluginId").as_deref(), Some("com.example.lamp"));
    assert_eq!(c.data.child_text("PluginVersion").as_deref(), Some("1.2.3"));
    assert_eq!(module.output_limit(), 300);
    assert_eq!(module.status(), ModuleStatus::Stopped);
    assert!(studio_external::problem(&config).is_none());
    // Data written for the type reads back as the same plugin controller.
    let back = ControllerConfig::from_xml(&type_id(), &config.to_xml().unwrap()).unwrap();
    assert_eq!(back.to_xml(), config.to_xml());
}

#[test]
fn an_instance_runs_through_its_lifecycle() {
    let _turn = turn();
    let sandbox = Sandbox::new("lifecycle");
    let host = sandbox.host();
    host.link_dev(&sandbox.plugin("lamp", "1.0.0", &[], &["json", "u8"])).unwrap();
    register(&host);
    let instance = Uuid::new_v4();
    let (config, mut module) = new_module(instance);
    module.set_output_count(4);
    module.start();
    wait_until("the instance to run", || module.status() == ModuleStatus::Running);
    let text = log_text(&host);
    assert!(text.contains(&format!("controller.create {instance} outputs=4 config={{\"host\":\"10.0.0.2\",\"port\":4048}}")), "{text}");
    assert!(text.contains(&format!("controller.start {instance}")), "{text}");
    // The plugin's normalised configuration is what the module now holds.
    assert_eq!(settings(&module.config()), json!({"host": "10.0.0.2", "port": 4048, "seen": true}));

    module.update(&[OutputCommand::U8(255), OutputCommand::None, OutputCommand::U8(7), OutputCommand::U16(9)]);
    wait_until("the frame", || count(&host, "frame ") == 1);
    assert!(log_text(&host).contains(&format!("frame {instance} seq=1 json [255,null,7,9]")), "{}", log_text(&host));

    module.set_config(with_settings(&config, json!({"host": "10.0.0.9", "port": 80}))).unwrap();
    wait_until("configure", || count(&host, "controller.configure") == 1);
    assert!(log_text(&host).contains("config={\"host\":\"10.0.0.9\",\"port\":80}"));
    module.set_output_count(6);
    wait_until("setOutputCount", || log_text(&host).contains(&format!("controller.setOutputCount {instance} outputs=6")));

    // A refused configuration: the status says why and the plugin's field errors are kept for the inspector.
    module.set_config(with_settings(&config, json!({"bad": true}))).unwrap();
    wait_until("the refusal", || matches!(module.status(), ModuleStatus::Error(m) if m.contains("bad is not allowed")));
    let info = host.info("com.example.lamp").unwrap();
    assert_eq!(info["instances"][0]["configErrors"], json!([{"field": "/bad", "message": "is not allowed"}]));
    module.set_config(with_settings(&config, json!({"host": "10.0.0.3"}))).unwrap();
    wait_until("a good configuration again", || module.status() == ModuleStatus::Running);

    module.pause();
    module.resume();
    wait_until("pause and resume", || count(&host, "controller.resume") == 1 && count(&host, "controller.pause") == 1);
    module.stop();
    assert_eq!(module.status(), ModuleStatus::Stopped);
    wait_until("stop and destroy", || count(&host, &format!("controller.destroy {instance}")) == 1);
    assert!(log_text(&host).contains(&format!("controller.stop {instance}")));
    // Stopped: frames go nowhere.
    module.update(&[OutputCommand::U8(1)]);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(count(&host, "frame "), 1);
    wait_until("the instance to leave plugin_info", || host.info("com.example.lamp").unwrap()["instances"] == json!([]));
}

#[test]
fn a_restarted_plugin_gets_its_running_instances_back() {
    let _turn = turn();
    let sandbox = Sandbox::new("restart");
    let host = sandbox.host();
    let pid = host.link_dev(&sandbox.plugin("lamp", "1.0.0", &[], &["u8"])).unwrap()["pid"].as_u64().unwrap();
    register(&host);
    let instance = Uuid::new_v4();
    let (_, mut module) = new_module(instance);
    module.set_output_count(3);
    module.start();
    wait_until("the instance to run", || module.status() == ModuleStatus::Running);
    // SAFETY: kills the fixture child this test started.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
    wait_until("a second create", || count(&host, &format!("controller.create {instance}")) == 2);
    wait_until("running again", || module.status() == ModuleStatus::Running);
    assert_ne!(host.info("com.example.lamp").unwrap()["pid"].as_u64(), Some(pid));
    module.update(&[OutputCommand::U8(200), OutputCommand::U8(0), OutputCommand::U8(1)]);
    wait_until("a frame to the new process", || log_text(&host).contains(&format!("frame {instance} seq=1 u8 \"yAAB\"")));
    module.stop();
}

#[test]
fn a_replacing_module_waits_for_the_old_instance_to_be_destroyed() {
    let _turn = turn();
    let sandbox = Sandbox::new("replace");
    let host = sandbox.host();
    host.link_dev(&sandbox.plugin("lamp", "1.0.0", &[], &["u8"])).unwrap();
    register(&host);
    let instance = Uuid::new_v4();
    let (config, mut old) = new_module(instance);
    old.start();
    wait_until("the first module to run", || old.status() == ModuleStatus::Running);
    // The runner rebuilt the controller: a second module for the same instance starts before the first has stopped.
    let mut new = create_module(config);
    new.start();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(count(&host, "controller.create"), 1, "the new module waits");
    old.stop();
    wait_until("the second create", || count(&host, "controller.create") == 2);
    let text = log_text(&host);
    assert!(text.find("controller.destroy").unwrap() < text.rfind("controller.create").unwrap(), "{text}");
    wait_until("the new module to run", || new.status() == ModuleStatus::Running);
    new.stop();
}

#[test]
fn status_stats_and_messages_reach_their_instance_only() {
    let _turn = turn();
    let sandbox = Sandbox::new("report");
    let host = sandbox.host();
    host.link_dev(&sandbox.plugin("lamp", "1.0.0", &["--report"], &["u8"])).unwrap();
    register(&host);
    let (_, mut module) = new_module(Uuid::new_v4());
    module.start();
    wait_until("the plugin's status", || module.status() == ModuleStatus::Unavailable("lamp 2 is offline".into()));
    let shared = module.shared().clone();
    wait_until("stats", || shared.packets_sent() == 10);
    assert_eq!((shared.bytes_sent(), shared.error_count() >= 1), (300, true));
    wait_until("the message", || !shared.take_messages().is_empty());
    // A status for an instance that isn't this plugin's is refused (and logged by the fixture).
    wait_until("the refusal", || log_text(&host).contains("host error \"no controller instance not-mine belongs to this plugin\""));
    module.stop();
}

#[test]
fn a_slow_plugin_loses_frames_instead_of_holding_up_the_engine() {
    let _turn = turn();
    let sandbox = Sandbox::new("slow");
    let host = sandbox.host();
    host.link_dev(&sandbox.plugin("lamp", "1.0.0", &["--slow-frames"], &["u8"])).unwrap();
    register(&host);
    let (_, mut module) = new_module(Uuid::new_v4());
    module.set_output_count(512);
    module.start();
    wait_until("the instance to run", || module.status() == ModuleStatus::Running);
    let frame = vec![OutputCommand::U8(9); 512];
    let started = Instant::now();
    for _ in 0..400 {
        module.update(&frame);
    }
    // 400 frames at 50 ms each would take 20 s if the module waited on the plugin.
    assert!(started.elapsed() < Duration::from_millis(1500), "updates took {:?}", started.elapsed());
    let info = host.info("com.example.lamp").unwrap();
    let dropped = info["instances"][0]["framesDropped"].as_u64().unwrap();
    assert!(dropped > 300, "dropped {dropped}");
    assert_eq!(info["instances"][0]["framesSent"], 400);
    // The plugin is still answering promptly: requests aren't stuck behind stale frames.
    let answered = Instant::now();
    module.stop();
    wait_until("destroy", || count(&host, "controller.destroy") == 1);
    assert!(answered.elapsed() < Duration::from_secs(2), "{:?}", answered.elapsed());
}

/// Module data for a plugin that isn't installed here, with a member a newer Studio might add.
fn foreign_data(instance: Uuid) -> XElement {
    XElement::parse(&format!(
        "<PluginControllerData><ModuleInstanceId>{instance}</ModuleInstanceId><ModuleTypeId>{TYPE_ID}</ModuleTypeId><PluginId>com.example.lamp</PluginId><PluginName>Lamp Output</PluginName><PluginVersion>1.2.0</PluginVersion><Config>{{\"host\":\"10.1.1.1\",\"port\":9}}</Config><Later a=\"1\">x</Later></PluginControllerData>"
    ))
    .unwrap()
}

#[test]
fn a_missing_plugin_keeps_the_controller_and_says_what_it_needs() {
    let _turn = turn();
    let sandbox = Sandbox::new("missing");
    let host = sandbox.host();
    register(&host);
    let instance = Uuid::new_v4();
    let data = foreign_data(instance);
    let config = ControllerConfig::from_xml(&type_id(), &data).expect("plugin data is claimed without its plugin");
    assert_eq!(config.to_xml().unwrap(), data, "saved back unchanged");
    let needs = "Needs plugin Lamp Output (com.example.lamp) ^1.2 — not installed";
    assert_eq!(studio_external::problem(&config).as_deref(), Some(needs));
    assert_eq!(studio_external::type_name(&type_id(), Some(&config)), Some("Lamp Output (plugin missing)"));
    let mut module = create_module(config.clone());
    module.set_output_count(3);
    module.start();
    assert_eq!(module.status(), ModuleStatus::Error(needs.into()));
    module.update(&vec![OutputCommand::U8(255); 3]);
    assert_eq!(module.config().to_xml(), config.to_xml());

    // Too old a version is named as such.
    let old = sandbox.plugin("old", "1.1.0", &[], &["u8"]);
    host.link_dev(&old).unwrap();
    wait_until("the version problem", || matches!(module.status(), ModuleStatus::Error(m) if m.ends_with("— version 1.1.0 is installed")));
    host.unlink_dev("com.example.lamp").unwrap();

    // Installing it brings the controller back without any edit, keeping the saved configuration.
    host.link_dev(&sandbox.plugin("lamp", "1.4.0", &[], &["u8"])).unwrap();
    wait_until("the controller to come back", || module.status() == ModuleStatus::Running);
    assert!(log_text(&host).contains(&format!("controller.create {instance} outputs=3 config={{\"host\":\"10.1.1.1\",\"port\":9}}")));
    // Disabling the plugin silences it again, with the reason.
    host.set_enabled("com.example.lamp", false).unwrap();
    wait_until("the disabled problem", || matches!(module.status(), ModuleStatus::Error(m) if m.ends_with("— it is disabled")));
    module.stop();
    // Without any provider, the same data is an unknown module type, left alone.
    studio_external::register(None);
    assert!(ControllerConfig::from_xml(&type_id(), &data).is_none());
}

#[test]
fn a_profile_creates_configures_saves_and_commissions_a_plugin_controller() {
    let _turn = turn();
    let sandbox = Sandbox::new("profile");
    let host = sandbox.host();
    host.link_dev(&sandbox.plugin("lamp", "2.3.0", &[], &["json"])).unwrap();
    register(&host);
    let root = sandbox.base.join("Profile");
    let core = vixen_session::app::AppCore::open(root.clone(), false).unwrap();
    let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, false).map(|r| r["result"].clone());
    let types = vixen_session::setup::controllers::controller_types();
    assert!(types.contains(&(type_id(), "Lamp (LAN)")), "{types:?}");

    let tree = run("element_template", json!({"kind": "megatree", "name": "Tree", "count": 1, "pixels": 2, "color": "rgb"})).unwrap();
    let controller = run("controller_create", json!({"name": "Lamps", "typeId": TYPE_ID, "outputs": 6})).unwrap()["id"].as_str().unwrap().to_string();
    run("patch_elements", json!({"elementId": tree["id"], "controllerId": controller})).unwrap();
    let refused = run("controller_configure", json!({"id": controller, "config": {"host": "lamp.local"}})).unwrap_err();
    assert!(refused.contains("Invalid configuration: /host: must be an IPv4 address"), "{refused}");
    assert!(run("controller_configure", json!({"id": controller, "address": "10.0.0.1"})).unwrap_err().contains("doesn't apply to a plugin controller"));
    let configured = run("controller_configure", json!({"id": controller, "config": {"host": "10.0.0.7", "port": 5000}})).unwrap();
    assert_eq!(configured["controller"]["plugin"]["config"], json!({"host": "10.0.0.7", "port": 5000}));
    assert_eq!(configured["controller"]["plugin"]["pluginId"], "com.example.lamp");
    assert_eq!(configured["controller"]["typeName"], "Lamp (LAN)");
    assert_eq!(overview(&core)["controllers"][0]["typeName"], "Lamp (LAN)");
    run("profile_save", json!({"confirm": true})).unwrap();

    // ModuleStore.xml has the plugin's data under its type id; StudioPlugins.json names the plugin.
    let store = std::fs::read_to_string(root.join("SystemData/ModuleStore.xml")).unwrap();
    assert!(store.contains("<PluginId>com.example.lamp</PluginId>") && store.contains(&format!("moduleType=\"{TYPE_ID}\"")), "{store}");
    assert!(store.contains("10.0.0.7"));
    let needs = host.requirements(&root).unwrap();
    assert_eq!(needs["plugins"], json!([{"id": "com.example.lamp", "versionRange": "^2.3", "installedVersion": "2.3.0", "state": "running", "satisfied": true, "message": null}]));

    // Live output: an agent may start a plugin controller like E1.31/DDP once allowed; then a commissioning test
    // reaches the plugin as frames.
    let id = Uuid::parse_str(&controller).unwrap();
    core.playback.open_execution();
    vixen_session::studio_live::set_allowed_locally(&core, true);
    let started = vixen_session::studio_live::execute(&core, "output_start", &json!({"controllerId": controller, "confirm": true})).unwrap();
    assert_eq!(started["running"], true);
    wait_until("the plugin instance", || log_text(&host).contains("controller.start"));
    let test = vixen_session::studio_commission::start(&core, vixen_session::studio_commission::Origin::Local, &json!({"controllerId": controller, "pattern": "allOn", "brightnessPct": 40}));
    test.unwrap();
    wait_until("lit frames", || log_text(&host).contains("json [102,102,102,102,102,102]"));
    vixen_session::studio_commission::stop(&core);
    // Revoking the permission stops what the agent started.
    vixen_session::studio_live::set_allowed_locally(&core, false);
    wait_until("the instance to stop", || log_text(&host).contains("controller.destroy"));
    assert!(!core.playback.running_controllers().unwrap_or_default().contains(&id));
    core.playback.close_execution();
    drop(core);

    // Reopened without the plugin installed: the controller and its patches stay, health says what's needed, and a
    // save keeps the plugin's data byte for byte.
    host.unlink_dev("com.example.lamp").unwrap();
    let core = vixen_session::app::AppCore::open(root.clone(), true).unwrap();
    let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, false).map(|r| r["result"].clone());
    let overview = overview(&core);
    let row = &overview["controllers"][0];
    assert_eq!(row["typeName"], "Lamp Output (plugin missing)");
    assert_eq!(row["patchedOutputs"], 6);
    assert_eq!(row["health"], json!({"level": "error", "text": "Needs plugin Lamp Output (com.example.lamp) ^2.3 — not installed"}));
    run("controller_rename", json!({"id": controller, "name": "Lamps Renamed"})).unwrap();
    run("profile_save", json!({"confirm": true})).unwrap();
    let after = std::fs::read_to_string(root.join("SystemData/ModuleStore.xml")).unwrap();
    assert_eq!(module_entry(&after), module_entry(&store));
    drop(core);
}

#[test]
fn a_plugin_that_sets_its_own_output_count_resizes_names_and_guards_patched_outputs() {
    let _turn = turn();
    let sandbox = Sandbox::new("owned-outputs");
    let host = sandbox.host();
    let dir = sandbox.plugin("lamp", "2.4.0", &[], &["u8"]);
    // The fields go at the configSchema root (x-outputCount, x-actions), where a Studio without them ignores them.
    let mut manifest: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("vixen-plugin.json")).unwrap()).unwrap();
    let schema = &mut manifest["contributes"]["controllers"][0]["configSchema"];
    schema["properties"]["lamps"] = json!({"type": "array", "default": [], "items": {"type": "object", "properties": {"name": {"type": "string"}}, "additionalProperties": false}});
    schema["x-outputCount"] = json!("fromConfig");
    schema["x-actions"] = json!([{"id": "detect", "title": "Detect Lamps", "tool": "detect"}]);
    manifest["contributes"]["tools"] = json!([{"name": "detect", "description": "Find lamps.", "readOnly": true}]);
    std::fs::write(dir.join("vixen-plugin.json"), manifest.to_string()).unwrap();
    host.link_dev(&dir).unwrap();
    register(&host);
    assert!(studio_external::outputs_from_config(&type_id()));
    let root = sandbox.base.join("Profile");
    let core = vixen_session::app::AppCore::open(root.clone(), false).unwrap();
    let run = |name: &str, args: Value| core.studio_display.execute(&core, name, &args, false).map(|r| r["result"].clone());
    let outputs = |id: &str| {
        let setup = vixen_session::setup::SetupSession::open(&core);
        let c = setup.controllers.iter().find(|c| c.alive && c.id.to_string() == id).unwrap();
        c.outputs.iter().map(|o| o.name.clone()).collect::<Vec<_>>()
    };

    let controller = run("controller_create", json!({"name": "Lamps", "typeId": TYPE_ID, "outputs": 3})).unwrap()["id"].as_str().unwrap().to_string();
    let lamps = |names: &[&str]| json!({"host": "10.0.0.7", "port": 4048, "lamps": names.iter().map(|n| json!({"name": n})).collect::<Vec<_>>()});
    let five = run("controller_configure", json!({"id": controller, "config": lamps(&["A", "B", "C", "D", "E"])})).unwrap();
    assert_eq!(five["controller"]["outputs"], 15);
    assert_eq!(five["controller"]["plugin"]["outputsFromConfig"], true);
    assert_eq!(five["controller"]["plugin"]["actions"], json!([{"id": "detect", "title": "Detect Lamps", "description": null}]));
    assert_eq!(outputs(&controller)[..4], ["A R", "A G", "A B", "B R"]);
    assert!(run("controller_configure", json!({"id": controller, "outputs": 20})).unwrap_err().contains("sets its output count from its settings"));

    // Patch five RGB elements to the 15 outputs; removing a lamp would drop patched outputs, so it's refused until allowed.
    let tree = run("element_template", json!({"kind": "megatree", "name": "Bulbs", "count": 5, "pixels": 1, "color": "rgb"})).unwrap();
    run("patch_elements", json!({"elementId": tree["id"], "controllerId": controller})).unwrap();
    let refused = run("controller_configure", json!({"id": controller, "config": lamps(&["A", "B", "C", "D"])})).unwrap_err();
    assert!(refused.starts_with(vixen_session::studio_controller_network::REMOVES_PATCHED) && refused.contains("outputs 13–15 of 15"), "{refused}");
    assert_eq!(outputs(&controller).len(), 15);
    let four = run("controller_configure", json!({"id": controller, "config": lamps(&["A", "B", "C", "D"]), "removePatchedOutputs": true})).unwrap();
    assert_eq!(four["controller"]["outputs"], 12);
    // One undo step brings back the settings, the outputs and their patches.
    run("display_undo", json!({})).unwrap();
    assert_eq!(outputs(&controller).len(), 15);
    assert_eq!(outputs(&controller)[14], "E B");
    let overview = overview(&core);
    assert_eq!(overview["controllers"][0]["patchedOutputs"], 15);

    // A settings action: the plugin's tool answers a merge patch, applied through the same check and resize.
    let ControllerConfig::External(config) = vixen_session::setup::SetupSession::open(&core).controllers.iter().find(|c| c.alive).unwrap().config.clone().unwrap() else { panic!() };
    let answer: Value = serde_json::from_str(&studio_external::run_action(&config, &lamps(&["A"]).to_string(), "detect").unwrap()).unwrap();
    assert_eq!(answer, json!({"config": lamps(&["Ceiling", "Porch"]), "message": "Found 2 lamps"}));
    assert!(studio_external::run_action(&config, "{}", "nope").unwrap_err().contains("no action nope"));
    drop(core);
}

fn overview(core: &vixen_session::app::AppCore) -> Value {
    let setup = vixen_session::setup::SetupSession::from_system(core.system.read().clone(), &Default::default(), core.playback.hardware_allowed());
    vixen_session::studio_controller_network::overview(&setup, Some(core))
}

/// The `<Module>` element holding the plugin's data.
fn module_entry(store: &str) -> String {
    let start = store.find(&format!("moduleType=\"{TYPE_ID}\"")).unwrap();
    let open = store[..start].rfind("<Module ").unwrap();
    let close = store[open..].find("</Module>").unwrap();
    store[open..open + close].to_string()
}
