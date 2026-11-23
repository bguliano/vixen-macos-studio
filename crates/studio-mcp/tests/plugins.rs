//! Plugin developer tools, plugin-contributed tools, plugins:// resources and the build_plugin prompt through the MCP
//! server, with a private plugin host and a plugin made by plugin_scaffold (python3).
mod support;
use serde_json::{json, Value};
use std::path::Path;

fn profile() -> std::path::PathBuf {
    support::target_dir().join("show-copy")
}

fn call(server: &mut studio_mcp::Server, id: u64, name: &str, arguments: Value) -> (bool, Value) {
    let reply = server.dispatch(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})).unwrap();
    let result = &reply["result"];
    let text = result["content"][0]["text"].as_str().unwrap_or("").to_string();
    let error = result["isError"] == json!(true);
    (error, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

#[test]
fn an_agent_scaffolds_validates_links_and_calls_a_plugin_through_mcp() {
    let base = std::env::temp_dir().join(format!("studio-mcp-plugins-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let base = base.canonicalize().unwrap();
    let host: &'static studio_plugins::PluginHost = Box::leak(Box::new(studio_plugins::PluginHost::new(&base.join("Plugins"), Default::default()).unwrap()));
    let mut server = studio_mcp::Server::open(&profile()).unwrap().with_plugin_host(host);
    let init = server.dispatch(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{},"capabilities":{}}})).unwrap();
    assert_eq!(init["result"]["capabilities"]["tools"]["listChanged"], json!(true));

    let tools = server.dispatch(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    for name in ["plugin_list", "plugin_info", "plugin_logs", "plugin_scaffold", "plugin_validate", "plugin_install", "plugin_uninstall", "plugin_enable", "plugin_disable", "plugin_link_dev", "plugin_unlink_dev", "plugin_reload", "plugin_send_test_frame"] {
        assert!(names.contains(&name), "{name}");
    }
    assert!(!names.iter().any(|n| n.starts_with("plugin__")));

    let (error, made) = call(&mut server, 3, "plugin_scaffold", json!({"id": "com.example.greeter", "name": "Greeter", "language": "python", "kind": "tools"}));
    assert!(!error, "{made}");
    let folder = made["folder"].as_str().unwrap().to_string();
    let (error, report) = call(&mut server, 4, "plugin_validate", json!({"path": folder}));
    assert!(!error && report["passed"] == json!(true), "{report:#}");

    let (error, refused) = call(&mut server, 5, "plugin_link_dev", json!({"path": folder, "confirm": true}));
    assert!(error && refused.as_str().unwrap().contains("Allow agents to manage plugins"), "{refused}");
    studio_plugins::set_agents_may_manage(true);
    let (error, linked) = call(&mut server, 6, "plugin_link_dev", json!({"path": folder, "confirm": true}));
    assert!(!error && linked["state"] == json!("running"), "{linked}");

    // The new tool shows up: the client is told before the next answer, then lists it and calls it.
    let mut out = Vec::new();
    studio_mcp::serve(&mut server, std::io::Cursor::new(format!("{}\n", json!({"jsonrpc":"2.0","id":7,"method":"ping"}))), &mut out).unwrap();
    let lines: Vec<Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines[0]["method"], json!("notifications/tools/list_changed"), "{lines:?}");
    assert_eq!(lines[1]["id"], json!(7));
    let tools = server.dispatch(json!({"jsonrpc":"2.0","id":8,"method":"tools/list"})).unwrap();
    let hello = tools["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "plugin__greeter__hello").cloned().expect("plugin tool listed");
    assert_eq!(hello["annotations"]["readOnlyHint"], json!(true));
    let reply = server.dispatch(json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"plugin__greeter__hello","arguments":{"name":"Vixen"}}})).unwrap();
    assert_eq!(reply["result"]["content"][0]["text"], json!("Hello, Vixen, from Greeter."));
    assert!(server.notifications().is_empty());

    let (error, info) = call(&mut server, 10, "plugin_info", json!({"id": "com.example.greeter"}));
    assert!(!error && info["mcpTools"] == json!(["plugin__greeter__hello"]), "{info}");
    let (_, list) = call(&mut server, 11, "plugin_list", json!({}));
    assert_eq!(list["agentsMayManage"], json!(true));

    // Disabling it takes the tool away again.
    let (error, _) = call(&mut server, 12, "plugin_disable", json!({"id": "com.example.greeter", "confirm": true}));
    studio_plugins::set_agents_may_manage(false);
    assert!(!error);
    assert_eq!(server.notifications().len(), 1);
    let reply = server.dispatch(json!({"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"plugin__greeter__hello","arguments":{}}})).unwrap();
    assert_eq!(reply["result"]["isError"], json!(true));

    // A test pattern needs the running app.
    let (error, message) = call(&mut server, 14, "plugin_send_test_frame", json!({"controllerId": "x", "confirm": true}));
    assert!(error && message.as_str().unwrap().contains("running app"), "{message}");

    // Resources and the prompt.
    let resources = server.dispatch(json!({"jsonrpc":"2.0","id":15,"method":"resources/list"})).unwrap();
    let uris: Vec<&str> = resources["result"]["resources"].as_array().unwrap().iter().filter_map(|r| r["uri"].as_str()).collect();
    assert!(uris.contains(&"studio://contract") && uris.contains(&"plugins://api") && uris.contains(&"plugins://com.example.greeter/manifest"), "{uris:?}");
    let read = server.dispatch(json!({"jsonrpc":"2.0","id":16,"method":"resources/read","params":{"uri":"plugins://schema/manifest"}})).unwrap();
    assert!(read["result"]["contents"][0]["text"].as_str().unwrap().contains("\"contributes\""));
    let missing = server.dispatch(json!({"jsonrpc":"2.0","id":17,"method":"resources/read","params":{"uri":"plugins://nothing"}})).unwrap();
    assert_eq!(missing["error"]["code"], json!(-32602));
    let prompts = server.dispatch(json!({"jsonrpc":"2.0","id":18,"method":"prompts/list"})).unwrap();
    assert!(prompts["result"]["prompts"].as_array().unwrap().iter().any(|p| p["name"] == "build_plugin"));
    let prompt = server.dispatch(json!({"jsonrpc":"2.0","id":19,"method":"prompts/get","params":{"name":"build_plugin","arguments":{"goal":"UDP lamps"}}})).unwrap();
    let text = prompt["result"]["messages"][0]["content"]["text"].as_str().unwrap();
    assert!(text.starts_with("Goal: UDP lamps") && text.contains("plugin_validate"), "{text}");

    host.shutdown_all();
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn plugin_required_reports_what_the_show_needs() {
    let base = std::env::temp_dir().join(format!("studio-mcp-plugins-required-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("show/SystemData")).unwrap();
    for file in ["SystemConfig.xml", "ModuleStore.xml"] {
        std::fs::copy(profile().join("SystemData").join(file), base.join("show/SystemData").join(file)).unwrap();
    }
    std::fs::write(base.join("show/SystemData/StudioPlugins.json"), r#"{"plugins":[{"id":"com.example.lamp","versionRange":"^1.2"}]}"#).unwrap();
    let host: &'static studio_plugins::PluginHost = Box::leak(Box::new(studio_plugins::PluginHost::new(&base.join("Plugins"), Default::default()).unwrap()));
    let mut server = studio_mcp::Server::open(&base.join("show")).unwrap().with_plugin_host(host);
    server.dispatch(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{},"capabilities":{}}})).unwrap();
    let tools = server.dispatch(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
    let listed = tools["result"]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "plugin_required").cloned().expect("listed");
    assert_eq!(listed["annotations"]["readOnlyHint"], json!(true));
    let (error, needs) = call(&mut server, 3, "plugin_required", json!({}));
    assert!(!error, "{needs}");
    assert_eq!(needs["plugins"][0]["id"], json!("com.example.lamp"));
    assert_eq!(needs["plugins"][0]["satisfied"], json!(false));
    assert_eq!(needs["plugins"][0]["message"], json!("Needs plugin com.example.lamp ^1.2 — not installed"));
    let _ = std::fs::remove_dir_all(&base);
}

/// studio/MCP-TOOLS.md is the server's tools/list as a table, and docs/plugins only names MCP tools that exist.
/// `STUDIO_REGENERATE_MCP_TOOLS=1 cargo test -p studio-mcp --test plugins mcp_tools` rewrites the table.
#[test]
fn mcp_tools_md_and_the_plugin_docs_match_tools_list() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let base = std::env::temp_dir().join(format!("studio-mcp-plugins-docs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let host: &'static studio_plugins::PluginHost = Box::leak(Box::new(studio_plugins::PluginHost::new(&base.join("Plugins"), Default::default()).unwrap()));
    let mut server = studio_mcp::Server::open(&profile()).unwrap().with_plugin_host(host);
    server.dispatch(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{},"capabilities":{}}})).unwrap();
    let tools = server.dispatch(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
    let tools: Vec<Value> = tools["result"]["tools"].as_array().unwrap().iter().filter(|t| !t["name"].as_str().unwrap().starts_with("plugin__")).cloned().collect();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();

    let mut table = format!(
        "# Implemented MCP tools\n\n{} tools, generated from the tested bridge tools/list result. No placeholder tools. Plugin-contributed tools (`plugin__<short-id>__<tool>`) are listed at runtime from the installed plugins.\n\n| Tool | Read only | Description |\n|---|---|---|\n",
        tools.len()
    );
    for tool in &tools {
        let read_only = tool["annotations"]["readOnlyHint"].as_bool().unwrap_or(false);
        table.push_str(&format!("| {} | {read_only} | {} |\n", tool["name"].as_str().unwrap(), tool["description"].as_str().unwrap().replace('|', "\\|").replace('\n', " ")));
    }
    let path = repo.join("studio/MCP-TOOLS.md");
    if std::env::var_os("STUDIO_REGENERATE_MCP_TOOLS").is_some() {
        std::fs::write(&path, &table).unwrap();
    }
    assert!(std::fs::read_to_string(&path).unwrap() == table, "studio/MCP-TOOLS.md is stale: run STUDIO_REGENERATE_MCP_TOOLS=1 cargo test -p studio-mcp --test plugins mcp_tools");

    // Every `plugin_…` or other snake_case tool name in backticks on the plugin pages is a real tool.
    for entry in std::fs::read_dir(repo.join("docs/plugins")).unwrap().flatten() {
        let text = std::fs::read_to_string(entry.path()).unwrap();
        for word in text.split('`').skip(1).step_by(2) {
            let name = word.split([' ', '{']).next().unwrap();
            let tool_like = name.contains('_') && name.chars().all(|c| c.is_ascii_lowercase() || c == '_') && !name.starts_with('_');
            let known_tool_family = ["plugin_", "controller_", "output_", "patch_", "command_"].iter().any(|p| name.starts_with(p));
            if tool_like && known_tool_family && !name.starts_with("plugin__") {
                assert!(names.contains(&name), "{}: `{name}` isn't an MCP tool", entry.file_name().to_string_lossy());
            }
        }
    }
    let _ = std::fs::remove_dir_all(&base);
}
