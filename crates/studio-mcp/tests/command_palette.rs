mod support;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
};
fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to)
        } else if entry.file_type().unwrap().is_file() {
            std::fs::copy(entry.path(), to).unwrap();
        }
    }
}
/// command_palette is advertised as read-only, validates its arguments, and from a headless bridge (no menu bar) says it
/// needs the running app instead of returning an empty list that would look like "no such command".
#[test]
fn command_palette_is_listed_and_needs_the_app() {
    let target = support::target_dir();
    let source = target.join("show-copy");
    let root = target.join(format!("studio-mcp-command-palette-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    copy_dir(&source.join("SystemData"), &root.join("SystemData"));
    copy_dir(&source.join("Module Data Files"), &root.join("Module Data Files"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_vixen-studio-mcp"))
        .args(["--profile", root.to_str().unwrap()])
        .env("VIXEN_MUTE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut next = 1;
    let mut rpc = |method: &str, params: Value| -> Value {
        let id = next;
        next += 1;
        writeln!(input, "{}", json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})).unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    };
    rpc("initialize", json!({"protocolVersion":"2024-11-05","clientInfo":{},"capabilities":{}}));
    let tools = rpc("tools/list", json!({}));
    let tool = tools["tools"].as_array().unwrap().iter().find(|t| t["name"] == "command_palette").expect("advertised");
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    let call = |rpc: &mut dyn FnMut(&str, Value) -> Value, args: Value| {
        let r = rpc("tools/call", json!({"name":"command_palette","arguments":args}));
        (r["isError"] == true, r["content"][0]["text"].as_str().unwrap().to_string())
    };
    let (failed, text) = call(&mut rpc, json!({"query":"save","window":"sequencer"}));
    assert!(failed && text.contains("running app"), "{text}");
    let (failed, text) = call(&mut rpc, json!({"window":"desktop"}));
    assert!(failed && text.contains("show or sequencer"), "{text}");
    let (failed, _) = call(&mut rpc, json!({"run":"Save"}));
    assert!(failed);
    drop(input);
    child.wait().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}
