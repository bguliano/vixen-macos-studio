//! vixen-plugin-sdk: the Vixen Studio plugin SDK for Rust (plugin API 1.1). One file, `serde_json` only.
//!
//! Use it as a crate (`vixen-plugin-sdk`, a path or git dependency on `crates/studio-plugin-sdk`) or copy this file
//! into your plugin as `src/vixen_plugin_sdk.rs` and write `mod vixen_plugin_sdk;` (what `plugin new` and
//! `plugin_scaffold` do). It speaks the plugin protocol for you: JSON-RPC 2.0 over stdin/stdout, one message per line,
//! frame decoding, logging, errors and configuration checks.
//!
//! ```ignore
//! use vixen_plugin_sdk::{Controller, Frame, Instance, Plugin, ConfigError};
//! use serde_json::Value;
//!
//! struct Lamp { host: String }
//! impl Controller for Lamp {
//!     fn configure(&mut self, _: &Instance, config: &Value) -> Result<Option<Value>, ConfigError> {
//!         self.host = config["host"].as_str().unwrap_or_default().to_string();
//!         Ok(None)
//!     }
//!     fn frame(&mut self, _: &Instance, frame: &Frame) { send_to_lamp(&self.host, &frame.levels) }
//! }
//! fn main() {
//!     Plugin::new().controller(None, |_| Box::new(Lamp { host: String::new() })).run();
//! }
//! ```
//!
//! Only protocol messages go to stdout; write anything else with `eprintln!` (Studio keeps it in the plugin's log).
//! The SDK checks each configuration against the type's `configSchema` in `vixen-plugin.json` before `configure` sees
//! it (the subset Studio checks) and fills in defaults. Frames are latest-wins: a slow `frame` skips frames rather than
//! falling behind. [`Host`] is `Clone + Send`: log, report status and ask Studio from any thread.
// Copied into a plugin as a module, most of the SDK goes unused; that's expected.
#![allow(dead_code)]

use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub const SDK_VERSION: &str = "1.1.0";
pub const API_VERSION: &str = "1.1.0";
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
pub const INVALID_CONFIG: i64 = -32004;

/// Refuses a configuration: each `{field, message}` is shown next to that field in the inspector.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigError {
    pub errors: Vec<FieldError>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldError {
    /// JSON Pointer of the setting; `/` for the whole configuration.
    pub field: String,
    pub message: String,
}

impl ConfigError {
    pub fn new(field: &str, message: impl Into<String>) -> ConfigError {
        ConfigError { errors: vec![FieldError { field: field.to_string(), message: message.into() }] }
    }

    fn describe(&self) -> String {
        let text: Vec<String> = self.errors.iter().map(|e| format!("{} {}", e.field, e.message)).collect();
        if text.is_empty() { "invalid configuration".into() } else { text.join("; ") }
    }
}

/// An error answer from Studio to one of the plugin's requests.
#[derive(Clone, Debug)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for RpcError {}

/// One `controller.frame`.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub seq: u64,
    pub time_ms: u64,
    /// `u8`, `u16` or `json`.
    pub encoding: String,
    /// 0–255 per output (u8) or 0–65535 (u16); empty for json.
    pub levels: Vec<u16>,
    /// The items as sent for json (`null`, a level, `{"r","g","b"}` or a command string); empty otherwise.
    pub values: Vec<Value>,
}

impl Frame {
    fn from_params(params: &Value) -> Frame {
        let encoding = params["encoding"].as_str().unwrap_or("u8").to_string();
        let mut frame = Frame { seq: params["seq"].as_u64().unwrap_or(0), time_ms: params["timeMs"].as_u64().unwrap_or(0), ..Frame::default() };
        match encoding.as_str() {
            "u8" => frame.levels = base64_decode(params["values"].as_str().unwrap_or("")).into_iter().map(u16::from).collect(),
            "u16" => frame.levels = base64_decode(params["values"].as_str().unwrap_or("")).chunks_exact(2).map(|p| u16::from_le_bytes([p[0], p[1]])).collect(),
            _ => frame.values = params["values"].as_array().cloned().unwrap_or_default(),
        }
        frame.encoding = encoding;
        frame
    }
}

/// Groups channel levels into `[r, g, b]` per light; `order` names the outputs' colour order (`rgb`, `grb`, …).
pub fn triplets(levels: &[u16], order: &str) -> Vec<[u16; 3]> {
    let order = order.to_ascii_lowercase();
    let index: Vec<usize> = "rgb".chars().map(|c| order.find(c).unwrap_or(0)).collect();
    levels.chunks_exact(3).map(|c| [c[index[0]], c[index[1]], c[index[2]]]).collect()
}

