use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};
fn profile() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/show-copy")
}
#[test]
fn real_show_stdio_round_trip_and_safety() {
    let root = profile();
    assert!(
        root.is_dir(),
        "prepare target/show-copy before running the integration test"
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_vixen-studio-mcp"))
        .args(["--profile", root.to_str().unwrap()])
        .env("VIXEN_MUTE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let requests = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{"name":"test","version":"1"},"capabilities":{}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"display_elements","arguments":{"limit":2}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"controller_list"}}),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"effect_catalog","arguments":{"limit":1}}}),
        json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"diagnostics_report"}}),
        json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"studio_status"}}),
        json!({"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"sequence_list"}}),
        json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"media_list"}}),
        json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"start_output","arguments":{"confirm":true}}}),
        json!({"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"display_elements","arguments":{"limit":501}}}),
        json!({"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"controller_overview"}}),
    ];
    let mut stdin = child.stdin.take().unwrap();
    for r in requests {
        writeln!(stdin, "{r}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(replies.len(), 12);
    assert!(replies[1]["result"]["tools"].as_array().unwrap().len() >= 46);
    for reply in &replies[2..9] {
        assert_eq!(reply["result"]["isError"], false, "{reply}");
    }
    let data: Value =
        serde_json::from_str(replies[2]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(data["items"].as_array().unwrap().len(), 2);
    assert!(data["total"].as_u64().unwrap() > 2);
    assert_eq!(replies[9]["error"]["code"], -32602);
    assert_eq!(replies[10]["result"]["isError"], true);
    // The Controllers section's list: every controller with a health level, nothing started.
    assert_eq!(replies[11]["result"]["isError"], false, "{}", replies[11]);
    let overview: Value =
        serde_json::from_str(replies[11]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let rows = overview["controllers"].as_array().unwrap();
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| r["health"]["level"].is_string() && r["universes"].is_string()), "{overview}");
    assert!(!root.join(".lock").exists());
}
#[test]
fn malformed_protocol_and_bounded_frames() {
    let mut server = studio_mcp::Server::open(&profile()).unwrap();
    assert_eq!(server.dispatch(json!([])).unwrap()["error"]["code"], -32600);
    assert_eq!(
        server
            .dispatch(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .unwrap()["error"]["code"],
        -32002
    );
    let mut out = Vec::new();
    studio_mcp::serve(
        &mut server,
        std::io::Cursor::new(
            b"not json
",
        ),
        &mut out,
    )
    .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&out).unwrap()["error"]["code"],
        -32700
    );
    // An oversized line is answered and skipped; the connection carries on with the next one (B74).
    let mut frames = vec![b'x'; studio_mcp::MAX_MESSAGE + 4096];
    frames.extend_from_slice(b"\n{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}\n");
    let mut out = Vec::new();
    studio_mcp::serve(&mut server, std::io::Cursor::new(frames), &mut out).unwrap();
    let replies: Vec<Value> = out.split(|b| *b == b'\n').filter(|l| !l.is_empty()).map(|l| serde_json::from_slice(l).unwrap()).collect();
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert_eq!(replies[0]["error"]["code"], -32600);
    assert!(replies[0]["error"]["message"].as_str().unwrap().contains("1 MiB"));
    assert_eq!(replies[1]["id"], 9);
    assert!(replies[1]["result"].is_object());
}

#[test]
fn existing_profile_lock_is_never_stolen() {
    let root = std::env::temp_dir().join(format!("studio-mcp-lock-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let sentinel = b"0@do-not-remove";
    std::fs::write(root.join(".lock"), sentinel).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_vixen-studio-mcp"))
        .args(["--profile", root.to_str().unwrap()])
        .env("VIXEN_MUTE", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(std::fs::read(root.join(".lock")).unwrap(), sentinel);
    std::fs::remove_file(root.join(".lock")).unwrap();
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn app_recognizes_running_bridge_lock() {
    use std::io::{BufRead, BufReader};
    let root = std::env::temp_dir().join(format!("studio-mcp-live-lock-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_vixen-studio-mcp"))
        .args(["--profile", root.to_str().unwrap()])
        .env("VIXEN_MUTE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin,"{}",json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{"name":"lock-test","version":"1"},"capabilities":{}}})).unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(serde_json::from_str::<Value>(&line)
        .unwrap()
        .get("result")
        .is_some());
    let locked = vixen_io::lock::check_profile_lock(&root).locked;
    drop(stdin);
    assert!(child.wait().unwrap().success());
    assert!(
        locked,
        "app must not mistake the active lowercase helper for a stale owner"
    );
    assert!(!root.join(".lock").exists());
    std::fs::remove_dir_all(root).unwrap();
}

/// Starts the bridge on `root`, answers one `initialize`, and returns it still running with its stdin open.
fn running_bridge(root: &std::path::Path) -> (std::process::Child, std::process::ChildStdin) {
    use std::io::{BufRead, BufReader};
    let mut child = Command::new(env!("CARGO_BIN_EXE_vixen-studio-mcp"))
        .args(["--profile", root.to_str().unwrap()])
        .env("VIXEN_MUTE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(stdin,"{}",json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"lock-test","version":"1"},"capabilities":{}}})).unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    assert!(serde_json::from_str::<Value>(&line).unwrap().get("result").is_some(), "{line}");
    (child, stdin)
}

#[test]
fn a_stop_signal_removes_the_lock() {
    use std::os::unix::process::ExitStatusExt;
    for (tag, signal) in [("term", libc::SIGTERM), ("int", libc::SIGINT), ("hup", libc::SIGHUP)] {
        let root = std::env::temp_dir().join(format!("studio-mcp-signal-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let (mut child, stdin) = running_bridge(&root);
        assert!(root.join(".lock").exists());
        // SAFETY: the pid is this test's own running child.
        assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
        let status = child.wait().unwrap();
        drop(stdin);
        assert_eq!((status.code(), status.signal()), (Some(128 + signal), None), "{tag}");
        assert!(!root.join(".lock").exists(), "{tag}: lock left behind");
        assert!(!studio_mcp::socket::socket_path(&root).unwrap().exists(), "{tag}: endpoint left behind");
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn a_killed_bridges_lock_is_taken_over_and_a_live_ones_names_its_owner() {
    let root = std::env::temp_dir().join(format!("studio-mcp-takeover-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    // SIGKILL can't be handled, so it leaves the lock naming a dead pid, as a crash would.
    let (mut killed, stdin) = running_bridge(&root);
    let dead = killed.id();
    killed.kill().unwrap();
    killed.wait().unwrap();
    drop(stdin);
    assert_eq!(std::fs::read_to_string(root.join(".lock")).unwrap().split('@').next(), Some(dead.to_string().as_str()));
    let (mut next, stdin) = running_bridge(&root);
    // A live bridge serves its session, so a second one attaches to it (issue #14) instead of being refused.
    let attached = Command::new(env!("CARGO_BIN_EXE_vixen-studio-mcp"))
        .args(["--profile", root.to_str().unwrap()])
        .env("VIXEN_MUTE", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(attached.status.success(), "{}", String::from_utf8_lossy(&attached.stderr));
    drop(stdin);
    assert!(next.wait().unwrap().success());
    assert!(!root.join(".lock").exists());
    // A live process that serves nothing (here this test itself) still keeps the profile, and the refusal names it.
    let live = std::process::id();
    std::fs::write(root.join(".lock"), format!("{live}@{}", vixen_io::lock::machine_name())).unwrap();
    let refused = Command::new(env!("CARGO_BIN_EXE_vixen-studio-mcp"))
        .args(["--profile", root.to_str().unwrap()])
        .env("VIXEN_MUTE", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!refused.status.success());
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains(&format!("locked by pid {live} (running)")), "{stderr}");
    std::fs::remove_file(root.join(".lock")).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
