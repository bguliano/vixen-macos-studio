//! Issue #14: many bridges and short-lived scripts share one session on a profile, with or without the app.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};

fn bin() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_vixen-studio-mcp"));
    command.env("VIXEN_MUTE", "1");
    command
}
fn profile(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("studio-shared-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root.canonicalize().unwrap()
}

/// A stdio bridge with its pipes, initialized.
struct Bridge {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u64,
}
impl Bridge {
    fn start(root: &std::path::Path, extra: &[&str]) -> Self {
        let mut child = bin()
            .arg("--profile")
            .arg(root)
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut bridge = Self {
            stdin: child.stdin.take().unwrap(),
            stdout: BufReader::new(child.stdout.take().unwrap()),
            child,
            next: 1,
        };
        bridge.rpc("initialize", json!({"protocolVersion":"2024-11-05","clientInfo":{"name":"t","version":"1"},"capabilities":{}}));
        bridge
    }
    fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        writeln!(self.stdin, "{}", json!({"jsonrpc":"2.0","id":self.next,"method":method,"params":params})).unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }
    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let reply = self.rpc("tools/call", json!({"name":name,"arguments":arguments}));
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(reply["result"]["isError"], false, "{name}: {text}");
        serde_json::from_str(text).unwrap()
    }
    /// Closes stdin, as a client quitting does, and leaves the process to exit.
    fn close(self) -> Child {
        drop(self.stdin);
        self.child
    }
    fn finish(self) -> bool {
        self.close().wait().unwrap().success()
    }
}
fn call(root: &std::path::Path, tool: &str, arguments: &str) -> Output {
    bin().arg("--profile").arg(root).args(["call", tool, arguments]).output().unwrap()
}
fn lock_pid(root: &std::path::Path) -> i32 {
    std::fs::read_to_string(root.join(".lock")).unwrap().split('@').next().unwrap().parse().unwrap()
}

#[test]
fn a_second_bridge_attaches_to_the_first_ones_session() {
    let root = profile("attach");
    let mut first = Bridge::start(&root, &[]);
    let created = first.tool("sequence_create", json!({"lengthMs":5000}));
    let id = created["sequenceId"].as_str().unwrap().to_string();
    // Before #14 this bridge failed with "the profile is already locked".
    let mut second = Bridge::start(&root, &[]);
    let status = second.tool("sequence_status", json!({"sequenceId":id}));
    assert_eq!(status["lengthMs"], 5000.0, "{status}");
    assert_eq!(second.tool("studio_status", json!({}))["sharedWithApp"], false);
    // The host stays up while an attached client still works, then leaves no lock or endpoint behind.
    let mut first = first.close();
    let status = second.tool("sequence_status", json!({"sequenceId":id}));
    assert_eq!(status["lengthMs"], 5000.0);
    assert!(first.try_wait().unwrap().is_none(), "the host left while a client was attached");
    assert!(second.finish());
    assert!(first.wait().unwrap().success());
    assert!(!root.join(".lock").exists());
    assert!(!studio_mcp::socket::socket_path(&root).unwrap().exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn call_starts_one_persistent_session_for_many_short_lived_clients() {
    let root = profile("call");
    let created = call(&root, "sequence_create", r#"{"lengthMs":7000}"#);
    assert!(created.status.success(), "{}", String::from_utf8_lossy(&created.stderr));
    let id = serde_json::from_slice::<Value>(&created.stdout).unwrap()["sequenceId"].as_str().unwrap().to_string();
    let host = lock_pid(&root);
    // Separate processes, one unsaved document.
    let status = call(&root, "sequence_status", &json!({"sequenceId":id}).to_string());
    assert!(status.status.success(), "{}", String::from_utf8_lossy(&status.stderr));
    assert_eq!(serde_json::from_slice::<Value>(&status.stdout).unwrap()["lengthMs"], 7000.0);
    // An MCP client through --persist joins the same session.
    let mut persistent = Bridge::start(&root, &["--persist"]);
    assert_eq!(persistent.tool("sequence_status", json!({"sequenceId":id}))["lengthMs"], 7000.0);
    assert!(persistent.finish());
    assert_eq!(lock_pid(&root), host, "one session serves every client");
    // A tool's refusal exits 1 with its message on stderr.
    let refused = call(&root, "sequence_status", r#"{"sequenceId":"nope"}"#);
    assert_eq!(refused.status.code(), Some(1));
    assert!(refused.stdout.is_empty() && !refused.stderr.is_empty());
    assert!(!call(&root, "sequence_status", "[1]").status.success());
    // A stop signal ends the session and removes its lock and endpoint.
    // SAFETY: the pid is the session this test started.
    assert_eq!(unsafe { libc::kill(host, libc::SIGTERM) }, 0);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while root.join(".lock").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(!root.join(".lock").exists());
    assert!(!studio_mcp::socket::socket_path(&root).unwrap().exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn serve_exits_after_its_idle_time_and_refuses_a_served_profile() {
    let root = profile("serve");
    let mut serve = bin().arg("--profile").arg(&root).args(["--serve", "--idle", "2"]).stdin(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
    let path = studio_mcp::socket::socket_path(&root).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !path.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let again = bin().arg("--profile").arg(&root).arg("--serve").output().unwrap();
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("already being served"));
    assert!(call(&root, "studio_status", "{}").status.success());
    assert!(serve.wait().unwrap().success());
    assert!(!root.join(".lock").exists() && !path.exists());
    std::fs::remove_dir_all(root).unwrap();
}