/// One controller instance's code. Every method has a default, so implement only what you need.
pub trait Controller: Send {
    /// The checked configuration, on create and on every edit. `Ok(Some(config))` normalises it.
    fn configure(&mut self, _instance: &Instance, _config: &Value) -> Result<Option<Value>, ConfigError> {
        Ok(None)
    }
    fn set_output_count(&mut self, _instance: &Instance, _count: usize) {}
    /// Live output began: connect to the device. An `Err` is Studio's error for `controller.start`.
    fn start(&mut self, _instance: &Instance) -> Result<(), String> {
        Ok(())
    }
    /// Live output ended: disconnect.
    fn stop(&mut self, _instance: &Instance) {}
    /// New channel values; called only while started and not paused.
    fn frame(&mut self, _instance: &Instance, _frame: &Frame) {}
    fn destroy(&mut self, _instance: &Instance) {}
    /// Optional output names, sent with create's answer.
    fn output_names(&self, _instance: &Instance) -> Option<Vec<String>> {
        None
    }
}

/// What a controller knows about itself, and its reports to Studio.
#[derive(Clone)]
pub struct Instance {
    pub host: Host,
    pub instance_id: String,
    pub type_id: String,
    pub output_count: usize,
    pub config: Value,
    pub running: bool,
    pub paused: bool,
    /// Packets, bytes and errors reported so far, shared by every clone of this instance.
    counted: Arc<Mutex<[u64; 3]>>,
}

impl Instance {
    /// `state`: `running`, `unavailable` or `error`; shown as the controller's health.
    pub fn status(&self, state: &str, message: Option<&str>) {
        let mut params = json!({"instanceId": self.instance_id, "state": state});
        if let Some(message) = message {
            params["message"] = json!(message);
        }
        self.host.notify("controller.status", params);
    }

    /// Adds to the counters `controller_overview` shows: pass what you sent since the last call. The SDK keeps the
    /// totals and sends them, as `controller.stats` wants.
    pub fn stats(&self, packets_sent: u64, bytes_sent: u64, errors: u64) {
        self.host.notify("controller.stats", self.count(packets_sent, bytes_sent, errors));
    }

    fn count(&self, packets_sent: u64, bytes_sent: u64, errors: u64) -> Value {
        let mut counted = self.counted.lock().unwrap_or_else(|p| p.into_inner());
        for (total, add) in counted.iter_mut().zip([packets_sent, bytes_sent, errors]) {
            *total = total.saturating_add(add);
        }
        json!({"instanceId": self.instance_id, "packetsSent": counted[0], "bytesSent": counted[1], "errors": counted[2]})
    }

    /// An alert for the person at Studio; never blocks the show.
    pub fn message(&self, title: &str, text: &str, heading: Option<&str>) {
        let mut params = json!({"instanceId": self.instance_id, "title": title, "text": text});
        if let Some(heading) = heading {
            params["heading"] = json!(heading);
        }
        self.host.notify("controller.message", params);
    }
}

struct Shared {
    out: Mutex<std::io::Stdout>,
    pending: Mutex<HashMap<String, Option<Value>>>,
    answered: Condvar,
    next_id: Mutex<u64>,
    state: Mutex<HostState>,
}

#[derive(Clone, Default)]
struct HostState {
    muted: bool,
    data_dir: Option<String>,
    profile: Value,
    capabilities: Value,
}

/// The connection to Studio. Clone it freely; every method works from any thread.
#[derive(Clone)]
pub struct Host {
    shared: Arc<Shared>,
}

impl Host {
    fn new() -> Host {
        let state = HostState { muted: true, data_dir: std::env::var("VIXEN_PLUGIN_DATA_DIR").ok(), profile: Value::Null, capabilities: Value::Null };
        Host {
            shared: Arc::new(Shared {
                out: Mutex::new(std::io::stdout()),
                pending: Mutex::new(HashMap::new()),
                answered: Condvar::new(),
                next_id: Mutex::new(0),
                state: Mutex::new(state),
            }),
        }
    }

    pub fn send(&self, mut message: Value) {
        message["jsonrpc"] = json!("2.0");
        let mut line = serde_json::to_string(&message).unwrap_or_default();
        line.push('\n');
        let mut out = self.shared.out.lock().unwrap_or_else(|p| p.into_inner());
        let _ = out.write_all(line.as_bytes());
        let _ = out.flush();
    }

    pub fn notify(&self, method: &str, params: Value) {
        self.send(json!({"method": method, "params": params}));
    }

