//! The example plugins in studio/plugins/examples pass the conformance harness with frames (live output to local test
//! receivers only), their SDK copies match the SDKs, and the CLI's validate/pack work on them. The Rust example is
//! tested from crates/studio-plugin-sdk, which builds it.
use serde_json::{json, Value};
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::time::Duration;
use studio_plugins::harness::{self, Options};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn example(name: &str) -> PathBuf {
    repo().join("studio/plugins/examples").join(name)
}

/// A copy of an example in a throwaway folder, its manifest changed by `edit`.
fn copy(name: &str, edit: impl FnOnce(&mut Value)) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vixen-example-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for entry in std::fs::read_dir(example(name)).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            std::fs::copy(entry.path(), dir.join(entry.file_name())).unwrap();
        }
    }
    let path = dir.join("vixen-plugin.json");
    let mut manifest: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    edit(&mut manifest);
    std::fs::write(&path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    dir
}

fn live() -> Options {
    Options { live_output: true, burst: Duration::from_millis(600), ..Options::default() }
}

fn check<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["checks"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap_or_else(|| panic!("no check {name}: {report:#}"))
}

fn available(program: &str) -> bool {
    studio_plugins::process::resolve_command(Path::new("/"), program).is_ok()
}

#[test]
fn sdk_copies_match_the_sdks() {
    let repo = repo();
    let read = |p: &str| std::fs::read_to_string(repo.join(p)).unwrap_or_else(|e| panic!("{p}: {e}"));
    for (canonical, copies) in [
        ("studio/plugins/sdk/python/vixen_plugin.py", &["studio/plugins/examples/console-output/vixen_plugin.py"][..]),
        ("studio/plugins/sdk/typescript/vixen-plugin.mjs", &["studio/plugins/examples/udp-pixel-output/vixen-plugin.mjs"][..]),
        ("studio/plugins/sdk/typescript/vixen-plugin.d.ts", &["studio/plugins/examples/udp-pixel-output/vixen-plugin.d.ts"][..]),
        (
            "crates/studio-plugin-sdk/src/lib.rs",
            &["studio/plugins/examples/rust-http-output/src/vixen_plugin_sdk.rs", "studio/plugins/templates/rust/src/vixen_plugin_sdk.rs"][..],
        ),
    ] {
        for copy in copies {
            assert!(read(canonical) == read(copy), "{copy} differs from {canonical}: copy the SDK over it (cp {canonical} {copy})");
        }
    }
}

