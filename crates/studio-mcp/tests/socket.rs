use serde_json::{json,Value};
use std::{io::{BufRead,BufReader,Write},sync::Arc,os::unix::fs::PermissionsExt};
#[test]
fn local_socket_edits_are_live_and_share_ui_undo() {
    let root=std::env::temp_dir().join(format!("studio-socket-{}",std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let core=Arc::new(vixen_session::app::AppCore::open(root.clone(),true).unwrap());
    let server=studio_mcp::socket::SocketServer::start(&core).unwrap();
    let path=studio_mcp::socket::socket_path(&root).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode()&0o777,0o600);
    let mut stream=studio_mcp::socket::connect(&root).unwrap().unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let mut reader=BufReader::new(stream.try_clone().unwrap());
    let mut request=|value:Value|->Value {
        writeln!(stream,"{value}").unwrap();let mut line=String::new();reader.read_line(&mut line).unwrap();serde_json::from_str(&line).unwrap()
    };
    assert!(request(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{},"capabilities":{}}}))["result"].is_object());
    let result=request(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"element_create","arguments":{"name":"From MCP"}}}));
    assert_eq!(result["result"]["isError"],false,"{result}");
    assert!(core.system.read().tree.all_nodes().any(|(_,n)|n.name=="From MCP"));
    assert!(core.studio_display.history()["undo"][0].as_str().unwrap().ends_with("(Agent)"));
    core.studio_display.execute(&core,"display_undo",&json!({}),false).unwrap();
    assert!(!core.system.read().tree.all_nodes().any(|(_,n)|n.name=="From MCP"));
    let events=core.studio_display.events(0);
    assert!(events.iter().any(|e|e.kind=="clientConnected"));
    assert!(events.iter().any(|e|e.kind=="commandFinished"));
    core.studio_display.stop();
    let result=request(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"element_create","arguments":{"name":"After shutdown"}}}));
    assert_eq!(result["result"]["isError"],true);
    drop(reader);drop(stream);drop(server);
    assert!(!path.exists());
    drop(core);
    std::fs::remove_dir_all(root).unwrap();
}