    /// Asks Studio and waits for the answer.
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, RpcError> {
        let id = {
            let mut next = self.shared.next_id.lock().unwrap_or_else(|p| p.into_inner());
            *next += 1;
            format!("p{next}")
        };
        self.shared.pending.lock().unwrap_or_else(|p| p.into_inner()).insert(id.clone(), None);
        self.send(json!({"id": id, "method": method, "params": params}));
        let deadline = Instant::now() + timeout;
        let mut pending = self.shared.pending.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if let Some(Some(_)) = pending.get(&id) {
                let reply = pending.remove(&id).flatten().unwrap_or(Value::Null);
                if let Some(error) = reply.get("error") {
                    return Err(RpcError {
                        code: error["code"].as_i64().unwrap_or(INTERNAL_ERROR),
                        message: error["message"].as_str().unwrap_or("error").to_string(),
                        data: error.get("data").cloned(),
                    });
                }
                return Ok(reply.get("result").cloned().unwrap_or(Value::Null));
            }
            let now = Instant::now();
            if now >= deadline {
                pending.remove(&id);
                return Err(RpcError { code: -32001, message: format!("Studio didn't answer {method} within {} ms", timeout.as_millis()), data: None });
            }
            pending = self.shared.answered.wait_timeout(pending, deadline - now).unwrap_or_else(|p| p.into_inner()).0;
        }
    }

    fn answered(&self, id: String, reply: Value) {
        let mut pending = self.shared.pending.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(slot) = pending.get_mut(&id) {
            *slot = Some(reply);
            self.shared.answered.notify_all();
        }
    }

    pub fn log(&self, level: &str, message: impl Into<String>) {
        self.notify("log", json!({"level": level, "message": message.into()}));
    }
    pub fn debug(&self, message: impl Into<String>) {
        self.log("debug", message);
    }
    pub fn info(&self, message: impl Into<String>) {
        self.log("info", message);
    }
    pub fn warn(&self, message: impl Into<String>) {
        self.log("warn", message);
    }
    pub fn error(&self, message: impl Into<String>) {
        self.log("error", message);
    }

    /// A small JSON value kept in pluginDataDir/state.json (`Value::Null` when unset). Needs `"files": ["plugin-data"]`.
    pub fn storage_get(&self, key: &str) -> Result<Value, RpcError> {
        Ok(self.request("storage.get", json!({"key": key}), Duration::from_secs(5))?.get("value").cloned().unwrap_or(Value::Null))
    }

    pub fn storage_set(&self, key: &str, value: Value) -> Result<(), RpcError> {
        self.request("storage.set", json!({"key": key, "value": value}), Duration::from_secs(5)).map(|_| ())
    }

    /// The open show's elements, controllers and patches, every page gathered into one value (the shape of a
    /// `display.snapshot` page, with `more: false`). Needs `"display": "read"`. Starts over if the show changes mid-way.
    pub fn display_snapshot(&self) -> Result<Value, RpcError> {
        'again: for _ in 0..5 {
            let mut snapshot = self.request("display.snapshot", json!({}), Duration::from_secs(5))?;
            while snapshot["more"] == true {
                let params = json!({"elementOffset": snapshot["elementOffset"], "patchOffset": snapshot["patchOffset"]});
                let page = self.request("display.snapshot", params, Duration::from_secs(5))?;
                if page["revision"] != snapshot["revision"] {
                    continue 'again;
                }
                for key in ["elements", "patches"] {
                    let more = page[key].as_array().cloned().unwrap_or_default();
                    if let Some(list) = snapshot[key].as_array_mut() {
                        list.extend(more);
                    }
                }
                for key in ["more", "elementOffset", "patchOffset"] {
                    snapshot[key] = page[key].clone();
                }
            }
            return Ok(snapshot);
        }
        Err(RpcError { code: -32603, message: "the show kept changing while it was read".into(), data: None })
    }

    /// Replaces the agent tools Studio lists (same shape as `contributes.tools`); `None` goes back to the manifest's.
    pub fn tools_changed(&self, tools: Option<Value>) {
        self.notify("tools.changed", tools.map(|t| json!({"tools": t})).unwrap_or_else(|| json!({})));
    }

    /// True while no real output may flow (headless, Live Output off).
    pub fn muted(&self) -> bool {
        self.shared.state.lock().unwrap_or_else(|p| p.into_inner()).muted
    }

    pub fn data_dir(&self) -> Option<String> {
        self.shared.state.lock().unwrap_or_else(|p| p.into_inner()).data_dir.clone()
    }

    /// `{name, id}` of the open show, or null.
    pub fn profile(&self) -> Value {
        self.shared.state.lock().unwrap_or_else(|p| p.into_inner()).profile.clone()
    }

    /// True when this Studio resizes `"outputCount": "fromConfig"` controllers itself (plugin API 1.1). Before that,
    /// refuse a configuration that needs more outputs than `output_count`, as plugins for API 1.0 did.
    pub fn sets_output_count(&self) -> bool {
        self.shared.state.lock().unwrap_or_else(|p| p.into_inner()).capabilities["controllerOutputs"] == json!(true)
    }
}

/// What a `"outputCount": "fromConfig"` type's configuration needs: the count, and optionally one name per output and
/// the lights the outputs drive (Studio's Single Bulbs makes one element per light from them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outputs {
    pub count: usize,
    pub names: Option<Vec<String>>,
    pub lights: Option<Vec<Light>>,
}

/// One light (device) a controller drives, on the next `outputs` outputs: 1 (single colour), 3 (RGB) or 4 (RGBW).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Light {
    pub name: String,
    pub outputs: usize,
}

impl Outputs {
    /// `count` outputs, unnamed.
    pub fn count(count: usize) -> Outputs {
        Outputs { count, names: None, lights: None }
    }

