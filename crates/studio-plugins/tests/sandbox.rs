//! A real plugin process under the generated sandbox profile: what its permissions leave out, it can't do.
use serde_json::{json, Value};
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use studio_plugins::logs::{Level, LogBuffer};
use studio_plugins::process::{Launch, PluginProcess};
use studio_plugins::Manifest;

/// A python3 "plugin" that tries each thing named in argv and prints one `<probe> ok|denied` line per try to stderr.
const PROBE: &str = r#"
import os, socket, sys
def attempt(name, action):
    try:
        action()
        print(name, "ok", file=sys.stderr, flush=True)
    except OSError as e:
        print(name, "denied" if e.errno in (1, 13) else "error %s" % e, file=sys.stderr, flush=True)
def write(path):
    with open(path, "w") as f:
        f.write("x")
def read(path):
    with open(path) as f:
        f.read()
def udp():
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.sendto(b"probe", ("127.0.0.1", int(sys.argv[1])))
here = os.path.dirname(os.path.abspath(__file__))
data = os.environ["VIXEN_PLUGIN_DATA_DIR"]
attempt("read-own", lambda: read(os.path.join(here, "main.py")))
attempt("read-outside", lambda: read(sys.argv[2]))
attempt("write-own", lambda: write(os.path.join(here, "written")))
attempt("write-data", lambda: write(os.path.join(data, "state.json")))
attempt("udp", udp)
print("done", file=sys.stderr, flush=True)
"#;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        // Not under the temporary folders: the profile lets every plugin write there.
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("studio-plugins-sandbox-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Scratch(base.canonicalize().unwrap())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs the probe with `permissions` and returns its result lines and whether the UDP packet arrived.
fn probe(name: &str, permissions: Value, outside: &Path) -> (String, bool) {
    let scratch = Scratch::new(name);
    let dir = scratch.0.join("plugin");
    let data = scratch.0.join("data");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.py"), PROBE).unwrap();
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
    let port = receiver.local_addr().unwrap().port().to_string();
    let manifest = Manifest::parse(
        &json!({
            "id": format!("com.example.sandbox-{name}"), "name": "Sandbox probe", "version": "1.0.0", "apiVersion": "^1.0",
            "entry": {"command": "python3", "args": ["main.py", port, outside.to_string_lossy()]},
            "permissions": permissions,
        })
        .to_string(),
    )
    .unwrap();
    let logs = Arc::new(LogBuffer::new(None));
    let launch = Launch { manifest: &manifest, dir: &dir, data_dir: &data, logs: logs.clone(), controllers: None, tools: None };
    let plugin = PluginProcess::spawn(launch).unwrap();
    assert!(plugin.sandbox.is_some(), "plugins start sandboxed");
    let text = || logs.tail(100, Level::Debug).iter().map(|e| e.to_json()["message"].as_str().unwrap_or_default().to_string()).collect::<Vec<_>>().join("\n");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !text().contains("done") {
        assert!(Instant::now() < deadline, "the probe never finished:\n{}", text());
        std::thread::sleep(Duration::from_millis(20));
    }
    let arrived = receiver.recv_from(&mut [0u8; 16]).is_ok();
    plugin.kill("test over");
    (text(), arrived)
}

/// A file the probe must not read: inside the home folder, outside every folder the profile allows.
fn outside_file() -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let path = home.join(format!(".studio-plugins-sandbox-probe-{}", std::process::id()));
    std::fs::write(&path, "secret").unwrap();
    path
}

#[test]
fn permissions_draw_the_walls() {
    if !studio_plugins::sandbox::enabled() {
        eprintln!("sandbox off here; nothing to check");
        return;
    }
    let outside = outside_file();
    let (none, arrived) = probe("none", json!({}), &outside);
    let (granted, granted_arrived) = probe("granted", json!({"network": ["udp:lan"], "files": ["plugin-data"]}), &outside);
    let _ = std::fs::remove_file(&outside);

    for line in ["read-own ok", "read-outside denied", "write-own denied", "write-data denied", "udp denied"] {
        assert!(none.contains(line), "no permissions: expected {line} in\n{none}");
    }
    assert!(!arrived, "a plugin without network permission sent a packet");
    for line in ["read-own ok", "read-outside denied", "write-own denied", "write-data ok", "udp ok"] {
        assert!(granted.contains(line), "udp:lan + plugin-data: expected {line} in\n{granted}");
    }
    assert!(granted_arrived, "the udp:lan plugin's packet never arrived");
}
