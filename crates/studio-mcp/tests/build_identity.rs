//! Issue #71: studio_status and initialize name the commit the session and the relaying bridge were built from.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn head() -> String {
    let out = Command::new("git").current_dir(env!("CARGO_MANIFEST_DIR")).args(["rev-parse", "HEAD"]).output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// One JSON-RPC exchange per request on `reader`/`writer`, skipping server notifications.
fn rpc(writer: &mut impl Write, reader: &mut impl BufRead, id: u64, method: &str, params: Value) -> Value {
    writeln!(writer, "{}", json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})).unwrap();
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0, "session closed");
        let reply: Value = serde_json::from_str(&line).unwrap();
        if reply["id"] == id {
            return reply;
        }
    }
}
fn initialize(writer: &mut impl Write, reader: &mut impl BufRead) -> Value {
    rpc(writer, reader, 1, "initialize", json!({"protocolVersion":"2024-11-05","clientInfo":{"name":"t","version":"1"},"capabilities":{}}))
}
fn status(writer: &mut impl Write, reader: &mut impl BufRead) -> Value {
    let reply = rpc(writer, reader, 2, "tools/call", json!({"name":"studio_status","arguments":{}}));
    serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn status_and_initialize_report_the_session_and_bridge_builds() {
    let root = std::env::temp_dir().join(format!("studio-build-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let spawn = || {
        let mut child = Command::new(env!("CARGO_BIN_EXE_vixen-studio-mcp"))
            .arg("--profile").arg(&root).env("VIXEN_MUTE", "1")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        (child, stdin, stdout)
    };

    // The first bridge hosts the session itself: its build is the session's, and nothing relays it.
    let (mut host, mut host_in, mut host_out) = spawn();
    let info = initialize(&mut host_in, &mut host_out)["result"]["serverInfo"].clone();
    assert_eq!(info["build"]["commit"], head(), "{info}");
    assert!(info["build"]["date"].as_str().is_some_and(|d| d.starts_with("20")), "{info}");
    assert!(info["build"]["dirty"].is_boolean(), "{info}");
    let own = status(&mut host_in, &mut host_out);
    assert_eq!(own["build"], info["build"]);
    assert_eq!(own["bridge"], Value::Null);

    // A second bridge relays to that session and announces its own build.
    let (mut relay, mut relay_in, mut relay_out) = spawn();
    let info = initialize(&mut relay_in, &mut relay_out)["result"]["serverInfo"].clone();
    assert_eq!(info["bridge"], own["build"], "{info}");
    assert_eq!(status(&mut relay_in, &mut relay_out)["bridge"], own["build"]);

    // A bridge from another build is reported as it announced itself; anything else in the announcement is dropped.
    let socket = studio_mcp::socket::connect(&root).unwrap().expect("the host serves the profile");
    let (mut writer, mut reader) = (socket.try_clone().unwrap(), BufReader::new(socket));
    let older = json!({"commit":"b637899f0","date":"2026-11-01T10:00:00-04:00","dirty":false});
    writeln!(writer, "{}", json!({"jsonrpc":"2.0","method":studio_mcp::BRIDGE_NOTIFICATION,"params":{"build":older}})).unwrap();
    initialize(&mut writer, &mut reader);
    assert_eq!(status(&mut writer, &mut reader)["bridge"], older);
    let forged = json!({"commit":"$(touch x)","date":"today","dirty":false});
    writeln!(writer, "{}", json!({"jsonrpc":"2.0","method":studio_mcp::BRIDGE_NOTIFICATION,"params":{"build":forged}})).unwrap();
    assert_eq!(status(&mut writer, &mut reader)["bridge"], Value::Null);
    drop((writer, reader));

    drop(relay_in);
    assert!(relay.wait().unwrap().success());
    drop(host_in);
    assert!(host.wait().unwrap().success());
    std::fs::remove_dir_all(root).unwrap();
}