    /// One output per name.
    pub fn named(names: Vec<String>) -> Outputs {
        Outputs { count: names.len(), names: Some(names), lights: None }
    }

    /// The outputs for these lights in order, with the lights listed: an RGB light's are named `<light> R`, `<light> G`,
    /// `<light> B` (an RGBW light's also `<light> W`), a single-colour light's output is named after it.
    pub fn for_lights(lights: Vec<Light>) -> Outputs {
        let names: Vec<String> = lights
            .iter()
            .flat_map(|l| match l.outputs {
                1 => vec![l.name.clone()],
                n => ["R", "G", "B", "W"].iter().take(n).map(|c| format!("{} {c}", l.name)).collect(),
            })
            .collect();
        let count = lights.iter().map(|l| l.outputs).sum();
        Outputs { count, names: (names.len() == count).then_some(names), lights: Some(lights) }
    }
}

type Factory = Box<dyn Fn(&Instance) -> Box<dyn Controller>>;
type ToolFn = Box<dyn FnMut(&Host, &Value) -> Result<Value, String>>;
type OutputsFn = Box<dyn Fn(&Value) -> Outputs>;
type EventFn = Box<dyn FnMut(&Host, &Value)>;
type ShutdownFn = Box<dyn FnMut(&Host)>;

struct Slot {
    instance: Instance,
    controller: Box<dyn Controller>,
}

enum Inbox {
    Message(Value),
    Frame(String),
    Closed,
}

pub struct Plugin {
    host: Host,
    manifest: Value,
    types: Vec<(Option<String>, Factory)>,
    outputs: Vec<(Option<String>, OutputsFn)>,
    tools: HashMap<String, ToolFn>,
    events: HashMap<String, EventFn>,
    on_initialize: Option<EventFn>,
    on_shutdown: Option<ShutdownFn>,
    slots: HashMap<String, Slot>,
    finished: bool,
}

impl Default for Plugin {
    fn default() -> Plugin {
        Plugin::new()
    }
}

