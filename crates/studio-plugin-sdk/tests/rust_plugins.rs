//! The Rust example plugin and the Rust template, as `cargo test` builds them (the `[[example]]`s in Cargo.toml), pass
//! Studio's conformance harness with frames; the example POSTs to a local test server, never anywhere else.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use studio_plugins::harness::{self, Options};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// A program cargo built from one of this crate's `[[example]]`s.
fn built(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let path = exe.parent().unwrap().parent().unwrap().join("examples").join(name);
    assert!(path.is_file(), "{} wasn't built; run cargo test -p vixen-plugin-sdk", path.display());
    path
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vixen-rust-plugin-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn install_program(dir: &Path, program: &Path, name: &str) {
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::copy(program, dir.join("bin").join(name)).unwrap();
}

fn live() -> Options {
    Options { live_output: true, burst: Duration::from_millis(600), ..Options::default() }
}

#[test]
fn rust_http_output_posts_the_colours() {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in server.incoming() {
            let Ok(mut stream) = stream else { return };
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            // Read the head, then as much body as Content-Length says.
            while let Ok(n) = stream.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let length = head.lines().find_map(|l| l.strip_prefix("Content-Length: ")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                    if body.len() >= length {
                        break;
                    }
                }
            }
            let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
            let _ = tx.send(String::from_utf8_lossy(&request).into_owned());
        }
    });

    let source = repo().join("studio/plugins/examples/rust-http-output");
    let dir = temp("http");
    let mut manifest: Value = serde_json::from_str(&std::fs::read_to_string(source.join("vixen-plugin.json")).unwrap()).unwrap();
    manifest["contributes"]["controllers"][0]["configSchema"]["properties"]["url"]["default"] = json!(format!("http://127.0.0.1:{port}/lights"));
    std::fs::write(dir.join("vixen-plugin.json"), manifest.to_string()).unwrap();
    install_program(&dir, &built("rust-http-output"), "rust-http-output");

    let report = harness::validate(&dir, &live());
    assert_eq!(report["passed"], json!(true), "{report:#}");
    assert_eq!(report["warnings"], json!(0), "{report:#}");
    let request = rx.recv_timeout(Duration::from_secs(5)).expect("a POST");
    assert!(request.starts_with("POST /lights HTTP/1.1"), "{request}");
    let body: Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
    let lights = body["lights"].as_array().unwrap();
    assert_eq!(lights.len(), 1, "three outputs make one light: {body}");
    let sum: u64 = ["r", "g", "b"].iter().map(|c| lights[0][c].as_u64().unwrap()).sum();
    assert_eq!(sum, 255, "the chase lights one channel: {body}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rust_template_passes_once_built() {
    let parent = temp("template");
    let made = studio_plugins::scaffold::create(&parent, "com.example.rusty", "Rusty Lamp", "rust", "controller", None).unwrap();
    let dir = PathBuf::from(made["folder"].as_str().unwrap());
    let before = harness::validate(&dir, &Options::default());
    assert_eq!(before["passed"], json!(false), "unbuilt: entry.command is missing");
    install_program(&dir, &built("rust-template"), "rusty");
    let report = harness::validate(&dir, &live());
    assert_eq!(report["passed"], json!(true), "{report:#}");
    assert_eq!(report["warnings"], json!(0), "{report:#}");
    std::fs::remove_dir_all(parent).unwrap();
}
