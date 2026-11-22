//! The plugin system's end-to-end agent check (studio/PLUGINS-DESIGN.md, P6): an agent with nothing but the MCP
//! connection and its own file tools builds a UDP output plugin and drives it from a show.
//!
//! On a copy of target/show-copy's system data, served by a live (unmuted) Studio core the way the app serves it, the
//! agent reads the docs, scaffolds a TypeScript controller plugin, rewrites it into a UDP sender, validates it, links
//! it once the user allows plugin management, creates and configures a controller of its type (a bad address is
//! refused first), patches a new element to it, is refused a test pattern while live output isn't allowed (nothing
//! reaches the receiver), and once the user turns Live Output on and allows it, sees the test pattern's bytes arrive
//! at a UDP listener on 127.0.0.1. Never real lights: the show's own controllers are deleted from the copy first, and
//! the plugin only ever sends to the loopback listener.
//!
//! The user's three steps (the two Settings switches and Live Output) are done here the way the app does them.
use serde_json::{json, Value};
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn show_copy() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/show-copy")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

struct Agent {
    server: studio_mcp::Server,
    next: u64,
}

impl Agent {
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        self.server.dispatch(json!({"jsonrpc": "2.0", "id": self.next, "method": method, "params": params})).unwrap()
    }

    /// A tool call: (isError, the answer's JSON or text).
    fn call(&mut self, name: &str, arguments: Value) -> (bool, Value) {
        let reply = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        let result = &reply["result"];
        let text = result["content"][0]["text"].as_str().unwrap_or_else(|| panic!("{name}: {reply}")).to_string();
        (result["isError"] == json!(true), serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    fn ok(&mut self, name: &str, arguments: Value) -> Value {
        let (error, answer) = self.call(name, arguments);
        assert!(!error, "{name}: {answer}");
        answer
    }

    /// A display edit: its `result` (edits answer `{changed, result, revision}`, one undo step each).
    fn edit(&mut self, name: &str, arguments: Value) -> Value {
        let answer = self.ok(name, arguments);
        assert_eq!(answer["changed"], json!(true), "{name}: {answer}");
        answer["result"].clone()
    }

    fn read(&mut self, uri: &str) -> String {
        let reply = self.request("resources/read", json!({"uri": uri}));
        reply["result"]["contents"][0]["text"].as_str().unwrap_or_else(|| panic!("{uri}: {reply}")).to_string()
    }
}

/// The agent's UDP output: each frame's levels as one datagram to host:port.
const UDP_MAIN: &str = r#"#!/usr/bin/env node
// Agent UDP: each frame's levels, one byte per output, as one UDP datagram to host:port.
import dgram from "node:dgram";
import { Plugin, Controller } from "./vixen-plugin.mjs";

const plugin = new Plugin();

class Udp extends Controller {
  configure(config) {
    this.host = config.host;
    this.port = config.port;
  }

  start() {
    this.socket = dgram.createSocket("udp4");
    this.sent = 0;
    this.status("running", `sending to ${this.host}:${this.port}`);
  }

  frame(frame) {
    if (!this.socket) return;
    const data = Buffer.from(frame.values);
    this.socket.send(data, this.port, this.host, (error) => {
      this.sent += 1;
      if (this.sent % 40 === 0 || error) this.stats({ packetsSent: this.sent, errors: error ? 1 : 0 });
    });
  }

  stop() {
    this.socket?.close();
    this.socket = null;
  }
}

plugin.controller(Udp);
plugin.run();
"#;

#[test]
fn an_agent_builds_a_udp_output_plugin_and_lights_a_test_receiver_through_mcp_only() {
    if std::process::Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipped: node isn't installed");
        return;
    }
    let base = std::env::temp_dir().join(format!("studio-mcp-plugin-agent-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let base = base.canonicalize().unwrap();
    let show = base.join("show");
    copy_dir(&show_copy().join("SystemData"), &show.join("SystemData"));
    std::fs::copy(show_copy().join("VixenApplicationData.xml"), show.join("VixenApplicationData.xml")).unwrap();

    // The app's start-up: the shared plugin host provides plugin controller types before the profile loads, and is
    // muted until Live Output turns on (VixenCore::start, studio_plugins_sync).
    let host: &'static studio_plugins::PluginHost = Box::leak(Box::new(studio_plugins::PluginHost::new(&base.join("Plugins"), Default::default()).unwrap()));
    host.set_muted(true);
    vixen_output::modules::studio_external::register(Some(host.controllers()));
    let core = Arc::new(vixen_session::app::AppCore::open(show.clone(), false).unwrap());
    assert!(core.playback.hardware_allowed());
    let mut agent = Agent { server: studio_mcp::Server::from_core(core.clone()).with_plugin_host(host), next: 0 };
    agent.request("initialize", json!({"protocolVersion": "2024-11-05", "clientInfo": {"name": "e2e"}, "capabilities": {}}));

    // 1. Read the docs and the TypeScript example.
    assert!(agent.read("plugins://docs/AGENTS").contains("plugin_scaffold"));
    assert!(agent.read("plugins://api").contains("### `controller.frame`"));
    assert!(agent.read("plugins://examples/typescript").contains("dgram"));

    // 2. Scaffold, then 3. make it a UDP output with its own file tools.
    let made = agent.ok("plugin_scaffold", json!({"id": "com.example.agent-udp", "name": "Agent UDP", "language": "typescript", "kind": "controller"}));
    let folder = PathBuf::from(made["folder"].as_str().unwrap());
    assert!(folder.starts_with(base.join("Plugins/dev")), "{folder:?}");
    let manifest_path = folder.join("vixen-plugin.json");
    let mut manifest: Value = serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    manifest["description"] = json!("Sends each frame's levels as one UDP datagram.");
    manifest["permissions"] = json!({"network": ["udp:lan"]});
    let controller = &mut manifest["contributes"]["controllers"][0];
    controller["configSchema"] = json!({
        "type": "object",
        "x-order": ["host", "port"],
        "properties": {
            "host": {"type": "string", "format": "ipv4", "title": "Receiver address", "default": "127.0.0.1"},
            "port": {"type": "integer", "title": "Port", "minimum": 1, "maximum": 65535, "default": 7777}
        },
        "required": ["host"],
        "additionalProperties": false
    });
    controller["defaultOutputs"] = json!(6);
    let type_id = controller["typeId"].as_str().unwrap().to_string();
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    std::fs::write(folder.join("main.mjs"), UDP_MAIN).unwrap();

    // 4. Validate: every check passes, no warnings.
    let report = agent.ok("plugin_validate", json!({"path": folder}));
    assert!(report["passed"] == json!(true) && report["warnings"] == json!(0), "{report:#}");

    // 5. Link: refused until the user allows agents to manage plugins.
    let (error, refused) = agent.call("plugin_link_dev", json!({"path": folder, "confirm": true}));
    assert!(error && refused.as_str().unwrap().contains("Allow agents to manage plugins"), "{refused}");
    studio_plugins::set_agents_may_manage(true); // the user, in Settings ▸ Plugins
    let linked = agent.ok("plugin_link_dev", json!({"path": folder, "confirm": true}));
    assert_eq!(linked["state"], json!("running"), "{linked}");

    // 6. Into the show: no real controllers left in this copy, a controller of the new type, a test element patched to it.
    let listed = agent.ok("controller_list", json!({}));
    for existing in listed["items"].as_array().unwrap() {
        agent.ok("controller_delete", json!({"id": existing["id"], "confirm": true}));
    }
    let types = agent.ok("controller_types", json!({}));
    assert!(types.to_string().contains(&type_id), "{types}");
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    let port = receiver.local_addr().unwrap().port();
    let created = agent.edit("controller_create", json!({"name": "Agent UDP", "typeId": type_id, "outputs": 6}));
    let controller_id = created["id"].as_str().unwrap().to_string();
    let (error, bad) = agent.call("controller_configure", json!({"id": controller_id, "config": {"host": "lamp.local"}}));
    assert!(error && bad.as_str().unwrap().contains("/host: must be an IPv4 address"), "{bad}");
    let configured = agent.edit("controller_configure", json!({"id": controller_id, "config": {"host": "127.0.0.1", "port": port}}));
    assert_eq!(configured["controller"]["plugin"]["config"], json!({"host": "127.0.0.1", "port": port}));
    let prop = agent.edit("element_template", json!({"kind": "megatree", "name": "Agent Test Prop", "count": 1, "pixels": 2, "color": "rgb"}));
    agent.ok("patch_elements", json!({"elementId": prop["id"], "controllerId": controller_id}));
    let overview = agent.ok("controller_overview", json!({}));
    let row = overview["controllers"].as_array().unwrap().iter().find(|c| c["id"] == json!(controller_id)).cloned().unwrap();
    assert_eq!(row["patchedOutputs"], json!(6), "{row}");

    // 7. Muted: refused, and nothing reaches the receiver.
    let pattern = json!({"controllerId": controller_id, "pattern": "allOn", "brightnessPct": 40, "durationMs": 5000, "confirm": true});
    let (error, refused) = agent.call("plugin_send_test_frame", pattern.clone());
    assert!(error && refused.as_str().unwrap().contains("Allow agents to control live output"), "{refused}");
    let (error, _) = agent.call("output_start", json!({"controllerId": controller_id, "confirm": true}));
    assert!(error);
    let mut buffer = [0u8; 2048];
    let quiet_until = Instant::now() + Duration::from_millis(800);
    while Instant::now() < quiet_until {
        assert!(receiver.recv(&mut buffer).is_err(), "something was sent while muted");
    }

    // Permitted: the user turns Live Output on and allows agents to control it (Settings ▸ Agents).
    core.playback.open_execution();
    host.set_muted(false);
    vixen_session::studio_live::set_allowed_locally(&core, true);
    let started = agent.ok("output_start", json!({"controllerId": controller_id, "confirm": true}));
    assert_eq!(started["running"], json!(true), "{started}");
    let running: Vec<String> = core.playback.running_controllers().unwrap_or_default().iter().map(|id| id.to_string()).collect();
    assert_eq!(running, vec![controller_id.clone()], "only the plugin controller runs");
    let test = agent.ok("plugin_send_test_frame", pattern);
    assert_eq!(test["pluginId"], json!("com.example.agent-udp"), "{test}");

    // All On at 40 %: every output 102, as one 6-byte datagram.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    loop {
        assert!(Instant::now() < deadline, "no lit datagram at the receiver; got {seen:?}");
        if let Ok(n) = receiver.recv(&mut buffer) {
            if buffer[..n] == [102u8; 6] {
                break;
            }
            seen.push(buffer[..n].to_vec());
        }
    }
    let info = agent.ok("plugin_info", json!({"id": "com.example.agent-udp"}));
    let instance = &info["instances"][0];
    assert!(instance["live"] == json!(true) && instance["framesSent"].as_u64().unwrap() > 0, "{info:#}");

    // Done: stop the test, the user revokes the permission, which stops what the agent started.
    agent.ok("output_test_stop", json!({}));
    vixen_session::studio_live::set_allowed_locally(&core, false);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !core.playback.running_controllers().unwrap_or_default().is_empty() {
        assert!(Instant::now() < deadline, "the plugin controller kept running after the permission was revoked");
        std::thread::sleep(Duration::from_millis(50));
    }
    core.playback.close_execution();
    studio_plugins::set_agents_may_manage(false);
    host.shutdown_all();
    vixen_output::modules::studio_external::register(None);
    drop(agent);
    drop(core);
    let _ = std::fs::remove_dir_all(&base);
}
