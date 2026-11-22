//! The host against a real child process: the fixture plugin in src/bin/studio-plugin-fixture.rs.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use studio_plugins::logs::Level;
use studio_plugins::{HostOptions, PluginHost, State};

const FIXTURE: &str = env!("CARGO_BIN_EXE_studio-plugin-fixture");

fn options() -> HostOptions {
    HostOptions {
        request_timeout: Duration::from_secs(2),
        init_timeout: Duration::from_millis(800),
        shutdown_grace: Duration::from_millis(400),
        backoff: [Duration::from_millis(50), Duration::from_millis(100), Duration::from_millis(150)],
        restart_window: Duration::from_secs(30),
        tick: Duration::from_millis(20),
        dev_poll: Duration::from_millis(60),
    }
}

struct Sandbox {
    base: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Sandbox {
        let base = std::env::temp_dir().join(format!("studio-plugins-it-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Sandbox { base: base.canonicalize().unwrap() }
    }

    fn root(&self) -> PathBuf {
        self.base.join("Plugins")
    }

    /// A development plugin folder running the fixture with `args`.
    fn plugin(&self, folder: &str, args: &[&str], extra: Value) -> PathBuf {
        let dir = self.base.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        let mut manifest = json!({
            "id": format!("com.example.{folder}"),
            "name": format!("Fixture {folder}"),
            "version": "0.1.0",
            "apiVersion": "^1.0",
            "entry": {"command": FIXTURE, "args": args},
        });
        for (key, value) in extra.as_object().unwrap() {
            manifest[key] = value.clone();
        }
        std::fs::write(dir.join("vixen-plugin.json"), manifest.to_string()).unwrap();
        dir
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn alive(pid: u64) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

fn log_text(host: &PluginHost, id: &str) -> String {
    let logs = host.logs(id, 2000, Level::Debug).unwrap();
    logs["entries"].as_array().unwrap().iter().map(|e| format!("{} {} {}\n", e["level"], e["source"], e["message"].as_str().unwrap())).collect()
}

fn wait_for_log(host: &PluginHost, id: &str, needle: &str) {
    wait_until(needle, || log_text(host, id).contains(needle));
}

#[test]
fn a_plugin_starts_initializes_logs_stores_and_stops_cleanly() {
    let sandbox = Sandbox::new("lifecycle");
    // Studio's own environment must not reach the plugin.
    std::env::set_var("STUDIO_PLUGIN_TEST_SECRET", "leaked");
    let dir = sandbox.plugin("lifecycle", &["normal", "--storage"], json!({"permissions": {"files": ["plugin-data"]}}));
    let host = PluginHost::new(&sandbox.root(), options()).unwrap();
    let info = host.link_dev(&dir).unwrap();
    let id = "com.example.lifecycle";
    assert_eq!(info["state"], json!("running"), "{info}");
    assert_eq!(info["negotiatedApiVersion"], json!("1.0.0"));
    assert_eq!(info["capabilities"], json!({"fixture": true}));
    assert_eq!(info["source"], json!("dev"));
    let pid = info["pid"].as_u64().unwrap();
    assert!(alive(pid));

    wait_for_log(&host, id, "\"warn\" \"plugin\" hello from fixture");
    wait_for_log(&host, id, "fixture ready muted=true");
    wait_for_log(&host, id, "storage got 41");
    assert!(log_text(&host, id).contains("env SECRET=unset"), "{}", log_text(&host, id));
    let state = std::fs::read_to_string(sandbox.root().join("data").join(id).join("state.json")).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&state).unwrap(), json!({"count": 41}));
    assert!(std::fs::read_to_string(sandbox.root().join("logs").join(format!("{id}.log"))).unwrap().contains("hello from fixture"));

    let list = host.list();
    assert_eq!(list["apiVersion"], json!("1.0.0"));
    assert_eq!(list["plugins"][0]["id"], json!(id));

    let stopped = host.stop(id).unwrap();
    assert_eq!(stopped["state"], json!("stopped"));
    assert!(stopped.get("pid").is_none());
    assert!(!alive(pid));
}

#[test]
fn storage_needs_the_plugin_data_permission() {
    let sandbox = Sandbox::new("nostorage");
    let dir = sandbox.plugin("nostorage", &["normal", "--storage"], json!({}));
    let host = PluginHost::new(&sandbox.root(), options()).unwrap();
    host.link_dev(&dir).unwrap();
    wait_for_log(&host, "com.example.nostorage", "host error \"storage needs");
    assert!(!sandbox.root().join("data/com.example.nostorage/state.json").exists());
}

#[test]
fn display_snapshot_needs_the_display_permission_and_reads_the_open_show() {
    let sandbox = Sandbox::new("display");
    let host = PluginHost::new(&sandbox.root(), options()).unwrap();

    // Without "display": "read", refused even with a show open.
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let log = seen.clone();
    host.set_display(Some(std::sync::Arc::new(move |params: &Value| {
        log.lock().unwrap().push(params.clone());
        Ok(json!({"revision": 3, "elements": [{"id": "e1", "name": "Porch", "children": []}], "more": false}))
    })));
    let denied = sandbox.plugin("nodisplay", &["normal", "--display={}"], json!({}));
    host.link_dev(&denied).unwrap();
    wait_for_log(&host, "com.example.nodisplay", "host error \"display.snapshot needs \\\"display\\\": \\\"read\\\"");
    assert!(log_text(&host, "com.example.nodisplay").contains("code=-32003"), "{}", log_text(&host, "com.example.nodisplay"));
    assert!(seen.lock().unwrap().is_empty(), "the show was read for a plugin without the permission");

    // With it, the app's view answers, given the plugin's paging params.
    let allowed = sandbox.plugin("display", &["normal", "--display={\"elementOffset\":2}"], json!({"permissions": {"display": "read"}}));
    host.link_dev(&allowed).unwrap();
    wait_for_log(&host, "com.example.display", "snapshot {");
    let text = log_text(&host, "com.example.display");
    assert!(text.contains("\"name\":\"Porch\"") && text.contains("\"revision\":3"), "{text}");
    assert_eq!(*seen.lock().unwrap(), vec![json!({"elementOffset": 2})]);

    // Bad params are refused before the show is read.
    let bad = sandbox.plugin("badparams", &["normal", "--display={\"elementOffset\":-1}"], json!({"permissions": {"display": "read"}}));
    host.link_dev(&bad).unwrap();
    wait_for_log(&host, "com.example.badparams", "code=-32602");
    assert_eq!(seen.lock().unwrap().len(), 1);

    // No show open: a clear error, and the running plugin sees it on its next call.
    host.set_display(None);
    host.reload("com.example.display").unwrap();
    wait_for_log(&host, "com.example.display", "host error \"no show is open\" code=-32603");
}

#[test]
fn a_crashing_plugin_restarts_with_backoff_then_gives_up() {
    let sandbox = Sandbox::new("crashy");
    let dir = sandbox.plugin("crashy", &["exit-after-init"], json!({}));
    let host = PluginHost::new(&sandbox.root(), options()).unwrap();
    host.link_dev(&dir).unwrap();
    let id = "com.example.crashy";
    wait_until("crashed", || host.state(id) == Some(State::Crashed));
    let info = host.info(id).unwrap();
    assert_eq!(info["restartCount"], json!(3));
    assert!(info["lastError"].as_str().unwrap().contains("exited with status 3"), "{info}");
    assert!(info.get("pid").is_none());
    let text = log_text(&host, id);
    assert!(text.contains("restarting in 50 ms") && text.contains("restarting in 100 ms") && text.contains("restarting in 150 ms"), "{text}");
    assert!(text.contains("gave up after 3 restarts"));
    // It stays down until reloaded.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(host.state(id), Some(State::Crashed));
    host.reload(id).unwrap();
    wait_until("restarted after reload", || log_text(&host, id).matches("started Fixture crashy").count() == 5);
}

#[test]
fn a_plugin_that_fails_at_launch_is_retried_like_a_crash() {
    let sandbox = Sandbox::new("launchfail");
    let dir = sandbox.plugin("launchfail", &["crash"], json!({}));
    let host = PluginHost::new(&sandbox.root(), options()).unwrap();
    assert!(host.link_dev(&dir).unwrap_err().contains("initialize failed"));
    wait_until("crashed", || host.state("com.example.launchfail") == Some(State::Crashed));
    assert!(log_text(&host, "com.example.launchfail").contains("fixture crashing at start"));
}

#[test]
fn printing_to_stdout_breaks_the_protocol_and_stops_the_plugin() {
    let sandbox = Sandbox::new("garbage");
    let dir = sandbox.plugin("garbage", &["garbage"], json!({}));
    let host = PluginHost::new(&sandbox.root(), options()).unwrap();
    let pid = host.link_dev(&dir).unwrap()["pid"].as_u64().unwrap();
    let id = "com.example.garbage";
    wait_until("restart scheduled", || host.info(id).unwrap()["restartCount"].as_u64().unwrap() >= 1);
    let info = host.info(id).unwrap();
    assert!(info["lastError"].as_str().unwrap().contains("other than JSON-RPC"), "{info}");
    assert!(info["lastError"].as_str().unwrap().contains("this is a print statement"));
    wait_until("first process gone", || !alive(pid));
}

#[test]
fn a_plugin_that_never_answers_initialize_is_killed() {
    let sandbox = Sandbox::new("silent");
    let dir = sandbox.plugin("silent", &["silent"], json!({}));
    let host = PluginHost::new(&sandbox.root(), options()).unwrap();
    let started = Instant::now();
    let error = host.link_dev(&dir).unwrap_err();
    assert!(error.contains("initialize failed") && error.contains("no answer within 800 ms"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(host.state("com.example.silent"), Some(State::Restarting));
    host.stop("com.example.silent").unwrap();
}

#[test]
fn api_versions_are_negotiated() {
    let sandbox = Sandbox::new("api");
    let host = PluginHost::new(&sandbox.root(), options()).unwrap();
    // The manifest asks for an API this host doesn't have: never launched.
    let future = sandbox.plugin("future", &["normal"], json!({"apiVersion": "^2.0"}));
    let info = host.link_dev(&future).unwrap();
    assert_eq!(info["state"], json!("incompatible"));
    assert!(info["problem"].as_str().unwrap().contains("needs plugin API ^2.0"));
    assert!(host.start("com.example.future").unwrap_err().contains("needs plugin API ^2.0"));
    assert!(!log_text(&host, "com.example.future").contains("started"));
    // The plugin answers initialize with another major version: stopped, not restarted.
    let old = sandbox.plugin("oldapi", &["old-api"], json!({}));
    assert!(host.link_dev(&old).unwrap_err().contains("speaks plugin API 2.0.0"));
    std::thread::sleep(Duration::from_millis(300));
    let info = host.info("com.example.oldapi").unwrap();
    assert_eq!(info["state"], json!("incompatible"));
    assert_eq!(info["restartCount"], json!(0));
}

#[test]
fn a_plugin_that_ignores_shutdown_is_killed_after_the_grace_period() {
    let sandbox = Sandbox::new("stubborn");
    let dir = sandbox.plugin("stubborn", &["normal", "--stubborn"], json!({}));
    let host = PluginHost::new(&sandbox.root(), options()).unwrap();
    let pid = host.link_dev(&dir).unwrap()["pid"].as_u64().unwrap();
    let started = Instant::now();
    host.stop("com.example.stubborn").unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(!alive(pid));
}

#[test]
fn live_output_changes_reach_plugins_that_declare_the_event() {
    let sandbox = Sandbox::new("events");
    let dir = sandbox.plugin("events", &["normal"], json!({"contributes": {"events": ["liveOutput.changed"]}}));
    let quiet = sandbox.plugin("quiet", &["normal"], json!({}));
    let host = PluginHost::new(&sandbox.root(), options()).unwrap();
    host.link_dev(&dir).unwrap();
    host.link_dev(&quiet).unwrap();
    assert!(host.muted());
    host.set_muted(false);
    wait_for_log(&host, "com.example.events", "event \"liveOutput.changed\" {\"muted\":false}");
    std::thread::sleep(Duration::from_millis(150));
    assert!(!log_text(&host, "com.example.quiet").contains("event \""));
    // A plugin started now is told it isn't muted.
    host.reload("com.example.quiet").unwrap();
    wait_for_log(&host, "com.example.quiet", "fixture ready muted=false");
}

#[test]
fn editing_a_development_folder_restarts_the_plugin() {
    let sandbox = Sandbox::new("hotreload");
    let dir = sandbox.plugin("hotreload", &["normal"], json!({}));
    let host = PluginHost::new(&sandbox.root(), options()).unwrap();
    let pid = host.link_dev(&dir).unwrap()["pid"].as_u64().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    std::fs::write(dir.join("main.py"), "print('changed')").unwrap();
    wait_until("a new process", || host.info("com.example.hotreload").unwrap()["pid"].as_u64().is_some_and(|p| p != pid));
    assert!(log_text(&host, "com.example.hotreload").contains("development folder changed"));
    assert!(!alive(pid));
}

#[test]
fn enabling_persists_and_removed_folders_become_invalid() {
    let sandbox = Sandbox::new("enable");
    let dir = sandbox.plugin("enable", &["normal"], json!({}));
    let id = "com.example.enable";
    {
        let host = PluginHost::new(&sandbox.root(), options()).unwrap();
        let pid = host.link_dev(&dir).unwrap()["pid"].as_u64().unwrap();
        let info = host.set_enabled(id, false).unwrap();
        assert_eq!(info["state"], json!("disabled"));
        assert!(!alive(pid));
    }
    let host = PluginHost::new(&sandbox.root(), options()).unwrap();
    assert_eq!(host.state(id), Some(State::Disabled));
    assert!(host.start_enabled().is_empty());
    let info = host.set_enabled(id, true).unwrap();
    assert_eq!(info["state"], json!("running"));
    let pid = info["pid"].as_u64().unwrap();
    // Dropping the host stops its plugins.
    drop(host);
    assert!(!alive(pid));

    std::fs::remove_dir_all(&dir).unwrap();
    let host = PluginHost::new(&sandbox.root(), options()).unwrap();
    assert_eq!(host.state(id), Some(State::Invalid));
    assert!(host.list()["plugins"][0]["problem"].as_str().unwrap().contains("development folder"));
    host.unlink_dev(id).unwrap();
    assert_eq!(host.state(id), None);
}

#[test]
fn start_enabled_starts_installed_plugins_in_the_background() {
    let sandbox = Sandbox::new("installed");
    let source = sandbox.plugin("installed", &["normal"], json!({}));
    let target = sandbox.root().join("com.example.installed").join("0.1.0");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::copy(source.join("vixen-plugin.json"), target.join("vixen-plugin.json")).unwrap();
    let host = PluginHost::new(&sandbox.root(), options()).unwrap();
    let id = "com.example.installed";
    // Found on disk but never installed through Studio: listed, not run.
    assert_eq!(host.state(id), Some(State::Disabled));
    host.set_enabled(id, false).unwrap();
    host.set_enabled(id, true).unwrap();
    host.stop(id).unwrap();
    assert_eq!(host.start_enabled(), vec![id.to_string()]);
    wait_until("running", || host.state(id) == Some(State::Running));
    assert_eq!(host.info(id).unwrap()["source"], json!("installed"));
    assert!(Path::new(&host.info(id).unwrap()["folder"].as_str().unwrap().to_string()).ends_with("com.example.installed/0.1.0"));
}

#[test]
fn a_show_reports_the_plugins_it_needs() {
    let sandbox = Sandbox::new("requirements");
    let profile = sandbox.base.join("Show");
    let dir = sandbox.plugin("needed", &["normal"], json!({}));
    let host = PluginHost::new(&sandbox.root(), options()).unwrap();
    studio_plugins::profile::require(&profile, "com.example.needed", "^0.1.0").unwrap();
    studio_plugins::profile::require(&profile, "com.example.absent", "^1.2.0").unwrap();
    let report = host.requirements(&profile).unwrap();
    assert_eq!(report["plugins"][0]["message"], json!("Needs plugin com.example.absent ^1.2.0 — not installed"));
    assert_eq!(report["plugins"][1]["satisfied"], json!(false));
    host.link_dev(&dir).unwrap();
    let report = host.requirements(&profile).unwrap();
    assert_eq!(report["plugins"][1]["satisfied"], json!(true));
    assert_eq!(report["plugins"][1]["installedVersion"], json!("0.1.0"));
    studio_plugins::profile::require(&profile, "com.example.needed", "^0.2.0").unwrap();
    let report = host.requirements(&profile).unwrap();
    assert_eq!(report["plugins"][1]["message"], json!("Needs plugin com.example.needed ^0.2.0 — version 0.1.0 is installed"));
}