/// Runs `code` with `program` in the SDK's folder and returns what it printed.
fn run_in(program: &str, args: &[&str], dir: &str) -> String {
    let out = std::process::Command::new(program).args(args).current_dir(repo().join(dir)).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// `stats()` takes what was sent since the last call and sends running totals, which the host keeps (it raises its
/// counters to them, so per-interval counts would show only the largest single report).
#[test]
fn sdk_stats_send_running_totals() {
    let want = json!({"instanceId": "i", "packetsSent": 3, "bytesSent": 150, "errors": 1});
    if available("python3") {
        let code = "import json, vixen_plugin as vp\n\
                    class P:\n    def notify(self, method, params): self.last = params\n\
                    p = P(); c = vp.Controller(p, 'i', 't', 3)\n\
                    c.stats(1, 100); c.stats(2, 50, 1)\nprint(json.dumps(p.last))";
        let sent: Value = serde_json::from_str(&run_in("python3", &["-c", code], "studio/plugins/sdk/python")).unwrap();
        assert_eq!(sent, want, "Python SDK");
    }
    if available("node") {
        let code = "import { Controller } from './vixen-plugin.mjs'; let last; const c = new Controller({ notify: (m, p) => (last = p) }, 'i', 't', 3); \
                    c.stats({ packetsSent: 1, bytesSent: 100 }); c.stats({ packetsSent: 2, bytesSent: 50, errors: 1 }); console.log(JSON.stringify(last));";
        let sent: Value = serde_json::from_str(&run_in("node", &["--input-type=module", "-e", code], "studio/plugins/sdk/typescript")).unwrap();
        assert_eq!(sent, want, "TypeScript SDK");
    }
}

#[test]
fn console_output_passes_with_frames() {
    if !available("python3") {
        eprintln!("skipped: python3 isn't installed");
        return;
    }
    let report = harness::validate(&example("console-output"), &live());
    assert_eq!(report["passed"], json!(true), "{report:#}");
    assert_eq!(report["warnings"], json!(0), "{report:#}");
    assert_eq!(check(&report, "controller.configure (invalid) Console (example)")["status"], json!("pass"));
    let logs = report["logs"].to_string();
    assert!(logs.contains("console #"), "printed frames: {logs}");
    assert!(report["reported"]["statuses"].to_string().contains("running"), "{report:#}");
}

#[test]
fn udp_pixel_output_sends_the_frames_it_gets() {
    if !available("node") {
        eprintln!("skipped: node isn't installed");
        return;
    }
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let port = receiver.local_addr().unwrap().port();
    let dir = copy("udp-pixel-output", |m| {
        let properties = &mut m["contributes"]["controllers"][0]["configSchema"]["properties"];
        properties["port"]["default"] = json!(port);
        properties["colorOrder"]["default"] = json!("grb");
        m["contributes"]["controllers"][0]["defaultOutputs"] = json!(6);
    });
    let report = harness::validate(&dir, &live());
    assert_eq!(report["passed"], json!(true), "{report:#}");
    assert_eq!(report["warnings"], json!(0), "{report:#}");
    let mut packet = [0u8; 64];
    let (size, _) = receiver.recv_from(&mut packet).expect("a datagram");
    assert_eq!(size, 6, "one byte per output");
    // The harness's chase lights one output at a time; in GRB order a pixel's red goes second.
    let lit: Vec<usize> = packet[..size].iter().enumerate().filter(|(_, b)| **b == 255).map(|(i, _)| i).collect();
    assert_eq!(lit.len(), 1, "{:?}", &packet[..size]);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cli_validates_and_packs_an_example() {
    if !available("python3") {
        eprintln!("skipped: python3 isn't installed");
        return;
    }
    let dir = copy("console-output", |_| {});
    let report = harness::validate(&dir, &Options::default());
    let text = studio_plugins::cli::describe_report(&report);
    assert!(text.contains("PASS manifest") && text.contains("Passed:"), "{text}");
    assert!(text.contains("skip controller.start Console (example)"), "{text}");
    let out = std::env::temp_dir().join(format!("vixen-example-packs-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let args: Vec<String> = ["pack", dir.to_str().unwrap(), "--out", out.to_str().unwrap()].iter().map(|s| s.to_string()).collect();
    assert_eq!(studio_plugins::cli::main(&args, no_profiles), 0);
    assert!(out.join("com.example.console-output-1.0.0.vixenplugin").is_file());
    assert!(out.join("com.example.console-output-1.0.0.vixenplugin.sha256").is_file());
    let bad: Vec<String> = ["validate", "/no/such/folder"].iter().map(|s| s.to_string()).collect();
    assert_eq!(studio_plugins::cli::main(&bad, no_profiles), 2);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(out).unwrap();
}

#[test]
fn mcp_serves_the_examples_and_sdks() {
    let root = std::env::temp_dir().join(format!("vixen-example-mcp-{}", std::process::id()));
    let host = studio_plugins::PluginHost::new(&root, studio_plugins::HostOptions::default()).unwrap();
    let uris: Vec<String> = studio_plugins::mcp::resources(&host).iter().filter_map(|r| r["uri"].as_str().map(str::to_string)).collect();
    for language in ["python", "typescript", "rust"] {
        assert!(uris.contains(&format!("plugins://examples/{language}")) && uris.contains(&format!("plugins://sdk/{language}")), "{uris:?}");
        let example = studio_plugins::mcp::read_resource(&host, &format!("plugins://examples/{language}")).unwrap().unwrap();
        assert!(example[0]["text"].as_str().unwrap().contains("## vixen-plugin.json"));
        let sdk = studio_plugins::mcp::read_resource(&host, &format!("plugins://sdk/{language}")).unwrap().unwrap();
        assert!(sdk[0]["text"].as_str().unwrap().contains("plugin API 1.0"));
    }
    assert!(studio_plugins::mcp::read_resource(&host, "plugins://sdk/cobol").unwrap().is_err());
    drop(host);
    let _ = std::fs::remove_dir_all(root);
}

/// Each SDK's configuration check answers exactly what Studio's (`schema::validate`) does, field and message.
#[test]
fn sdk_config_checks_agree_with_studio() {
    let schema = json!({
        "type": "object", "additionalProperties": false, "required": ["host", "port"],
        "properties": {
            "host": {"type": "string", "format": "hostname", "maxLength": 20},
            "ip": {"type": "string", "format": "ipv4"},
            "port": {"type": "integer", "minimum": 1, "maximum": 65535},
            "gain": {"type": "number", "minimum": 0.5, "maximum": 2.5},
            "order": {"enum": ["rgb", "grb"]},
            "on": {"type": "boolean"},
            "fixed": {"const": 3},
            "devices": {"type": "array", "maxItems": 2, "items": {"type": "object", "required": ["id"], "properties": {"id": {"type": "string", "minLength": 2}}}},
            "a/b": {"type": ["string", "null"]},
            "extra": {"type": "object", "additionalProperties": {"type": "integer"}}
        }
    });
    let cases = json!([
        {"host": "lamp.local", "port": 80},
        {"host": "-bad-", "port": 0, "nope": true},
        {"host": "x", "port": 1.5, "gain": 3, "order": "bgr", "on": 1, "fixed": 4},
        {"host": "x", "port": 2, "ip": "10.0.0.256", "devices": [{"id": "a"}, {}, {"id": "ok"}]},
        {"host": "x", "port": 2, "ip": "01.2.3.4", "a/b": 5, "extra": {"k": "v", "n": 2}},
        {"host": "aaaaaaaaaaaaaaaaaaaaaa", "port": 65536, "gain": 1.0},
        []
    ]);
    let expected: Vec<Value> = cases
        .as_array()
        .unwrap()
        .iter()
        .map(|case| Value::Array(studio_plugins::schema::validate(&schema, case).iter().map(|e| json!({"field": e.field, "message": e.message})).collect()))
        .collect();
    let sorted = |list: &Value| {
        let mut items: Vec<String> = list.as_array().unwrap().iter().map(Value::to_string).collect();
        items.sort();
        items
    };
    let rust: Vec<Value> = cases
        .as_array()
        .unwrap()
        .iter()
        .map(|case| Value::Array(vixen_plugin_sdk_validate(&schema, case)))
        .collect();
    let mut answers = vec![("rust", Value::Array(rust))];
    let input = json!({"schema": schema, "cases": cases}).to_string();
    let sdk = repo().join("studio/plugins/sdk");
    if available("python3") {
        let script = "import json,sys; sys.path.insert(0, sys.argv[1]); import vixen_plugin as vp; d=json.load(sys.stdin); print(json.dumps([vp.validate(d['schema'], c) for c in d['cases']]))";
        answers.push(("python", run(&["python3", "-c", script, sdk.join("python").to_str().unwrap()], &input)));
    }
    if available("node") {
        let module = format!("file://{}", sdk.join("typescript/vixen-plugin.mjs").display());
        let script = format!("import {{ validate }} from '{module}'; let s=''; process.stdin.on('data', c => s += c).on('end', () => {{ const d = JSON.parse(s); console.log(JSON.stringify(d.cases.map(c => validate(d.schema, c)))); }});");
        answers.push(("typescript", run(&["node", "--input-type=module", "-e", &script], &input)));
    }
    for (language, answer) in answers {
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(sorted(&answer[i]), sorted(want), "{language}, case {i}: {}", cases[i]);
        }
    }
}

/// The Rust SDK's check, through its copy in the Rust template (the same file as crates/studio-plugin-sdk).
fn vixen_plugin_sdk_validate(schema: &Value, value: &Value) -> Vec<Value> {
    rust_sdk::validate(schema, value).into_iter().map(|e| json!({"field": e.field, "message": e.message})).collect()
}

#[path = "../../studio-plugin-sdk/src/lib.rs"]
#[allow(dead_code)]
mod rust_sdk;

fn run(command: &[&str], input: &str) -> Value {
    use std::io::Write;
    let mut child = std::process::Command::new(command[0])
        .args(&command[1..])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{command:?} failed");
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{command:?}: {e}: {}", String::from_utf8_lossy(&out.stdout)))
}

fn no_profiles(_: &std::path::Path) -> Result<studio_plugins::process::DisplaySink, String> {
    Err("no profiles in this test".into())
}
