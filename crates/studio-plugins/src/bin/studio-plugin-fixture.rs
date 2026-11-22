//! A tiny plugin for the host's tests. Its arguments pick a behaviour:
//! `normal` (default), `exit-after-init`, `garbage`, `silent`, `old-api`, `crash`, plus `--storage` (exercise
//! storage.set/get after initialize) and `--stubborn` (ignore shutdown and end of input).
//!
//! It is also a controller plugin: it logs every `controller.*` request and frame to stderr, adds `"seen": true` to the
//! configuration it is given, refuses a configuration containing `"bad"` with field errors, and with `--report` sends
//! status, stats and a message after `controller.start`. `--slow-frames` sleeps 50 ms per frame.
use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn send(value: Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.iter().find(|a| !a.starts_with("--")).map(String::as_str).unwrap_or("normal");
    let storage = args.iter().any(|a| a == "--storage");
    let stubborn = args.iter().any(|a| a == "--stubborn");
    let report = args.iter().any(|a| a == "--report");
    let slow_frames = args.iter().any(|a| a == "--slow-frames");
    if mode == "crash" {
        eprintln!("fixture crashing at start");
        std::process::exit(7);
    }
    let stdin = std::io::stdin();
    let mut next_id = 100;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let id = message.get("id").cloned();
        match method {
            "initialize" => {
                if mode == "silent" {
                    continue;
                }
                let api = if mode == "old-api" { "2.0.0" } else { "1.0.0" };
                send(json!({"jsonrpc": "2.0", "id": id, "result": {"apiVersion": api, "capabilities": {"fixture": true}}}));
                let params = &message["params"];
                eprintln!("fixture ready muted={} data={}", params["muted"], params["pluginDataDir"].as_str().unwrap_or(""));
                eprintln!("env SECRET={}", std::env::var("STUDIO_PLUGIN_TEST_SECRET").unwrap_or_else(|_| "unset".into()));
                send(json!({"jsonrpc": "2.0", "method": "log", "params": {"level": "warn", "message": "hello from fixture"}}));
                if storage {
                    send(json!({"jsonrpc": "2.0", "id": next_id, "method": "storage.set", "params": {"key": "count", "value": 41}}));
                    send(json!({"jsonrpc": "2.0", "id": next_id + 1, "method": "storage.get", "params": {"key": "count"}}));
                    next_id += 2;
                }
                match mode {
                    "exit-after-init" => {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        std::process::exit(3);
                    }
                    "garbage" => {
                        let mut out = std::io::stdout().lock();
                        let _ = writeln!(out, "hello, this is a print statement");
                        let _ = out.flush();
                    }
                    _ => {}
                }
            }
            "shutdown" => {
                if stubborn {
                    continue;
                }
                send(json!({"jsonrpc": "2.0", "id": id, "result": {}}));
                std::process::exit(0);
            }
            "event" => eprintln!("event {} {}", message["params"]["name"], message["params"]["data"]),
            "controller.frame" => {
                let p = &message["params"];
                eprintln!("frame {} seq={} {} {}", p["instanceId"].as_str().unwrap_or(""), p["seq"], p["encoding"].as_str().unwrap_or(""), p["values"]);
                if slow_frames {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            "controller.create" | "controller.configure" => {
                let p = &message["params"];
                let instance = p["instanceId"].as_str().unwrap_or("");
                eprintln!("{method} {instance} outputs={} config={}", p["outputCount"], p["config"]);
                if p["config"].get("bad").is_some() {
                    send(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32004, "message": "bad is not allowed", "data": {"errors": [{"field": "/bad", "message": "is not allowed"}]}}}));
                } else {
                    let mut config = p["config"].clone();
                    config["seen"] = json!(true);
                    send(json!({"jsonrpc": "2.0", "id": id, "result": {"config": config}}));
                }
            }
            "controller.start" | "controller.stop" | "controller.pause" | "controller.resume" | "controller.destroy" | "controller.setOutputCount" => {
                let p = &message["params"];
                let instance = p["instanceId"].as_str().unwrap_or("").to_string();
                eprintln!("{method} {instance} outputs={}", p["outputCount"]);
                send(json!({"jsonrpc": "2.0", "id": id, "result": {}}));
                if report && method == "controller.start" {
                    send(json!({"jsonrpc": "2.0", "method": "controller.stats", "params": {"instanceId": instance, "packetsSent": 10, "bytesSent": 300, "errors": 1}}));
                    send(json!({"jsonrpc": "2.0", "method": "controller.message", "params": {"instanceId": instance, "title": "Lamp", "text": "found 2 lamps"}}));
                    send(json!({"jsonrpc": "2.0", "method": "controller.status", "params": {"instanceId": instance, "state": "unavailable", "message": "lamp 2 is offline"}}));
                    send(json!({"jsonrpc": "2.0", "id": 900, "method": "controller.status", "params": {"instanceId": "not-mine", "state": "running"}}));
                }
            }
            "" => {
                // A response to one of our requests.
                if let Some(result) = message.get("result") {
                    if let Some(value) = result.get("value") {
                        eprintln!("storage got {value}");
                    }
                } else if let Some(error) = message.get("error") {
                    eprintln!("host error {}", error["message"]);
                }
            }
            other => {
                if id.is_some() {
                    send(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("fixture doesn't know {other}")}}));
                }
            }
        }
    }
    if stubborn {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
}
