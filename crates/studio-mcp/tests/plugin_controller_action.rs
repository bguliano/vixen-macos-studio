//! A plugin controller that sets its own output count and runs a settings action, through MCP only (backlog B61): a
//! Python plugin on the real SDK answers `controller.outputsFor` from its `outputs_for` class method, `controller_types`
//! and `controller_network` say so, `controller_configure` resizes and names the outputs and refuses `outputs`, and
//! `controller_action` runs the plugin's Detect tool and applies its merge patch as one undo step.
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

const TYPE_ID: &str = "4c2b0a6e-1d3f-4e5a-9b7c-8d6e5f4a3b2c";

const MAIN: &str = r#"
import vixen_plugin as vp

plugin = vp.Plugin()


@plugin.controller()
class Lamps(vp.Controller):
    @classmethod
    def outputs_for(cls, config):
        return [f"{lamp['name']} {c}" for lamp in config["lamps"] for c in "RGB"]


@plugin.tool("detect")
def detect(arguments):
    found = [{"name": "Ceiling"}, {"name": "Porch"}]
    rows = list(arguments["config"].get("lamps", []))
    rows += [lamp for lamp in found if lamp not in rows]
    return {"mergePatch": {"lamps": rows}, "message": f"Found {len(found)} lamps"}


plugin.run()
"#;

fn call(server: &mut studio_mcp::Server, name: &str, arguments: Value) -> (bool, Value) {
    let reply = server.dispatch(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": arguments}})).unwrap();
    let result = &reply["result"];
    let text = result["content"][0]["text"].as_str().unwrap_or_else(|| panic!("{name}: {reply}")).to_string();
    (result["isError"] == json!(true), serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

#[test]
fn a_plugin_sets_its_own_output_count_and_runs_a_settings_action() {
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        eprintln!("skipped: python3 isn't installed");
        return;
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let base = std::env::temp_dir().join(format!("studio-mcp-plugin-action-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let plugin = base.join("lamps");
    std::fs::create_dir_all(&plugin).unwrap();
    let base = base.canonicalize().unwrap();
    std::fs::copy(repo.join("studio/plugins/sdk/python/vixen_plugin.py"), plugin.join("vixen_plugin.py")).unwrap();
    std::fs::write(plugin.join("main.py"), MAIN).unwrap();
    let manifest = json!({
        "id": "com.example.lamps", "name": "Lamps", "version": "1.0.0", "apiVersion": "^1.1",
        "entry": {"command": "python3", "args": ["main.py"]},
        "contributes": {
            "controllers": [{
                "typeId": TYPE_ID, "name": "Lamps", "defaultOutputs": 3, "maxOutputs": 300, "outputCount": "fromConfig",
                "actions": [{"id": "detect", "title": "Detect Lamps", "tool": "detect"}],
                "configSchema": {"type": "object", "additionalProperties": false, "properties": {
                    "lamps": {"type": "array", "default": [], "items": {"type": "object", "additionalProperties": false, "required": ["name"], "properties": {"name": {"type": "string"}}}}
                }}
            }],
            "tools": [{"name": "detect", "description": "Find lamps.", "readOnly": true}]
        }
    });
    std::fs::write(plugin.join("vixen-plugin.json"), manifest.to_string()).unwrap();

    let host: &'static studio_plugins::PluginHost = Box::leak(Box::new(studio_plugins::PluginHost::new(&base.join("Plugins"), Default::default()).unwrap()));
    host.set_muted(true);
    host.link_dev(&plugin).unwrap();
    vixen_output::modules::studio_external::register(Some(host.controllers()));
    let core = Arc::new(vixen_session::app::AppCore::open(base.join("show"), true).unwrap());
    let mut server = studio_mcp::Server::from_core(core.clone()).with_plugin_host(host);
    server.dispatch(json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2024-11-05", "clientInfo": {}, "capabilities": {}}})).unwrap();

    let (_, types) = call(&mut server, "controller_types", json!({}));
    let lamps = types["items"].as_array().unwrap().iter().find(|t| t["id"] == TYPE_ID).cloned().unwrap();
    assert_eq!(lamps["outputsFromConfig"], true, "{lamps}");
    let (error, created) = call(&mut server, "controller_create", json!({"name": "Lamps", "typeId": TYPE_ID, "outputs": 3}));
    assert!(!error, "{created}");
    let id = created["result"]["id"].as_str().unwrap().to_string();

    let (error, configured) = call(&mut server, "controller_configure", json!({"id": id, "config": {"lamps": [{"name": "Desk"}]}}));
    assert!(!error, "{configured}");
    assert_eq!(configured["result"]["controller"]["outputs"], 3);
    let (error, refused) = call(&mut server, "controller_configure", json!({"id": id, "outputs": 9}));
    assert!(error && refused.as_str().unwrap().contains("sets its output count from its settings"), "{refused}");

    let (error, network) = call(&mut server, "controller_network", json!({"id": id}));
    assert!(!error, "{network}");
    assert_eq!(network["plugin"]["actions"], json!([{"id": "detect", "title": "Detect Lamps", "description": null}]));

    let (error, detected) = call(&mut server, "controller_action", json!({"id": id, "action": "detect"}));
    assert!(!error, "{detected}");
    assert_eq!((detected["applied"].clone(), detected["message"].clone()), (json!(true), json!("Found 2 lamps")), "{detected}");
    assert_eq!(detected["result"]["result"]["controller"]["outputs"], 9, "{detected}");
    let setup = vixen_session::setup::SetupSession::open(&core);
    let names: Vec<String> = setup.controllers.iter().find(|c| c.alive && c.id.to_string() == id).unwrap().outputs.iter().map(|o| o.name.clone()).collect();
    assert_eq!(names, ["Desk R", "Desk G", "Desk B", "Ceiling R", "Ceiling G", "Ceiling B", "Porch R", "Porch G", "Porch B"]);
    let (_, again) = call(&mut server, "controller_action", json!({"id": id, "action": "detect"}));
    assert_eq!(again["applied"], false, "{again}");
    let (error, unknown) = call(&mut server, "controller_action", json!({"id": id, "action": "nope"}));
    assert!(error && unknown.as_str().unwrap().contains("no action nope"), "{unknown}");

    // One undo step takes the action back: settings and outputs.
    let (error, undone) = call(&mut server, "display_undo", json!({}));
    assert!(!error, "{undone}");
    let setup = vixen_session::setup::SetupSession::open(&core);
    assert_eq!(setup.controllers.iter().find(|c| c.alive && c.id.to_string() == id).unwrap().outputs.len(), 3);

    vixen_output::modules::studio_external::register(None);
    host.shutdown_all();
    drop(server);
    drop(core);
    let _ = std::fs::remove_dir_all(&base);
}