impl Plugin {
    /// Reads `vixen-plugin.json` from the working folder (Studio starts plugins in their folder).
    pub fn new() -> Plugin {
        let manifest = std::fs::read_to_string("vixen-plugin.json").ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null);
        Plugin::with_manifest(manifest)
    }

    pub fn with_manifest(manifest: Value) -> Plugin {
        Plugin { host: Host::new(), manifest, types: Vec::new(), outputs: Vec::new(), tools: HashMap::new(), events: HashMap::new(), on_initialize: None, on_shutdown: None, slots: HashMap::new(), finished: false }
    }

    pub fn host(&self) -> Host {
        self.host.clone()
    }

    /// Serves the controller type `type_id` (its typeId or name; `None` when the manifest declares one type) with
    /// controllers made by `factory`.
    pub fn controller(mut self, type_id: Option<&str>, factory: impl Fn(&Instance) -> Box<dyn Controller> + 'static) -> Plugin {
        let resolved = self.resolve_type(type_id);
        self.types.push((resolved, Box::new(factory)));
        self
    }

    /// For a type with `"outputCount": "fromConfig"`: the outputs a configuration needs (`controller.outputsFor`). No
    /// instance exists (Studio asks while Live Output is off too); the configuration is checked, with defaults filled in.
    pub fn outputs_for(mut self, type_id: Option<&str>, f: impl Fn(&Value) -> Outputs + 'static) -> Plugin {
        let resolved = self.resolve_type(type_id);
        self.outputs.push((resolved, Box::new(f)));
        self
    }

    fn resolve_type(&self, type_id: Option<&str>) -> Option<String> {
        let types = self.manifest["contributes"]["controllers"].as_array().cloned().unwrap_or_default();
        match type_id {
            None if types.len() == 1 => types[0]["typeId"].as_str().map(str::to_string),
            None => None,
            Some(wanted) => Some(types.iter().find(|t| t["typeId"] == wanted || t["name"] == wanted).and_then(|t| t["typeId"].as_str()).unwrap_or(wanted).to_string()),
        }
    }

    /// An agent tool. `Ok(Value::String)` is text; `Ok({"content": […]})` is sent as it is; anything else as JSON text;
    /// `Err` answers with isError.
    pub fn tool(mut self, name: &str, f: impl FnMut(&Host, &Value) -> Result<Value, String> + 'static) -> Plugin {
        self.tools.insert(name.to_string(), Box::new(f));
        self
    }

    /// A declared event (`liveOutput.changed` also updates [`Host::muted`]).
    pub fn event(mut self, name: &str, f: impl FnMut(&Host, &Value) + 'static) -> Plugin {
        self.events.insert(name.to_string(), Box::new(f));
        self
    }

    pub fn on_initialize(mut self, f: impl FnMut(&Host, &Value) + 'static) -> Plugin {
        self.on_initialize = Some(Box::new(f));
        self
    }

    pub fn on_shutdown(mut self, f: impl FnMut(&Host) + 'static) -> Plugin {
        self.on_shutdown = Some(Box::new(f));
        self
    }

    /// Serves Studio until shutdown or stdin closes.
    pub fn run(mut self) {
        let (tx, rx) = channel();
        let frames: Arc<Mutex<HashMap<String, Value>>> = Arc::default();
        let reader_frames = frames.clone();
        let host = self.host.clone();
        std::thread::Builder::new().name("vixen-plugin-reader".into()).spawn(move || read(host, tx, reader_frames)).expect("reader thread");
        self.serve(rx, frames);
    }

    fn serve(&mut self, rx: Receiver<Inbox>, frames: Arc<Mutex<HashMap<String, Value>>>) {
        loop {
            match rx.recv().unwrap_or(Inbox::Closed) {
                Inbox::Closed => {
                    self.finish();
                    return;
                }
                Inbox::Frame(id) => {
                    let params = frames.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
                    if let (Some(params), Some(slot)) = (params, self.slots.get_mut(&id)) {
                        if slot.instance.running && !slot.instance.paused {
                            slot.controller.frame(&slot.instance, &Frame::from_params(&params));
                        }
                    }
                }
                Inbox::Message(message) => {
                    if self.dispatch(message) {
                        return;
                    }
                }
            }
        }
    }

    /// Handles one request or notification; true once shutdown has been answered.
    fn dispatch(&mut self, message: Value) -> bool {
        let method = message["method"].as_str().unwrap_or("").to_string();
        let params = message.get("params").cloned().filter(Value::is_object).unwrap_or_else(|| json!({}));
        let id = message.get("id").cloned().filter(|id| !id.is_null());
        let outcome = self.handle(&method, &params);
        match (id, outcome) {
            (Some(id), Ok(result)) => self.host.send(json!({"id": id, "result": result})),
            (Some(id), Err(error)) => {
                let mut body = json!({"code": error.code, "message": error.message});
                if let Some(data) = error.data {
                    body["data"] = data;
                }
                self.host.send(json!({"id": id, "error": body}));
            }
            (None, Err(error)) => self.host.error(error.message),
            (None, Ok(_)) => {}
        }
        method == "shutdown"
    }

    fn handle(&mut self, method: &str, params: &Value) -> Result<Value, RpcError> {
        let not_found = || RpcError { code: METHOD_NOT_FOUND, message: format!("{method} is not implemented"), data: None };
        match method {
            "initialize" => {
                {
                    let mut state = self.host.shared.state.lock().unwrap_or_else(|p| p.into_inner());
                    state.muted = params["muted"].as_bool().unwrap_or(true);
                    state.profile = params["profile"].clone();
                    state.capabilities = params["capabilities"].clone();
                    if let Some(dir) = params["pluginDataDir"].as_str() {
                        state.data_dir = Some(dir.to_string());
                    }
                }
                if let Some(f) = self.on_initialize.as_mut() {
                    f(&self.host, params);
                }
                return Ok(json!({"apiVersion": API_VERSION, "capabilities": {}}));
            }
            "shutdown" => {
                self.finish(); // before answering: Studio closes stdin once it has the answer
                return Ok(json!({}));
            }
            "event" => {
                let name = params["name"].as_str().unwrap_or("");
                if name == "liveOutput.changed" {
                    if let Some(muted) = params["data"]["muted"].as_bool() {
                        self.host.shared.state.lock().unwrap_or_else(|p| p.into_inner()).muted = muted;
                    }
                }
                if let Some(f) = self.events.get_mut(name) {
                    f(&self.host, &params["data"]);
                }
                return Ok(json!({}));
            }
            "tool.call" => return Ok(self.call_tool(params["name"].as_str().unwrap_or(""), &params["arguments"])),
            "controller.create" => return self.create(params),
            "controller.outputsFor" => return self.answer_outputs(params).ok_or_else(not_found)?,
            _ if !method.starts_with("controller.") => return Err(not_found()),
            _ => {}
        }
        let id = params["instanceId"].as_str().unwrap_or("").to_string();
        let schema = self.schema_for(self.slots.get(&id).map(|s| s.instance.type_id.as_str()).unwrap_or(""));
        let slot = self.slots.get_mut(&id).ok_or_else(|| RpcError { code: INVALID_PARAMS, message: format!("unknown instance {id}"), data: None })?;
        match method {
            "controller.configure" => {
                let config = configure(slot, schema.as_ref(), &params["config"])?;
                slot.instance.config = config.clone();
                Ok(json!({"config": config}))
            }
            "controller.setOutputCount" => {
                let count = params["outputCount"].as_u64().unwrap_or(0) as usize;
                slot.instance.output_count = count;
                slot.controller.set_output_count(&slot.instance, count);
                Ok(json!({}))
            }
            "controller.start" => {
                slot.controller.start(&slot.instance).map_err(|why| RpcError { code: INTERNAL_ERROR, message: why, data: None })?;
                slot.instance.running = true;
                slot.instance.paused = false;
                Ok(json!({}))
            }
            "controller.stop" => {
                slot.instance.running = false;
                slot.controller.stop(&slot.instance);
                Ok(json!({}))
            }
            "controller.pause" => {
                slot.instance.paused = true;
                Ok(json!({}))
            }
            "controller.resume" => {
                slot.instance.paused = false;
                Ok(json!({}))
            }
            "controller.destroy" => {
                let mut slot = self.slots.remove(&id).expect("looked up");
                if slot.instance.running {
                    slot.instance.running = false;
                    slot.controller.stop(&slot.instance);
                }
                slot.controller.destroy(&slot.instance);
                Ok(json!({}))
            }
            _ => Err(not_found()),
        }
    }

    fn schema_for(&self, type_id: &str) -> Option<Value> {
        self.manifest["contributes"]["controllers"].as_array()?.iter().find(|t| t["typeId"] == type_id).map(|t| t["configSchema"].clone())
    }

    /// `None` when no `outputs_for` serves the type.
    fn answer_outputs(&self, params: &Value) -> Option<Result<Value, RpcError>> {
        let type_id = params["typeId"].as_str().unwrap_or("");
        let f = match self.outputs.iter().find(|(t, _)| t.as_deref() == Some(type_id)) {
            Some((_, f)) => f,
            None => match self.outputs.iter().filter(|(t, _)| t.is_none()).collect::<Vec<_>>().as_slice() {
                [(_, only)] => only,
                _ => return None,
            },
        };
        let mut config = if params["config"].is_object() { params["config"].clone() } else { json!({}) };
        if let Some(schema) = self.schema_for(type_id).filter(|s| s.is_object()) {
            config = fill_defaults(&schema, &config);
            let errors = validate(&schema, &config);
            if !errors.is_empty() {
                return Some(Err(config_error(ConfigError { errors })));
            }
        }
        let outputs = f(&config);
        let mut answer = json!({"outputCount": outputs.count});
        if let Some(names) = outputs.names {
            answer["outputNames"] = json!(names);
        }
        if let Some(lights) = outputs.lights {
            answer["lights"] = lights.iter().map(|l| json!({"name": l.name, "outputCount": l.outputs})).collect();
        }
        Some(Ok(answer))
    }

    fn create(&mut self, params: &Value) -> Result<Value, RpcError> {
        let type_id = params["typeId"].as_str().unwrap_or("").to_string();
        let factory = match self.types.iter().find(|(t, _)| t.as_deref() == Some(type_id.as_str())) {
            Some((_, f)) => f,
            None => {
                let untyped: Vec<&Factory> = self.types.iter().filter(|(t, _)| t.is_none()).map(|(_, f)| f).collect();
                match untyped.as_slice() {
                    [only] => *only,
                    _ => return Err(RpcError { code: METHOD_NOT_FOUND, message: format!("no controller registered for type {type_id}"), data: None }),
                }
            }
        };
        let instance = Instance {
            host: self.host.clone(),
            instance_id: params["instanceId"].as_str().unwrap_or("").to_string(),
            type_id: type_id.clone(),
            output_count: params["outputCount"].as_u64().unwrap_or(0) as usize,
            config: json!({}),
            running: false,
            paused: false,
            counted: Arc::default(),
        };
        let controller = factory(&instance);
        let mut slot = Slot { instance, controller };
        let config = configure(&mut slot, self.schema_for(&type_id).as_ref(), &params["config"])?;
        slot.instance.config = config.clone();
        let mut answer = json!({"config": config});
        if let Some(names) = slot.controller.output_names(&slot.instance) {
            answer["outputNames"] = json!(names);
        }
        self.slots.insert(slot.instance.instance_id.clone(), slot);
        Ok(answer)
    }

    fn call_tool(&mut self, name: &str, arguments: &Value) -> Value {
        let text = |text: String, error: bool| json!({"content": [{"type": "text", "text": text}], "isError": error});
        let Some(f) = self.tools.get_mut(name) else {
            return text(format!("unknown tool {name}"), true);
        };
        let arguments = if arguments.is_object() { arguments.clone() } else { json!({}) };
        match f(&self.host, &arguments) {
            Ok(Value::String(s)) => text(s, false),
            Ok(result) if result["content"].is_array() => result,
            Ok(other) => text(other.to_string(), false),
            Err(why) => text(why, true),
        }
    }

    fn finish(&mut self) {
        if std::mem::replace(&mut self.finished, true) {
            return;
        }
        for (_, mut slot) in self.slots.drain() {
            if slot.instance.running {
                slot.instance.running = false;
                slot.controller.stop(&slot.instance);
            }
            slot.controller.destroy(&slot.instance);
        }
        if let Some(f) = self.on_shutdown.as_mut() {
            f(&self.host);
        }
    }
}

