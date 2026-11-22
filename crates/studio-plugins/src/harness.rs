//! The conformance harness behind `plugin_validate`: checks a plugin folder's manifest, then runs the plugin on its own,
//! away from the plugin host and the engine, through the whole protocol: `initialize`, for each controller type
//! `controller.create` / `configure` (with a valid configuration made from its `configSchema` and an invalid one) /
//! `setOutputCount`, then `controller.start`, a burst of frames at 40 fps in every encoding it accepts and
//! `controller.stop` (only with `live_output`, since a real output plugin lights real lights), `controller.destroy` and
//! `shutdown`. Each check passes, fails, warns or is skipped, with its time and what went wrong.
use crate::logs::{Level, LogBuffer};
use crate::manifest::{ControllerType, Manifest};
use crate::process::{self, Launch, PluginProcess};
use crate::rpc::{self, RpcError};
use crate::{schema, API_VERSION};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use vixen_output::OutputCommand;

/// The engine's frame rate at its fastest.
pub const FRAME_INTERVAL: Duration = Duration::from_millis(25);
/// Error code a plugin answers an invalid configuration with.
pub const INVALID_CONFIG: i64 = -32004;

#[derive(Clone, Debug)]
pub struct Options {
    /// Send `controller.start` and frames. The caller has checked the live-output permission and confirmation.
    pub live_output: bool,
    /// How long each encoding's frame burst lasts.
    pub burst: Duration,
    pub request_timeout: Duration,
    pub start_timeout: Duration,
    pub shutdown_grace: Duration,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            live_output: false,
            burst: Duration::from_secs(1),
            request_timeout: Duration::from_secs(5),
            start_timeout: Duration::from_secs(15),
            shutdown_grace: Duration::from_secs(3),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pass,
    Warn,
    Fail,
    Skip,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Warn => "warn",
            Status::Fail => "fail",
            Status::Skip => "skip",
        }
    }
}

#[derive(Default)]
struct Report {
    checks: Vec<Value>,
    failed: usize,
    warned: usize,
}

impl Report {
    fn add(&mut self, name: &str, status: Status, ms: u64, detail: impl Into<String>) {
        match status {
            Status::Fail => self.failed += 1,
            Status::Warn => self.warned += 1,
            _ => {}
        }
        let detail: String = detail.into();
        let mut check = json!({"name": name, "status": status.as_str(), "ms": ms});
        if !detail.is_empty() {
            check["detail"] = json!(detail);
        }
        self.checks.push(check);
    }

    /// Times `f` and records its outcome: `Ok((status, detail))` or a failure.
    fn time<T>(&mut self, name: &str, f: impl FnOnce() -> Result<(Status, String, T), String>) -> Option<T> {
        let started = Instant::now();
        let outcome = f();
        let ms = started.elapsed().as_millis() as u64;
        match outcome {
            Ok((status, detail, value)) => {
                self.add(name, status, ms, detail);
                Some(value)
            }
            Err(why) => {
                self.add(name, Status::Fail, ms, why);
                None
            }
        }
    }
}

/// What the plugin sent back on its own during the run.
#[derive(Default)]
struct Seen {
    statuses: Vec<Value>,
    messages: u64,
    stats: u64,
    tools_changed: u64,
}

