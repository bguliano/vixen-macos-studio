use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::fs::PermissionsExt,
    sync::Arc,
};
#[test]
fn local_socket_edits_are_live_and_share_ui_undo() {
    let root = std::env::temp_dir().join(format!("studio-socket-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let core = Arc::new(vixen_session::app::AppCore::open(root.clone(), true).unwrap());
    let server = studio_mcp::socket::SocketServer::start(&core).unwrap();
    let path = studio_mcp::socket::socket_path(&root).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let mut stream = studio_mcp::socket::connect(&root).unwrap().unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request = |value: Value| -> Value {
        writeln!(stream, "{value}").unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    assert!(request(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{},"capabilities":{}}}))["result"].is_object());
    let result = request(
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"element_create","arguments":{"name":"From MCP"}}}),
    );
    assert_eq!(result["result"]["isError"], false, "{result}");
    assert!(core
        .system
        .read()
        .tree
        .all_nodes()
        .any(|(_, n)| n.name == "From MCP"));
    assert!(core.studio_display.history()["undo"][0]
        .as_str()
        .unwrap()
        .ends_with("(Agent)"));
    core.studio_display
        .execute(&core, "display_undo", &json!({}), false)
        .unwrap();
    assert!(!core
        .system
        .read()
        .tree
        .all_nodes()
        .any(|(_, n)| n.name == "From MCP"));
    let events = core.studio_display.events(0);
    assert!(events.iter().any(|e| e.kind == "clientConnected"));
    assert!(events.iter().any(|e| e.kind == "commandFinished"));
    core.studio_display.stop();
    let result = request(
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"element_create","arguments":{"name":"After shutdown"}}}),
    );
    assert_eq!(result["result"]["isError"], true);
    drop(reader);
    drop(stream);
    drop(server);
    assert!(!path.exists());
    drop(core);
    std::fs::remove_dir_all(root).unwrap();
}

/// A proxying bridge (one attached to the app's or another bridge's session) runs `socket::proxy` with an MCP client on
/// `client`. The endpoint here closes connections after one idle second.
fn proxied(tag: &str, keepalive: std::time::Duration) -> (std::os::unix::net::UnixStream, BufReader<std::os::unix::net::UnixStream>, std::path::PathBuf, studio_mcp::socket::SocketServer, Arc<vixen_session::app::AppCore>) {
    use std::os::unix::net::UnixStream;
    let root = std::env::temp_dir().join(format!("studio-keepalive-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let core = Arc::new(vixen_session::app::AppCore::open(root.clone(), true).unwrap());
    let server = studio_mcp::socket::start_headless_idle(&core, std::time::Duration::from_secs(1)).unwrap();
    let socket = studio_mcp::socket::connect(&root).unwrap().unwrap();
    let (client, bridge_in) = UnixStream::pair().unwrap();
    let (bridge_out, client_out) = UnixStream::pair().unwrap();
    std::thread::spawn(move || studio_mcp::socket::proxy(socket, BufReader::new(bridge_in), bridge_out, keepalive));
    client_out.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    (client, BufReader::new(client_out), root, server, core)
}

#[test]
fn a_proxying_bridge_outlives_the_idle_limit_and_hides_its_pings() {
    let (mut client, mut replies, root, _server, _core) = proxied("ping", std::time::Duration::from_millis(200));
    let mut request = |id: u32, method: &str, params: Value| -> Value {
        writeln!(client, "{}", json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})).unwrap();
        let mut line = String::new();
        replies.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or(Value::Null)
    };
    assert!(request(1, "initialize", json!({"protocolVersion":"2024-11-05","clientInfo":{},"capabilities":{}}))["result"].is_object());
    std::thread::sleep(std::time::Duration::from_millis(2500));
    // The next line read is this request's reply: the bridge's own ping replies never reach the client.
    let reply = request(2, "tools/list", json!({}));
    assert_eq!(reply["id"], 2, "{reply}");
    assert!(reply["result"]["tools"].is_array());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_idle_connection_without_pings_is_still_closed() {
    let (mut client, mut replies, root, server, _core) = proxied("quiet", std::time::Duration::from_secs(60));
    writeln!(client, "{}", json!({"jsonrpc":"2.0","id":1,"method":"ping"})).unwrap();
    let mut line = String::new();
    replies.read_line(&mut line).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2500));
    assert_eq!(server.clients(), 0, "the idle limit still frees the slot");
    let _ = std::fs::remove_dir_all(root);
}