fn configure(slot: &mut Slot, schema: Option<&Value>, config: &Value) -> Result<Value, RpcError> {
    let mut config = if config.is_object() { config.clone() } else { json!({}) };
    if let Some(schema) = schema.filter(|s| s.is_object()) {
        config = fill_defaults(schema, &config);
        let errors = validate(schema, &config);
        if !errors.is_empty() {
            return Err(config_error(ConfigError { errors }));
        }
    }
    match slot.controller.configure(&slot.instance, &config) {
        Ok(Some(normalised)) => Ok(normalised),
        Ok(None) => Ok(config),
        Err(error) => Err(config_error(error)),
    }
}

fn config_error(error: ConfigError) -> RpcError {
    let errors: Vec<Value> = error.errors.iter().map(|e| json!({"field": e.field, "message": e.message})).collect();
    RpcError { code: INVALID_CONFIG, message: error.describe(), data: Some(json!({"errors": errors})) }
}

fn read(host: Host, tx: Sender<Inbox>, frames: Arc<Mutex<HashMap<String, Value>>>) {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            eprintln!("vixen-plugin-sdk: ignored a line that isn't JSON: {}", line.chars().take(80).collect::<String>());
            continue;
        };
        if message.get("method").is_none() {
            let id = match &message["id"] {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            host.answered(id, message);
            continue;
        }
        if message["method"] == "controller.frame" && message.get("id").is_none() {
            let id = message["params"]["instanceId"].as_str().unwrap_or("").to_string();
            let waiting = frames.lock().unwrap_or_else(|p| p.into_inner()).insert(id.clone(), message["params"].clone()).is_some();
            if !waiting && tx.send(Inbox::Frame(id)).is_err() {
                return;
            }
            continue;
        }
        if tx.send(Inbox::Message(message)).is_err() {
            return;
        }
    }
    let _ = tx.send(Inbox::Closed);
}