/// A private, throwaway `pluginDataDir`, removed afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Scratch, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("vixen-plugin-validate-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        crate::registry::private_dir(&dir)?;
        Ok(Scratch(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Validates the plugin in `dir`: `{folder, id?, passed, failed, warnings, checks: [{name, status, ms, detail?}],
/// frames?, liveOutput, logs}`. `passed` means no check failed.
pub fn validate(dir: &Path, options: &Options) -> Value {
    let mut report = Report::default();
    let logs = Arc::new(LogBuffer::new(None));
    let mut out = json!({"folder": dir.to_string_lossy(), "liveOutput": options.live_output});
    let mut frames = Vec::new();
    run(dir, options, &mut report, &logs, &mut out, &mut frames);
    let entries: Vec<Value> = logs.tail(40, Level::Debug).iter().map(|e| e.to_json()).collect();
    out["passed"] = json!(report.failed == 0);
    out["failed"] = json!(report.failed);
    out["warnings"] = json!(report.warned);
    out["checks"] = Value::Array(report.checks);
    if !frames.is_empty() {
        out["frames"] = Value::Array(frames);
    }
    out["logs"] = Value::Array(entries);
    out
}

fn run(dir: &Path, options: &Options, report: &mut Report, logs: &Arc<LogBuffer>, out: &mut Value, frames: &mut Vec<Value>) {
    let Some(manifest) = report.time("manifest", || {
        let manifest = Manifest::load(dir)?;
        if !manifest.api_version.matches(&API_VERSION) {
            return Err(format!("apiVersion {} doesn't include this Vixen Studio's plugin API {API_VERSION}", manifest.api_version.as_str()));
        }
        let what = format!("{} {}: {} controller type(s), {} tool(s), {} command(s)", manifest.id, manifest.version, manifest.controllers.len(), manifest.tools.len(), manifest.commands.len());
        Ok((Status::Pass, what, manifest))
    }) else {
        return;
    };
    out["id"] = json!(manifest.id);
    for controller in &manifest.controllers {
        let name = format!("configSchema {}", controller.name);
        report.time(&name, || {
            let config = example(&controller.config_schema);
            let errors = schema::validate(&controller.config_schema, &config);
            if errors.is_empty() {
                Ok((Status::Pass, String::new(), ()))
            } else {
                Ok((Status::Warn, format!("couldn't make a valid configuration from it: {}", describe(&errors)), ()))
            }
        });
    }
    if report.time("entry.command", || process::resolve_command(dir, &manifest.entry.command).map(|p| (Status::Pass, p.to_string_lossy().into_owned(), ()))).is_none() {
        return;
    }
    let Ok(scratch) = Scratch::new() else {
        report.add("launch", Status::Fail, 0, "couldn't make a scratch data folder");
        return;
    };
    let seen = Arc::new(Mutex::new(Seen::default()));
    let sink_seen = seen.clone();
    let controllers: process::ControllerSink = Arc::new(move |method: &str, params: &Value| {
        let mut seen = sink_seen.lock().unwrap_or_else(|p| p.into_inner());
        match method {
            "controller.status" => seen.statuses.push(params.clone()),
            "controller.message" => seen.messages += 1,
            _ => seen.stats += 1,
        }
        Ok(json!({}))
    });
    let tools_seen = seen.clone();
    let tools: process::ToolsSink = Arc::new(move |params: &Value| {
        if let Some(list) = params.get("tools").filter(|t| !t.is_null()) {
            Manifest::parse_tools(list).map_err(|why| RpcError::new(rpc::INVALID_PARAMS, why))?;
        }
        tools_seen.lock().unwrap_or_else(|p| p.into_inner()).tools_changed += 1;
        Ok(json!({}))
    });
    let Some(plugin) = report.time("launch", || {
        let launch = Launch { manifest: &manifest, dir, data_dir: &scratch.0, logs: logs.clone(), controllers: Some(controllers), tools: Some(tools) };
        PluginProcess::spawn(launch).map(|p| (Status::Pass, format!("pid {}", p.pid), p))
    }) else {
        return;
    };
    let initialized = report.time("initialize", || {
        let params = json!({
            "apiVersion": API_VERSION.to_string(),
            "hostVersion": crate::HOST_VERSION,
            "pluginDataDir": scratch.0.to_string_lossy(),
            "profile": null,
            "muted": !options.live_output,
            "capabilities": crate::host_capabilities(),
        });
        let answer = plugin.request("initialize", params, options.request_timeout).map_err(|e| e.message)?;
        let version = answer.get("apiVersion").and_then(Value::as_str).ok_or("answered without an apiVersion")?;
        let parsed = crate::semver::Version::parse(version).map_err(|why| format!("answered an invalid apiVersion: {why}"))?;
        if parsed.major != API_VERSION.major {
            return Err(format!("speaks plugin API {version}; this Vixen Studio has {API_VERSION}"));
        }
        Ok((Status::Pass, format!("plugin API {version}"), ()))
    });
    if initialized.is_some() {
        for controller in &manifest.controllers {
            exercise(&plugin, controller, options, report, frames);
        }
        if !manifest.tools.is_empty() {
            let names: Vec<&str> = manifest.tools.iter().map(|t| t.name.as_str()).collect();
            report.add("tools", Status::Skip, 0, format!("{} declared; not called, since a tool can act on the world (call them through MCP once linked)", names.join(", ")));
        }
    }
    let alive = !plugin.is_dead() && plugin.try_wait().is_none();
    report.time("protocol", || {
        if alive {
            Ok((Status::Pass, "only JSON-RPC on stdout; still running".into(), ()))
        } else {
            Err(plugin.describe_exit(plugin.try_wait()))
        }
    });
    let seen = seen.lock().unwrap_or_else(|p| p.into_inner());
    out["reported"] = json!({"statuses": seen.statuses, "messages": seen.messages, "stats": seen.stats, "toolsChanged": seen.tools_changed});
    drop(seen);
    if alive {
        shutdown(&plugin, options, report);
    } else {
        plugin.kill("validation finished");
    }
}

/// Every controller check for one type, on one instance.
fn exercise(plugin: &Arc<PluginProcess>, controller: &ControllerType, options: &Options, report: &mut Report, frames: &mut Vec<Value>) {
    let instance = uuid::Uuid::new_v4().to_string();
    let label = |step: &str| format!("{step} {}", controller.name);
    let request = |method: &str, params: Value, timeout: Duration| plugin.request(method, params, timeout);
    let config = Some(example(&controller.config_schema)).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let created = report.time(&label("controller.create"), || {
        let params = json!({"instanceId": instance, "typeId": controller.type_id, "config": config, "outputCount": controller.default_outputs});
        let answer = request("controller.create", params, options.request_timeout).map_err(|e| failure(&e))?;
        match answer.get("config") {
            Some(normalised) if !normalised.is_object() => Err("answered a config that isn't an object".into()),
            Some(normalised) => {
                let errors = schema::validate(&controller.config_schema, normalised);
                if errors.is_empty() {
                    Ok((Status::Pass, String::new(), ()))
                } else {
                    Ok((Status::Warn, format!("its normalised config breaks its own configSchema: {}", describe(&errors)), ()))
                }
            }
            None => Ok((Status::Pass, String::new(), ())),
        }
    });
    if created.is_none() {
        return;
    }
    report.time(&label("controller.configure"), || {
        request("controller.configure", json!({"instanceId": instance, "config": config}), options.request_timeout).map_err(|e| failure(&e))?;
        Ok((Status::Pass, String::new(), ()))
    });
    match invalid(&controller.config_schema) {
        None => report.add(&label("controller.configure (invalid)"), Status::Skip, 0, "configSchema has no typed property to get wrong"),
        Some((bad, why)) => {
            report.time(&label("controller.configure (invalid)"), || {
                match request("controller.configure", json!({"instanceId": instance, "config": bad}), options.request_timeout) {
                    Ok(_) => Ok((Status::Warn, format!("accepted a configuration where {why}; Studio checks configSchema first, but the plugin should refuse it with {INVALID_CONFIG}"), ())),
                    Err(e) if e.code == INVALID_CONFIG => {
                        let listed = e.data.as_ref().and_then(|d| d.get("errors")).and_then(Value::as_array).is_some_and(|list| !list.is_empty());
                        if listed {
                            Ok((Status::Pass, String::new(), ()))
                        } else {
                            Ok((Status::Warn, format!("refused with {INVALID_CONFIG} but without data.errors [{{field, message}}] for the inspector"), ()))
                        }
                    }
                    Err(e) if e.code == rpc::TIMED_OUT || e.code == rpc::PLUGIN_EXITED => Err(failure(&e)),
                    Err(e) => Ok((Status::Warn, format!("refused with code {} instead of {INVALID_CONFIG}: {}", e.code, e.message), ())),
                }
            });
            // Put the good configuration back before going on.
            let _ = request("controller.configure", json!({"instanceId": instance, "config": config}), options.request_timeout);
        }
    }
    let more = (controller.default_outputs + 1).min(controller.max_outputs);
    report.time(&label("controller.setOutputCount"), || {
        request("controller.setOutputCount", json!({"instanceId": instance, "outputCount": more}), options.request_timeout).map_err(|e| failure(&e))?;
        request("controller.setOutputCount", json!({"instanceId": instance, "outputCount": controller.default_outputs}), options.request_timeout).map_err(|e| failure(&e))?;
        Ok((Status::Pass, format!("{} → {more} → {}", controller.default_outputs, controller.default_outputs), ()))
    });
    let id = json!({"instanceId": instance});
    if options.live_output {
        let started = report.time(&label("controller.start"), || {
            request("controller.start", id.clone(), options.start_timeout).map_err(|e| failure(&e))?;
            Ok((Status::Pass, String::new(), ()))
        });
        if started.is_some() {
            for encoding in &controller.frame_encodings {
                let (status, detail, summary) = burst(plugin, &instance, controller, encoding, options);
                report.add(&label(&format!("frames {encoding}")), status, summary["ms"].as_u64().unwrap_or(0), detail);
                frames.push(summary);
            }
            report.time(&label("controller.stop"), || {
                request("controller.stop", id.clone(), options.request_timeout).map_err(|e| failure(&e))?;
                Ok((Status::Pass, String::new(), ()))
            });
        }
    } else {
        let why = "skipped: real output plugins light real lights, so start and frames need liveOutput:true (with Allow agents to control live output and confirm:true)";
        report.add(&label("controller.start"), Status::Skip, 0, why);
        report.add(&label("frames"), Status::Skip, 0, why);
    }
    report.time(&label("controller.destroy"), || {
        request("controller.destroy", id.clone(), options.request_timeout).map_err(|e| failure(&e))?;
        Ok((Status::Pass, String::new(), ()))
    });
}

/// Frames at 40 fps for `options.burst`: a chase over the outputs. Reports how many the plugin left unread (dropped).
fn burst(plugin: &Arc<PluginProcess>, instance: &str, controller: &ControllerType, encoding: &str, options: &Options) -> (Status, String, Value) {
    let codec = match encoding {
        "json" => crate::controllers::Encoding::Json,
        "u16" => crate::controllers::Encoding::U16,
        _ => crate::controllers::Encoding::U8,
    };
    let outputs = controller.default_outputs.min(4096) as usize;
    let started = Instant::now();
    let (mut sent, mut dropped, mut refused) = (0u64, 0u64, 0u64);
    let mut seq = 0u64;
    while started.elapsed() < options.burst {
        let tick = Instant::now();
        seq += 1;
        let commands: Vec<OutputCommand> = (0..outputs).map(|i| OutputCommand::U8(if i == (seq as usize) % outputs.max(1) { 255 } else { 0 })).collect();
        let params = json!({"instanceId": instance, "seq": seq, "timeMs": started.elapsed().as_millis() as u64, "encoding": encoding, "values": codec.encode(&commands)});
        match plugin.send_frame(instance, params) {
            Ok(process::FrameSent::Queued) => sent += 1,
            Ok(process::FrameSent::Replaced) => {
                sent += 1;
                dropped += 1;
            }
            Err(_) => refused += 1,
        }
        std::thread::sleep(FRAME_INTERVAL.saturating_sub(tick.elapsed()));
    }
    // Ask something harmless, so the frames before it have been read when the answer comes.
    let reply = plugin.request("controller.setOutputCount", json!({"instanceId": instance, "outputCount": controller.default_outputs}), options.request_timeout);
    let ms = started.elapsed().as_millis() as u64;
    let summary = json!({"encoding": encoding, "sent": sent, "dropped": dropped, "refused": refused, "ms": ms});
    let detail = format!("{sent} frames in {ms} ms, {dropped} dropped (latest frame wins)");
    if refused > 0 || plugin.is_dead() {
        return (Status::Fail, format!("{detail}; {refused} could not be sent: {}", plugin.describe_exit(plugin.try_wait())), summary);
    }
    if let Err(e) = reply {
        if e.code == rpc::TIMED_OUT {
            return (Status::Fail, format!("{detail}; then didn't answer within {} ms: frames block its input", options.request_timeout.as_millis()), summary);
        }
    }
    let status = if dropped * 10 > sent.max(1) { Status::Warn } else { Status::Pass };
    let detail = if status == Status::Warn { format!("{detail}: it reads frames slower than 40 fps") } else { detail };
    (status, detail, summary)
}

fn shutdown(plugin: &Arc<PluginProcess>, options: &Options, report: &mut Report) {
    report.time("shutdown", || {
        plugin.request("shutdown", json!({}), options.shutdown_grace).map_err(|e| failure(&e))?;
        Ok((Status::Pass, String::new(), ()))
    });
    let started = Instant::now();
    plugin.shutdown(options.shutdown_grace);
    let ms = started.elapsed().as_millis() as u64;
    if Duration::from_millis(ms) + Duration::from_millis(50) >= options.shutdown_grace {
        report.add("exit", Status::Fail, ms, format!("still running {} ms after shutdown and stdin closed; it was killed", options.shutdown_grace.as_millis()));
    } else {
        report.add("exit", Status::Pass, ms, "exited after shutdown");
    }
}

fn failure(e: &RpcError) -> String {
    match e.code {
        rpc::TIMED_OUT => format!("no answer: {}", e.message),
        rpc::PLUGIN_EXITED => format!("the plugin exited: {}", e.message),
        rpc::METHOD_NOT_FOUND => format!("not implemented ({}): {}", e.code, e.message),
        code => format!("error {code}: {}", e.message),
    }
}

fn describe(errors: &[schema::FieldError]) -> String {
    errors.iter().take(5).map(|e| format!("{} {}", if e.field.is_empty() { "/" } else { &e.field }, e.message)).collect::<Vec<_>>().join("; ")
}

fn type_name(schema: &Value) -> Option<&str> {
    match schema.get("type") {
        Some(Value::String(t)) => Some(t),
        Some(Value::Array(list)) => list.iter().filter_map(Value::as_str).find(|t| *t != "null"),
        _ => None,
    }
}

/// A configuration `schema` accepts: defaults where given, otherwise the smallest value each keyword allows.
pub fn example(schema: &Value) -> Value {
    if let Some(value) = schema.get("default").or_else(|| schema.get("const")) {
        return value.clone();
    }
    if let Some(first) = schema.get("enum").and_then(Value::as_array).and_then(|e| e.first()) {
        return first.clone();
    }
    let number = |integer: bool| {
        let min = schema.get("minimum").and_then(Value::as_f64);
        let max = schema.get("maximum").and_then(Value::as_f64);
        let value = match (min, max) {
            (Some(min), _) => min,
            (None, Some(max)) if max < 0.0 => max,
            _ => 0.0,
        };
        if integer { json!(value.ceil() as i64) } else { json!(value) }
    };
    match type_name(schema) {
        Some("object") | None if schema.get("properties").is_some() || type_name(schema).is_some() => {
            let mut out = Map::new();
            let required: Vec<&str> = schema.get("required").and_then(Value::as_array).map(|r| r.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            for (name, property) in schema.get("properties").and_then(Value::as_object).into_iter().flatten() {
                let value = example(property);
                if required.contains(&name.as_str()) || property.get("default").is_some() {
                    out.insert(name.clone(), value);
                }
            }
            Value::Object(out)
        }
        Some("string") => match schema.get("format").and_then(Value::as_str) {
            Some("ipv4") => json!("127.0.0.1"),
            Some("hostname") => json!("localhost"),
            _ => {
                let min = schema.get("minLength").and_then(Value::as_u64).unwrap_or(0) as usize;
                json!("x".repeat(min))
            }
        },
        Some("integer") => number(true),
        Some("number") => number(false),
        Some("boolean") => json!(false),
        Some("array") => {
            let min = schema.get("minItems").and_then(Value::as_u64).unwrap_or(0) as usize;
            let item = schema.get("items").map(example).unwrap_or(Value::Null);
            Value::Array(vec![item; min])
        }
        Some("null") => Value::Null,
        _ => Value::Null,
    }
}

/// A configuration `schema` refuses, and what is wrong with it: an undeclared field when `additionalProperties` is
/// false, otherwise a typed property set to a value of the wrong type.
pub fn invalid(schema: &Value) -> Option<(Value, String)> {
    let mut config = example(schema);
    let object = config.as_object_mut()?;
    let properties = schema.get("properties").and_then(Value::as_object);
    if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
        let name = ["bad", "bad_field", "__bad__"].into_iter().find(|n| !properties.is_some_and(|p| p.contains_key(*n)))?;
        object.insert(name.into(), json!(true));
        return Some((config, format!("\"{name}\" isn't a field it declares")));
    }
    for (name, property) in properties.into_iter().flatten() {
        let wrong = match type_name(property) {
            Some("string") => json!(12345),
            Some("integer") | Some("number") | Some("boolean") | Some("array") | Some("object") => json!("not a valid value"),
            _ => continue,
        };
        object.insert(name.clone(), wrong.clone());
        return Some((config, format!("\"{name}\" is {wrong} instead of a {}", type_name(property).unwrap_or("value"))));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_satisfy_the_schema() {
        let schema = json!({
            "type": "object",
            "required": ["host", "port", "devices", "mode", "label"],
            "properties": {
                "host": {"type": "string", "format": "ipv4"},
                "port": {"type": "integer", "minimum": 1, "maximum": 65535},
                "gamma": {"type": "number", "default": 2.2},
                "mode": {"enum": ["rgb", "grb"]},
                "label": {"type": "string", "minLength": 2},
                "devices": {"type": "array", "minItems": 1, "items": {"type": "object", "required": ["ip"], "properties": {"ip": {"type": "string", "format": "ipv4"}}}},
            },
            "additionalProperties": false,
        });
        let config = example(&schema);
        assert!(schema::validate(&schema, &config).is_empty(), "{config}");
        assert_eq!(config["port"], json!(1));
        assert_eq!(config["gamma"], json!(2.2));
        let (bad, why) = invalid(&schema).unwrap();
        assert!(!schema::validate(&schema, &bad).is_empty(), "{why}");
    }

    #[test]
    fn invalid_configs_get_a_typed_field_wrong() {
        let schema = json!({"type": "object", "properties": {"note": {"type": "string"}}});
        let (bad, why) = invalid(&schema).unwrap();
        assert_eq!(bad["note"], json!(12345), "{why}");
        assert!(invalid(&json!({"type": "object"})).is_none());
    }
}
