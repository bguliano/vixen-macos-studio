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

/// Runs one bridge on the profile with its own settings folder, makes the calls, and returns each tool's (failed, text).
fn session(root: &Path, settings: &Path, calls: &[Value]) -> Vec<(bool, Value)> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_vixen-studio-mcp"))
        .args(["--profile", root.to_str().unwrap()])
        .env("VIXEN_MUTE", "1")
        .env("VIXEN_SETTINGS_DIR", settings)
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
    let tool = tools["tools"].as_array().unwrap().iter().find(|t| t["name"] == "effects_library").expect("advertised");
    assert_eq!(tool["annotations"]["readOnlyHint"], false);
    assert_eq!(tool["annotations"]["destructiveHint"], false);
    let results = calls
        .iter()
        .map(|args| {
            let r = rpc("tools/call", json!({"name":"effects_library","arguments":args}));
            let text = r["content"][0]["text"].as_str().unwrap().to_string();
            (r["isError"] == true, serde_json::from_str(&text).unwrap_or(Value::String(text)))
        })
        .collect();
    drop(input);
    child.wait().unwrap();
    results
}

/// An agent pins favorites as the star does; they are the user's, kept in the settings folder, so a later session (a
/// relaunch) reads them back, and bad arguments change nothing.
#[test]
fn effects_library_pins_favorites_that_survive_a_new_session() {
    let target = support::target_dir();
    let source = target.join("show-copy");
    let root = target.join(format!("studio-mcp-effects-library-{}", std::process::id()));
    let settings = root.with_extension("settings");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&settings);
    copy_dir(&source.join("SystemData"), &root.join("SystemData"));
    copy_dir(&source.join("Module Data Files"), &root.join("Module Data Files"));
    let pulse = "cbd76d3b-c924-40ff-bad6-d1437b3dbdc0";
    let chase = "affea852-85b1-418f-9cdf-0b9735154bb5";
    let first = session(
        &root,
        &settings,
        &[
            json!({}),
            json!({"favorite": pulse, "on": true}),
            json!({"favorite": chase, "on": true}),
            json!({"favorite": pulse, "on": true}),
            json!({"favorite": "not-an-effect", "on": true}),
            json!({"on": false}),
        ],
    );
    assert_eq!(first[0], (false, json!({"changed": false, "favorites": [], "recent": []})));
    assert!(!first[1].0 && first[1].1["changed"] == true && first[1].1["favorites"][0]["name"] == "Pulse", "{:?}", first[1]);
    assert_eq!(first[2].1["favorites"].as_array().unwrap().len(), 2);
    assert_eq!(first[3].1["changed"], false);
    assert!(first[4].0 && first[5].0);
    assert!(settings.join("Studio Effects Library.json").is_file());

    let second = session(&root, &settings, &[json!({}), json!({"favorite": pulse, "on": false, "clearRecent": true})]);
    let names = |v: &Value| v["favorites"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(names(&second[0].1), ["Pulse", "Chase"]);
    assert_eq!(names(&second[1].1), ["Chase"]);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&settings);
}