// ---- Base64 (standard alphabet, padded) -------------------------------------------------------------------------

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    out
}

// ---- Configuration checks: the configSchema subset Studio checks (crates/studio-plugins/src/schema.rs) -----------

/// Every way `value` breaks `schema` (at most 50).
pub fn validate(schema: &Value, value: &Value) -> Vec<FieldError> {
    let mut errors = Vec::new();
    check(schema, value, "", &mut errors);
    errors.truncate(50);
    errors
}

fn type_names(schema: &Value) -> Vec<&str> {
    match schema.get("type") {
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(list)) => list.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() || n.as_f64().is_some_and(|f| f.fract() == 0.0) => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn check(schema: &Value, value: &Value, at: &str, errors: &mut Vec<FieldError>) {
    let mut push = |message: String| errors.push(FieldError { field: if at.is_empty() { "/".into() } else { at.into() }, message });
    let types = type_names(schema);
    let actual = type_of(value);
    if !types.is_empty() && !types.iter().any(|t| *t == actual || (*t == "number" && actual == "integer")) {
        push(format!("must be {}", types.join(" or ")));
        return;
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.contains(value) {
            let list: Vec<String> = allowed.iter().map(Value::to_string).collect();
            push(format!("must be one of {}", list.join(", ")));
        }
    }
    if let Some(constant) = schema.get("const") {
        if constant != value {
            push(format!("must be {constant}"));
        }
    }
    match value {
        Value::Number(n) => {
            let n = n.as_f64().unwrap_or(0.0);
            if let Some(min) = schema.get("minimum").and_then(Value::as_f64).filter(|m| n < *m) {
                push(format!("must be at least {min}"));
            }
            if let Some(max) = schema.get("maximum").and_then(Value::as_f64).filter(|m| n > *m) {
                push(format!("must be at most {max}"));
            }
        }
        Value::String(text) => {
            let chars = text.chars().count() as u64;
            if let Some(min) = schema.get("minLength").and_then(Value::as_u64).filter(|m| chars < *m) {
                push(format!("must be at least {min} characters"));
            }
            if let Some(max) = schema.get("maxLength").and_then(Value::as_u64).filter(|m| chars > *m) {
                push(format!("must be at most {max} characters"));
            }
            match schema.get("format").and_then(Value::as_str) {
                Some("ipv4") if text.parse::<std::net::Ipv4Addr>().is_err() => push("must be an IPv4 address such as 192.168.1.50".into()),
                Some("hostname") if !valid_hostname(text) => push("must be a host name".into()),
                _ => {}
            }
        }
        Value::Array(items) => {
            if let Some(min) = schema.get("minItems").and_then(Value::as_u64).filter(|m| (items.len() as u64) < *m) {
                push(format!("must have at least {min} items"));
            }
            if let Some(max) = schema.get("maxItems").and_then(Value::as_u64).filter(|m| (items.len() as u64) > *m) {
                push(format!("must have at most {max} items"));
            }
            if let Some(item_schema) = schema.get("items").filter(|s| s.is_object()) {
                for (i, item) in items.iter().enumerate() {
                    check(item_schema, item, &format!("{at}/{i}"), errors);
                }
            }
        }
        Value::Object(map) => {
            let properties = schema.get("properties").and_then(Value::as_object);
            for name in schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                if !map.contains_key(name) {
                    errors.push(FieldError { field: format!("{at}/{}", escape(name)), message: "is required".into() });
                }
            }
            for (name, item) in map {
                let field = format!("{at}/{}", escape(name));
                match properties.and_then(|p| p.get(name)) {
                    Some(property) => check(property, item, &field, errors),
                    None => match schema.get("additionalProperties") {
                        Some(Value::Bool(false)) => errors.push(FieldError { field, message: "is not a known setting".into() }),
                        Some(extra @ Value::Object(_)) => check(extra, item, &field, errors),
                        _ => {}
                    },
                }
            }
        }
        _ => {}
    }
}

