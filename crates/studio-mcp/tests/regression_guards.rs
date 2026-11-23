//! Regression guards through the real MCP server (roadmap R44): how long the sample show's busiest sequence takes to open
//! and render, and how much memory each agent edit costs. Both run on `target/show-copy` and are skipped without it.
//!
//! The budgets are several times what a healthy build measures on the Mac mini, so a busy machine doesn't trip them;
//! the regressions they guard against cost far more than that (e8a531dbc's re-queued renders made 200 edits take 4.2 GB
//! instead of 455 MB).
mod support;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

/// The sample show's sequence with the most effects (1,440 on the full display).
const OPEN_SEQUENCE: &str = "Wizards in Winter.tim";
/// Opening and rendering it takes about 50 ms in a test build; twenty times that has gone wrong somewhere.
const OPEN_BUDGET: Duration = Duration::from_secs(1);
/// Megatree size and edit count for the memory guard: 200 lights, 150 back-to-back effect_add calls.
const STRINGS: u64 = 4;
const PIXELS: u64 = 50;
const EDITS: u64 = 150;
/// Resident memory each edit may add once rendered. A healthy build adds well under 100 KB; e8a531dbc's bug added
/// several MB per edit at this size.
const BYTES_PER_EDIT_BUDGET: u64 = 1024 * 1024;

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

fn sample_show() -> Option<PathBuf> {
    let show = support::target_dir().join("show-copy");
    if show.join("SystemData").is_dir() {
        Some(show)
    } else {
        eprintln!("skipped: no target/show-copy");
        None
    }
}

/// A profile under target/ holding the sample show's display, and `sequence` from it if given. The show itself is only read.
fn profile(name: &str, sequence: Option<&str>) -> Option<PathBuf> {
    let show = sample_show()?;
    let root = show.parent().unwrap().join(format!("studio-mcp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    copy_dir(&show.join("SystemData"), &root.join("SystemData"));
    copy_dir(&show.join("Module Data Files"), &root.join("Module Data Files"));
    if let Some(file) = sequence {
        std::fs::create_dir_all(root.join("Sequence")).unwrap();
        std::fs::copy(show.join("Sequence").join(file), root.join("Sequence").join(file)).unwrap();
    }
    Some(root)
}

struct Server {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next: u64,
    root: PathBuf,
    endpoint: PathBuf,
}

impl Server {
    fn start(root: PathBuf) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_vixen-studio-mcp"))
            .args(["--profile", root.to_str().unwrap()])
            .env("VIXEN_MUTE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let endpoint = studio_mcp::socket::socket_path(&root).unwrap();
        let mut server = Server { child, input, output, next: 1, root, endpoint };
        server.rpc("initialize", json!({"protocolVersion":"2024-11-05","clientInfo":{},"capabilities":{}}));
        server
    }

    fn rpc(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        writeln!(self.input, "{}", json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})).unwrap();
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    }

    fn call(&mut self, name: &str, args: Value) -> Value {
        let result = self.rpc("tools/call", json!({"name":name,"arguments":args}));
        let text = result["content"][0]["text"].as_str().unwrap();
        assert_ne!(result["isError"], true, "{name}: {text}");
        serde_json::from_str(text).unwrap()
    }

    /// The server's resident memory, from `ps`.
    fn resident_bytes(&self) -> u64 {
        let out = Command::new("ps").args(["-o", "rss=", "-p", &self.child.id().to_string()]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().parse::<u64>().unwrap() * 1024
    }
}

impl Drop for Server {
    /// A stop signal, so the bridge removes its lock and endpoint (SIGKILL left an orphaned socket per run, B82); a kill
    /// only if it doesn't exit within five seconds.
    fn drop(&mut self) {
        // SAFETY: the pid is this test's own running child.
        unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(5);
        while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
        if !std::thread::panicking() {
            assert!(!self.endpoint.exists(), "the bridge left its MCP socket behind");
        }
    }
}

#[test]
fn the_sample_shows_busiest_sequence_opens_and_renders_within_budget() {
    let Some(root) = profile("open-time", Some(OPEN_SEQUENCE)) else { return };
    let mut server = Server::start(root);
    let started = Instant::now();
    let opened = server.call("sequence_open", json!({"file":OPEN_SEQUENCE}));
    let id = opened["sequenceId"].clone();
    let rendered = server.call("sequence_render", json!({"sequenceId":id}));
    let took = started.elapsed();
    eprintln!("{OPEN_SEQUENCE}: opened and rendered {} effects in {took:?}", rendered["rendered"]);
    assert!(rendered["rendered"].as_u64().unwrap() > 1000, "{rendered}");
    assert!(took < OPEN_BUDGET, "opening and rendering {OPEN_SEQUENCE} took {took:?}, over the {OPEN_BUDGET:?} budget");
}

#[test]
fn each_agent_edit_costs_bounded_memory() {
    let Some(root) = profile("edit-memory", None) else { return };
    let mut server = Server::start(root);
    let group = server.call("element_template", json!({"kind":"megatree","name":"Guard Tree","count":STRINGS,"pixels":PIXELS}))["result"]["id"].clone();
    let id = server.call("sequence_create", json!({"lengthMs":(EDITS + 1) * 450}))["sequenceId"].clone();
    let catalog = server.call("effect_catalog", json!({"limit":500}));
    let pulse = catalog["items"].as_array().unwrap().iter().find(|e| e["name"] == "Pulse").expect("Pulse effect")["id"].clone();
    let add = |server: &mut Server, i: u64| {
        server.call("effect_add", json!({"sequenceId":id,"typeId":pulse,"targetId":group,"startMs":i * 450,"durationMs":400}));
    };

    // One effect, rendered on its own, gives the schedule size of one edit and a baseline once the render threads are warm.
    add(&mut server, 0);
    let one = server.call("sequence_render", json!({"sequenceId":id}))["scheduleEntries"].as_u64().unwrap();
    assert!(one > 0);
    let before = server.resident_bytes();

    // Back-to-back edits, as an agent sends them, without waiting for renders in between.
    for i in 1..=EDITS {
        add(&mut server, i);
    }
    let all = server.call("sequence_render", json!({"sequenceId":id}));
    let after = server.resident_bytes();
    let per_edit = after.saturating_sub(before) / EDITS;
    eprintln!("{EDITS} edits: {} -> {} MB resident, {per_edit} bytes per edit, {} schedule entries", before >> 20, after >> 20, all["scheduleEntries"]);
    assert!(per_edit < BYTES_PER_EDIT_BUDGET, "each agent edit added {per_edit} bytes, over the {BYTES_PER_EDIT_BUDGET} budget");
    assert_eq!(all["scheduleEntries"].as_u64().unwrap(), one * (EDITS + 1), "every effect is scheduled exactly once");
}