fn escape(name: &str) -> String {
    name.replace('~', "~0").replace('/', "~1")
}

fn valid_hostname(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 253
        && text.split('.').all(|label| {
            !label.is_empty() && label.len() <= 63 && !label.starts_with('-') && !label.ends_with('-') && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// `config` with each missing property that has a default (or an object with defaults inside) filled in.
pub fn fill_defaults(schema: &Value, config: &Value) -> Value {
    let mut out = config.as_object().cloned().unwrap_or_default();
    for (name, property) in schema.get("properties").and_then(Value::as_object).into_iter().flatten() {
        let nested = property.get("properties").is_some_and(Value::is_object);
        match out.get(name) {
            None => {
                if let Some(default) = property.get("default") {
                    out.insert(name.clone(), default.clone());
                } else if nested && type_names(property).contains(&"object") {
                    let inner = fill_defaults(property, &Value::Object(Map::new()));
                    if inner.as_object().is_some_and(|m| !m.is_empty()) {
                        out.insert(name.clone(), inner);
                    }
                }
            }
            Some(existing @ Value::Object(_)) if nested => {
                let filled = fill_defaults(property, existing);
                out.insert(name.clone(), filled);
            }
            Some(_) => {}
        }
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_send_running_totals() {
        let instance = Instance { host: Host::new(), instance_id: "i".into(), type_id: "t".into(), output_count: 3, config: json!({}), running: true, paused: false, counted: Arc::default() };
        instance.count(1, 100, 0);
        let copy = instance.clone();
        assert_eq!(copy.count(2, 50, 1), json!({"instanceId": "i", "packetsSent": 3, "bytesSent": 150, "errors": 1}));
    }

    #[test]
    fn base64_round_trips() {
        for bytes in [&b""[..], b"a", b"ab", b"abc", b"\x00\xff\x10\x80"] {
            assert_eq!(base64_decode(&base64_encode(bytes)), bytes);
        }
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
    }

    #[test]
    fn frames_decode_each_encoding() {
        let u8 = Frame::from_params(&json!({"encoding": "u8", "values": base64_encode(&[0, 128, 255]), "seq": 3}));
        assert_eq!((u8.levels, u8.seq), (vec![0, 128, 255], 3));
        let u16 = Frame::from_params(&json!({"encoding": "u16", "values": base64_encode(&[0x34, 0x12, 0xff, 0xff])}));
        assert_eq!(u16.levels, vec![0x1234, 0xffff]);
        let js = Frame::from_params(&json!({"encoding": "json", "values": [null, 7, {"r": 1, "g": 2, "b": 3}]}));
        assert_eq!(js.values.len(), 3);
        assert_eq!(triplets(&[1, 2, 3, 4, 5, 6, 7], "grb"), vec![[2, 1, 3], [5, 4, 6]]);
    }

    #[test]
    fn defaults_fill_and_checks_refuse() {
        let schema = json!({"type": "object", "additionalProperties": false, "required": ["host"],
            "properties": {"host": {"type": "string", "format": "hostname"}, "port": {"type": "integer", "default": 80, "minimum": 1},
                           "opts": {"type": "object", "properties": {"x": {"type": "boolean", "default": true}}}}});
        assert_eq!(fill_defaults(&schema, &json!({"host": "a"})), json!({"host": "a", "port": 80, "opts": {"x": true}}));
        let fields: Vec<String> = validate(&schema, &json!({"port": 0, "nope": 1})).into_iter().map(|e| e.field).collect();
        assert_eq!(fields, ["/host", "/nope", "/port"]);
    }

    #[test]
    fn lights_name_their_outputs_and_are_answered() {
        let light = |name: &str, outputs| Light { name: name.into(), outputs };
        let plugin = Plugin::new().outputs_for(None, move |_| Outputs::for_lights(vec![light("Ceiling", 3), light("Porch", 1), light("Strip", 4)]));
        let answer = plugin.answer_outputs(&json!({"typeId": "x", "config": {}})).unwrap().unwrap();
        assert_eq!(answer["outputCount"], 8);
        assert_eq!(answer["outputNames"], json!(["Ceiling R", "Ceiling G", "Ceiling B", "Porch", "Strip R", "Strip G", "Strip B", "Strip W"]));
        assert_eq!(answer["lights"], json!([{"name": "Ceiling", "outputCount": 3}, {"name": "Porch", "outputCount": 1}, {"name": "Strip", "outputCount": 4}]));
        assert_eq!(Outputs::count(2), Outputs { count: 2, names: None, lights: None });
    }
}
